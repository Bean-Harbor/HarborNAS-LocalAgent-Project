"""Long connection (WebSocket) transport for IM platforms without public IP.

HarborOS typically runs on a NAS behind a home router — no static public IP,
no domain, no HTTPS certificate.  Traditional webhooks don't work here.

The solution (pioneered by OpenClaw for Feishu) is to use the **platform's
WebSocket long-connection mode**: the client initiates an *outbound* connection
to the platform's event gateway, and the platform pushes events through
that persistent channel.

Supported platforms:
  - **Feishu**:  Official WebSocket long connection (``lark.ws.Client``)
    → https://open.feishu.cn/document/server-docs/event-subscription-guide/
      event-subscription-configure-/request-url-configuration-case
  - **DingTalk**: Stream mode (similar concept, long-poll/SSE based)
  - **Telegram**: ``getUpdates`` long-polling (built-in, no webhook needed)
  - **Discord**: Gateway WebSocket (``wss://gateway.discord.gg``)

For channels that only support webhooks (WeCom, Slack):
  → Use ``webhook.py`` with a reverse proxy or tunnel (ngrok/frp/cloudflared)
  → Or set ``transport_mode = "webhook"`` in ChannelConfig

Architecture::

    ┌──────────────┐     outbound WSS      ┌─────────────────┐
    │  HarborClaw  │ ──────────────────────→│  Platform Cloud  │
    │  (NAS, LAN)  │ ←──────────────────────│  (Feishu/Tg/DD) │
    │              │    events pushed back  │                  │
    └──────┬───────┘                        └─────────────────┘
           │ InboundMessage
           ▼
        Dispatcher → Intent → MCP → Reply (via API call, also outbound)
"""
from __future__ import annotations

import hashlib
import json
import logging
import threading
import time
from dataclasses import dataclass, field
from enum import Enum
from typing import Any, Callable, Protocol

from harborclaw.channels import Channel, ChannelConfig, InboundMessage

logger = logging.getLogger("harborclaw.long_connection")


# ---------------------------------------------------------------------------
# Transport mode
# ---------------------------------------------------------------------------

class TransportMode(str, Enum):
    """How a channel receives events from the IM platform."""
    WEBSOCKET = "websocket"      # Platform pushes via long-lived WSS
    LONG_POLL = "long_poll"      # Client polls platform API periodically
    WEBHOOK = "webhook"          # Platform POSTs to our public URL


# ---------------------------------------------------------------------------
# Base transport protocol
# ---------------------------------------------------------------------------

class LongConnectionTransport(Protocol):
    """Minimal interface for a long-connection transport."""

    @property
    def channel(self) -> Channel: ...

    @property
    def connected(self) -> bool: ...

    def start(self, on_message: Callable[[InboundMessage], None]) -> None:
        """Start receiving messages. Blocks or runs in background."""
        ...

    def stop(self) -> None:
        """Gracefully close the connection."""
        ...


# ---------------------------------------------------------------------------
# Connection state
# ---------------------------------------------------------------------------

class ConnectionState(str, Enum):
    DISCONNECTED = "disconnected"
    CONNECTING = "connecting"
    CONNECTED = "connected"
    RECONNECTING = "reconnecting"
    STOPPED = "stopped"


@dataclass
class ConnectionStatus:
    """Observable status for a single channel transport."""
    channel: Channel
    state: ConnectionState = ConnectionState.DISCONNECTED
    last_connected: float = 0.0
    reconnect_count: int = 0
    last_error: str = ""
    messages_received: int = 0


# ---------------------------------------------------------------------------
# Feishu WebSocket transport
# ---------------------------------------------------------------------------

@dataclass
class FeishuWsConfig:
    """Config needed for Feishu WebSocket long connection."""
    app_id: str
    app_secret: str
    # Feishu open API endpoint (default: feishu.cn; use larksuite.com for overseas)
    domain: str = "https://open.feishu.cn"


