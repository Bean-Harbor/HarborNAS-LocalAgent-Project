"""browser.automation handler stub.

In production, this would drive a headless browser (Playwright, Selenium, etc.).
"""
from __future__ import annotations

from typing import Any


def handle(operation: str, url: str | None = None, selector: str | None = None,
           output_path: str | None = None) -> dict[str, Any]:
    handlers = {
        "navigate": _navigate,
        "click": _click,
        "scrape": _scrape,
        "screenshot": _screenshot,
    }
    fn = handlers.get(operation)
    if fn is None:
        return {"error": f"Unknown operation: {operation!r}"}
    return fn(url=url, selector=selector, output_path=output_path)


def _navigate(*, url: str | None, selector: str | None, output_path: str | None) -> dict[str, Any]:
    return {"status": "ok", "operation": "navigate", "url": url}


def _click(*, url: str | None, selector: str | None, output_path: str | None) -> dict[str, Any]:
    return {"status": "ok", "operation": "click", "selector": selector}


def _scrape(*, url: str | None, selector: str | None, output_path: str | None) -> dict[str, Any]:
    return {"status": "ok", "operation": "scrape", "url": url, "selector": selector, "content": ""}


def _screenshot(*, url: str | None, selector: str | None, output_path: str | None) -> dict[str, Any]:
    return {"status": "ok", "operation": "screenshot", "url": url, "output": output_path}
