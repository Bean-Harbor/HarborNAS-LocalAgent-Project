"""IM channel configuration and message dispatch.

HarborBeacon supports multiple IM integrations.  On first boot the user
picks which channels to enable; the config is stored at
``/etc/harborbeacon/channels.yaml``.

Supported channels:
  - feishu      (飞书 / Lark)
  - wecom       (企业微信 / WeCom)
  - telegram
  - discord
  - dingtalk    (钉钉)
  - slack
  - mqtt        (for IoT / custom bots)

Each channel has:
  - A ``ChannelConfig`` with credentials / webhook URLs
  - An ``InboundMessage`` (what the user sends)
  - An ``OutboundMessage`` (what HarborBeacon replies)

The ``ChannelRouter`` dispatches an inbound message through the MCP
adapter and formats the result back to the originating channel.
"""
from __future__ import annotations

import json
from dataclasses import dataclass, field
from enum import Enum
from typing import Any, Callable


# ---------------------------------------------------------------------------
# Attachment (rich media)
# ---------------------------------------------------------------------------

class AttachmentType(str, Enum):
    """Attachment media types."""
    IMAGE = "image"
    VIDEO = "video"
    AUDIO = "audio"
    FILE = "file"


@dataclass
class Attachment:
    """A media attachment (image, file, etc.) on a message."""
    type: AttachmentType
    # For inbound: base64 data-URL, file path, or URL.
    # For outbound: local file path or URL to send.
    content: str
    mime_type: str = "application/octet-stream"
    file_name: str = ""


class ChatType(str, Enum):
    """Whether a message comes from a 1-on-1 or a group chat."""
    P2P = "p2p"
    GROUP = "group"
    UNKNOWN = "unknown"


# ---------------------------------------------------------------------------
# Channel enum & config
# ---------------------------------------------------------------------------

class Channel(str, Enum):
    """Supported IM channels."""
    FEISHU = "feishu"
    WECOM = "wecom"
    TELEGRAM = "telegram"
    DISCORD = "discord"
    DINGTALK = "dingtalk"
    SLACK = "slack"
    MQTT = "mqtt"


@dataclass
class ChannelConfig:
    """Credentials and settings for one IM channel."""
    channel: Channel
    enabled: bool = False
    webhook_url: str | None = None
    app_id: str | None = None
    app_secret: str | None = None
    bot_token: str | None = None
    # Transport mode: "websocket" | "long_poll" | "webhook" (auto-detected if empty)
    transport_mode: str = ""
    extra: dict[str, Any] = field(default_factory=dict)

    def is_configured(self) -> bool:
        """Return True if minimum credentials are present."""
        if not self.enabled:
            return False
        if self.channel in (Channel.FEISHU, Channel.WECOM, Channel.DINGTALK):
            return bool(self.app_id and self.app_secret)
        if self.channel in (Channel.TELEGRAM, Channel.DISCORD, Channel.SLACK):
            return bool(self.bot_token)
        if self.channel == Channel.MQTT:
            return bool(self.extra.get("broker"))
        return False


# ---------------------------------------------------------------------------
# Messages
# ---------------------------------------------------------------------------

@dataclass
class InboundMessage:
    """A message received from an IM channel."""
    channel: Channel
    sender_id: str
    text: str
    raw: dict[str, Any] = field(default_factory=dict)
    # ---- OpenClaw-inspired fields ----
    message_id: str = ""                        # Platform message ID (for dedup & reply)
    chat_type: ChatType = ChatType.UNKNOWN       # p2p or group
    chat_id: str = ""                            # Chat/conversation ID
    mentions: list[str] = field(default_factory=list)  # Bot/user mentions in message
    attachments: list[Attachment] = field(default_factory=list)  # Rich media


@dataclass
class OutboundMessage:
    """A message to send back to an IM channel."""
    channel: Channel
    recipient_id: str
    text: str
    payload: dict[str, Any] = field(default_factory=dict)
    # ---- OpenClaw-inspired fields ----
    attachments: list[Attachment] = field(default_factory=list)  # Rich media to send
    reply_to_message_id: str = ""                # Platform message ID to update/replace
    update_message_id: str = ""                  # Edit existing message instead of new


# ---------------------------------------------------------------------------
# Channel registry
# ---------------------------------------------------------------------------

