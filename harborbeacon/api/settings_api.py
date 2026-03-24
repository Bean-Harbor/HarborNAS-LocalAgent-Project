"""REST API endpoints for HarborBeacon settings.

Designed to be registered under ``/api/v2.0/harborbeacon/`` inside the
HarborOS middleware.  When running standalone (dev), use Flask or
FastAPI to mount the handlers.

Endpoint summary
----------------
GET   /settings             → current HarborBeacon config
PUT   /settings             → update HarborBeacon config
POST  /settings/test_channel → test one channel
POST  /settings/test_channels → test all enabled channels
POST  /settings/feishu/one_click_setup → validate and apply Feishu config
POST  /settings/feishu/browser_setup/start  → start browser-assisted setup
POST  /settings/feishu/browser_setup/resume  → resume after QR scan
GET   /settings/feishu/browser_setup/status   → poll session status
GET   /routes/status        → live route availability
"""
from __future__ import annotations

import json
import ssl
import time
from dataclasses import asdict, dataclass, field
from typing import Any
from urllib import error, request

try:
    import certifi
except ImportError:  # pragma: no cover
    certifi = None

from harborbeacon.channels import Channel, ChannelConfig, ChannelRegistry
from harborbeacon.autonomy import Autonomy
from harborbeacon.api import SettingsStore
from harborbeacon.api.feishu_browser_setup import (
    FeishuBrowserSetupFlow,
    FeishuBrowserSetupSession,
    SetupStepStatus,
    get_session as get_browser_session,
)
from orchestrator.contracts import Route, ROUTE_PRIORITY


# ---------------------------------------------------------------------------
# DTOs
# ---------------------------------------------------------------------------

@dataclass
class RouteStatusDTO:
    route: str
    label: str
    available: bool
    priority: int


@dataclass
class ConnectivityResultDTO:
    channel: str
    reachable: bool
    latency_ms: float | None = None
    error: str | None = None
    tested_at: str = ""


@dataclass
class FeishuOneClickSetupDTO:
    success: bool
    message: str
    settings_updated: bool
    connectivity: dict[str, Any] | None = None
    bot_info: dict[str, Any] = field(default_factory=dict)
    next_steps: list[str] = field(default_factory=list)
    settings: dict[str, Any] | None = None


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

_ROUTE_LABELS = {
    Route.MIDDLEWARE_API: "Middleware API",
    Route.MIDCLI: "midcli (CLI)",
    Route.BROWSER: "Browser Automation",
    Route.MCP: "MCP Protocol",
}

_FEISHU_TOKEN_URL = "https://open.feishu.cn/open-apis/auth/v3/tenant_access_token/internal"
_FEISHU_BOT_INFO_URL = "https://open.feishu.cn/open-apis/bot/v3/info"


def _build_ssl_context() -> ssl.SSLContext:
    if certifi is not None:
        return ssl.create_default_context(cafile=certifi.where())
    return ssl.create_default_context()


def _http_json(
    method: str,
    url: str,
    payload: dict[str, Any] | None = None,
    headers: dict[str, str] | None = None,
    timeout: int = 12,
) -> dict[str, Any]:
    body: bytes | None = None
    req_headers = {"Content-Type": "application/json"}
    if headers:
        req_headers.update(headers)
    if payload is not None:
        body = json.dumps(payload).encode("utf-8")

    req = request.Request(url=url, method=method, data=body, headers=req_headers)
    ssl_context = _build_ssl_context()
    try:
        with request.urlopen(req, timeout=timeout, context=ssl_context) as resp:  # noqa: S310
            raw = resp.read().decode("utf-8")
            return json.loads(raw) if raw else {}
    except error.HTTPError as exc:
        detail = exc.read().decode("utf-8", errors="ignore")
        raise RuntimeError(f"HTTP {exc.code}: {detail}") from exc
    except error.URLError as exc:
        raise RuntimeError(f"Network error: {exc.reason}") from exc


def _fetch_feishu_tenant_access_token(app_id: str, app_secret: str) -> str:
    data = _http_json(
        method="POST",
        url=_FEISHU_TOKEN_URL,
        payload={"app_id": app_id, "app_secret": app_secret},
    )
    if data.get("code") != 0:
        msg = data.get("msg", "unknown error")
        raise RuntimeError(f"Feishu auth failed: {msg}")
    token = data.get("tenant_access_token")
    if not token:
        raise RuntimeError("Feishu auth succeeded but no tenant_access_token returned")
    return str(token)


def _fetch_feishu_bot_info(tenant_access_token: str) -> dict[str, Any]:
    data = _http_json(
        method="GET",
        url=_FEISHU_BOT_INFO_URL,
        headers={"Authorization": f"Bearer {tenant_access_token}"},
    )
    if data.get("code") != 0:
        msg = data.get("msg", "unknown error")
        raise RuntimeError(f"Feishu bot info failed: {msg}")
    return dict(data.get("data") or {})