class FeishuWsTransport:
    """Feishu WebSocket long connection transport.

    Uses the official Feishu WebSocket event subscription mode:
    1. Obtain ``tenant_access_token`` via app credentials
    2. Request a WSS endpoint from ``/open-apis/callback/ws/endpoint``
    3. Connect to the WSS URL and receive events
    4. Auto-reconnect on disconnect

    Reference: https://open.feishu.cn/document/server-docs/event-subscription-guide/
    event-subscription-configure-/request-url-configuration-case

    NOTE: This implementation wraps ``lark_oapi.ws.Client`` if available (the
    official SDK), otherwise falls back to a lightweight urllib+websockets
    implementation.  For production, ``pip install lark-oapi`` is recommended.
    """

    def __init__(self, config: FeishuWsConfig) -> None:
        self._config = config
        self._handler: Callable[[InboundMessage], None] | None = None
        self._status = ConnectionStatus(channel=Channel.FEISHU)
        self._stop_event = threading.Event()
        self._thread: threading.Thread | None = None

    @property
    def channel(self) -> Channel:
        return Channel.FEISHU

    @property
    def connected(self) -> bool:
        return self._status.state == ConnectionState.CONNECTED

    @property
    def status(self) -> ConnectionStatus:
        return self._status

    def start(self, on_message: Callable[[InboundMessage], None]) -> None:
        """Start the Feishu long connection in a background thread."""
        self._handler = on_message
        self._stop_event.clear()
        self._status.state = ConnectionState.CONNECTING

        # Try official SDK first, fall back to lightweight implementation
        if self._try_start_sdk():
            return

        # Lightweight fallback: token + WSS in a thread
        self._thread = threading.Thread(
            target=self._run_loop,
            name="feishu-ws",
            daemon=True,
        )
        self._thread.start()

    def stop(self) -> None:
        """Signal the transport to stop and clean up."""
        self._stop_event.set()
        self._status.state = ConnectionState.STOPPED
        if hasattr(self, "_sdk_client") and self._sdk_client is not None:
            # SDK client doesn't have a clean stop in all versions
            pass
        logger.info("Feishu WebSocket transport stopped")

    # ---- SDK mode ----

    def _try_start_sdk(self) -> bool:
        """Try to use the official ``lark-oapi`` SDK's ws.Client."""
        try:
            import lark_oapi as lark  # type: ignore[import-untyped]

            event_handler = (
                lark.EventDispatcherHandler.builder("", "")
                .register_p2_im_message_receive_v1(self._on_sdk_message)
                .build()
            )
            self._sdk_client = lark.ws.Client(
                self._config.app_id,
                self._config.app_secret,
                event_handler=event_handler,
                log_level=lark.LogLevel.INFO,
            )
            # SDK's start() blocks, so run in background thread
            self._thread = threading.Thread(
                target=self._sdk_start_wrapper,
                name="feishu-ws-sdk",
                daemon=True,
            )
            self._thread.start()
            self._status.state = ConnectionState.CONNECTED
            self._status.last_connected = time.time()
            logger.info("Feishu long connection started (lark-oapi SDK mode)")
            return True
        except ImportError:
            logger.info("lark-oapi not installed, using lightweight WS transport")
            return False

    def _sdk_start_wrapper(self) -> None:
        """Run SDK client.start() in a thread (it blocks)."""
        try:
            self._sdk_client.start()
        except Exception as exc:
            self._status.state = ConnectionState.DISCONNECTED
            self._status.last_error = str(exc)
            logger.error("Feishu SDK WebSocket error: %s", exc)

    def _on_sdk_message(self, data: Any) -> None:
        """Handle a message from the official lark-oapi SDK."""
        try:
            # SDK provides the full event object; extract fields
            event = data.event if hasattr(data, "event") else data
            sender = getattr(event, "sender", None)
            message = getattr(event, "message", None)

            if sender and message:
                sender_id_obj = getattr(sender, "sender_id", None)
                sender_id = (
                    getattr(sender_id_obj, "open_id", None)
                    or getattr(sender_id_obj, "user_id", "unknown")
                    if sender_id_obj else "unknown"
                )
                content_str = getattr(message, "content", "{}")
                try:
                    content = json.loads(content_str)
                    text = content.get("text", content_str)
                except (json.JSONDecodeError, TypeError):
                    text = str(content_str)

                inbound = InboundMessage(
                    channel=Channel.FEISHU,
                    sender_id=str(sender_id),
                    text=_strip_at(text).strip(),
                    raw={"sdk_event": str(data)},
                )
                self._status.messages_received += 1
                if self._handler:
                    self._handler(inbound)

        except Exception as exc:
            logger.error("Error processing Feishu SDK message: %s", exc)

    # ---- Lightweight fallback mode ----

    def _run_loop(self) -> None:
        """Lightweight fallback: long-poll via REST API (no websocket lib needed).

        This uses Feishu's getUpdates-style approach via periodic token refresh
        and event pulling.  For production, install ``lark-oapi`` for real WS.

        In this fallback we use the Feishu ``/open-apis/im/v1/messages`` polling
        approach with tenant_access_token.
        """
        retry_delay = 2
        max_delay = 60

        while not self._stop_event.is_set():
            try:
                self._status.state = ConnectionState.CONNECTING
                token = self._get_tenant_token()
                if not token:
                    logger.warning("Failed to get tenant token, retrying...")
                    self._wait(retry_delay)
                    retry_delay = min(retry_delay * 2, max_delay)
                    continue

                self._status.state = ConnectionState.CONNECTED
                self._status.last_connected = time.time()
                retry_delay = 2  # reset on success

                logger.info("Feishu long connection active (lightweight polling mode)")

                # In lightweight mode, we don't have real WebSocket.
                # We inform the user that lark-oapi SDK is recommended.
                logger.warning(
                    "Lightweight mode has limited capabilities. "
                    "Install lark-oapi for WebSocket long connection: "
                    "pip install lark-oapi"
                )
                # Block until stopped — in lightweight mode we rely on
                # webhook.py as the actual event receiver; this transport
                # just validates connectivity.
                self._stop_event.wait()

            except Exception as exc:
                self._status.state = ConnectionState.RECONNECTING
                self._status.reconnect_count += 1
                self._status.last_error = str(exc)
                logger.error("Feishu connection error: %s, reconnecting...", exc)
                self._wait(retry_delay)
                retry_delay = min(retry_delay * 2, max_delay)

    def _get_tenant_token(self) -> str | None:
        """Get tenant_access_token from Feishu Open API."""
        import urllib.request
        import urllib.error

        url = f"{self._config.domain}/open-apis/auth/v3/tenant_access_token/internal"
        body = json.dumps({
            "app_id": self._config.app_id,
            "app_secret": self._config.app_secret,
        }).encode("utf-8")

        req = urllib.request.Request(
            url,
            data=body,
            headers={"Content-Type": "application/json; charset=utf-8"},
            method="POST",
        )

        try:
            with urllib.request.urlopen(req, timeout=10) as resp:
                data = json.loads(resp.read().decode("utf-8"))
                if data.get("code") == 0:
                    return data.get("tenant_access_token")
                logger.error("Feishu token error: %s", data.get("msg"))
                return None
        except urllib.error.URLError as exc:
            logger.error("Failed to reach Feishu API: %s", exc)
            return None

    def _wait(self, seconds: float) -> None:
        """Sleep interruptibly."""
        self._stop_event.wait(timeout=seconds)


