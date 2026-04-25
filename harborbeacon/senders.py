"""Outbound sender implementations for HarborBeacon channels."""
from __future__ import annotations

import json
import logging
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid
from dataclasses import dataclass, field
from typing import Any, Callable, Sequence

from harborbeacon.channels import Channel, ChannelConfig, OutboundMessage

logger = logging.getLogger("harborbeacon.senders")

Sender = Callable[[OutboundMessage], None]

DEFAULT_FEISHU_DOMAIN = "https://open.feishu.cn"
DEFAULT_TELEGRAM_API_BASE = "https://api.telegram.org"


class SenderError(RuntimeError):
    """Raised when an outbound message cannot be delivered."""


@dataclass
class FeishuMessageSender:
    """Send HarborBeacon replies through the Feishu Open Platform API."""

    config: ChannelConfig
    timeout_s: float = 15.0
    _token_cache: dict[tuple[str, str], tuple[str, int]] = field(default_factory=dict)

    def __post_init__(self) -> None:
        if self.config.channel != Channel.FEISHU:
            raise ValueError("FeishuMessageSender requires a feishu channel config")
        if not self.config.app_id or not self.config.app_secret:
            raise ValueError("Feishu sender requires app_id and app_secret")
        self._domain = str(
            self.config.extra.get("domain") or DEFAULT_FEISHU_DOMAIN
        ).rstrip("/")

    def __call__(self, msg: OutboundMessage) -> None:
        body = _serialize_feishu_body(msg)

        if msg.update_message_id:
            response = self._send_with_retry(
                self._message_url(msg.update_message_id),
                body,
                method="PATCH",
            )
            msg.payload.setdefault("sent_message_id", msg.update_message_id)
            self._attach_message_id(msg, response, fallback=msg.update_message_id)
            return

        if msg.reply_to_message_id:
            reply_body = dict(body)
            reply_body.setdefault("reply_in_thread", False)
            response = self._send_with_retry(
                self._reply_url(msg.reply_to_message_id),
                reply_body,
                method="POST",
            )
            self._attach_message_id(msg, response)
            return

        receive_id_type = _resolve_feishu_receive_id_type(self.config, msg)
        response = self._send_with_retry(
            f"{self._messages_url()}?receive_id_type={receive_id_type}",
            {
                "receive_id": msg.recipient_id,
                **body,
            },
            method="POST",
        )
        self._attach_message_id(msg, response)

    def _send_with_retry(
        self,
        url: str,
        body: dict[str, Any],
        *,
        method: str,
    ) -> dict[str, Any]:
        token = self._get_tenant_token(force_refresh=False)
        try:
            return self._request_json(url, body, method=method, token=token)
        except SenderError:
            refreshed = self._get_tenant_token(force_refresh=True)
            return self._request_json(url, body, method=method, token=refreshed)

    def _request_json(
        self,
        url: str,
        body: dict[str, Any],
        *,
        method: str,
        token: str,
    ) -> dict[str, Any]:
        request = urllib.request.Request(
            url,
            data=json.dumps(body, ensure_ascii=False).encode("utf-8"),
            headers={
                "Authorization": f"Bearer {token}",
                "Content-Type": "application/json; charset=utf-8",
            },
            method=method,
        )
        payload = _load_json_response(request, timeout_s=self.timeout_s)
        if payload.get("code") != 0:
            raise SenderError(
                f"Feishu API returned code {payload.get('code')}: "
                f"{payload.get('msg', 'unknown error')}"
            )
        return payload

    def _get_tenant_token(self, *, force_refresh: bool) -> str:
        cache_key = (self._domain, self.config.app_id or "")
        now = int(time.time())
        if not force_refresh:
            cached = self._token_cache.get(cache_key)
            if cached and cached[1] > now:
                return cached[0]

        request = urllib.request.Request(
            f"{self._domain}/open-apis/auth/v3/tenant_access_token/internal",
            data=json.dumps(
                {
                    "app_id": self.config.app_id,
                    "app_secret": self.config.app_secret,
                }
            ).encode("utf-8"),
            headers={"Content-Type": "application/json; charset=utf-8"},
            method="POST",
        )
        payload = _load_json_response(request, timeout_s=self.timeout_s)
        if payload.get("code") != 0 or not payload.get("tenant_access_token"):
            raise SenderError(
                "Failed to obtain Feishu tenant token: "
                f"{payload.get('msg', 'unknown error')}"
            )

        expires_in = int(payload.get("expire", payload.get("expires_in", 7200)))
        token = str(payload["tenant_access_token"])
        self._token_cache[cache_key] = (token, now + max(expires_in - 60, 60))
        return token

    def _attach_message_id(
        self,
        msg: OutboundMessage,
        response: dict[str, Any],
        *,
        fallback: str = "",
    ) -> None:
        data = response.get("data")
        message_id = ""
        if isinstance(data, dict):
            message_id = str(
                data.get("message_id")
                or data.get("message", {}).get("message_id", "")
            ).strip()
        if not message_id:
            message_id = fallback
        if message_id:
            msg.payload["sent_message_id"] = message_id

    def _messages_url(self) -> str:
        return f"{self._domain}/open-apis/im/v1/messages"

    def _message_url(self, message_id: str) -> str:
        quoted = urllib.parse.quote(message_id, safe="")
        return f"{self._messages_url()}/{quoted}"

    def _reply_url(self, message_id: str) -> str:
        quoted = urllib.parse.quote(message_id, safe="")
        return f"{self._messages_url()}/{quoted}/reply"