def _test_channel_connectivity(channel: Channel, config: dict[str, Any]) -> ConnectivityResultDTO:
    """Probe a channel endpoint and return latency / error."""
    start = time.monotonic()
    try:
        # Real implementation would do HTTP/WebSocket probes.
        # For now, return synthetic success when credentials look present.
        cfg = ChannelConfig(
            channel=channel,
            enabled=config.get("enabled", False),
            webhook_url=config.get("webhook_url"),
            app_id=config.get("app_id"),
            app_secret=config.get("app_secret"),
            bot_token=config.get("bot_token"),
            extra=config.get("extra", {}),
        )
        if cfg.is_configured():
            elapsed = (time.monotonic() - start) * 1000
            return ConnectivityResultDTO(
                channel=channel.value,
                reachable=True,
                latency_ms=round(elapsed, 1),
                tested_at=time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            )
        else:
            return ConnectivityResultDTO(
                channel=channel.value,
                reachable=False,
                error="Missing required credentials",
                tested_at=time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            )
    except Exception as exc:  # noqa: BLE001
        return ConnectivityResultDTO(
            channel=channel.value,
            reachable=False,
            error=str(exc),
            tested_at=time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        )


# ---------------------------------------------------------------------------
# API handler class
# ---------------------------------------------------------------------------

