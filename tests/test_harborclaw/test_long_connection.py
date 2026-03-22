"""Tests for harborclaw.long_connection — WebSocket/long-poll transports.

Tests cover:
  - TransportMode / ConnectionState / ConnectionStatus data classes
  - FeishuWsConfig construction
  - FeishuWsTransport (without real network — mock SDK / token fetch)
  - TelegramLongPollTransport (without real network — mock getUpdates)
  - Gateway registration, start/stop, status aggregation
  - recommended_transport() helper
  - Transport mode added to ChannelConfig
"""
from __future__ import annotations

import json
import threading
import time
from unittest.mock import MagicMock, patch

import pytest

from harborclaw.channels import Channel, ChannelConfig
from harborclaw.long_connection import (
    ConnectionState,
    ConnectionStatus,
    FeishuWsConfig,
    FeishuWsTransport,
    Gateway,
    TelegramLongPollTransport,
    TransportMode,
    recommended_transport,
)


# ===========================================================================
# TransportMode / ConnectionState / ConnectionStatus
# ===========================================================================

class TestTransportMode:
    def test_values(self):
        assert TransportMode.WEBSOCKET == "websocket"
        assert TransportMode.LONG_POLL == "long_poll"
        assert TransportMode.WEBHOOK == "webhook"

    def test_enum_from_string(self):
        assert TransportMode("websocket") == TransportMode.WEBSOCKET
        assert TransportMode("webhook") == TransportMode.WEBHOOK

    def test_invalid_mode_raises(self):
        with pytest.raises(ValueError):
            TransportMode("invalid_mode")


class TestConnectionState:
    def test_all_states(self):
        states = {s.value for s in ConnectionState}
        assert states == {
            "disconnected", "connecting", "connected", "reconnecting", "stopped",
        }


class TestConnectionStatus:
    def test_defaults(self):
        status = ConnectionStatus(channel=Channel.FEISHU)
        assert status.channel == Channel.FEISHU
        assert status.state == ConnectionState.DISCONNECTED
        assert status.reconnect_count == 0
        assert status.messages_received == 0
        assert status.last_error == ""

    def test_update_fields(self):
        status = ConnectionStatus(channel=Channel.TELEGRAM)
        status.state = ConnectionState.CONNECTED
        status.messages_received = 42
        assert status.state == ConnectionState.CONNECTED
        assert status.messages_received == 42


# ===========================================================================
# FeishuWsConfig
# ===========================================================================

class TestFeishuWsConfig:
    def test_construction(self):
        cfg = FeishuWsConfig(app_id="cli_123", app_secret="secret456")
        assert cfg.app_id == "cli_123"
        assert cfg.app_secret == "secret456"
        assert cfg.domain == "https://open.feishu.cn"

    def test_custom_domain(self):
        cfg = FeishuWsConfig(
            app_id="a", app_secret="b",
            domain="https://open.larksuite.com",
        )
        assert cfg.domain == "https://open.larksuite.com"


# ===========================================================================
# FeishuWsTransport
# ===========================================================================