@dataclass
class TelegramMessageSender:
    """Send HarborBeacon replies through the Telegram Bot API."""

    config: ChannelConfig
    timeout_s: float = 15.0

    def __post_init__(self) -> None:
        if self.config.channel != Channel.TELEGRAM:
            raise ValueError("TelegramMessageSender requires a telegram channel config")
        if not self.config.bot_token:
            raise ValueError("Telegram sender requires bot_token")
        self._api_base = str(
            self.config.extra.get("api_base_url") or DEFAULT_TELEGRAM_API_BASE
        ).rstrip("/")

    def __call__(self, msg: OutboundMessage) -> None:
        method_name, body = _serialize_telegram_request(msg)
        request = urllib.request.Request(
            f"{self._api_base}/bot{self.config.bot_token}/{method_name}",
            data=json.dumps(body, ensure_ascii=False).encode("utf-8"),
            headers={"Content-Type": "application/json; charset=utf-8"},
            method="POST",
        )
        payload = _load_json_response(request, timeout_s=self.timeout_s)
        if not payload.get("ok"):
            description = payload.get("description", "unknown error")
            raise SenderError(f"Telegram API returned an error: {description}")

        result = payload.get("result")
        if msg.update_message_id:
            msg.payload.setdefault("sent_message_id", msg.update_message_id)
        if isinstance(result, dict) and result.get("message_id") is not None:
            msg.payload["sent_message_id"] = str(result["message_id"])


def build_channel_sender(
    config: ChannelConfig,
    *,
    fallback_to_logging: bool = True,
) -> Sender:
    """Build the best sender for one channel config."""
    sender = _build_primary_sender(config)
    if not fallback_to_logging:
        return sender

    logging_sender = build_logging_sender(config)

    def wrapped(msg: OutboundMessage) -> None:
        try:
            sender(msg)
        except SenderError as exc:
            logger.warning(
                "Falling back to logging sender for %s after delivery failure: %s",
                config.channel.value,
                exc,
            )
            logging_sender(msg)

    return wrapped


def build_channel_senders(
    channel_configs: Sequence[ChannelConfig],
    *,
    fallback_to_logging: bool = True,
) -> dict[Channel, Sender]:
    """Build sender callbacks for a list of channel configs."""
    return {
        config.channel: build_channel_sender(
            config,
            fallback_to_logging=fallback_to_logging,
        )
        for config in channel_configs
    }


def build_logging_sender(config: ChannelConfig) -> Sender:
    """Build a sender that logs the platform payload instead of sending it."""

    def sender(msg: OutboundMessage) -> None:
        logger.info(
            "Outbound %s reply to %s: %s",
            config.channel.value,
            msg.recipient_id,
            json.dumps(
                render_outbound_request(config, msg),
                ensure_ascii=False,
            ),
        )

    return sender


def build_logging_senders(channel_configs: Sequence[ChannelConfig]) -> dict[Channel, Sender]:
    """Build logging senders for all provided channel configs."""
    return {
        config.channel: build_logging_sender(config)
        for config in channel_configs
    }