class ChannelRegistry:
    """Holds per-channel configuration and send callbacks."""

    def __init__(self) -> None:
        self._configs: dict[Channel, ChannelConfig] = {}
        self._senders: dict[Channel, Callable[[OutboundMessage], None]] = {}

    def register(self, config: ChannelConfig,
                 sender: Callable[[OutboundMessage], None] | None = None) -> None:
        self._configs[config.channel] = config
        if sender:
            self._senders[config.channel] = sender

    def get_config(self, channel: Channel) -> ChannelConfig | None:
        return self._configs.get(channel)

    def enabled_channels(self) -> list[Channel]:
        return [ch for ch, cfg in self._configs.items() if cfg.is_configured()]

    def send(self, msg: OutboundMessage) -> None:
        sender = self._senders.get(msg.channel)
        if sender is None:
            raise RuntimeError(f"No sender registered for channel {msg.channel.value}")
        sender(msg)

    def summary(self) -> dict[str, Any]:
        return {
            "total": len(self._configs),
            "enabled": [ch.value for ch in self.enabled_channels()],
            "channels": {
                ch.value: {"enabled": cfg.enabled, "configured": cfg.is_configured()}
                for ch, cfg in self._configs.items()
            },
        }


# ---------------------------------------------------------------------------
# Channel router — connects IM ↔ MCP adapter
# ---------------------------------------------------------------------------

class ChannelRouter:
    """Receives IM messages, runs them through the MCP adapter, and replies.

    Usage::

        from harborbeacon.mcp_adapter import McpServerAdapter

        router = ChannelRouter(
            channel_registry=ch_reg,
            mcp_adapter=adapter,
            intent_parser=my_parse_fn,  # text → (tool_name, arguments)
        )
        router.handle(inbound_msg)
    """

    def __init__(
        self,
        channel_registry: ChannelRegistry,
        mcp_adapter: Any,  # McpServerAdapter — import avoided for loose coupling
        intent_parser: Callable[[str], tuple[str, dict[str, Any]]] | None = None,
    ):
        self._channels = channel_registry
        self._mcp = mcp_adapter
        self._parse = intent_parser or self._default_parse

    def handle(self, msg: InboundMessage) -> OutboundMessage:
        """Process an inbound IM message end-to-end.

        1. Parse intent → (tool_name, arguments)
        2. call_tool via MCP adapter
        3. Format result as OutboundMessage
        """
        tool_name, arguments = self._parse(msg.text)
        mcp_result = self._mcp.call_tool(tool_name, arguments)

        # Extract text from MCP result
        text_parts = []
        for item in mcp_result.content:
            if item.get("type") == "text":
                text_parts.append(item["text"])
        reply_text = "\n".join(text_parts) if text_parts else "No result."

        out = OutboundMessage(
            channel=msg.channel,
            recipient_id=msg.sender_id,
            text=reply_text,
            payload={"tool": tool_name, "is_error": mcp_result.isError},
        )

        # Try to send via channel registry
        sender = self._channels._senders.get(msg.channel)
        if sender:
            sender(out)

        return out

    @staticmethod
    def _default_parse(text: str) -> tuple[str, dict[str, Any]]:
        """Naive parser: first word = tool name, rest = JSON or empty.

        Real deployments should use LLM-based intent parsing.
        """
        parts = text.strip().split(None, 1)
        tool_name = parts[0] if parts else ""
        arguments: dict[str, Any] = {}
        if len(parts) > 1:
            try:
                arguments = json.loads(parts[1])
            except (json.JSONDecodeError, TypeError):
                arguments = {"text": parts[1]}
        return tool_name, arguments


# ---------------------------------------------------------------------------
# Config loader helper
# ---------------------------------------------------------------------------

def load_channel_configs(data: dict[str, Any]) -> list[ChannelConfig]:
    """Parse a channels.yaml dict into ChannelConfig objects.

    Expected YAML structure::

        channels:
          feishu:
            enabled: true
            app_id: "cli_xxx"
            app_secret: "..."
          telegram:
            enabled: true
            bot_token: "123:ABC..."
    """
    configs: list[ChannelConfig] = []
    channels_data = data.get("channels", {})
    for name, cfg in channels_data.items():
        try:
            ch = Channel(name)
        except ValueError:
            continue  # skip unknown channels
        configs.append(ChannelConfig(
            channel=ch,
            enabled=cfg.get("enabled", False),
            webhook_url=cfg.get("webhook_url"),
            app_id=cfg.get("app_id"),
            app_secret=cfg.get("app_secret"),
            bot_token=cfg.get("bot_token"),
            extra={k: v for k, v in cfg.items()
                   if k not in ("enabled", "webhook_url", "app_id", "app_secret", "bot_token")},
        ))
    return configs
