"""IM channel adapters: platform-specific message parsing and sending.

Each adapter converts between the platform's native JSON format and the
``InboundMessage`` / ``OutboundMessage`` contracts defined in channels.py.

Supported platform adapters:
  - Feishu  (飞书 / Lark)
  - WeCom   (企业微信)
  - Telegram
  - Discord
  - DingTalk (钉钉)
  - Slack
  - MQTT

Each adapter provides:
  1. ``parse_inbound(raw: dict) -> InboundMessage``  — decode platform webhook
  2. ``build_outbound(msg: OutboundMessage) -> dict`` — encode reply payload
  3. ``verify_signature(headers, body, secret) -> bool`` — webhook signature check
"""
from __future__ import annotations

import hashlib
import hmac
import json
import time
from abc import ABC, abstractmethod
from dataclasses import dataclass
from typing import Any

from harborclaw.channels import Channel, InboundMessage, OutboundMessage


# ---------------------------------------------------------------------------
# Base adapter
# ---------------------------------------------------------------------------

class ChannelAdapter(ABC):
    """Abstract base for all IM adapters."""

    @property
    @abstractmethod
    def channel(self) -> Channel: ...

    @abstractmethod
    def parse_inbound(self, raw: dict[str, Any]) -> InboundMessage:
        """Decode platform-specific webhook payload → InboundMessage."""

    @abstractmethod
    def build_outbound(self, msg: OutboundMessage) -> dict[str, Any]:
        """Encode OutboundMessage → platform-specific reply payload."""

    def verify_signature(
        self, headers: dict[str, str], body: bytes, secret: str,
    ) -> bool:
        """Verify webhook signature. Default: always True (override per platform)."""
        return True


# ---------------------------------------------------------------------------
# Feishu adapter
# ---------------------------------------------------------------------------

class FeishuAdapter(ChannelAdapter):
    """Adapter for 飞书 (Feishu / Lark) events API v2."""

    @property
    def channel(self) -> Channel:
        return Channel.FEISHU

    def parse_inbound(self, raw: dict[str, Any]) -> InboundMessage:
        # Feishu event structure:
        # {"header": {...}, "event": {"sender": {"sender_id": {...}}, "message": {"content": ...}}}
        event = raw.get("event", {})
        sender = event.get("sender", {}).get("sender_id", {})
        sender_id = sender.get("open_id", sender.get("user_id", "unknown"))

        message = event.get("message", {})
        content_str = message.get("content", "{}")
        try:
            content = json.loads(content_str)
            text = content.get("text", content_str)
        except (json.JSONDecodeError, TypeError):
            text = str(content_str)

        # Strip @bot mention
        text = _strip_at_mention(text)

        return InboundMessage(
            channel=Channel.FEISHU,
            sender_id=sender_id,
            text=text.strip(),
            raw=raw,
        )

    def build_outbound(self, msg: OutboundMessage) -> dict[str, Any]:
        # If payload has card data, use interactive message
        if "card" in msg.payload:
            return {
                "msg_type": "interactive",
                "card": msg.payload["card"],
            }
        return {
            "msg_type": "text",
            "content": json.dumps({"text": msg.text}, ensure_ascii=False),
        }

    def verify_signature(
        self, headers: dict[str, str], body: bytes, secret: str,
    ) -> bool:
        timestamp = headers.get("X-Lark-Request-Timestamp", "")
        nonce = headers.get("X-Lark-Request-Nonce", "")
        signature = headers.get("X-Lark-Signature", "")
        if not (timestamp and nonce and signature):
            return False
        payload = timestamp + nonce + secret + body.decode("utf-8", errors="replace")
        expected = hashlib.sha256(payload.encode("utf-8")).hexdigest()
        return hmac.compare_digest(expected, signature)


# ---------------------------------------------------------------------------
# WeCom adapter
# ---------------------------------------------------------------------------