def render_outbound_request(config: ChannelConfig, msg: OutboundMessage) -> dict[str, Any]:
    """Render the platform-specific request payload for debugging/logging."""
    if config.channel == Channel.FEISHU:
        body = _serialize_feishu_body(msg)
        if msg.update_message_id:
            return {
                "method": "PATCH",
                "path": f"/open-apis/im/v1/messages/{msg.update_message_id}",
                "body": body,
            }
        if msg.reply_to_message_id:
            return {
                "method": "POST",
                "path": f"/open-apis/im/v1/messages/{msg.reply_to_message_id}/reply",
                "body": {
                    **body,
                    "reply_in_thread": False,
                },
            }
        return {
            "method": "POST",
            "path": "/open-apis/im/v1/messages",
            "query": {
                "receive_id_type": _resolve_feishu_receive_id_type(config, msg),
            },
            "body": {
                "receive_id": msg.recipient_id,
                **body,
            },
        }

    if config.channel == Channel.TELEGRAM:
        method_name, body = _serialize_telegram_request(msg)
        return {
            "method": "POST",
            "path": f"/bot{config.bot_token}/{method_name}",
            "body": body,
        }

    return {
        "method": "LOG_ONLY",
        "body": {
            "channel": config.channel.value,
            "recipient_id": msg.recipient_id,
            "text": msg.text,
            "payload": msg.payload,
        },
    }


def _build_primary_sender(config: ChannelConfig) -> Sender:
    if config.channel == Channel.FEISHU and config.app_id and config.app_secret:
        return FeishuMessageSender(config)
    if config.channel == Channel.TELEGRAM and config.bot_token:
        return TelegramMessageSender(config)
    return build_logging_sender(config)


def _serialize_feishu_body(msg: OutboundMessage) -> dict[str, Any]:
    if "card" in msg.payload:
        content = json.dumps(msg.payload["card"], ensure_ascii=False)
        return {
            "msg_type": "interactive",
            "content": content,
            "uuid": uuid.uuid4().hex,
        }
    return {
        "msg_type": "text",
        "content": json.dumps({"text": msg.text}, ensure_ascii=False),
        "uuid": uuid.uuid4().hex,
    }


def _resolve_feishu_receive_id_type(config: ChannelConfig, msg: OutboundMessage) -> str:
    configured = str(config.extra.get("receive_id_type") or "").strip()
    if configured:
        return configured

    recipient_id = msg.recipient_id.strip()
    if recipient_id.startswith("oc_"):
        return "chat_id"
    if recipient_id.startswith("on_"):
        return "union_id"
    if recipient_id.startswith("ou_"):
        return "open_id"
    return "open_id"


def _serialize_telegram_request(msg: OutboundMessage) -> tuple[str, dict[str, Any]]:
    body: dict[str, Any] = {
        "chat_id": msg.recipient_id,
        "text": msg.text,
        "parse_mode": "Markdown",
    }
    if msg.update_message_id:
        body["message_id"] = _coerce_numeric_id(msg.update_message_id)
        return "editMessageText", body

    if msg.reply_to_message_id:
        body["reply_to_message_id"] = _coerce_numeric_id(msg.reply_to_message_id)
    return "sendMessage", body


def _coerce_numeric_id(value: str) -> int | str:
    try:
        return int(value)
    except (TypeError, ValueError):
        return value


def _load_json_response(request: urllib.request.Request, *, timeout_s: float) -> dict[str, Any]:
    try:
        with urllib.request.urlopen(request, timeout=timeout_s) as response:
            raw = response.read().decode("utf-8")
    except urllib.error.HTTPError as exc:
        raw = exc.read().decode("utf-8", errors="replace")
        if raw:
            try:
                payload = json.loads(raw)
            except json.JSONDecodeError as decode_exc:
                raise SenderError(f"HTTP {exc.code}: {raw}") from decode_exc
            if isinstance(payload, dict):
                return payload
        raise SenderError(f"HTTP {exc.code}: {exc.reason}") from exc
    except urllib.error.URLError as exc:
        raise SenderError(str(exc.reason)) from exc

    try:
        payload = json.loads(raw) if raw else {}
    except json.JSONDecodeError as exc:
        raise SenderError(f"Invalid JSON response: {raw}") from exc
    if not isinstance(payload, dict):
        raise SenderError(f"Expected JSON object response, got: {type(payload).__name__}")
    return payload
