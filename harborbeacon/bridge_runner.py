"""Minimal external HarborBeacon bridge runner.

This module turns the new bootstrap assembly into a runnable bridge process for
external IM integrations. It focuses on three responsibilities:

1. Load channel configuration from YAML, admin state, or environment variables.
2. Build the HarborBeacon runtime through ``build_harborbeacon_app(...)``.
3. Start webhook and/or long-connection entry points based on transport mode.

The built-in runner now ships with real outbound senders for Feishu and
Telegram. Unsupported platforms, or delivery failures on supported platforms,
fall back to a logging sender so the bridge can keep running while still
showing the exact payload HarborBeacon attempted to send.
"""
from __future__ import annotations

import argparse
import json
import logging
import os
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any, Callable, Sequence

import yaml

from harborbeacon.bootstrap import HarborBeaconApp, build_harborbeacon_app
from harborbeacon.channels import Channel, ChannelConfig, OutboundMessage, load_channel_configs
from harborbeacon.long_connection import TransportMode, recommended_transport
from harborbeacon.senders import (
    build_channel_senders as _build_channel_senders,
    build_logging_senders as _build_logging_senders,
)
from harborbeacon.webhook import WebhookRequest

logger = logging.getLogger("harborbeacon.bridge_runner")

def _resolve_admin_state_path(path: Path) -> Path:
    if path.exists() or ".harborbeacon" not in str(path):
        return path
    legacy = Path(str(path).replace(".harborbeacon", ".harbornas"))
    if legacy.exists():
        print(
            f"warning: legacy HarborNAS state path {legacy} is deprecated; prefer {path}",
            flush=True,
        )
        return legacy
    return path


DEFAULT_CHANNELS_CONFIG_PATH = Path(
    os.environ.get("HARBORBEACON_CHANNELS_CONFIG", "/etc/harborbeacon/channels.yaml")
)
DEFAULT_ADMIN_STATE_PATH = _resolve_admin_state_path(
    Path(os.environ.get("HARBORBEACON_ADMIN_STATE", ".harborbeacon/admin-console.json"))
)


def load_channel_configs_from_file(path: str | Path) -> list[ChannelConfig]:
    """Load channel configs from a YAML file."""
    config_path = Path(path)
    data = yaml.safe_load(config_path.read_text(encoding="utf-8")) or {}
    if not isinstance(data, dict):
        raise ValueError(f"Channel config file must contain a YAML mapping: {config_path}")
    return load_channel_configs(data)


def load_channel_configs_from_admin_state(path: str | Path) -> list[ChannelConfig]:
    """Load channel configs from the persisted Agent Hub admin state."""
    state_path = Path(path)
    data = json.loads(state_path.read_text(encoding="utf-8")) or {}
    if not isinstance(data, dict):
        raise ValueError(f"Admin state file must contain a JSON object: {state_path}")

    bridge_provider = data.get("bridge_provider") or {}
    if not isinstance(bridge_provider, dict):
        raise ValueError(
            f"Admin state bridge_provider must be a JSON object: {state_path}"
        )

    app_id = str(bridge_provider.get("app_id") or "").strip()
    app_secret = str(bridge_provider.get("app_secret") or "").strip()
    configured = bool(bridge_provider.get("configured"))
    if not (configured and app_id and app_secret):
        return []

    config = ChannelConfig(
        channel=Channel.FEISHU,
        enabled=True,
        app_id=app_id,
        app_secret=app_secret,
        transport_mode=TransportMode.WEBSOCKET.value,
        extra={
            "receive_id_type": "open_id",
            "app_name": str(bridge_provider.get("app_name") or "").strip(),
            "bot_open_id": str(bridge_provider.get("bot_open_id") or "").strip(),
        },
    )
    domain = os.getenv("FEISHU_DOMAIN", "").strip()
    if domain:
        config.extra["domain"] = domain
    return [config]


