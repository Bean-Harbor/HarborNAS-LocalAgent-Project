"""Tests for harborbeacon.adapters — IM channel adapters."""
import json
import pytest

from harborbeacon.adapters import (
    ChannelAdapter,
    DingTalkAdapter,
    DiscordAdapter,
    FeishuAdapter,
    MqttAdapter,
    SlackAdapter,
    TelegramAdapter,
    WeComAdapter,
    get_adapter,
    supported_channels,
)
from harborbeacon.channels import Channel, InboundMessage, OutboundMessage


# ---------------------------------------------------------------------------
# Feishu
# ---------------------------------------------------------------------------

class TestFeishuAdapter:
    def setup_method(self):
        self.adapter = FeishuAdapter()

    def test_channel(self):
        assert self.adapter.channel == Channel.FEISHU

    def test_parse_inbound(self):
        raw = {
            "event": {
                "sender": {"sender_id": {"open_id": "ou_abc123"}},
                "message": {"content": json.dumps({"text": "查看 plex 状态"})},
            }
        }
        msg = self.adapter.parse_inbound(raw)
        assert isinstance(msg, InboundMessage)
        assert msg.channel == Channel.FEISHU
        assert msg.sender_id == "ou_abc123"
        assert "plex" in msg.text

    def test_parse_strips_at_mention(self):
        raw = {
            "event": {
                "sender": {"sender_id": {"open_id": "ou_1"}},
                "message": {"content": json.dumps({"text": "@bot 查看 plex 状态"})},
            }
        }
        msg = self.adapter.parse_inbound(raw)
        assert "@" not in msg.text
        assert "plex" in msg.text

    def test_build_outbound_text(self):
        out = OutboundMessage(channel=Channel.FEISHU, recipient_id="ou_1", text="hello")
        payload = self.adapter.build_outbound(out)
        assert payload["msg_type"] == "text"

    def test_build_outbound_card(self):
        out = OutboundMessage(
            channel=Channel.FEISHU,
            recipient_id="ou_1",
            text="",
            payload={"card": {"header": {}}},
        )
        payload = self.adapter.build_outbound(out)
        assert payload["msg_type"] == "interactive"

    def test_verify_signature_missing_headers(self):
        assert not self.adapter.verify_signature({}, b"body", "secret")


# ---------------------------------------------------------------------------
# WeCom
# ---------------------------------------------------------------------------

class TestWeComAdapter:
    def setup_method(self):
        self.adapter = WeComAdapter()

    def test_channel(self):
        assert self.adapter.channel == Channel.WECOM

    def test_parse_inbound(self):
        raw = {"FromUserName": "user123", "Content": "查看 samba 状态"}
        msg = self.adapter.parse_inbound(raw)
        assert msg.sender_id == "user123"
        assert "samba" in msg.text

    def test_build_outbound(self):
        out = OutboundMessage(channel=Channel.WECOM, recipient_id="u1", text="OK")
        payload = self.adapter.build_outbound(out)
        assert payload["touser"] == "u1"
        assert payload["msgtype"] == "text"


# ---------------------------------------------------------------------------
# Telegram
# ---------------------------------------------------------------------------

class TestTelegramAdapter:
    def setup_method(self):
        self.adapter = TelegramAdapter()

    def test_parse_inbound(self):
        raw = {"message": {"from": {"id": 999}, "text": "status plex"}}
        msg = self.adapter.parse_inbound(raw)
        assert msg.sender_id == "999"
        assert "status plex" == msg.text

    def test_strips_command_prefix(self):
        raw = {"message": {"from": {"id": 1}, "text": "/harborbeacon status plex"}}
        msg = self.adapter.parse_inbound(raw)
        assert msg.text == "status plex"

    def test_build_outbound(self):
        out = OutboundMessage(channel=Channel.TELEGRAM, recipient_id="123", text="done")
        payload = self.adapter.build_outbound(out)
        assert payload["chat_id"] == "123"
        assert payload["parse_mode"] == "Markdown"


