"""Browser-assisted Feishu setup flow.

Orchestrates a multi-step process that uses browser automation
to help users configure a Feishu Open Platform app *without* needing to
manually provide ``app_id`` / ``app_secret``.

Flow (fully automatic after QR scan)
-------------------------------------
1. Open Feishu Open Platform login page → user scans QR code.
2. **Auto-detect login** — poll until the URL leaves the passport domain.
3. Create a new custom app for HarborBeacon.
4. Enable bot capability.
5. Configure event-subscription callback URL.
6. Grant required permissions (``im:message``, ``im:message.group_at_msg``).
7. Extract ``app_id`` and ``app_secret`` from the credentials page.
8. Save credentials into HarborBeacon settings.

The ``start_and_run()`` entry point launches a daemon thread that runs
the *entire* flow.  Login detection happens automatically via Playwright
polling — the user only needs to scan the QR code; no manual confirm
button or Enter key is required.

In **stub / dev** mode the handler returns simulated step results so the
frontend wizard can be exercised without a real browser.
"""
from __future__ import annotations

import logging
import threading
import time
import uuid
from dataclasses import asdict, dataclass, field
from enum import Enum
from typing import Any

log = logging.getLogger("harborbeacon.feishu_browser_setup")


# ---------------------------------------------------------------------------
# Data contracts
# ---------------------------------------------------------------------------

class SetupStepStatus(str, Enum):
    PENDING = "pending"
    RUNNING = "running"
    WAIT_USER = "wait_user"       # paused – needs user action (e.g. QR scan)
    SUCCESS = "success"
    FAILED = "failed"
    SKIPPED = "skipped"


@dataclass
class SetupStep:
    key: str
    label: str
    label_zh: str
    status: SetupStepStatus = SetupStepStatus.PENDING
    detail: str = ""
    started_at: str = ""
    finished_at: str = ""


@dataclass
class FeishuBrowserSetupSession:
    """Represents a single browser-assisted setup attempt."""
    session_id: str = field(default_factory=lambda: uuid.uuid4().hex[:12])
    status: str = "created"          # created | running | wait_user | done | error
    current_step: str = ""
    steps: list[SetupStep] = field(default_factory=list)
    app_id: str = ""
    app_secret: str = ""
    app_name: str = ""
    error: str = ""
    created_at: str = field(default_factory=lambda: _now())
    updated_at: str = field(default_factory=lambda: _now())

    def to_dict(self) -> dict[str, Any]:
        d = asdict(self)
        d["steps"] = [asdict(s) for s in self.steps]
        return d


# All steps in order
STEP_DEFINITIONS: list[tuple[str, str, str]] = [
    ("open_login",       "Open Feishu login page",        "打开飞书登录页"),
    ("wait_qr_scan",     "Wait for QR code scan",         "等待扫码登录"),
    ("create_app",       "Create custom app",             "创建自建应用"),
    ("enable_bot",       "Enable bot capability",         "启用机器人能力"),
    ("set_callback_url", "Set event callback URL",        "配置事件回调地址"),
    ("grant_permissions","Grant required permissions",     "授予所需权限"),
    ("extract_creds",    "Extract app credentials",       "提取应用凭证"),
    ("save_settings",    "Save to HarborBeacon settings", "保存至 HarborBeacon 配置"),
]


def _now() -> str:
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())


def _build_steps() -> list[SetupStep]:
    return [
        SetupStep(key=key, label=label, label_zh=label_zh)
        for key, label, label_zh in STEP_DEFINITIONS
    ]


# ---------------------------------------------------------------------------
# In-memory session store (single-node; production would use Redis / DB)
# ---------------------------------------------------------------------------

_sessions: dict[str, FeishuBrowserSetupSession] = {}

# Holds live Playwright drivers keyed by session_id so the same browser
# instance survives across the start → resume API boundary.
_drivers: dict[str, Any] = {}


def get_session(session_id: str) -> FeishuBrowserSetupSession | None:
    return _sessions.get(session_id)


def get_driver(session_id: str) -> Any | None:
    return _drivers.get(session_id)


def set_driver(session_id: str, driver: Any) -> None:
    _drivers[session_id] = driver


def remove_driver(session_id: str) -> None:
    _drivers.pop(session_id, None)


def list_sessions() -> list[FeishuBrowserSetupSession]:
    return list(_sessions.values())


def _save_session(session: FeishuBrowserSetupSession) -> None:
    session.updated_at = _now()
    _sessions[session.session_id] = session