class TestFeishuWsTransport:
    def make_transport(self) -> FeishuWsTransport:
        return FeishuWsTransport(
            FeishuWsConfig(app_id="cli_test", app_secret="test_secret"),
        )

    def test_channel(self):
        t = self.make_transport()
        assert t.channel == Channel.FEISHU

    def test_initial_state(self):
        t = self.make_transport()
        assert not t.connected
        assert t.status.state == ConnectionState.DISCONNECTED

    def test_stop_sets_stopped(self):
        t = self.make_transport()
        t.stop()
        assert t.status.state == ConnectionState.STOPPED

    @patch("harborclaw.long_connection.FeishuWsTransport._try_start_sdk", return_value=True)
    def test_start_with_sdk(self, mock_sdk):
        t = self.make_transport()
        handler = MagicMock()
        t.start(handler)
        mock_sdk.assert_called_once()

    @patch("harborclaw.long_connection.FeishuWsTransport._try_start_sdk", return_value=False)
    def test_start_fallback_creates_thread(self, mock_sdk):
        t = self.make_transport()
        # Patch _run_loop to avoid actual network calls
        t._run_loop = MagicMock()
        handler = MagicMock()
        t.start(handler)
        assert t._thread is not None
        t.stop()

    def test_on_sdk_message_extracts_text(self):
        """Simulate an SDK event object with nested attributes."""
        t = self.make_transport()
        received = []
        t._handler = lambda msg: received.append(msg)

        # Build a mock SDK event data
        sender_id_obj = MagicMock()
        sender_id_obj.open_id = "ou_user123"
        sender_obj = MagicMock()
        sender_obj.sender_id = sender_id_obj

        message_obj = MagicMock()
        message_obj.content = json.dumps({"text": "重启 samba"})

        event_obj = MagicMock()
        event_obj.sender = sender_obj
        event_obj.message = message_obj

        data = MagicMock()
        data.event = event_obj

        t._on_sdk_message(data)

        assert len(received) == 1
        assert received[0].channel == Channel.FEISHU
        assert received[0].sender_id == "ou_user123"
        assert received[0].text == "重启 samba"
        assert t.status.messages_received == 1

    def test_on_sdk_message_handles_error(self):
        """Bad event data should not crash."""
        t = self.make_transport()
        t._handler = MagicMock()
        t._on_sdk_message(None)  # Should log error, not raise
        t._handler.assert_not_called()

    def test_get_tenant_token_success(self):
        t = self.make_transport()
        mock_response = MagicMock()
        mock_response.read.return_value = json.dumps({
            "code": 0,
            "tenant_access_token": "t-abc123",
        }).encode("utf-8")
        mock_response.__enter__ = MagicMock(return_value=mock_response)
        mock_response.__exit__ = MagicMock(return_value=False)

        with patch("urllib.request.urlopen", return_value=mock_response):
            token = t._get_tenant_token()
        assert token == "t-abc123"

    def test_get_tenant_token_failure(self):
        t = self.make_transport()
        mock_response = MagicMock()
        mock_response.read.return_value = json.dumps({
            "code": 10003,
            "msg": "invalid app_secret",
        }).encode("utf-8")
        mock_response.__enter__ = MagicMock(return_value=mock_response)
        mock_response.__exit__ = MagicMock(return_value=False)

        with patch("urllib.request.urlopen", return_value=mock_response):
            token = t._get_tenant_token()
        assert token is None


# ===========================================================================
# TelegramLongPollTransport
# ===========================================================================

class TestTelegramLongPollTransport:
    def make_transport(self) -> TelegramLongPollTransport:
        return TelegramLongPollTransport(bot_token="123456:ABC-DEF", poll_timeout=1)

    def test_channel(self):
        t = self.make_transport()
        assert t.channel == Channel.TELEGRAM

    def test_initial_state(self):
        t = self.make_transport()
        assert not t.connected
        assert t.status.state == ConnectionState.DISCONNECTED

    def test_stop_sets_stopped(self):
        t = self.make_transport()
        t.stop()
        assert t.status.state == ConnectionState.STOPPED

    def test_process_update_text(self):
        t = self.make_transport()
        received = []
        t._handler = lambda msg: received.append(msg)

        update = {
            "update_id": 1001,
            "message": {
                "from": {"id": 42},
                "text": "查看 samba 状态",
            },
        }
        t._process_update(update)

        assert len(received) == 1
        assert received[0].channel == Channel.TELEGRAM
        assert received[0].sender_id == "42"
        assert received[0].text == "查看 samba 状态"
        assert t.status.messages_received == 1

    def test_process_update_command(self):
        """'/start hello' should strip the /start prefix."""
        t = self.make_transport()
        received = []
        t._handler = lambda msg: received.append(msg)

        update = {
            "update_id": 1002,
            "message": {
                "from": {"id": 99},
                "text": "/status samba",
            },
        }
        t._process_update(update)
        assert received[0].text == "samba"

    def test_process_update_empty_text(self):
        """Empty message text should be ignored."""
        t = self.make_transport()
        t._handler = MagicMock()

        update = {
            "update_id": 1003,
            "message": {"from": {"id": 1}, "text": ""},
        }
        t._process_update(update)
        t._handler.assert_not_called()

    def test_process_update_no_message(self):
        """Update without 'message' key should be skipped."""
        t = self.make_transport()
        t._handler = MagicMock()
        t._process_update({"update_id": 1004})
        t._handler.assert_not_called()

    def test_poll_loop_processes_results(self):
        """Simulate one successful poll cycle."""
        t = self.make_transport()
        received = []
        t._handler = lambda msg: received.append(msg)

        api_response = json.dumps({
            "ok": True,
            "result": [
                {
                    "update_id": 2001,
                    "message": {"from": {"id": 7}, "text": "hello"},
                },
            ],
        }).encode("utf-8")

        mock_resp = MagicMock()
        mock_resp.read.return_value = api_response
        mock_resp.__enter__ = MagicMock(return_value=mock_resp)
        mock_resp.__exit__ = MagicMock(return_value=False)

        call_count = 0

        def side_effect(*args, **kwargs):
            nonlocal call_count
            call_count += 1
            if call_count > 1:
                t._stop_event.set()  # Stop after first successful poll
                raise Exception("stop")
            return mock_resp

        with patch("urllib.request.urlopen", side_effect=side_effect):
            t.start(lambda msg: received.append(msg))
            time.sleep(0.3)
            t.stop()

        assert len(received) >= 1
        assert received[0].text == "hello"
        assert t._offset == 2002