# ---------------------------------------------------------------------------
# Discord
# ---------------------------------------------------------------------------

class TestDiscordAdapter:
    def setup_method(self):
        self.adapter = DiscordAdapter()

    def test_parse_inbound(self):
        raw = {"author": {"id": "disc_1"}, "content": "restart nginx"}
        msg = self.adapter.parse_inbound(raw)
        assert msg.sender_id == "disc_1"
        assert "restart nginx" == msg.text

    def test_build_outbound(self):
        out = OutboundMessage(channel=Channel.DISCORD, recipient_id="ch1", text="ok")
        payload = self.adapter.build_outbound(out)
        assert payload["content"] == "ok"


# ---------------------------------------------------------------------------
# DingTalk
# ---------------------------------------------------------------------------

class TestDingTalkAdapter:
    def setup_method(self):
        self.adapter = DingTalkAdapter()

    def test_parse_inbound(self):
        raw = {
            "senderStaffId": "staff_1",
            "text": {"content": "查看 plex 状态"},
        }
        msg = self.adapter.parse_inbound(raw)
        assert msg.sender_id == "staff_1"
        assert "plex" in msg.text

    def test_build_outbound(self):
        out = OutboundMessage(channel=Channel.DINGTALK, recipient_id="s1", text="done")
        payload = self.adapter.build_outbound(out)
        assert payload["msgtype"] == "text"

    def test_verify_signature_missing(self):
        assert not self.adapter.verify_signature({}, b"body", "secret")


# ---------------------------------------------------------------------------
# Slack
# ---------------------------------------------------------------------------

class TestSlackAdapter:
    def setup_method(self):
        self.adapter = SlackAdapter()

    def test_parse_inbound(self):
        raw = {"event": {"user": "U123", "text": "check plex"}}
        msg = self.adapter.parse_inbound(raw)
        assert msg.sender_id == "U123"

    def test_build_outbound(self):
        out = OutboundMessage(channel=Channel.SLACK, recipient_id="C1", text="ok")
        payload = self.adapter.build_outbound(out)
        assert payload["channel"] == "C1"


# ---------------------------------------------------------------------------
# MQTT
# ---------------------------------------------------------------------------

class TestMqttAdapter:
    def setup_method(self):
        self.adapter = MqttAdapter()

    def test_parse_inbound(self):
        raw = {"client_id": "iot_1", "payload": "status plex"}
        msg = self.adapter.parse_inbound(raw)
        assert msg.sender_id == "iot_1"
        assert msg.text == "status plex"

    def test_parse_bytes_payload(self):
        raw = {"client_id": "c1", "payload": b"hello"}
        msg = self.adapter.parse_inbound(raw)
        assert msg.text == "hello"

    def test_build_outbound(self):
        out = OutboundMessage(
            channel=Channel.MQTT,
            recipient_id="c1",
            text="ok",
            payload={"topic": "harborbeacon/reply/c1"},
        )
        payload = self.adapter.build_outbound(out)
        assert payload["topic"] == "harborbeacon/reply/c1"

    def test_build_outbound_default_topic(self):
        out = OutboundMessage(channel=Channel.MQTT, recipient_id="c1", text="ok")
        payload = self.adapter.build_outbound(out)
        assert "c1" in payload["topic"]


# ---------------------------------------------------------------------------
# Registry functions
# ---------------------------------------------------------------------------

class TestAdapterRegistry:
    def test_get_adapter_all_channels(self):
        for ch in Channel:
            adapter = get_adapter(ch)
            assert isinstance(adapter, ChannelAdapter)
            assert adapter.channel == ch

    def test_supported_channels_complete(self):
        assert set(supported_channels()) == set(Channel)

    def test_unknown_channel_raises(self):
        with pytest.raises(ValueError):
            get_adapter("nonexistent")  # type: ignore