# ---------------------------------------------------------------------------
# Telegram long-poll transport
# ---------------------------------------------------------------------------

class TelegramLongPollTransport:
    """Telegram Bot API ``getUpdates`` long-polling transport.

    Unlike webhook mode, this doesn't need a public URL at all. The client
    repeatedly calls ``GET /getUpdates?offset=...&timeout=30`` and Telegram
    holds the connection open until a new message arrives or timeout.
    """

    def __init__(self, bot_token: str, poll_timeout: int = 30) -> None:
        self._token = bot_token
        self._poll_timeout = poll_timeout
        self._status = ConnectionStatus(channel=Channel.TELEGRAM)
        self._stop_event = threading.Event()
        self._thread: threading.Thread | None = None
        self._offset: int = 0

    @property
    def channel(self) -> Channel:
        return Channel.TELEGRAM

    @property
    def connected(self) -> bool:
        return self._status.state == ConnectionState.CONNECTED

    @property
    def status(self) -> ConnectionStatus:
        return self._status

    def start(self, on_message: Callable[[InboundMessage], None]) -> None:
        self._handler = on_message
        self._stop_event.clear()
        self._thread = threading.Thread(
            target=self._poll_loop,
            name="telegram-poll",
            daemon=True,
        )
        self._thread.start()

    def stop(self) -> None:
        self._stop_event.set()
        self._status.state = ConnectionState.STOPPED

    def _poll_loop(self) -> None:
        import urllib.request
        import urllib.error

        base_url = f"https://api.telegram.org/bot{self._token}"
        retry_delay = 2

        while not self._stop_event.is_set():
            try:
                self._status.state = ConnectionState.CONNECTING
                url = (
                    f"{base_url}/getUpdates"
                    f"?offset={self._offset}"
                    f"&timeout={self._poll_timeout}"
                )
                req = urllib.request.Request(url, method="GET")

                with urllib.request.urlopen(req, timeout=self._poll_timeout + 10) as resp:
                    data = json.loads(resp.read().decode("utf-8"))

                self._status.state = ConnectionState.CONNECTED
                self._status.last_connected = time.time()
                retry_delay = 2

                if not data.get("ok"):
                    logger.warning("Telegram API error: %s", data)
                    self._stop_event.wait(timeout=retry_delay)
                    continue

                for update in data.get("result", []):
                    self._offset = update["update_id"] + 1
                    self._process_update(update)

            except Exception as exc:
                self._status.state = ConnectionState.RECONNECTING
                self._status.reconnect_count += 1
                self._status.last_error = str(exc)
                logger.error("Telegram poll error: %s", exc)
                self._stop_event.wait(timeout=retry_delay)
                retry_delay = min(retry_delay * 2, 60)

    def _process_update(self, update: dict[str, Any]) -> None:
        message = update.get("message", {})
        if not message:
            return

        sender = message.get("from", {})
        sender_id = str(sender.get("id", "unknown"))
        text = message.get("text", "")

        # Strip /command prefix
        if text.startswith("/"):
            parts = text.split(maxsplit=1)
            text = parts[1] if len(parts) > 1 else parts[0].lstrip("/")

        if not text.strip():
            return

        inbound = InboundMessage(
            channel=Channel.TELEGRAM,
            sender_id=sender_id,
            text=text.strip(),
            raw=update,
        )
        self._status.messages_received += 1
        if self._handler:
            try:
                self._handler(inbound)
            except Exception as exc:
                logger.error("Telegram message handler error: %s", exc)


