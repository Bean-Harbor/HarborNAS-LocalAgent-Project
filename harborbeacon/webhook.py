"""Webhook receiver: HTTP endpoint for IM platform callbacks.

Provides a lightweight ASGI/WSGI-agnostic ``WebhookReceiver`` that:
  1. Accepts incoming POST requests from IM platforms
  2. Verifies webhook signatures
  3. Handles platform-specific challenge/verification handshakes
  4. Forwards valid messages to the Dispatcher

Designed to be mounted inside HarborOS middleware (which already runs
a Python HTTP server) or used standalone with any ASGI framework.

This module does NOT depend on any HTTP framework — it operates on
plain dicts and bytes, making it testable and embeddable.
"""
from __future__ import annotations

import json
import logging
from dataclasses import dataclass, field
from typing import Any, Callable

from harborbeacon.adapters import ChannelAdapter, get_adapter
from harborbeacon.channels import Channel, ChannelConfig, InboundMessage

logger = logging.getLogger("harborbeacon.webhook")


# ---------------------------------------------------------------------------
# Request / Response types (framework-agnostic)
# ---------------------------------------------------------------------------

@dataclass
class WebhookRequest:
    """Normalized HTTP request from any framework."""
    method: str
    path: str
    headers: dict[str, str]
    body: bytes
    query_params: dict[str, str] = field(default_factory=dict)


@dataclass
class WebhookResponse:
    """HTTP response to send back to the IM platform."""
    status_code: int = 200
    body: str = ""
    headers: dict[str, str] = field(default_factory=dict)
    json_body: dict[str, Any] | None = None

    def to_bytes(self) -> bytes:
        if self.json_body is not None:
            return json.dumps(self.json_body, ensure_ascii=False).encode("utf-8")
        return self.body.encode("utf-8")


# Type for the async dispatcher callback
MessageHandler = Callable[[InboundMessage], None]


# ---------------------------------------------------------------------------
# Challenge handlers (platform verification)
# ---------------------------------------------------------------------------

def _handle_feishu_challenge(data: dict[str, Any]) -> WebhookResponse | None:
    """Feishu URL verification: respond with the challenge token."""
    challenge = data.get("challenge")
    if challenge and data.get("type") == "url_verification":
        return WebhookResponse(
            status_code=200,
            json_body={"challenge": challenge},
            headers={"Content-Type": "application/json"},
        )
    return None


def _handle_slack_challenge(data: dict[str, Any]) -> WebhookResponse | None:
    """Slack URL verification."""
    if data.get("type") == "url_verification":
        return WebhookResponse(
            status_code=200,
            json_body={"challenge": data.get("challenge", "")},
            headers={"Content-Type": "application/json"},
        )
    return None


_CHALLENGE_HANDLERS: dict[Channel, Callable[[dict[str, Any]], WebhookResponse | None]] = {
    Channel.FEISHU: _handle_feishu_challenge,
    Channel.SLACK: _handle_slack_challenge,
}


# ---------------------------------------------------------------------------
# Webhook Receiver
# ---------------------------------------------------------------------------

class WebhookReceiver:
    """Framework-agnostic webhook endpoint for IM callbacks.

    Usage::

        receiver = WebhookReceiver()
        receiver.register_channel(Channel.FEISHU, config, on_message=dispatcher.handle)

        # Inside your HTTP handler:
        req = WebhookRequest(method="POST", path="/webhook/feishu", headers=..., body=...)
        resp = receiver.handle(req)
    """

    def __init__(self) -> None:
        self._adapters: dict[Channel, ChannelAdapter] = {}
        self._configs: dict[Channel, ChannelConfig] = {}
        self._handlers: dict[Channel, MessageHandler] = {}
        self._default_handler: MessageHandler | None = None

    def register_channel(
        self,
        channel: Channel,
        config: ChannelConfig,
        *,
        on_message: MessageHandler | None = None,
    ) -> None:
        """Register a channel with its config and message handler."""
        self._adapters[channel] = get_adapter(channel)
        self._configs[channel] = config
        if on_message:
            self._handlers[channel] = on_message

    def set_default_handler(self, handler: MessageHandler) -> None:
        """Set a fallback handler for all channels without a specific handler."""
        self._default_handler = handler

    def handle(self, request: WebhookRequest) -> WebhookResponse:
        """Process an incoming webhook request.

        Route: ``POST /webhook/<channel_name>``
        """
        if request.method.upper() != "POST":
            return WebhookResponse(status_code=405, body="Method Not Allowed")

        # Extract channel from path
        channel = self._resolve_channel(request.path)
        if channel is None:
            return WebhookResponse(status_code=404, body="Unknown channel")

        adapter = self._adapters.get(channel)
        config = self._configs.get(channel)
        if adapter is None or config is None:
            return WebhookResponse(status_code=404, body="Channel not configured")

        if not config.enabled:
            return WebhookResponse(status_code=403, body="Channel disabled")

        # Parse body
        try:
            data = json.loads(request.body) if request.body else {}
        except json.JSONDecodeError:
            return WebhookResponse(status_code=400, body="Invalid JSON")

        # Challenge/verification handshake
        challenge_handler = _CHALLENGE_HANDLERS.get(channel)
        if challenge_handler:
            challenge_resp = challenge_handler(data)
            if challenge_resp is not None:
                return challenge_resp

        # Verify signature (only if the adapter provides real verification
        # and the request includes signature headers)
        secret = config.app_secret or config.bot_token or ""
        if secret:
            # Only verify if the request actually contains signature headers
            has_sig_headers = any(
                k.lower() in (
                    "x-lark-signature", "x-lark-request-timestamp",
                    "msg_signature", "x-slack-signature",
                    "sign", "x-hub-signature-256",
                )
                for k in request.headers
            )
            if has_sig_headers and not adapter.verify_signature(request.headers, request.body, secret):
                logger.warning("Signature verification failed for %s", channel.value)
                return WebhookResponse(status_code=401, body="Signature verification failed")

        # Parse inbound message
        try:
            inbound = adapter.parse_inbound(data)
        except Exception as exc:
            logger.error("Failed to parse inbound for %s: %s", channel.value, exc)
            return WebhookResponse(status_code=400, body="Failed to parse message")

        if not inbound.text:
            return WebhookResponse(status_code=200, body="ok")

        # Dispatch to handler
        handler = self._handlers.get(channel) or self._default_handler
        if handler:
            try:
                handler(inbound)
            except Exception as exc:
                logger.error("Handler error for %s: %s", channel.value, exc)
                return WebhookResponse(status_code=500, body="Internal error")

        return WebhookResponse(status_code=200, body="ok")

    def _resolve_channel(self, path: str) -> Channel | None:
        """Extract Channel from URL path like /webhook/feishu."""
        # Try last path segment
        segment = path.rstrip("/").rsplit("/", 1)[-1].lower()
        try:
            return Channel(segment)
        except ValueError:
            return None

    @property
    def registered_channels(self) -> list[Channel]:
        return list(self._adapters.keys())
