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

from harborbeacon.channels import (
    Attachment,
    AttachmentType,
    Channel,
    ChatType,
    InboundMessage,
    OutboundMessage,
)


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
    """Adapter for 飞书 (Feishu / Lark) events API v2.

    Enhanced with OpenClaw-inspired rich message parsing:
    - text, post (rich text), image, file, audio, media (video)
    - message_id for dedup and reply-to
    - chat_type (p2p vs group) for group chat filtering
    - mentions list for @detection
    """

    @property
    def channel(self) -> Channel:
        return Channel.FEISHU

    def parse_inbound(self, raw: dict[str, Any]) -> InboundMessage:
        event = raw.get("event", {})
        sender = event.get("sender", {}).get("sender_id", {})
        sender_id = sender.get("open_id", sender.get("user_id", "unknown"))

        message = event.get("message", {})
        message_id = message.get("message_id", "")
        chat_id = message.get("chat_id", "")
        chat_type_str = message.get("chat_type", "")
        message_type = message.get("message_type", "text")
        content_str = message.get("content", "{}")

        # Parse chat type
        if chat_type_str == "p2p":
            chat_type = ChatType.P2P
        elif chat_type_str == "group":
            chat_type = ChatType.GROUP
        else:
            chat_type = ChatType.UNKNOWN

        # Parse mentions
        raw_mentions = message.get("mentions", [])
        mentions = []
        if isinstance(raw_mentions, list):
            for m in raw_mentions:
                key = m.get("key", "") if isinstance(m, dict) else ""
                if key:
                    mentions.append(key)

        # Parse content based on message_type
        text = ""
        attachments: list[Attachment] = []

        try:
            content = json.loads(content_str) if content_str else {}
        except (json.JSONDecodeError, TypeError):
            content = {}

        if message_type == "text":
            text = content.get("text", str(content_str))
        elif message_type == "post":
            text, post_images = _extract_post_text(content)
            for img_key in post_images:
                attachments.append(Attachment(
                    type=AttachmentType.IMAGE,
                    content=img_key,
                    file_name="feishu_post_image.png",
                ))
        elif message_type == "image":
            image_key = content.get("image_key", "")
            text = "[图片]"
            if image_key:
                attachments.append(Attachment(
                    type=AttachmentType.IMAGE,
                    content=image_key,
                    file_name="feishu_image.png",
                ))
        elif message_type == "file":
            file_key = content.get("file_key", "")
            file_name = content.get("file_name", "file.bin")
            text = f"[文件] {file_name}"
            if file_key:
                attachments.append(Attachment(
                    type=AttachmentType.FILE,
                    content=file_key,
                    file_name=file_name,
                ))
        elif message_type in ("media", "video"):
            file_key = content.get("file_key", "")
            file_name = content.get("file_name", "video.mp4")
            text = f"[视频] {file_name}"
            if file_key:
                attachments.append(Attachment(
                    type=AttachmentType.VIDEO,
                    content=file_key,
                    file_name=file_name,
                ))
        elif message_type == "audio":
            file_key = content.get("file_key", "")
            file_name = content.get("file_name", "audio.opus")
            text = f"[语音] {file_name}"
            if file_key:
                attachments.append(Attachment(
                    type=AttachmentType.AUDIO,
                    content=file_key,
                    file_name=file_name,
                ))
        else:
            # Unknown type — best effort
            text = content.get("text", str(content_str))

        # Strip @bot mention
        text = _strip_at_mention(text)

        return InboundMessage(
            channel=Channel.FEISHU,
            sender_id=sender_id,
            text=text.strip(),
            raw=raw,
            message_id=message_id,
            chat_type=chat_type,
            chat_id=chat_id,
            mentions=mentions,
            attachments=attachments,
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
        message_id = str(message.get("message_id", ""))
        chat = message.get("chat", {})
        chat_id = str(chat.get("id", ""))
        chat_type_str = chat.get("type", "")

        if chat_type_str in ("private",):
            chat_type = ChatType.P2P
        elif chat_type_str in ("group", "supergroup"):
            chat_type = ChatType.GROUP
        else:
            chat_type = ChatType.UNKNOWN

        # Parse mentions from entities
        mentions = []
        entities = message.get("entities", [])
        if isinstance(entities, list):
            for ent in entities:
                if isinstance(ent, dict) and ent.get("type") == "mention":
                    offset = ent.get("offset", 0)
                    length = ent.get("length", 0)
                    mentions.append(text[offset:offset + length])

        # Parse photo attachments
        attachments: list[Attachment] = []
        photos = message.get("photo", [])
        if isinstance(photos, list) and photos:
            best = photos[-1]  # largest resolution
            attachments.append(Attachment(
                type=AttachmentType.IMAGE,
                content=best.get("file_id", ""),
                file_name="telegram_photo.jpg",
            ))
        doc = message.get("document")
        if isinstance(doc, dict):
            attachments.append(Attachment(
                type=AttachmentType.FILE,
                content=doc.get("file_id", ""),
                file_name=doc.get("file_name", "file.bin"),
                mime_type=doc.get("mime_type", "application/octet-stream"),
            ))

        # Strip /command prefix
        if text.startswith("/"):
            parts = text.split(maxsplit=1)
            text = parts[1] if len(parts) > 1 else parts[0].lstrip("/")
        return InboundMessage(
            channel=Channel.TELEGRAM,
            sender_id=sender_id,
            text=text.strip(),
            raw=raw,
            message_id=message_id,
            chat_type=chat_type,
            chat_id=chat_id,
            mentions=mentions,
            attachments=attachments,
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
        sender = raw.get("author", raw.get("member", {}).get("user", {}))
        sender_id = str(sender.get("id", "unknown"))
        text = raw.get("content", raw.get("data", {}).get("name", ""))
        message_id = str(raw.get("id", ""))
        channel_id = str(raw.get("channel_id", ""))
        # Discord DM channels have type=1, guild channels are 0
        guild_id = raw.get("guild_id", "")
        chat_type = ChatType.P2P if not guild_id else ChatType.GROUP

        # Parse mentions
        mentions = []
        raw_mentions = raw.get("mentions", [])
        if isinstance(raw_mentions, list):
            for m in raw_mentions:
                if isinstance(m, dict):
                    mentions.append(m.get("id", ""))

        text = _strip_at_mention(text)
        return InboundMessage(
            channel=Channel.DISCORD,
            sender_id=sender_id,
            text=text.strip(),
            raw=raw,
            message_id=message_id,
            chat_type=chat_type,
            chat_id=channel_id,
            mentions=mentions,
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
        msg_id = raw.get("msgId", "")
        conversation_id = raw.get("conversationId", "")
        conversation_type = raw.get("conversationType", "")

        if conversation_type == "1":
            chat_type = ChatType.P2P
        elif conversation_type == "2":
            chat_type = ChatType.GROUP
        else:
            chat_type = ChatType.UNKNOWN

        # DingTalk @mentions in atUsers list
        mentions = []
        at_users = raw.get("atUsers", [])
        if isinstance(at_users, list):
            for u in at_users:
                if isinstance(u, dict):
                    mentions.append(u.get("dingtalkId", ""))

        text = _strip_at_mention(text)
        return InboundMessage(
            channel=Channel.DINGTALK,
            sender_id=str(sender_id),
            text=text.strip(),
            raw=raw,
            message_id=msg_id,
            chat_type=chat_type,
            chat_id=conversation_id,
            mentions=mentions,
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
        message_ts = event.get("ts", "")
        channel_id = event.get("channel", "")
        channel_type = event.get("channel_type", "")

        if channel_type == "im":
            chat_type = ChatType.P2P
        elif channel_type in ("channel", "group", "mpim"):
            chat_type = ChatType.GROUP
        else:
            chat_type = ChatType.UNKNOWN

        text = _strip_at_mention(text)
        return InboundMessage(
            channel=Channel.SLACK,
            sender_id=sender_id,
            text=text.strip(),
            raw=raw,
            message_id=message_ts,
            chat_type=chat_type,
            chat_id=channel_id,
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
            "topic": msg.payload.get("topic", f"harborbeacon/reply/{msg.recipient_id}"),
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


def _extract_post_text(post_json: dict[str, Any]) -> tuple[str, list[str]]:
    """Extract plain text and image keys from Feishu post (rich text) content.

    Follows OpenClaw's ``extractFromPostJson`` logic: walk paragraphs of
    inline nodes, collecting text and image_key references.

    Returns ``(text, image_keys)``.
    """
    lines: list[str] = []
    image_keys: list[str] = []

    def inline(node: Any) -> str:
        if not node:
            return ""
        if isinstance(node, list):
            return "".join(inline(n) for n in node)
        if not isinstance(node, dict):
            return ""
        tag = node.get("tag", "")
        if tag == "text":
            return str(node.get("text", ""))
        if tag == "a":
            return str(node.get("text", node.get("href", "")))
        if tag == "at":
            name = node.get("user_name", "")
            return f"@{name}" if name else "@"
        if tag == "md":
            return str(node.get("text", ""))
        if tag == "img":
            key = node.get("image_key", "")
            if key:
                image_keys.append(key)
            return "[图片]"
        if tag == "file":
            return "[文件]"
        if tag == "media":
            return "[视频]"
        if tag == "code_block":
            lang = str(node.get("language", "")).strip()
            code = str(node.get("text", ""))
            return f"\n```{lang}\n{code}\n```\n"
        # Fallback: traverse children
        acc = ""
        for v in node.values():
            if isinstance(v, (dict, list)):
                acc += inline(v)
        return acc

    title = post_json.get("title", "")
    if title:
        lines.append(str(title).strip())

    content = post_json.get("content")
    if isinstance(content, list):
        for paragraph in content:
            if isinstance(paragraph, list):
                joined = "".join(inline(n) for n in paragraph).strip()
            else:
                joined = inline(paragraph).strip()
            if joined:
                lines.append(joined)
    elif content:
        joined = inline(content).strip()
        if joined:
            lines.append(joined)

    text = "\n".join(lines).strip()
    return text, list(dict.fromkeys(image_keys))  # dedup preserving order