class WeComAdapter(ChannelAdapter):
    """Adapter for 企业微信 (WeCom) callback messages."""

    @property
    def channel(self) -> Channel:
        return Channel.WECOM

    def parse_inbound(self, raw: dict[str, Any]) -> InboundMessage:
        # WeCom XML-decoded structure (pre-parsed to dict):
        # {"FromUserName": "...", "Content": "...", "MsgType": "text"}
        sender_id = raw.get("FromUserName", "unknown")
        text = raw.get("Content", "")
        return InboundMessage(
            channel=Channel.WECOM,
            sender_id=sender_id,
            text=str(text).strip(),
            raw=raw,
        )

    def build_outbound(self, msg: OutboundMessage) -> dict[str, Any]:
        return {
            "touser": msg.recipient_id,
            "msgtype": "text",
            "text": {"content": msg.text},
        }

    def verify_signature(
        self, headers: dict[str, str], body: bytes, secret: str,
    ) -> bool:
        msg_signature = headers.get("msg_signature", "")
        timestamp = headers.get("timestamp", "")
        nonce = headers.get("nonce", "")
        if not (msg_signature and timestamp and nonce):
            return False
        parts = sorted([secret, timestamp, nonce])
        expected = hashlib.sha1("".join(parts).encode("utf-8")).hexdigest()
        return hmac.compare_digest(expected, msg_signature)


# ---------------------------------------------------------------------------
# Telegram adapter
# ---------------------------------------------------------------------------

class TelegramAdapter(ChannelAdapter):
    """Adapter for Telegram Bot API webhook updates."""

    @property
    def channel(self) -> Channel:
        return Channel.TELEGRAM

    def parse_inbound(self, raw: dict[str, Any]) -> InboundMessage:
        message = raw.get("message", {})
        sender = message.get("from", {})
        sender_id = str(sender.get("id", "unknown"))
        text = message.get("text", "")
        # Strip /command prefix
        if text.startswith("/"):
            parts = text.split(maxsplit=1)
            text = parts[1] if len(parts) > 1 else parts[0].lstrip("/")
        return InboundMessage(
            channel=Channel.TELEGRAM,
            sender_id=sender_id,
            text=text.strip(),
            raw=raw,
        )

    def build_outbound(self, msg: OutboundMessage) -> dict[str, Any]:
        return {
            "method": "sendMessage",
            "chat_id": msg.recipient_id,
            "text": msg.text,
            "parse_mode": "Markdown",
        }


# ---------------------------------------------------------------------------
# Discord adapter
# ---------------------------------------------------------------------------

class DiscordAdapter(ChannelAdapter):
    """Adapter for Discord webhook / bot interactions."""

    @property
    def channel(self) -> Channel:
        return Channel.DISCORD

    def parse_inbound(self, raw: dict[str, Any]) -> InboundMessage:
        # Discord interaction or message create event
        sender = raw.get("author", raw.get("member", {}).get("user", {}))
        sender_id = str(sender.get("id", "unknown"))
        text = raw.get("content", raw.get("data", {}).get("name", ""))
        text = _strip_at_mention(text)
        return InboundMessage(
            channel=Channel.DISCORD,
            sender_id=sender_id,
            text=text.strip(),
            raw=raw,
        )

    def build_outbound(self, msg: OutboundMessage) -> dict[str, Any]:
        return {
            "content": msg.text,
        }


# ---------------------------------------------------------------------------
# DingTalk adapter
# ---------------------------------------------------------------------------

class DingTalkAdapter(ChannelAdapter):
    """Adapter for 钉钉 (DingTalk) robot callback."""

    @property
    def channel(self) -> Channel:
        return Channel.DINGTALK

    def parse_inbound(self, raw: dict[str, Any]) -> InboundMessage:
        sender_id = raw.get("senderStaffId", raw.get("senderId", "unknown"))
        text_content = raw.get("text", {})
        text = text_content.get("content", "") if isinstance(text_content, dict) else str(text_content)
        text = _strip_at_mention(text)
        return InboundMessage(
            channel=Channel.DINGTALK,
            sender_id=str(sender_id),
            text=text.strip(),
            raw=raw,
        )

    def build_outbound(self, msg: OutboundMessage) -> dict[str, Any]:
        return {
            "msgtype": "text",
            "text": {"content": msg.text},
        }

    def verify_signature(
        self, headers: dict[str, str], body: bytes, secret: str,
    ) -> bool:
        timestamp = headers.get("timestamp", "")
        sign = headers.get("sign", "")
        if not (timestamp and sign):
            return False
        string_to_sign = f"{timestamp}\n{secret}"
        hmac_code = hmac.new(
            secret.encode("utf-8"),
            string_to_sign.encode("utf-8"),
            hashlib.sha256,
        ).hexdigest()
        return hmac.compare_digest(hmac_code, sign)