def load_channel_configs_from_env() -> list[ChannelConfig]:
    """Load a minimal channel config set from environment variables."""
    channels_json = os.getenv("HARBORBEACON_CHANNELS_JSON", "").strip()
    if channels_json:
        data = json.loads(channels_json)
        if not isinstance(data, dict):
            raise ValueError("HARBORBEACON_CHANNELS_JSON must decode to a JSON object")
        return load_channel_configs(data)

    configs: list[ChannelConfig] = []

    feishu_app_id = os.getenv("FEISHU_APP_ID", "").strip()
    feishu_app_secret = os.getenv("FEISHU_APP_SECRET", "").strip()
    if feishu_app_id and feishu_app_secret:
        config = ChannelConfig(
            channel=Channel.FEISHU,
            enabled=True,
            app_id=feishu_app_id,
            app_secret=feishu_app_secret,
            transport_mode=os.getenv("HARBORBEACON_FEISHU_TRANSPORT_MODE", "").strip(),
        )
        domain = os.getenv("FEISHU_DOMAIN", "").strip()
        if domain:
            config.extra["domain"] = domain
        configs.append(config)

    telegram_bot_token = os.getenv("TELEGRAM_BOT_TOKEN", "").strip()
    if telegram_bot_token:
        config = ChannelConfig(
            channel=Channel.TELEGRAM,
            enabled=True,
            bot_token=telegram_bot_token,
            transport_mode=os.getenv("HARBORBEACON_TELEGRAM_TRANSPORT_MODE", "").strip(),
        )
        poll_timeout = os.getenv("HARBORBEACON_TELEGRAM_POLL_TIMEOUT", "").strip()
        if poll_timeout:
            config.extra["poll_timeout"] = int(poll_timeout)
        configs.append(config)

    return configs


def resolve_channel_configs(
    *,
    config_path: str | Path,
    admin_state_path: str | Path,
) -> list[ChannelConfig]:
    """Resolve effective channel configs from env, admin state, and YAML."""
    merged: dict[Channel, ChannelConfig] = {}

    for config in load_channel_configs_from_env():
        merged[config.channel] = config

    admin_path = Path(admin_state_path)
    if admin_path.is_file():
        for config in load_channel_configs_from_admin_state(admin_path):
            merged[config.channel] = config
    else:
        logger.info(
            "Admin state file not found at %s; skipping persisted bridge config",
            admin_path,
        )

    channels_path = Path(config_path)
    if channels_path.is_file():
        for config in load_channel_configs_from_file(channels_path):
            merged[config.channel] = config
    else:
        logger.info(
            "Channel config file not found at %s; using admin state and environment fallbacks",
            channels_path,
        )

    return list(merged.values())


def build_logging_senders(
    channel_configs: Sequence[ChannelConfig],
) -> dict[Channel, Callable[[OutboundMessage], None]]:
    """Return sender callbacks that only log outbound payloads."""
    return _build_logging_senders(channel_configs)


def build_channel_senders(
    channel_configs: Sequence[ChannelConfig],
) -> dict[Channel, Callable[[OutboundMessage], None]]:
    """Return the default sender callbacks for the configured channels."""
    return _build_channel_senders(channel_configs)


def dispatch_webhook_request(
    app: HarborBeaconApp,
    method: str,
    path: str,
    headers: dict[str, str],
    body: bytes,
) -> tuple[int, dict[str, str], bytes]:
    """Dispatch one HTTP request into HarborBeacon's webhook receiver."""
    response = app.webhook_receiver.handle(
        WebhookRequest(
            method=method,
            path=path,
            headers=headers,
            body=body,
        )
    )
    return response.status_code, response.headers, response.to_bytes()


def resolve_runtime_modes(
    channel_configs: Sequence[ChannelConfig],
    mode: str,
) -> tuple[bool, bool]:
    """Resolve whether webhook and/or gateway transports should start."""
    normalized = mode.strip().lower() or "auto"
    if normalized == "webhook":
        return True, False
    if normalized == "gateway":
        return False, True
    if normalized == "both":
        return True, True
    if normalized != "auto":
        raise ValueError(f"Unsupported runtime mode: {mode}")

    enable_webhook = False
    enable_gateway = False
    for config in channel_configs:
        transport = _resolved_transport_mode(config)
        if transport == TransportMode.WEBHOOK:
            enable_webhook = True
        else:
            enable_gateway = True
    return enable_webhook, enable_gateway


def create_webhook_server(
    app: HarborBeaconApp,
    host: str,
    port: int,
) -> ThreadingHTTPServer:
    """Create a stdlib HTTP server that forwards requests to WebhookReceiver."""

    class BridgeWebhookHandler(BaseHTTPRequestHandler):
        server_version = "HarborBeaconBridge/0.1"

        def do_POST(self) -> None:  # noqa: N802
            length = int(self.headers.get("Content-Length", "0") or "0")
            body = self.rfile.read(length) if length > 0 else b""
            headers = {k: v for k, v in self.headers.items()}
            status_code, response_headers, response_body = dispatch_webhook_request(
                self.server.harborbeacon_app,  # type: ignore[attr-defined]
                "POST",
                self.path,
                headers,
                body,
            )
            self.send_response(status_code)
            for key, value in response_headers.items():
                self.send_header(key, value)
            if "Content-Type" not in response_headers:
                self.send_header("Content-Type", "application/json; charset=utf-8")
            self.end_headers()
            self.wfile.write(response_body)

        def log_message(self, fmt: str, *args: object) -> None:
            logger.info("Webhook %s - %s", self.address_string(), fmt % args)

    server = ThreadingHTTPServer((host, port), BridgeWebhookHandler)
    server.harborbeacon_app = app  # type: ignore[attr-defined]
    return server