# ===========================================================================
# Gateway
# ===========================================================================

class TestGateway:
    def test_register_feishu(self):
        gw = Gateway(on_message=MagicMock())
        gw.register_feishu(FeishuWsConfig(app_id="a", app_secret="b"))
        assert Channel.FEISHU in gw._transports

    def test_register_telegram(self):
        gw = Gateway(on_message=MagicMock())
        gw.register_telegram(bot_token="123:ABC")
        assert Channel.TELEGRAM in gw._transports

    def test_register_custom_transport(self):
        gw = Gateway(on_message=MagicMock())
        mock_transport = MagicMock()
        gw.register_transport(Channel.DISCORD, mock_transport)
        assert Channel.DISCORD in gw._transports

    def test_start_all_calls_start(self):
        handler = MagicMock()
        gw = Gateway(on_message=handler)
        t1 = MagicMock()
        t2 = MagicMock()
        gw.register_transport(Channel.FEISHU, t1)
        gw.register_transport(Channel.TELEGRAM, t2)

        gw.start_all()

        t1.start.assert_called_once_with(handler)
        t2.start.assert_called_once_with(handler)

    def test_stop_all_calls_stop(self):
        gw = Gateway(on_message=MagicMock())
        t1 = MagicMock()
        t2 = MagicMock()
        gw.register_transport(Channel.FEISHU, t1)
        gw.register_transport(Channel.TELEGRAM, t2)

        gw.stop_all()

        t1.stop.assert_called_once()
        t2.stop.assert_called_once()

    def test_get_status(self):
        gw = Gateway(on_message=MagicMock())
        fs = FeishuWsTransport(FeishuWsConfig(app_id="a", app_secret="b"))
        gw.register_transport(Channel.FEISHU, fs)

        status = gw.get_status(Channel.FEISHU)
        assert status is not None
        assert status.channel == Channel.FEISHU

    def test_get_status_unknown_channel(self):
        gw = Gateway(on_message=MagicMock())
        assert gw.get_status(Channel.SLACK) is None

    def test_all_statuses(self):
        gw = Gateway(on_message=MagicMock())
        gw.register_feishu(FeishuWsConfig(app_id="a", app_secret="b"))
        gw.register_telegram(bot_token="123:ABC")
        statuses = gw.all_statuses()
        assert Channel.FEISHU in statuses
        assert Channel.TELEGRAM in statuses

    def test_active_channels_initially_empty(self):
        gw = Gateway(on_message=MagicMock())
        gw.register_feishu(FeishuWsConfig(app_id="a", app_secret="b"))
        # Not started yet → not connected
        assert gw.active_channels == []


# ===========================================================================
# recommended_transport()
# ===========================================================================

class TestRecommendedTransport:
    def test_feishu_websocket(self):
        assert recommended_transport(Channel.FEISHU) == TransportMode.WEBSOCKET

    def test_telegram_long_poll(self):
        assert recommended_transport(Channel.TELEGRAM) == TransportMode.LONG_POLL

    def test_wecom_webhook(self):
        assert recommended_transport(Channel.WECOM) == TransportMode.WEBHOOK

    def test_slack_webhook(self):
        assert recommended_transport(Channel.SLACK) == TransportMode.WEBHOOK

    def test_discord_websocket(self):
        assert recommended_transport(Channel.DISCORD) == TransportMode.WEBSOCKET

    def test_dingtalk_websocket(self):
        assert recommended_transport(Channel.DINGTALK) == TransportMode.WEBSOCKET

    def test_mqtt_websocket(self):
        assert recommended_transport(Channel.MQTT) == TransportMode.WEBSOCKET


# ===========================================================================
# ChannelConfig.transport_mode
# ===========================================================================

class TestChannelConfigTransportMode:
    def test_default_empty(self):
        cfg = ChannelConfig(channel=Channel.FEISHU, enabled=True)
        assert cfg.transport_mode == ""

    def test_set_websocket(self):
        cfg = ChannelConfig(
            channel=Channel.FEISHU,
            enabled=True,
            app_id="cli_x",
            app_secret="sec",
            transport_mode="websocket",
        )
        assert cfg.transport_mode == "websocket"

    def test_set_webhook(self):
        cfg = ChannelConfig(
            channel=Channel.FEISHU,
            enabled=True,
            transport_mode="webhook",
            webhook_url="https://example.com/hook",
        )
        assert cfg.transport_mode == "webhook"