# ---------------------------------------------------------------------------
# Slack adapter
# ---------------------------------------------------------------------------

class SlackAdapter(ChannelAdapter):
    """Adapter for Slack Events API."""

    @property
    def channel(self) -> Channel:
        return Channel.SLACK

    def parse_inbound(self, raw: dict[str, Any]) -> InboundMessage:
        event = raw.get("event", raw)
        sender_id = event.get("user", "unknown")
        text = event.get("text", "")
        text = _strip_at_mention(text)
        return InboundMessage(
            channel=Channel.SLACK,
            sender_id=sender_id,
            text=text.strip(),
            raw=raw,
        )

    def build_outbound(self, msg: OutboundMessage) -> dict[str, Any]:
        return {
            "channel": msg.recipient_id,
            "text": msg.text,
        }

    def verify_signature(
        self, headers: dict[str, str], body: bytes, secret: str,
    ) -> bool:
        timestamp = headers.get("X-Slack-Request-Timestamp", "")
        signature = headers.get("X-Slack-Signature", "")
        if not (timestamp and signature):
            return False
        # Reject old timestamps (> 5 minutes)
        try:
            if abs(time.time() - int(timestamp)) > 300:
                return False
        except ValueError:
            return False
        sig_basestring = f"v0:{timestamp}:{body.decode('utf-8', errors='replace')}"
        expected = "v0=" + hmac.new(
            secret.encode("utf-8"),
            sig_basestring.encode("utf-8"),
            hashlib.sha256,
        ).hexdigest()
        return hmac.compare_digest(expected, signature)


# ---------------------------------------------------------------------------
# MQTT adapter
# ---------------------------------------------------------------------------

class MqttAdapter(ChannelAdapter):
    """Adapter for MQTT-based custom bots / IoT commands."""

    @property
    def channel(self) -> Channel:
        return Channel.MQTT

    def parse_inbound(self, raw: dict[str, Any]) -> InboundMessage:
        sender_id = raw.get("client_id", raw.get("sender", "mqtt-client"))
        text = raw.get("payload", raw.get("message", ""))
        if isinstance(text, bytes):
            text = text.decode("utf-8", errors="replace")
        return InboundMessage(
            channel=Channel.MQTT,
            sender_id=str(sender_id),
            text=str(text).strip(),
            raw=raw,
        )

    def build_outbound(self, msg: OutboundMessage) -> dict[str, Any]:
        return {
            "topic": msg.payload.get("topic", f"harborclaw/reply/{msg.recipient_id}"),
            "payload": msg.text,
        }


# ---------------------------------------------------------------------------
# Adapter registry
# ---------------------------------------------------------------------------

_ADAPTERS: dict[Channel, type[ChannelAdapter]] = {
    Channel.FEISHU: FeishuAdapter,
    Channel.WECOM: WeComAdapter,
    Channel.TELEGRAM: TelegramAdapter,
    Channel.DISCORD: DiscordAdapter,
    Channel.DINGTALK: DingTalkAdapter,
    Channel.SLACK: SlackAdapter,
    Channel.MQTT: MqttAdapter,
}


def get_adapter(channel: Channel | str) -> ChannelAdapter:
    """Return a new adapter instance for the given channel."""
    if isinstance(channel, str):
        try:
            channel = Channel(channel)
        except ValueError:
            raise ValueError(f"No adapter for channel: {channel}")
    cls = _ADAPTERS.get(channel)
    if cls is None:
        raise ValueError(f"No adapter for channel: {channel.value}")
    return cls()


def supported_channels() -> list[Channel]:
    """Return list of channels that have adapters."""
    return list(_ADAPTERS.keys())


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _strip_at_mention(text: str) -> str:
    """Remove @bot mentions from message text."""
    import re
    # Common patterns: @bot_name, <@U123>, @_user_1
    text = re.sub(r"@\S+", "", text)
    # Feishu @_user_N patterns
    text = re.sub(r"@_user_\d+", "", text)
    return text.strip()
