"""browser.automation handler stub.

In production, this would drive a headless browser (Playwright, Selenium, etc.).
Supports generic operations (navigate, click, scrape, screenshot) and a
high-level ``feishu_setup`` operation tailored for the HarborBeacon
browser-assisted Feishu configuration flow.
"""
from __future__ import annotations

from typing import Any


def handle(operation: str, url: str | None = None, selector: str | None = None,
           output_path: str | None = None, **kwargs: Any) -> dict[str, Any]:
    handlers = {
        "navigate": _navigate,
        "click": _click,
        "scrape": _scrape,
        "screenshot": _screenshot,
        "feishu_setup": _feishu_setup,
    }
    fn = handlers.get(operation)
    if fn is None:
        return {"error": f"Unknown operation: {operation!r}"}
    return fn(url=url, selector=selector, output_path=output_path, **kwargs)


def _navigate(*, url: str | None, **_kw: Any) -> dict[str, Any]:
    return {"status": "ok", "operation": "navigate", "url": url}


def _click(*, selector: str | None, **_kw: Any) -> dict[str, Any]:
    return {"status": "ok", "operation": "click", "selector": selector}


def _scrape(*, url: str | None, selector: str | None, **_kw: Any) -> dict[str, Any]:
    return {"status": "ok", "operation": "scrape", "url": url, "selector": selector, "content": ""}


def _screenshot(*, url: str | None, output_path: str | None, **_kw: Any) -> dict[str, Any]:
    return {"status": "ok", "operation": "screenshot", "url": url, "output": output_path}


def _feishu_setup(*, url: str | None, **_kw: Any) -> dict[str, Any]:
    """High-level operation for Feishu browser-assisted setup.

    In production, this starts a Playwright context, navigates to the
    Feishu Open Platform, waits for QR login, creates an app, enables
    bot capability, and extracts credentials.

    Currently returns a stub response.
    """
    return {
        "status": "ok",
        "operation": "feishu_setup",
        "message": "stub – Playwright integration pending",
        "login_url": url or "https://open.feishu.cn/app",
    }