# ---------------------------------------------------------------------------
# Flow orchestrator
# ---------------------------------------------------------------------------

class FeishuBrowserSetupFlow:
    """Drive the multi-step browser-assisted Feishu configuration.

    Two modes
    ---------
    * **stub** (``use_playwright=False``) – simulates results for dev/test.
    * **playwright** (``use_playwright=True``) – launches a real Chromium
      session via ``PlaywrightFeishuDriver``.

    The ``browser_handler`` parameter is kept for backward-compat with the
    generic ``skills.builtins.browser.automation.handler.handle()`` interface.
    When *use_playwright* is True, it is ignored and the dedicated driver
    is used instead.
    """

    FEISHU_LOGIN_URL = "https://open.feishu.cn/app"
    FEISHU_CREATE_APP_URL = "https://open.feishu.cn/app?lang=zh-CN"

    REQUIRED_PERMISSIONS = [
        "im:message",
        "im:message.group_at_msg",
        "im:message:send_as_bot",
    ]

    def __init__(
        self,
        browser_handler: Any | None = None,
        callback_url: str = "",
        app_name: str = "HarborBeacon-Bot",
        use_playwright: bool = False,
        headless: bool = False,
    ) -> None:
        self._browser = browser_handler
        self._callback_url = callback_url
        self._app_name = app_name
        self._use_playwright = use_playwright
        self._headless = headless
        self._driver: Any | None = None  # PlaywrightFeishuDriver instance

    # -- public API ---------------------------------------------------------

    def start(self) -> FeishuBrowserSetupSession:
        """Create a new session and execute up to the QR-scan wait point.

        For non-Playwright flows this pauses at ``wait_user``.
        For Playwright flows, prefer ``start_and_run()`` which auto-detects
        login and continues automatically.
        """
        session = FeishuBrowserSetupSession(steps=_build_steps())
        _save_session(session)

        # Step 1: open login page
        self._run_step(session, "open_login", self._step_open_login)

        if not self._use_playwright:
            # Non-Playwright: pause for manual resume
            self._mark_step(session, "wait_qr_scan", SetupStepStatus.WAIT_USER,
                            detail="请使用飞书 App 扫描浏览器中的二维码完成登录。")
            session.status = "wait_user"
            session.current_step = "wait_qr_scan"
            _save_session(session)
            return session

        # Playwright: mark QR scan as "running" — auto-detection will handle it
        self._mark_step(session, "wait_qr_scan", SetupStepStatus.RUNNING,
                        detail="等待飞书 App 扫码（自动检测中…）")
        session.status = "running"
        session.current_step = "wait_qr_scan"
        _save_session(session)
        return session

    def start_and_run(self) -> FeishuBrowserSetupSession:
        """Launch the full flow in a **background daemon thread**.

        1. Opens login page (browser window appears with QR code).
        2. Automatically polls for login — **no user confirmation needed**.
        3. Once login detected, runs all remaining steps.
        4. Updates session status throughout.

        The caller should poll ``get_session(session_id)`` to track progress.
        """
        session = self.start()

        if not self._use_playwright:
            # Non-Playwright: can't auto-detect login, stay at wait_user
            return session

        # Launch background thread for the remaining flow
        def _background_run() -> None:
            try:
                self._auto_login_and_continue(session)
            except Exception as exc:
                log.error("Background flow error: %s", exc, exc_info=True)
                session.status = "error"
                session.error = str(exc)
                _save_session(session)
                self._cleanup_driver(session.session_id)

        t = threading.Thread(target=_background_run, daemon=True, name=f"feishu-setup-{session.session_id}")
        t.start()
        return session

    def run_blocking(self) -> FeishuBrowserSetupSession:
        """Run the entire flow **synchronously** (blocks until done).

        Useful for scripts and tests.
        """
        session = self.start()
        if self._use_playwright:
            self._auto_login_and_continue(session)
        return session

    def _auto_login_and_continue(self, session: FeishuBrowserSetupSession) -> None:
        """Wait for login polling, then run all remaining steps."""
        # Step 2: auto-detect login via Playwright polling
        if self._driver:
            try:
                self._mark_step(session, "wait_qr_scan", SetupStepStatus.RUNNING,
                                detail="等待飞书扫码登录（自动检测中…）")
                _save_session(session)

                self._driver.wait_for_login(timeout_sec=180)

                self._mark_step(session, "wait_qr_scan", SetupStepStatus.SUCCESS,
                                detail="登录成功")
                _save_session(session)
            except TimeoutError as exc:
                self._mark_step(session, "wait_qr_scan", SetupStepStatus.FAILED,
                                detail=str(exc))
                session.status = "error"
                session.error = str(exc)
                _save_session(session)
                self._cleanup_driver(session.session_id)
                return

        # Run remaining automated steps
        self._run_step(session, "create_app",       self._step_create_app)
        self._run_step(session, "enable_bot",        self._step_enable_bot)
        self._run_step(session, "set_callback_url",  self._step_set_callback_url)
        self._run_step(session, "grant_permissions", self._step_grant_permissions)
        self._run_step(session, "extract_creds",     self._step_extract_creds)

        if session.status != "error":
            session.status = "done"

        self._cleanup_driver(session.session_id)
        _save_session(session)

    def resume_after_scan(self, session_id: str) -> FeishuBrowserSetupSession:
        """Resume flow after the user has scanned the QR code.

        This is a **fallback** for non-Playwright modes.  When using
        Playwright, prefer ``start_and_run()`` which auto-detects login.
        """
        session = get_session(session_id)
        if session is None:
            raise ValueError(f"Session {session_id} not found")

        # Re-attach the Playwright driver that was persisted during start()
        if self._use_playwright:
            self._attach_driver(session_id)

        # If using Playwright, verify login first
        if self._use_playwright and self._driver:
            try:
                self._driver.wait_for_login(timeout_sec=10)
            except TimeoutError:
                pass  # user already clicked "resume" so trust them

        # Mark QR step done
        self._mark_step(session, "wait_qr_scan", SetupStepStatus.SUCCESS,
                        detail="登录成功")
        session.status = "running"

        # Remaining automated steps
        self._run_step(session, "create_app",       self._step_create_app)
        self._run_step(session, "enable_bot",        self._step_enable_bot)
        self._run_step(session, "set_callback_url",  self._step_set_callback_url)
        self._run_step(session, "grant_permissions", self._step_grant_permissions)
        self._run_step(session, "extract_creds",     self._step_extract_creds)

        # Step 8 handled by caller (save_settings) so we mark it pending
        if session.status != "error":
            session.status = "done"

        # Clean up browser
        self._cleanup_driver(session_id)

        _save_session(session)
        return session

    # -- step implementations -----------------------------------------------

    def _step_open_login(self, session: FeishuBrowserSetupSession) -> str:
        if self._use_playwright:
            return self._playwright_open_login(session)
        if self._browser:
            result = self._browser.handle(
                "navigate", url=self.FEISHU_LOGIN_URL,
            )
            if result.get("error"):
                raise RuntimeError(result["error"])
            return f"已打开 {self.FEISHU_LOGIN_URL}"
        # stub
        return f"[stub] 已打开 {self.FEISHU_LOGIN_URL}"

    def _step_create_app(self, session: FeishuBrowserSetupSession) -> str:
        if self._use_playwright and self._driver:
            app_id = self._driver.create_app(
                name=self._app_name,
                desc=f"{self._app_name} - HarborNAS 本地 AI 助手",
            )
            session.app_name = self._app_name
            if app_id:
                session.app_id = app_id
                return f"应用 '{self._app_name}' 已创建 (app_id={app_id})"
            return f"应用 '{self._app_name}' 创建请求已提交，等待页面跳转"
        if self._browser:
            self._browser.handle("navigate", url=self.FEISHU_CREATE_APP_URL)
            self._browser.handle("click", selector='button:has-text("创建自建应用")')
            self._browser.handle("click", selector='input[name="app_name"]')
            return f"应用 '{self._app_name}' 创建请求已提交"
        session.app_name = self._app_name
        return f"[stub] 应用 '{self._app_name}' 已创建"

    def _step_enable_bot(self, session: FeishuBrowserSetupSession) -> str:
        if self._use_playwright and self._driver:
            self._driver.enable_bot()
            return "机器人能力已启用"
        if self._browser:
            self._browser.handle("click", selector='[data-capability="bot"]')
            return "机器人能力已启用"
        return "[stub] 机器人能力已启用"

    def _step_set_callback_url(self, session: FeishuBrowserSetupSession) -> str:
        url = self._callback_url or "https://<your-nas-ip>:8443/api/v2.0/harborbeacon/webhook/feishu"
        if self._use_playwright and self._driver:
            self._driver.set_callback_url(url)
            return f"事件回调已设为 {url}"
        if self._browser:
            self._browser.handle("click", selector='input[name="callback_url"]')
            return f"事件回调已设为 {url}"
        return f"[stub] 事件回调已设为 {url}"

    def _step_grant_permissions(self, session: FeishuBrowserSetupSession) -> str:
        permissions = self.REQUIRED_PERMISSIONS
        if self._use_playwright and self._driver:
            toggled = self._driver.grant_permissions(permissions)
            return f"已授予 {toggled}/{len(permissions)} 项权限"
        if self._browser:
            for perm in permissions:
                self._browser.handle("click", selector=f'[data-permission="{perm}"]')
            return f"已授予 {len(permissions)} 项权限"
        return f"[stub] 已授予 {len(permissions)} 项权限: {', '.join(permissions)}"

    def _step_extract_creds(self, session: FeishuBrowserSetupSession) -> str:
        if self._use_playwright and self._driver:
            creds = self._driver.extract_credentials()
            session.app_id = creds.get("app_id", "")
            session.app_secret = creds.get("app_secret", "")
            if not session.app_id or not session.app_secret:
                raise RuntimeError("无法提取凭证，请检查页面是否已加载完毕")
            return f"已获取 app_id={session.app_id[:8]}…"
        if self._browser:
            result = self._browser.handle("scrape", selector='[data-field="app_id"]')
            session.app_id = str(result.get("content", ""))
            result = self._browser.handle("scrape", selector='[data-field="app_secret"]')
            session.app_secret = str(result.get("content", ""))
            if not session.app_id or not session.app_secret:
                raise RuntimeError("无法提取凭证，请检查页面是否已加载完毕")
            return f"已获取 app_id={session.app_id[:8]}…"
        # Stub: generate fake credentials for dev
        session.app_id = f"cli_stub_{session.session_id[:6]}"
        session.app_secret = f"secret_stub_{session.session_id[:6]}"
        return f"[stub] app_id={session.app_id}"

    # -- Playwright helpers -------------------------------------------------

    def _playwright_open_login(self, session: FeishuBrowserSetupSession) -> str:
        from harborbeacon.api.feishu_playwright import PlaywrightFeishuDriver
        self._driver = PlaywrightFeishuDriver(
            headless=self._headless,
            timeout_ms=60_000,
        )
        self._driver.launch()
        url = self._driver.open_login()
        # Persist driver in the module-level registry so resume can find it
        set_driver(session.session_id, self._driver)
        return f"已打开 {url}（请在弹出的浏览器窗口中扫码登录）"

    def _attach_driver(self, session_id: str) -> None:
        """Re-attach a persisted Playwright driver for this session."""
        if self._driver is None:
            stored = get_driver(session_id)
            if stored is not None:
                self._driver = stored

    def _cleanup_driver(self, session_id: str = "") -> None:
        if self._driver:
            try:
                self._driver.close()
            except Exception:  # noqa: BLE001
                log.warning("Failed to close Playwright driver", exc_info=True)
            self._driver = None
        if session_id:
            remove_driver(session_id)

    # -- helpers ------------------------------------------------------------

    def _run_step(
        self,
        session: FeishuBrowserSetupSession,
        key: str,
        fn: Any,
    ) -> None:
        if session.status == "error":
            self._mark_step(session, key, SetupStepStatus.SKIPPED)
            return

        self._mark_step(session, key, SetupStepStatus.RUNNING)
        session.current_step = key
        _save_session(session)

        try:
            detail = fn(session)
            self._mark_step(session, key, SetupStepStatus.SUCCESS, detail=detail)
        except Exception as exc:  # noqa: BLE001
            self._mark_step(session, key, SetupStepStatus.FAILED, detail=str(exc))
            session.status = "error"
            session.error = str(exc)
        _save_session(session)

    def _mark_step(
        self,
        session: FeishuBrowserSetupSession,
        key: str,
        status: SetupStepStatus,
        detail: str = "",
    ) -> None:
        for step in session.steps:
            if step.key == key:
                step.status = status
                if detail:
                    step.detail = detail
                ts = _now()
                if status == SetupStepStatus.RUNNING:
                    step.started_at = ts
                elif status in (
                    SetupStepStatus.SUCCESS,
                    SetupStepStatus.FAILED,
                    SetupStepStatus.SKIPPED,
                ):
                    step.finished_at = ts
                break
