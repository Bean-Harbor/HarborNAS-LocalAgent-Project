"""Playwright-based browser adapter for the Feishu setup flow.

Architecture — two layers of browser control
---------------------------------------------
1. **API layer** (primary): After QR login, intercept the SPA's own
   network requests to discover Feishu's console API endpoints, then call
   them directly via ``page.evaluate(() => fetch(...))``.  This inherits
   all session cookies / CSRF tokens automatically and is immune to DOM
   changes.
2. **JS-DOM layer** (fallback): When API endpoints are unknown, use
   ``page.evaluate()`` to find clickable elements by **visible text** and
   fill inputs by **placeholder / name** attributes.  Much more resilient
   than CSS selectors because text content is the most stable part of a UI.

The browser is needed *only* for QR login.  All subsequent operations go
through ``fetch()`` or JS-DOM manipulation — never brittle CSS locators.

Debug output
------------
* Every step saves a screenshot to ``/tmp/feishu_step_<name>.png``.
* All captured XHR/Fetch requests → ``/tmp/feishu_api_log.json``.
* Page HTML → ``/tmp/feishu_page_<step>.html`` on failures.
"""
from __future__ import annotations

import json
import logging
import os
import re
import time
from typing import Any

try:
    from playwright.sync_api import (
        Browser,
        BrowserContext,
        Page,
        Playwright,
        sync_playwright,
    )
    HAS_PLAYWRIGHT = True
except ImportError:  # pragma: no cover
    HAS_PLAYWRIGHT = False

log = logging.getLogger("harborbeacon.playwright")

_SCREENSHOT_DIR = "/tmp"
_FEISHU_OPEN_URL = "https://open.feishu.cn/app"
_DEFAULT_AVATAR = "https://s1-imfile.feishucdn.com/static-resource/v1/v3_00s0_576232a3-1abe-46bf-8298-e0966ecd2eeg"
_DEFAULT_EVENTS = ["im.message.receive_v1"]