def run_bridge(
    channel_configs: Sequence[ChannelConfig],
    *,
    task_api_url: str | None = None,
    mode: str = "auto",
    webhook_host: str = "0.0.0.0",
    webhook_port: int = 9000,
) -> HarborBeaconApp:
    """Build and start the external HarborBeacon bridge."""
    configs = list(channel_configs)
    if not configs:
        raise ValueError("No enabled HarborBeacon channel configs were provided")

    app = build_harborbeacon_app(
        configs,
        channel_senders=build_channel_senders(configs),
        task_api_base_url=task_api_url,
    )
    enable_webhook, enable_gateway = resolve_runtime_modes(app.channel_configs, mode)

    server = None
    if enable_webhook:
        server = create_webhook_server(app, webhook_host, webhook_port)
        server.timeout = 1
        logger.info("Starting webhook server on %s:%s", webhook_host, webhook_port)

    if enable_gateway:
        logger.info("Starting long-connection gateway")
        app.gateway.start_all()

    try:
        if server is None:
            while True:
                time.sleep(1)
        else:
            while True:
                server.handle_request()
    except KeyboardInterrupt:
        logger.info("Stopping HarborBeacon bridge")
    finally:
        if server is not None:
            server.server_close()
        if enable_gateway:
            app.gateway.stop_all()

    return app


def main(argv: Sequence[str] | None = None) -> int:
    """CLI entry point for the external HarborBeacon bridge."""
    parser = argparse.ArgumentParser(description="Run an external HarborBeacon IM bridge")
    parser.add_argument(
        "--channels-config",
        default=str(DEFAULT_CHANNELS_CONFIG_PATH),
        help="Path to channels YAML. Overrides admin state and environment when present.",
    )
    parser.add_argument(
        "--admin-state",
        default=str(DEFAULT_ADMIN_STATE_PATH),
        help="Path to Agent Hub admin state JSON used for saved bridge credentials.",
    )
    parser.add_argument(
        "--task-api-url",
        default=os.getenv("HARBOR_TASK_API_URL", "http://127.0.0.1:4175"),
        help="Assistant Task API base URL",
    )
    parser.add_argument(
        "--mode",
        choices=["auto", "webhook", "gateway", "both"],
        default=os.getenv("HARBORBEACON_RUN_MODE", "auto"),
        help="Inbound transport mode selection",
    )
    parser.add_argument(
        "--webhook-host",
        default=os.getenv("HARBORBEACON_WEBHOOK_HOST", "0.0.0.0"),
        help="Webhook bind host",
    )
    parser.add_argument(
        "--webhook-port",
        type=int,
        default=int(os.getenv("HARBORBEACON_WEBHOOK_PORT", "9000")),
        help="Webhook bind port",
    )
    parser.add_argument(
        "--print-config",
        action="store_true",
        help="Print resolved channel config and exit",
    )
    parser.add_argument(
        "--log-level",
        default=os.getenv("HARBORBEACON_LOG_LEVEL", "INFO"),
        help="Python logging level",
    )
    args = parser.parse_args(list(argv) if argv is not None else None)
    args.admin_state = str(_resolve_admin_state_path(Path(args.admin_state)))

    logging.basicConfig(
        level=getattr(logging, args.log_level.upper(), logging.INFO),
        format="%(asctime)s %(levelname)s %(name)s: %(message)s",
    )

    channel_configs = resolve_channel_configs(
        config_path=args.channels_config,
        admin_state_path=args.admin_state,
    )

    if args.print_config:
        printable = [
            {
                "channel": config.channel.value,
                "enabled": config.enabled,
                "transport_mode": config.transport_mode or _resolved_transport_mode(config).value,
                "configured": config.is_configured(),
            }
            for config in channel_configs
        ]
        print(json.dumps(printable, ensure_ascii=False, indent=2))
        return 0

    run_bridge(
        channel_configs,
        task_api_url=args.task_api_url,
        mode=args.mode,
        webhook_host=args.webhook_host,
        webhook_port=args.webhook_port,
    )
    return 0


def _resolved_transport_mode(config: ChannelConfig) -> TransportMode:
    if config.transport_mode:
        return TransportMode(config.transport_mode)
    return recommended_transport(config.channel)