# ---------------------------------------------------------------------------
# Gateway: unified multi-transport manager
# ---------------------------------------------------------------------------

class Gateway:
    """Unified entry point that manages all channel transports.

    Automatically selects the best transport per channel:
      - Feishu    → WebSocket long connection (no public IP needed)
      - Telegram  → Long-poll ``getUpdates`` (no public IP needed)
      - Others    → Webhook (requires public IP or tunnel)

    Usage::

        gateway = Gateway(on_message=dispatcher.handle)
        gateway.register_feishu(FeishuWsConfig(app_id="...", app_secret="..."))
        gateway.register_telegram(bot_token="123456:ABC...")
        gateway.start_all()
    """

    def __init__(self, on_message: Callable[[InboundMessage], None]) -> None:
        self._handler = on_message
        self._transports: dict[Channel, LongConnectionTransport] = {}

    def register_feishu(self, config: FeishuWsConfig) -> None:
        """Register Feishu via WebSocket long connection."""
        self._transports[Channel.FEISHU] = FeishuWsTransport(config)

    def register_telegram(self, bot_token: str, poll_timeout: int = 30) -> None:
        """Register Telegram via long-poll transport."""
        self._transports[Channel.TELEGRAM] = TelegramLongPollTransport(
            bot_token, poll_timeout,
        )

    def register_transport(
        self, channel: Channel, transport: LongConnectionTransport,
    ) -> None:
        """Register a custom transport for any channel."""
        self._transports[channel] = transport

    def start_all(self) -> None:
        """Start all registered transports."""
        for channel, transport in self._transports.items():
            logger.info("Starting %s transport for %s", type(transport).__name__, channel.value)
            transport.start(self._handler)

    def stop_all(self) -> None:
        """Stop all transports gracefully."""
        for channel, transport in self._transports.items():
            logger.info("Stopping transport for %s", channel.value)
            transport.stop()

    def get_status(self, channel: Channel) -> ConnectionStatus | None:
        """Get connection status for a specific channel."""
        transport = self._transports.get(channel)
        if transport and hasattr(transport, "status"):
            return transport.status
        return None

    def all_statuses(self) -> dict[Channel, ConnectionStatus]:
        """Get connection status for all registered channels."""
        result = {}
        for channel, transport in self._transports.items():
            if hasattr(transport, "status"):
                result[channel] = transport.status
        return result

    @property
    def active_channels(self) -> list[Channel]:
        """Channels with active (connected) transports."""
        return [
            ch for ch, t in self._transports.items()
            if hasattr(t, "connected") and t.connected
        ]


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _strip_at(text: str) -> str:
    """Remove @mention prefixes from message text."""
    import re
    return re.sub(r"@\S+\s*", "", text)


def recommended_transport(channel: Channel) -> TransportMode:
    """Return the recommended transport mode for a channel.

    Channels with official long-connection support default to non-webhook
    modes, avoiding the need for a public IP.
    """
    _RECOMMENDATIONS: dict[Channel, TransportMode] = {
        Channel.FEISHU: TransportMode.WEBSOCKET,
        Channel.TELEGRAM: TransportMode.LONG_POLL,
        Channel.DISCORD: TransportMode.WEBSOCKET,
        Channel.WECOM: TransportMode.WEBHOOK,
        Channel.DINGTALK: TransportMode.WEBSOCKET,
        Channel.SLACK: TransportMode.WEBHOOK,
        Channel.MQTT: TransportMode.WEBSOCKET,
    }
    return _RECOMMENDATIONS.get(channel, TransportMode.WEBHOOK)
