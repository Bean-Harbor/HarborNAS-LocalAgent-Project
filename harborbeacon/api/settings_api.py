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
GET   /routes/status        → live route availability
"""
from __future__ import annotations

import time
from dataclasses import asdict, dataclass, field
from typing import Any

from harborbeacon.channels import Channel, ChannelConfig, ChannelRegistry
from harborbeacon.autonomy import Autonomy
from harborbeacon.api import SettingsStore
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


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

_ROUTE_LABELS = {
    Route.MIDDLEWARE_API: "Middleware API",
    Route.MIDCLI: "midcli (CLI)",
    Route.BROWSER: "Browser Automation",
    Route.MCP: "MCP Protocol",
}


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