class PlaywrightFeishuDriver:
    """Manages a Playwright browser session for Feishu Open Platform automation.

    Lifecycle::

        driver = PlaywrightFeishuDriver()
        driver.launch()          # opens browser + enables request capture
        driver.open_login()      # navigates to Feishu login
        driver.wait_for_login()  # blocks until login auto-detected
        driver.create_app(name, desc)
        driver.enable_bot()
        driver.set_callback_url(url)
        driver.grant_permissions([...])
        creds = driver.extract_credentials()
        driver.close()
    """

    def __init__(self, *, headless: bool = False, timeout_ms: int = 60_000) -> None:
        if not HAS_PLAYWRIGHT:
            raise RuntimeError(
                "playwright is not installed – run: "
                "pip install playwright && python -m playwright install chromium"
            )
        self._headless = headless
        self._timeout = timeout_ms
        self._pw: Playwright | None = None
        self._browser: Browser | None = None
        self._context: BrowserContext | None = None
        self._page: Page | None = None
        # Network request capture for API discovery
        self._captured_requests: list[dict[str, Any]] = []
        self._last_app_id: str = ""
        self._x_csrf_token: str = ""

    # ================================================================
    # Lifecycle
    # ================================================================

    def launch(self) -> None:
        """Start Playwright and open a Chromium browser."""
        self._pw = sync_playwright().start()
        self._browser = self._pw.chromium.launch(
            headless=self._headless,
            args=["--disable-blink-features=AutomationControlled"],
        )
        self._context = self._browser.new_context(
            viewport={"width": 1280, "height": 900},
            locale="zh-CN",
            user_agent=(
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) "
                "AppleWebKit/537.36 (KHTML, like Gecko) "
                "Chrome/124.0.0.0 Safari/537.36"
            ),
        )
        self._page = self._context.new_page()
        self._page.set_default_timeout(self._timeout)
        self._setup_request_capture()
        log.info("Playwright Chromium launched (headless=%s)", self._headless)

    def close(self) -> None:
        """Tear down browser resources and dump captured API log."""
        self._dump_api_log()
        if self._context:
            self._context.close()
        if self._browser:
            self._browser.close()
        if self._pw:
            self._pw.stop()
        self._page = self._context = self._browser = self._pw = None
        log.info("Playwright browser closed")

    @property
    def page(self) -> Page:
        if self._page is None:
            raise RuntimeError("Browser not launched – call launch() first")
        return self._page

    # ================================================================
    # Network capture — discover console API endpoints
    # ================================================================

    def _setup_request_capture(self) -> None:
        """Intercept all XHR/Fetch requests to learn API patterns."""
        def _on_request(req: Any) -> None:
            try:
                if req.resource_type in ("xhr", "fetch"):
                    # post_data can throw on binary bodies — guard it
                    try:
                        pd = req.post_data
                        post_data = pd[:500] if pd else None
                    except Exception:
                        post_data = "(binary)"
                    entry = {
                        "method": req.method,
                        "url": req.url,
                        "post_data": post_data,
                    }
                    self._captured_requests.append(entry)
                    log.debug("API captured: %s %s", req.method, req.url)
            except Exception:
                pass  # never let the listener crash Playwright
        self.page.on("request", _on_request)

    def _dump_api_log(self) -> None:
        """Save all captured API requests to /tmp for debugging."""
        if not self._captured_requests:
            return
        path = os.path.join(_SCREENSHOT_DIR, "feishu_api_log.json")
        try:
            with open(path, "w", encoding="utf-8") as f:
                json.dump(self._captured_requests, f, ensure_ascii=False, indent=2)
            log.info("API log saved: %s (%d requests)", path, len(self._captured_requests))
        except Exception:
            log.warning("Failed to save API log")

    # ================================================================
    # Debug helpers
    # ================================================================

    def _screenshot_step(self, step_name: str) -> str:
        path = os.path.join(_SCREENSHOT_DIR, f"feishu_step_{step_name}.png")
        try:
            self.page.screenshot(path=path, full_page=True)
            log.info("Screenshot: %s", path)
        except Exception:
            log.warning("Failed to screenshot step %s", step_name)
        return path

    def _save_page_html(self, step_name: str) -> str:
        path = os.path.join(_SCREENSHOT_DIR, f"feishu_page_{step_name}.html")
        try:
            with open(path, "w", encoding="utf-8") as f:
                f.write(self.page.content())
            log.info("HTML saved: %s", path)
        except Exception:
            log.warning("Failed to save HTML for %s", step_name)
        return path

    # ================================================================
    # JS-DOM helpers — resilient element interaction via page.evaluate()
    # ================================================================

    def _js_click(self, *texts: str, timeout_sec: int = 8) -> dict[str, Any]:
        """Find a clickable element by visible text and click it.

        Searches ALL interactive elements by ``innerText``.
        Retries until *timeout_sec* to handle async rendering.
        """
        deadline = time.monotonic() + timeout_sec
        while time.monotonic() < deadline:
            result: dict[str, Any] = self.page.evaluate(
                """(texts) => {
                    const els = document.querySelectorAll(
                        'button, a, [role="button"], [onclick], span[class*="btn"], div[class*="btn"]'
                    );
                    for (const el of els) {
                        const t = (el.innerText || el.textContent || '').trim();
                        if (!t || el.offsetParent === null) continue;
                        for (const txt of texts) {
                            if (t.includes(txt)) {
                                el.scrollIntoView({block: 'center'});
                                el.click();
                                return {found: true, text: t.slice(0, 80), tag: el.tagName};
                            }
                        }
                    }
                    return {found: false};
                }""",
                list(texts),
            )
            if result.get("found"):
                log.info("JS click: '%s' (%s)", result.get("text"), result.get("tag"))
                return result
            self.page.wait_for_timeout(1000)
        log.warning("JS click: none of %s found after %ds", texts, timeout_sec)
        return {"found": False}

    def _js_fill(self, value: str, *hints: str, timeout_sec: int = 5) -> dict[str, Any]:
        """Find an input by placeholder / name and fill it (React/Vue-friendly).

        Uses the native value setter + dispatches input/change events so
        that framework reactivity picks up the value.
        """
        deadline = time.monotonic() + timeout_sec
        while time.monotonic() < deadline:
            result: dict[str, Any] = self.page.evaluate(
                """([hints, value]) => {
                    const inputs = document.querySelectorAll('input, textarea');
                    for (const el of inputs) {
                        const ph = (el.placeholder || '').toLowerCase();
                        const nm = (el.name || '').toLowerCase();
                        const lb = (el.getAttribute('aria-label') || '').toLowerCase();
                        if (el.offsetParent === null) continue;
                        for (const hint of hints) {
                            const h = hint.toLowerCase();
                            if (ph.includes(h) || nm.includes(h) || lb.includes(h)) {
                                el.focus();
                                const setter = Object.getOwnPropertyDescriptor(
                                    Object.getPrototypeOf(el), 'value'
                                );
                                if (setter && setter.set) {
                                    setter.set.call(el, value);
                                } else {
                                    el.value = value;
                                }
                                el.dispatchEvent(new Event('input', {bubbles: true}));
                                el.dispatchEvent(new Event('change', {bubbles: true}));
                                return {found: true, placeholder: ph, name: nm};
                            }
                        }
                    }
                    return {found: false};
                }""",
                [list(hints), value],
            )
            if result.get("found"):
                log.info("JS fill: input ph=%s", result.get("placeholder"))
                return result
            self.page.wait_for_timeout(1000)
        log.warning("JS fill: no input for %s after %ds", hints, timeout_sec)
        return {"found": False}

    def _js_fetch(
        self,
        url: str,
        method: str = "GET",
        body: Any = None,
        headers: dict[str, str] | None = None,
    ) -> dict[str, Any]:
        """Execute a fetch() call from within the page context.

        Inherits all session cookies, CSRF tokens, and headers automatically.
        """
        result: dict[str, Any] = self.page.evaluate(
            """async ([url, method, body, headers]) => {
                try {
                    const opts = {
                        method,
                        credentials: 'include',
                        headers: {...(headers || {})},
                    };
                    if (body !== null && body !== undefined) {
                        opts.body = JSON.stringify(body);
                    }
                    const resp = await fetch(url, opts);
                    const ct = resp.headers.get('content-type') || '';
                    const data = ct.includes('json') ? await resp.json() : await resp.text();
                    return {status: resp.status, ok: resp.ok, data};
                } catch (e) {
                    return {status: 0, ok: false, error: e.message};
                }
            }""",
            [url, method, body, headers or {}],
        )
        log.info("JS fetch: %s %s → %s", method, url, result.get("status"))
        return result

    def _current_app_id(self) -> str:
        if self._last_app_id:
            return self._last_app_id
        match = re.search(r"/app/(cli_[a-zA-Z0-9]+)", self.page.url)
        if match:
            self._last_app_id = match.group(1)
        return self._last_app_id

    def _ensure_console_csrf_token(self, force_refresh: bool = False) -> str:
        if self._x_csrf_token and not force_refresh:
            return self._x_csrf_token

        token = ""
        try:
            token = str(self.page.evaluate(
                """() => {
                    return window.csrfToken || window._csrfToken || window.__csrfToken || '';
                }"""
            ) or "")
        except Exception:
            token = ""

        if not token:
            try:
                token = str(self.page.evaluate(
                    f"""async () => {{
                        try {{
                            const resp = await fetch('{_FEISHU_OPEN_URL}?lang=zh-CN', {{
                                method: 'GET',
                                credentials: 'include',
                            }});
                            const html = await resp.text();
                            const match = html.match(/window\\.csrfToken=\"([^\"]+)\";/);
                            return match ? match[1] : '';
                        }} catch (e) {{
                            return '';
                        }}
                    }}"""
                ) or "")
            except Exception:
                token = ""

        self._x_csrf_token = token
        if token:
            log.info("Resolved Feishu console CSRF token")
        else:
            log.warning("Failed to resolve Feishu console CSRF token")
        return token

    def _console_api_request(
        self,
        path: str,
        *,
        method: str = "GET",
        body: Any = None,
        require_csrf: bool = False,
    ) -> dict[str, Any]:
        url = path if path.startswith("http") else f"https://open.feishu.cn/{path.lstrip('/')}"
        headers: dict[str, str] = {}
        if require_csrf:
            token = self._ensure_console_csrf_token()
            if token:
                headers["x-csrf-token"] = token
        return self._js_fetch(url, method=method, body=body, headers=headers)

    def _console_api_post(self, path: str, body: Any = None, *, require_csrf: bool = False) -> dict[str, Any]:
        return self._console_api_request(
            path,
            method="POST",
            body=body,
            require_csrf=require_csrf,
        )

    def _console_api_get(self, path: str, *, require_csrf: bool = False) -> dict[str, Any]:
        return self._console_api_request(path, method="GET", require_csrf=require_csrf)

    def _unwrap_result_data(self, result: dict[str, Any]) -> dict[str, Any]:
        payload = result.get("data")
        if not isinstance(payload, dict):
            return {}
        nested = payload.get("data")
        if isinstance(nested, dict):
            return nested
        return payload

    def _normalize_scope_types(self, scope: dict[str, Any]) -> list[int]:
        raw_types = scope.get("scopeType")
        if isinstance(raw_types, list):
            return [int(item) for item in raw_types if isinstance(item, (int, str)) and str(item).isdigit()]
        if isinstance(raw_types, (int, str)) and str(raw_types).isdigit():
            return [int(raw_types)]
        return []

    def _js_page_info(self) -> dict[str, Any]:
        """Collect current page info for debugging."""
        return self.page.evaluate(
            """() => {
                const buttons = [];
                document.querySelectorAll('button, a, [role="button"]').forEach(el => {
                    const t = (el.innerText || '').trim();
                    if (t && el.offsetParent !== null) buttons.push(t.slice(0, 60));
                });
                const inputs = [];
                document.querySelectorAll('input, textarea').forEach(el => {
                    if (el.offsetParent !== null) inputs.push({
                        tag: el.tagName, type: el.type || '',
                        placeholder: el.placeholder || '', name: el.name || '',
                    });
                });
                return {
                    url: location.href,
                    title: document.title,
                    buttons: buttons.slice(0, 30),
                    inputs: inputs.slice(0, 20),
                };
            }"""
        )

    def _js_has_internal_error(self) -> bool:
        """Check whether current page is Feishu's generic internal-error view."""
        try:
            return bool(self.page.evaluate(
                """() => {
                    const txt = (document.body?.innerText || '');
                    return txt.includes('服务器出错了') || txt.includes('Internal Error');
                }"""
            ))
        except Exception:
            return False

    def _ensure_not_internal_error_page(self) -> None:
        """Recover from Feishu internal-error page by navigating back to a known route."""
        if not self._js_has_internal_error():
            return
        log.warning("Detected Feishu internal error page, recovering navigation")
        if self._last_app_id:
            recover_url = f"{_FEISHU_OPEN_URL}/{self._last_app_id}/baseinfo"
        else:
            recover_url = _FEISHU_OPEN_URL
        self.page.goto(recover_url, wait_until="domcontentloaded")
        self.page.wait_for_timeout(3000)

    def _extract_app_id_from_result(self, result: dict[str, Any]) -> str:
        data = result.get("data")
        candidates: list[Any] = []
        if isinstance(data, dict):
            candidates.extend([
                data.get("ClientID"),
                data.get("client_id"),
                data.get("app_id"),
            ])
        nested = self._unwrap_result_data(result)
        if nested:
            candidates.extend([
                nested.get("ClientID"),
                nested.get("client_id"),
                nested.get("app_id"),
            ])
        for value in candidates:
            if isinstance(value, str) and value.startswith("cli_"):
                return value
        return ""

    def _js_click_in_dialog(self, *texts: str, timeout_sec: int = 8) -> dict[str, Any]:
        """Click a button by text inside the currently visible modal dialog."""
        deadline = time.monotonic() + timeout_sec
        while time.monotonic() < deadline:
            result: dict[str, Any] = self.page.evaluate(
                """(texts) => {
                    const normalize = (s) => (s || '').replace(/\\s+/g, '').trim();
                    const dialogs = Array.from(document.querySelectorAll('[role="dialog"], .ant-modal, .semi-modal'))
                        .filter(el => el && el.offsetParent !== null);
                    if (dialogs.length === 0) return {found: false, reason: 'no-dialog'};
                    const dialog = dialogs[dialogs.length - 1];

                    // Restrict to genuinely clickable controls, avoid container DIVs.
                    const els = dialog.querySelectorAll('button, [role="button"], a, span[class*="btn"]');

                    // First pass: exact text match after whitespace normalization.
                    for (const txt of texts) {
                        const target = normalize(txt);
                        if (!target) continue;
                        for (const el of els) {
                            if (el.offsetParent === null) continue;
                            const t = normalize(el.innerText || el.textContent || '');
                            if (!t) continue;
                            if (t === target) {
                                el.scrollIntoView({block: 'center'});
                                el.click();
                                return {found: true, text: (el.innerText || '').trim().slice(0, 80), tag: el.tagName, mode: 'exact'};
                            }
                        }
                    }

                    // Second pass: contains match on short control text only.
                    for (const el of els) {
                        const t = (el.innerText || el.textContent || '').trim();
                        if (!t || el.offsetParent === null) continue;
                        for (const txt of texts) {
                            if (t.length <= 12 && t.includes(txt)) {
                                el.scrollIntoView({block: 'center'});
                                el.click();
                                return {found: true, text: t.slice(0, 80), tag: el.tagName, mode: 'contains'};
                            }
                        }
                    }
                    return {found: false};
                }""",
                list(texts),
            )
            if result.get("found"):
                log.info("JS click(dialog): '%s' (%s)", result.get("text"), result.get("tag"))
                return result
            self.page.wait_for_timeout(1000)
        log.warning("JS click(dialog): none of %s found after %ds", texts, timeout_sec)
        return {"found": False}

    def _js_fill_in_dialog(self, value: str, *hints: str, timeout_sec: int = 5) -> dict[str, Any]:
        """Fill input by hint inside visible modal dialog to avoid matching page search boxes."""
        deadline = time.monotonic() + timeout_sec
        while time.monotonic() < deadline:
            result: dict[str, Any] = self.page.evaluate(
                """([hints, value]) => {
                    const dialogs = Array.from(document.querySelectorAll('[role="dialog"], .ant-modal, .semi-modal'))
                        .filter(el => el && el.offsetParent !== null);
                    if (dialogs.length === 0) return {found: false, reason: 'no-dialog'};
                    const dialog = dialogs[dialogs.length - 1];
                    const inputs = Array.from(dialog.querySelectorAll('input, textarea'))
                        .filter(el => el && el.offsetParent !== null && !el.disabled && !el.readOnly);
                    for (const el of inputs) {
                        const ph = (el.placeholder || '').toLowerCase();
                        const nm = (el.name || '').toLowerCase();
                        const lb = (el.getAttribute('aria-label') || '').toLowerCase();
                        for (const hint of hints) {
                            const h = hint.toLowerCase();
                            if (ph.includes(h) || nm.includes(h) || lb.includes(h)) {
                                el.focus();
                                const setter = Object.getOwnPropertyDescriptor(
                                    Object.getPrototypeOf(el), 'value'
                                );
                                if (setter && setter.set) {
                                    setter.set.call(el, value);
                                } else {
                                    el.value = value;
                                }
                                el.dispatchEvent(new Event('input', {bubbles: true}));
                                el.dispatchEvent(new Event('change', {bubbles: true}));
                                return {found: true, placeholder: ph, name: nm};
                            }
                        }
                    }

                    // Fallback for Feishu create-app modal where fields often have no placeholder/name.
                    const hintText = hints.map(h => h.toLowerCase()).join(' ');
                    const textInputs = inputs.filter(el => el.tagName === 'INPUT');
                    const textareas = inputs.filter(el => el.tagName === 'TEXTAREA');

                    let target = null;
                    if (hintText.includes('名称') || hintText.includes('name')) {
                        target = textInputs[0] || null;
                    } else if (hintText.includes('描述') || hintText.includes('description')) {
                        target = textareas[0] || textInputs[1] || null;
                    }

                    if (target) {
                        target.focus();
                        const setter = Object.getOwnPropertyDescriptor(
                            Object.getPrototypeOf(target), 'value'
                        );
                        if (setter && setter.set) {
                            setter.set.call(target, value);
                        } else {
                            target.value = value;
                        }
                        target.dispatchEvent(new Event('input', {bubbles: true}));
                        target.dispatchEvent(new Event('change', {bubbles: true}));
                        return {
                            found: true,
                            placeholder: (target.placeholder || '').toLowerCase(),
                            name: (target.name || '').toLowerCase(),
                            fallback: true,
                            tag: target.tagName,
                        };
                    }

                    return {found: false};
                }""",
                [list(hints), value],
            )
            if result.get("found"):
                log.info("JS fill(dialog): input ph=%s", result.get("placeholder"))
                return result
            self.page.wait_for_timeout(1000)
        log.warning("JS fill(dialog): no input for %s after %ds", hints, timeout_sec)
        return {"found": False}

    # ================================================================
    # Navigation & Login
    # ================================================================

    def open_login(self) -> str:
        """Navigate to Feishu Open Platform (shows QR code)."""
        self.page.goto(_FEISHU_OPEN_URL, wait_until="domcontentloaded")
        self.page.wait_for_timeout(3000)
        log.info("Opened %s → %s", _FEISHU_OPEN_URL, self.page.url)
        return _FEISHU_OPEN_URL

    def wait_for_login(self, timeout_sec: int = 120) -> bool:
        """Block until login auto-detected via URL change."""
        deadline = time.monotonic() + timeout_sec
        log.info("Waiting for login (timeout=%ds)…", timeout_sec)
        while time.monotonic() < deadline:
            url = self.page.url
            if ("open.feishu.cn" in url
                    and "passport" not in url
                    and "login" not in url
                    and "accounts" not in url):
                log.info("Login detected: %s", url)
                self.page.wait_for_timeout(3000)
                return True
            self.page.wait_for_timeout(2000)
        self._screenshot_step("wait_login_timeout")
        raise TimeoutError("飞书登录超时，请在 %d 秒内完成扫码" % timeout_sec)

    # ================================================================
    # App creation
    # ================================================================

    def create_app(self, name: str = "HarborBeacon-Bot", desc: str = "") -> str:
        """Create a new custom app on Feishu Open Platform.

        Strategy: try console API first, then fall back to JS-DOM click-through.
        After creation, navigates to app detail page.
        """
        if "open.feishu.cn/app" not in self.page.url:
            self.page.goto(_FEISHU_OPEN_URL, wait_until="domcontentloaded")
            self.page.wait_for_timeout(3000)

        self._screenshot_step("create_app_0_page")
        info = self._js_page_info()
        log.info("Page before create: %s", json.dumps(info, ensure_ascii=False))

        # --- Attempt 1: Console API ---
        app_id = self._try_create_app_via_api(name, desc)
        if app_id:
            self.navigate_to_app_detail(app_id)
            return app_id

        # --- Attempt 2: JS-DOM click-through ---
        app_id = self._create_app_via_dom(name, desc)
        if app_id:
            self._last_app_id = app_id
            self.navigate_to_app_detail(app_id)
            return app_id
        raise RuntimeError("应用已提交创建，但未能获取 app_id，请稍后重试")

    def navigate_to_app_detail(self, app_id: str) -> None:
        """Navigate to the app detail page where configuration is done."""
        self._last_app_id = app_id
        detail_url = f"{_FEISHU_OPEN_URL}/{app_id}"
        log.info("Navigating to app detail: %s", detail_url)
        self.page.goto(detail_url, wait_until="domcontentloaded")
        self.page.wait_for_timeout(3000)
        self._ensure_not_internal_error_page()
        self._screenshot_step("app_detail_loaded")
        info = self._js_page_info()
        log.info("App detail page loaded: %s", json.dumps(info, ensure_ascii=False))

    def _try_create_app_via_api(self, name: str, desc: str) -> str:
        """Try known Feishu console API patterns."""
        description = desc or f"{name} - HarborNAS AI"
        body = {
            "appSceneType": 0,
            "name": name,
            "desc": description,
            "avatar": _DEFAULT_AVATAR,
            "i18n": {
                "zh_cn": {"name": name, "description": description, "help_use": ""},
                "en_us": {"name": name, "description": description, "help_use": ""},
            },
            "primaryLang": "en_us",
        }

        result = self._console_api_post("developers/v1/app/create", body, require_csrf=True)
        if result.get("ok"):
            aid = self._extract_app_id_from_result(result)
            if aid:
                log.info("App created via internal API: %s", aid)
                return aid

        create_apis = [
            r for r in self._captured_requests
            if "create" in r["url"].lower() and r["method"] == "POST"
        ]
        if create_apis:
            api_url = create_apis[-1]["url"]
            log.info("Trying captured create API: %s", api_url)
            result = self._js_fetch(api_url, "POST", body, headers={
                "x-csrf-token": self._ensure_console_csrf_token(),
            })
            if result.get("ok"):
                aid = self._extract_app_id_from_result(result)
                if aid:
                    log.info("App created via API: %s", aid)
                    return aid

        for api_path in [
            "https://open.feishu.cn/open-apis/app/v1/create",
            "https://open.feishu.cn/api/app/create",
        ]:
            result = self._js_fetch(api_path, "POST", body, headers={
                "x-csrf-token": self._ensure_console_csrf_token(),
            })
            if result.get("ok"):
                aid = self._extract_app_id_from_result(result)
                if aid:
                    log.info("App via known API: %s", aid)
                    return aid

        log.info("No console API available, falling back to DOM")
        return ""

    def _create_app_via_dom(self, name: str, desc: str) -> str:
        """Create app by clicking through the UI via JS-DOM.
        
        Returns the app_id if successfully extracted from URL or page.
        """
        self._screenshot_step("create_app_1_before")

        cr = self._js_click(
            "创建自建应用", "创建企业自建应用", "创建应用",
            "Create Custom App", "Create App",
        )
        if not cr["found"]:
            self._screenshot_step("create_app_ERR_no_btn")
            self._save_page_html("create_app_ERR_no_btn")
            raise RuntimeError(
                "找不到「创建自建应用」按钮。\n"
                "  截图: /tmp/feishu_step_create_app_ERR_no_btn.png\n"
                "  HTML: /tmp/feishu_page_create_app_ERR_no_btn.html\n"
                "  页面按钮: " + str(self._js_page_info().get("buttons", []))
            )

        self.page.wait_for_timeout(2000)
        self._screenshot_step("create_app_2_dialog")

        fr = self._js_fill_in_dialog(name, "应用名称", "名称", "App Name", "name")
        if not fr["found"]:
            self._screenshot_step("create_app_ERR_no_input")
            self._save_page_html("create_app_ERR_no_input")
            raise RuntimeError(
                "找不到应用名称输入框。\n"
                "  截图: /tmp/feishu_step_create_app_ERR_no_input.png\n"
                "  HTML: /tmp/feishu_page_create_app_ERR_no_input.html\n"
                "  页面输入框: " + str(self._js_page_info().get("inputs", []))
            )

        # Description (optional, don't fail)
        self._js_fill_in_dialog(
            desc or f"{name} - HarborNAS 本地 AI 助手",
            "描述", "应用描述", "Description", "description",
        )
        self.page.wait_for_timeout(500)
        self._screenshot_step("create_app_3_filled")

        click_result = self._js_click_in_dialog("确定", "确认创建", "确认", "创建", "Confirm", "Create")
        if not click_result.get("found"):
            self._screenshot_step("create_app_ERR_no_confirm")
            self._save_page_html("create_app_ERR_no_confirm")
            raise RuntimeError(
                "找不到弹窗内「创建/确认」按钮。\n"
                "  截图: /tmp/feishu_step_create_app_ERR_no_confirm.png\n"
                "  HTML: /tmp/feishu_page_create_app_ERR_no_confirm.html"
            )
        self.page.wait_for_timeout(4000)
        self._screenshot_step("create_app_4_done")

        # Try to extract app_id from URL
        match = re.search(r"/app/(cli_[a-zA-Z0-9]+)", self.page.url)
        if match:
            app_id = match.group(1)
            log.info("App created, extracted ID from URL: %s", app_id)
            return app_id
        
        # If not in URL, try to find it in the newly-created app list
        log.info("App created, but ID not in URL. Searching in list…")
        self.page.wait_for_timeout(2000)
        result = self._js_evaluate_find_app_in_list(name)
        if result.get("app_id"):
            app_id = result["app_id"]
            log.info("Found new app in list: %s", app_id)
            return app_id
        
        # Do not fabricate app_id; this causes downstream navigation failures.
        log.warning("Could not extract app_id after app creation")
        return ""
    
    def _js_evaluate_find_app_in_list(self, app_name: str) -> dict:
        """Find app in list by name and return app_id."""
        try:
            result = self.page.evaluate(
                r"""(target_name) => {
                    // 在列表中查找包含应用名称的元素
                    const matches = [];
                    document.querySelectorAll('a, div, span').forEach(el => {
                        const text = (el.innerText || el.textContent || '').trim();
                        if (text.includes(target_name)) {
                            // 尝试从 href 或 data 属性获取 app_id
                            const href = el.href || el.closest('a')?.href || '';
                            const match = href.match(/[?&/]app[?/=]?(cli_[^?&\/ ]+)/);
                            if (match) {
                                matches.push({name: text, app_id: match[1]});
                            }
                        }
                    });
                    return matches.length > 0 ? matches[0] : {app_id: ''};
                }""",
                app_name
            )
            return result or {}
        except Exception as e:
            log.warning("Failed to find app in list: %s", e)
            return {}

    # ================================================================
    # Bot capability
    # ================================================================

    def enable_bot(self) -> None:
        """Enable the bot capability on the current app."""
        self._ensure_not_internal_error_page()
        app_id = self._current_app_id()
        if app_id:
            result = self._console_api_post(
                f"developers/v1/robot/switch/{app_id}",
                {"enable": True},
                require_csrf=True,
            )
            if result.get("ok"):
                log.info("Bot enabled via internal API for %s", app_id)
                return

        self._screenshot_step("enable_bot_0")
        info = self._js_page_info()
        log.info("Page before enable_bot: %s", json.dumps(info, ensure_ascii=False))

        # 尝试多个可能的菜单项来找到应用能力配置
        # 首先尝试点击"添加应用能力"菜单项
        cr = self._js_click(
            "添加应用能力", "应用能力", "Features", "Capabilities",
            timeout_sec=3
        )
        
        if not cr.get("found"):
            # 如果没有找到，尝试在侧边栏或菜单中查找
            log.info("'添加应用能力' not found, trying to find via sidebar menu...")
            # 在某些页面上，功能在左侧菜单或选项卡中
            self.page.wait_for_timeout(1000)
            cr = self._js_click(
                "机器人", "Bot", "机器人能力", "Robot",
                timeout_sec=5
            )
            if cr.get("found"):
                self.page.wait_for_timeout(2000)
                self._screenshot_step("enable_bot_1_done")
                log.info("Bot capability found and clicked directly")
                return
        
        self.page.wait_for_timeout(2000)

        self._js_click("机器人", "Bot", timeout_sec=3)
        self.page.wait_for_timeout(1500)

        self._js_click("开启", "确认开启", "Enable", timeout_sec=3)
        self.page.wait_for_timeout(2000)
        self._screenshot_step("enable_bot_1_done")
        log.info("Bot capability step done")

    # ================================================================
    # Event subscription
    # ================================================================

    def set_callback_url(self, url: str) -> None:
        """Configure event subscription for the current app.

        Feishu's current console no longer exposes a stable callback URL form
        without first configuring subscription mode. Follow the reference
        implementation instead: add required events via internal API and switch
        the app to WebSocket long-connection mode.
        """
        self._ensure_not_internal_error_page()
        app_id = self._current_app_id()
        if app_id:
            event_info = self._console_api_post(
                f"developers/v1/event/{app_id}",
                {},
                require_csrf=True,
            )
            event_data = self._unwrap_result_data(event_info)

            existing_events: set[str] = set()
            for key in ("appEvents", "events"):
                items = event_data.get(key)
                if isinstance(items, list):
                    existing_events.update(item for item in items if isinstance(item, str))

            desired_events = set(_DEFAULT_EVENTS)
            to_add = sorted(desired_events - existing_events)

            if to_add:
                update_result = self._console_api_post(
                    f"developers/v1/event/update/{app_id}",
                    {
                        "operation": "add",
                        "events": [],
                        "appEvents": to_add,
                        "userEvents": [],
                        "eventMode": 4,
                    },
                    require_csrf=True,
                )
                if not update_result.get("ok"):
                    log.warning("Failed to add Feishu events via internal API: %s", update_result)
                else:
                    log.info("Added Feishu events via internal API: %s", to_add)

            switch_result = self._console_api_post(
                f"developers/v1/event/switch/{app_id}",
                {"eventMode": 4},
                require_csrf=True,
            )
            if switch_result.get("ok"):
                if url:
                    log.info(
                        "Configured Feishu event subscription via WebSocket mode for %s; callback URL not required: %s",
                        app_id,
                        url,
                    )
                else:
                    log.info("Configured Feishu event subscription via WebSocket mode for %s", app_id)
                return

        self._screenshot_step("set_callback_0")
        click_result = self._js_click(
            "事件与回调",
            "事件订阅",
            "Event Subscriptions",
            "Events",
            "Callbacks",
            timeout_sec=5,
        )
        if not click_result.get("found"):
            self._save_page_html("set_callback_ERR_no_nav")
            raise RuntimeError("找不到「事件与回调」入口，且内部事件订阅接口调用失败")
        self.page.wait_for_timeout(2000)

        fr = self._js_fill(url, "请求地址", "URL", "Request URL", "callback", "request_url")
        if not fr["found"]:
            self._screenshot_step("set_callback_ERR_no_input")
            self._save_page_html("set_callback_ERR_no_input")
            raise RuntimeError("内部事件订阅接口调用失败，且当前页面找不到事件回调地址输入框")

        self.page.wait_for_timeout(500)
        save_result = self._js_click("保存", "Save", timeout_sec=3)
        if not save_result.get("found"):
            self._screenshot_step("set_callback_ERR_no_save")
            self._save_page_html("set_callback_ERR_no_save")
            raise RuntimeError("找不到保存按钮，无法提交事件回调地址")
        self.page.wait_for_timeout(2000)

        self._screenshot_step("set_callback_1_done")
        log.info("Callback URL set to %s", url)

    # ================================================================
    # Permissions
    # ================================================================

    def grant_permissions(self, scopes: list[str]) -> int:
        """Navigate to permissions page and enable listed scopes."""
        self._ensure_not_internal_error_page()
        app_id = self._current_app_id()
        if app_id:
            all_scopes = self._console_api_post(
                f"developers/v1/scope/all/{app_id}",
                {},
                require_csrf=True,
            )
            all_scope_data = self._unwrap_result_data(all_scopes)
            scope_items = all_scope_data.get("scopes") if isinstance(all_scope_data.get("scopes"), list) else []
            if scope_items:
                scope_map: dict[str, dict[str, Any]] = {}
                for item in scope_items:
                    if not isinstance(item, dict):
                        continue
                    name = item.get("name")
                    scope_id = item.get("id")
                    if not isinstance(name, str) or not scope_id:
                        continue
                    scope_map[name] = {
                        "id": scope_id,
                        "scopeType": self._normalize_scope_types(item),
                    }

                app_scopes_result = self._console_api_post(
                    f"developers/v1/scope/list/{app_id}",
                    {},
                    require_csrf=True,
                )
                app_scope_data = self._unwrap_result_data(app_scopes_result)
                app_scope_items = app_scope_data.get("scopes") if isinstance(app_scope_data.get("scopes"), list) else []

                current_tenant_scopes: set[str] = set()
                current_user_scopes: set[str] = set()
                for item in app_scope_items:
                    if not isinstance(item, dict):
                        continue
                    name = item.get("name")
                    if not isinstance(name, str):
                        continue
                    info = scope_map.get(name)
                    if not info:
                        continue
                    scope_types = info.get("scopeType") or []
                    if 2 in scope_types:
                        current_tenant_scopes.add(name)
                    if 1 in scope_types:
                        current_user_scopes.add(name)

                desired_tenant_scopes: set[str] = set()
                desired_user_scopes: set[str] = set()
                for name in scopes:
                    info = scope_map.get(name)
                    if not info:
                        log.warning("Scope not found in Feishu catalog: %s", name)
                        continue
                    scope_types = info.get("scopeType") or []
                    if 2 in scope_types or not scope_types:
                        desired_tenant_scopes.add(name)
                    if 1 in scope_types:
                        desired_user_scopes.add(name)

                to_add = {
                    "tenant": sorted(desired_tenant_scopes - current_tenant_scopes),
                    "user": sorted(desired_user_scopes - current_user_scopes),
                }

                added_scope_names: set[str] = set()
                for scope_names in (to_add["tenant"], to_add["user"]):
                    if not scope_names:
                        continue
                    scope_ids = [scope_map[name]["id"] for name in scope_names if name in scope_map]
                    if not scope_ids:
                        continue
                    result = self._console_api_post(
                        f"developers/v1/scope/update/{app_id}",
                        {"scopeIds": scope_ids, "operation": "add"},
                        require_csrf=True,
                    )
                    if result.get("ok"):
                        added_scope_names.update(scope_names)

                if added_scope_names:
                    log.info("Granted %d permissions via internal API", len(added_scope_names))
                    return len(added_scope_names)

        self._screenshot_step("grant_perms_0")
        self._js_click("权限管理", "Permissions", timeout_sec=5)
        self.page.wait_for_timeout(2000)

        toggled = 0
        for scope in scopes:
            cr = self._js_click(scope, timeout_sec=2)
            if cr.get("found"):
                ar = self._js_click("开通", "申请", "Enable", "Activate", timeout_sec=2)
                if ar.get("found"):
                    toggled += 1
                    self.page.wait_for_timeout(500)

        self._screenshot_step("grant_perms_1_done")
        log.info("Granted %d/%d permissions", toggled, len(scopes))
        return toggled

    # ================================================================
    # Credential extraction
    # ================================================================

    def extract_credentials(self) -> dict[str, str]:
        """Extract app_id + app_secret from the credentials page."""
        self._ensure_not_internal_error_page()
        app_id = self._current_app_id()
        if app_id:
            result = self._console_api_get(f"developers/v1/secret/{app_id}", require_csrf=True)
            data = self._unwrap_result_data(result)
            app_secret = ""
            if isinstance(data, dict):
                app_secret = str(data.get("secret") or data.get("app_secret") or "")
            if result.get("ok") and app_secret:
                self._screenshot_step("extract_creds_2_result")
                log.info("Extracted credentials via internal API for %s", app_id)
                return {"app_id": app_id, "app_secret": app_secret}

        self._screenshot_step("extract_creds_0")
        self._js_click("凭证与基础信息", "凭证", "Credentials", timeout_sec=5)
        self.page.wait_for_timeout(2000)
        self._screenshot_step("extract_creds_1_page")

        app_id = ""
        app_secret = ""

        page_text = self.page.content()
        id_match = re.search(r'(cli_[a-zA-Z0-9]{16,})', page_text)
        if id_match:
            app_id = id_match.group(1)
            log.info("Found app_id: %s", app_id)

        self._js_click("显示", "Show", "查看", timeout_sec=3)
        self.page.wait_for_timeout(1500)
        page_text = self.page.content()

        secret_match = re.search(
            r'(?:app.?secret|App Secret)[^a-zA-Z0-9]{0,30}([a-zA-Z0-9]{20,})',
            page_text,
        )
        if secret_match:
            app_secret = secret_match.group(1)
            log.info("Found app_secret via regex")

        if not app_id:
            url_match = re.search(r"/app/(cli_[a-zA-Z0-9]+)", self.page.url)
            if url_match:
                app_id = url_match.group(1)

        self._screenshot_step("extract_creds_2_result")
        log.info("Extracted: app_id=%s…", app_id[:8] if app_id else "(empty)")
        return {"app_id": app_id, "app_secret": app_secret}

    # ================================================================
    # Utilities
    # ================================================================

    def screenshot(self, path: str = "/tmp/feishu_setup_debug.png") -> str:
        """Take a debug screenshot."""
        self.page.screenshot(path=path, full_page=True)
        log.info("Screenshot saved to %s", path)
        return path

    def current_url(self) -> str:
        return self.page.url