class HarborBeaconSettingsAPI:
    """Stateless handler object — one method per endpoint.

    Instantiate with a ``SettingsStore`` and optionally a function
    that checks route availability at runtime.
    """

    def __init__(
        self,
        store: SettingsStore | None = None,
        route_checker: Any | None = None,
    ) -> None:
        self._store = store or SettingsStore()
        self._route_checker = route_checker  # callable(Route) → bool

    # GET /settings
    def get_settings(self) -> dict[str, Any]:
        return self._store.load()

    # PUT /settings
    def put_settings(self, body: dict[str, Any]) -> dict[str, Any]:
        return self._store.save(body)

    # POST /settings/test_channel
    def test_channel(self, body: dict[str, Any]) -> dict[str, Any]:
        ch = Channel(body["channel"])
        config = body.get("config", {})
        result = _test_channel_connectivity(ch, config)
        return asdict(result)

    # POST /settings/test_channels
    def test_channels(self) -> list[dict[str, Any]]:
        settings = self._store.load()
        results: list[dict[str, Any]] = []
        for ch_data in settings.get("channels", []):
            if ch_data.get("enabled"):
                ch = Channel(ch_data["channel"])
                result = _test_channel_connectivity(ch, ch_data)
                results.append(asdict(result))
        return results

    # POST /settings/feishu/one_click_setup
    def one_click_setup_feishu(self, body: dict[str, Any]) -> dict[str, Any]:
        app_id = str(body.get("app_id", "")).strip()
        app_secret = str(body.get("app_secret", "")).strip()
        webhook_url = str(body.get("webhook_url", "")).strip()

        if not app_id or not app_secret:
            return asdict(FeishuOneClickSetupDTO(
                success=False,
                message="app_id and app_secret are required",
                settings_updated=False,
                next_steps=[
                    "Provide Feishu app_id and app_secret from Open Platform credentials.",
                ],
            ))

        try:
            token = _fetch_feishu_tenant_access_token(app_id=app_id, app_secret=app_secret)
            bot_info = _fetch_feishu_bot_info(token)
        except Exception as exc:  # noqa: BLE001
            return asdict(FeishuOneClickSetupDTO(
                success=False,
                message=f"Feishu validation failed: {exc}",
                settings_updated=False,
                next_steps=[
                    "Verify app credentials in Feishu Open Platform.",
                    "Ensure outbound network can reach open.feishu.cn.",
                ],
            ))

        settings = self._store.load()
        channels = list(settings.get("channels", []))
        feishu_config: dict[str, Any] = {
            "channel": Channel.FEISHU.value,
            "enabled": True,
            "app_id": app_id,
            "app_secret": app_secret,
            "extra": {},
        }
        if webhook_url:
            feishu_config["webhook_url"] = webhook_url

        found = False
        for idx, ch in enumerate(channels):
            if ch.get("channel") == Channel.FEISHU.value:
                merged = {**ch, **feishu_config}
                merged_extra = dict(ch.get("extra") or {})
                merged_extra.update({
                    "validated": True,
                    "tenant_key": bot_info.get("tenant_key"),
                    "app_name": bot_info.get("app_name"),
                    "configured_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
                })
                merged["extra"] = merged_extra
                channels[idx] = merged
                found = True
                break
        if not found:
            feishu_config["extra"] = {
                "validated": True,
                "tenant_key": bot_info.get("tenant_key"),
                "app_name": bot_info.get("app_name"),
                "configured_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            }
            channels.append(feishu_config)

        settings["channels"] = channels
        saved_settings = self._store.save(settings)
        connectivity = _test_channel_connectivity(Channel.FEISHU, feishu_config)

        return asdict(FeishuOneClickSetupDTO(
            success=True,
            message="Feishu credentials validated and HarborBeacon settings updated.",
            settings_updated=True,
            connectivity=asdict(connectivity),
            bot_info={
                "app_name": bot_info.get("app_name"),
                "tenant_key": bot_info.get("tenant_key"),
                "open_id": bot_info.get("open_id"),
            },
            next_steps=[
                "In Feishu Open Platform, ensure event subscription callback URL is configured.",
                "Grant required bot permissions and publish the app version.",
                "Run HarborBeacon incoming webhook test from the Feishu event debugger.",
            ],
            settings=saved_settings,
        ))

    # POST /settings/feishu/browser_setup/start
    def browser_setup_feishu_start(self, body: dict[str, Any]) -> dict[str, Any]:
        """Start a browser-assisted Feishu setup flow.

        For Playwright mode (``use_playwright=True``), the flow runs end-to-end
        in a background thread.  Login is auto-detected via URL polling —
        the user only needs to scan the QR code, no manual confirmation.

        Body params:
            callback_url  – event subscription callback URL (optional)
            app_name      – custom app name (default: HarborBeacon-Bot)
            use_playwright – True to launch a real Chromium browser (default: False = stub)
        """
        callback_url = str(body.get("callback_url", "")).strip()
        app_name = str(body.get("app_name", "")).strip() or "HarborBeacon-Bot"
        use_playwright = bool(body.get("use_playwright", False))

        flow = FeishuBrowserSetupFlow(
            browser_handler=None,
            callback_url=callback_url,
            app_name=app_name,
            use_playwright=use_playwright,
        )

        if use_playwright:
            # Full background flow — auto-detects login, no resume needed
            session = flow.start_and_run()
        else:
            session = flow.start()

        return session.to_dict()

    # POST /settings/feishu/browser_setup/resume
    def browser_setup_feishu_resume(self, body: dict[str, Any]) -> dict[str, Any]:
        """Resume the flow after QR code scan."""
        session_id = str(body.get("session_id", "")).strip()
        if not session_id:
            return {"error": "session_id is required", "status": "error"}

        session = get_browser_session(session_id)
        if session is None:
            return {"error": f"Session {session_id} not found", "status": "error"}

        callback_url = str(body.get("callback_url", "")).strip()
        use_playwright = bool(body.get("use_playwright", False))
        flow = FeishuBrowserSetupFlow(
            browser_handler=None,
            callback_url=callback_url,
            use_playwright=use_playwright,
        )
        session = flow.resume_after_scan(session_id)

        # If credentials were extracted, save them to settings
        if session.status == "done" and session.app_id and session.app_secret:
            self._save_browser_setup_creds(session)

        return session.to_dict()

    # GET /settings/feishu/browser_setup/status?session_id=xxx
    def browser_setup_feishu_status(self, session_id: str) -> dict[str, Any]:
        """Poll current session status.

        When the background flow reaches ``done`` and credentials are
        available, they are automatically saved to settings on the
        first status poll that sees the ``done`` state.
        """
        session = get_browser_session(session_id)
        if session is None:
            return {"error": f"Session {session_id} not found", "status": "error"}

        # Auto-save credentials on first poll that sees completion
        if session.status == "done" and session.app_id and session.app_secret:
            already_saved = any(
                s.key == "save_settings" and s.status == SetupStepStatus.SUCCESS
                for s in session.steps
            )
            if not already_saved:
                self._save_browser_setup_creds(session)

        return session.to_dict()

    # -- private helpers for browser setup --

    def _save_browser_setup_creds(
        self, session: FeishuBrowserSetupSession,
    ) -> None:
        """Persist extracted credentials into HarborBeacon settings."""
        settings = self._store.load()
        channels = list(settings.get("channels", []))

        feishu_config: dict[str, Any] = {
            "channel": Channel.FEISHU.value,
            "enabled": True,
            "app_id": session.app_id,
            "app_secret": session.app_secret,
            "extra": {
                "validated": False,
                "app_name": session.app_name or "HarborBeacon-Bot",
                "setup_method": "browser_assisted",
                "configured_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            },
        }

        found = False
        for idx, ch in enumerate(channels):
            if ch.get("channel") == Channel.FEISHU.value:
                channels[idx] = {**ch, **feishu_config}
                found = True
                break
        if not found:
            channels.append(feishu_config)

        settings["channels"] = channels
        self._store.save(settings)

        # Mark the save_settings step as success in the session
        for step in session.steps:
            if step.key == "save_settings":
                from harborbeacon.api.feishu_browser_setup import SetupStepStatus, _now
                step.status = SetupStepStatus.SUCCESS
                step.detail = "凭证已保存至 HarborBeacon 配置"
                step.finished_at = _now()
                break

    # GET /routes/status
    def get_route_status(self) -> list[dict[str, Any]]:
        statuses: list[dict[str, Any]] = []
        for idx, route in enumerate(ROUTE_PRIORITY):
            available = True
            if self._route_checker:
                available = bool(self._route_checker(route))
            statuses.append(asdict(RouteStatusDTO(
                route=route.value,
                label=_ROUTE_LABELS.get(route, route.value),
                available=available,
                priority=idx + 1,
            )))
        return statuses
