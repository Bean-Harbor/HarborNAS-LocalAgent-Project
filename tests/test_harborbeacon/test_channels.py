"""Tests for harborbeacon.channels — IM channel config, routing, dispatch."""
import json

import pytest

from harborbeacon.channels import (
    Channel,
    ChannelConfig,
    ChannelRegistry,
    ChannelRouter,
    InboundMessage,
    OutboundMessage,
    load_channel_configs,
)


# ---------------------------------------------------------------------------
# ChannelConfig
# ---------------------------------------------------------------------------

class TestChannelConfig:
    def test_feishu_configured(self):
        cfg = ChannelConfig(channel=Channel.FEISHU, enabled=True,
                            app_id="cli_xxx", app_secret="sec")
        assert cfg.is_configured()

    def test_feishu_missing_secret(self):
        cfg = ChannelConfig(channel=Channel.FEISHU, enabled=True,
                            app_id="cli_xxx")
        assert not cfg.is_configured()

    def test_feishu_disabled(self):
        cfg = ChannelConfig(channel=Channel.FEISHU, enabled=False,
                            app_id="cli_xxx", app_secret="sec")
        assert not cfg.is_configured()

    def test_telegram_configured(self):
        cfg = ChannelConfig(channel=Channel.TELEGRAM, enabled=True,
                            bot_token="123:ABC")
        assert cfg.is_configured()

    def test_telegram_missing_token(self):
        cfg = ChannelConfig(channel=Channel.TELEGRAM, enabled=True)
        assert not cfg.is_configured()

    def test_discord_configured(self):
        cfg = ChannelConfig(channel=Channel.DISCORD, enabled=True,
                            bot_token="xyz")
        assert cfg.is_configured()

    def test_wecom_configured(self):
        cfg = ChannelConfig(channel=Channel.WECOM, enabled=True,
                            app_id="ww_id", app_secret="ww_sec")
        assert cfg.is_configured()

    def test_dingtalk_configured(self):
        cfg = ChannelConfig(channel=Channel.DINGTALK, enabled=True,
                            app_id="dk_id", app_secret="dk_sec")
        assert cfg.is_configured()

    def test_slack_configured(self):
        cfg = ChannelConfig(channel=Channel.SLACK, enabled=True,
                            bot_token="xoxb-tok")
        assert cfg.is_configured()

    def test_mqtt_configured(self):
        cfg = ChannelConfig(channel=Channel.MQTT, enabled=True,
                            extra={"broker": "mqtt://localhost:1883"})
        assert cfg.is_configured()

    def test_mqtt_missing_broker(self):
        cfg = ChannelConfig(channel=Channel.MQTT, enabled=True)
        assert not cfg.is_configured()


# ---------------------------------------------------------------------------
# ChannelRegistry
# ---------------------------------------------------------------------------

class TestChannelRegistry:
    def test_register_and_list_enabled(self):
        reg = ChannelRegistry()
        reg.register(ChannelConfig(
            channel=Channel.TELEGRAM, enabled=True, bot_token="t"))
        reg.register(ChannelConfig(
            channel=Channel.FEISHU, enabled=False))
        assert reg.enabled_channels() == [Channel.TELEGRAM]

    def test_get_config(self):
        reg = ChannelRegistry()
        cfg = ChannelConfig(channel=Channel.DISCORD, enabled=True, bot_token="d")
        reg.register(cfg)
        assert reg.get_config(Channel.DISCORD) is cfg

    def test_get_config_missing(self):
        reg = ChannelRegistry()
        assert reg.get_config(Channel.TELEGRAM) is None

    def test_send_calls_sender(self):
        sent = []
        reg = ChannelRegistry()
        reg.register(
            ChannelConfig(channel=Channel.TELEGRAM, enabled=True, bot_token="t"),
            sender=lambda m: sent.append(m),
        )
        msg = OutboundMessage(channel=Channel.TELEGRAM,
                              recipient_id="u1", text="hi")
        reg.send(msg)
        assert len(sent) == 1
        assert sent[0].text == "hi"

    def test_send_without_sender_raises(self):
        reg = ChannelRegistry()
        reg.register(ChannelConfig(channel=Channel.TELEGRAM, enabled=True, bot_token="t"))
        msg = OutboundMessage(channel=Channel.TELEGRAM,
                              recipient_id="u1", text="hi")
        with pytest.raises(RuntimeError, match="No sender"):
            reg.send(msg)

    def test_summary(self):
        reg = ChannelRegistry()
        reg.register(ChannelConfig(
            channel=Channel.FEISHU, enabled=True,
            app_id="x", app_secret="y"))
        reg.register(ChannelConfig(
            channel=Channel.TELEGRAM, enabled=False))
        s = reg.summary()
        assert s["total"] == 2
        assert "feishu" in s["enabled"]
        assert "telegram" not in s["enabled"]


# ---------------------------------------------------------------------------
# load_channel_configs
# ---------------------------------------------------------------------------

class TestLoadChannelConfigs:
    def test_load_from_dict(self):
        data = {
            "channels": {
                "feishu": {
                    "enabled": True,
                    "app_id": "cli_xxx",
                    "app_secret": "sec123",
                },
                "telegram": {
                    "enabled": True,
                    "bot_token": "123:ABC",
                },
            }
        }
        configs = load_channel_configs(data)
        assert len(configs) == 2
        feishu = next(c for c in configs if c.channel == Channel.FEISHU)
        assert feishu.app_id == "cli_xxx"
        telegram = next(c for c in configs if c.channel == Channel.TELEGRAM)
        assert telegram.bot_token == "123:ABC"

    def test_unknown_channel_skipped(self):
        data = {"channels": {"unknown_im": {"enabled": True}}}
        configs = load_channel_configs(data)
        assert configs == []

    def test_empty_channels(self):
        data = {"channels": {}}
        assert load_channel_configs(data) == []

    def test_no_channels_key(self):
        assert load_channel_configs({}) == []

    def test_extra_fields_preserved(self):
        data = {
            "channels": {
                "mqtt": {
                    "enabled": True,
                    "broker": "mqtt://nas:1883",
                    "topic": "harborbeacon/cmd",
                }
            }
        }
        configs = load_channel_configs(data)
        assert configs[0].extra["broker"] == "mqtt://nas:1883"
        assert configs[0].extra["topic"] == "harborbeacon/cmd"


# ---------------------------------------------------------------------------
# ChannelRouter
# ---------------------------------------------------------------------------

class FakeMcpAdapter:
    """Minimal MCP adapter stub for channel router tests."""

    def __init__(self, result_text: str = '{"status":"SUCCESS"}', is_error: bool = False):
        self._result_text = result_text
        self._is_error = is_error
        self.last_call: tuple[str, dict] | None = None

    def call_tool(self, name, arguments=None):
        self.last_call = (name, arguments or {})

        class _Result:
            content = [{"type": "text", "text": self._result_text}]  # noqa: RUF012
            isError = self._is_error  # noqa: RUF012

        return _Result()


class TestChannelRouter:
    def _router(self, mcp=None, parser=None):
        ch_reg = ChannelRegistry()
        ch_reg.register(
            ChannelConfig(channel=Channel.TELEGRAM, enabled=True, bot_token="t"),
        )
        return ChannelRouter(
            channel_registry=ch_reg,
            mcp_adapter=mcp or FakeMcpAdapter(),
            intent_parser=parser,
        )

    def test_handle_returns_outbound(self):
        router = self._router()
        msg = InboundMessage(channel=Channel.TELEGRAM,
                             sender_id="u1", text="service.status")
        out = router.handle(msg)
        assert isinstance(out, OutboundMessage)
        assert out.channel == Channel.TELEGRAM
        assert out.recipient_id == "u1"

    def test_handle_calls_mcp_adapter(self):
        mcp = FakeMcpAdapter()
        router = self._router(mcp=mcp)
        msg = InboundMessage(channel=Channel.TELEGRAM,
                             sender_id="u1", text="service.status")
        router.handle(msg)
        assert mcp.last_call is not None
        assert mcp.last_call[0] == "service.status"

    def test_default_parser_tool_only(self):
        mcp = FakeMcpAdapter()
        router = self._router(mcp=mcp)
        msg = InboundMessage(channel=Channel.TELEGRAM,
                             sender_id="u1", text="service.status")
        router.handle(msg)
        assert mcp.last_call == ("service.status", {})

    def test_default_parser_with_json_args(self):
        mcp = FakeMcpAdapter()
        router = self._router(mcp=mcp)
        msg = InboundMessage(
            channel=Channel.TELEGRAM,
            sender_id="u1",
            text='service.status {"resource": {"service_name": "plex"}}',
        )
        router.handle(msg)
        assert mcp.last_call[1] == {"resource": {"service_name": "plex"}}

    def test_default_parser_with_plain_text_args(self):
        mcp = FakeMcpAdapter()
        router = self._router(mcp=mcp)
        msg = InboundMessage(
            channel=Channel.TELEGRAM,
            sender_id="u1",
            text="service.status check plex",
        )
        router.handle(msg)
        assert mcp.last_call[1] == {"text": "check plex"}

    def test_custom_intent_parser(self):
        mcp = FakeMcpAdapter()

        def my_parser(text):
            return "service.restart", {"resource": {"service_name": "emby"}}

        router = self._router(mcp=mcp, parser=my_parser)
        msg = InboundMessage(channel=Channel.TELEGRAM,
                             sender_id="u1", text="restart emby")
        router.handle(msg)
        assert mcp.last_call[0] == "service.restart"

    def test_handle_error_result(self):
        mcp = FakeMcpAdapter(result_text='{"error":"FAIL"}', is_error=True)
        router = self._router(mcp=mcp)
        msg = InboundMessage(channel=Channel.TELEGRAM,
                             sender_id="u1", text="service.start")
        out = router.handle(msg)
        assert out.payload["is_error"] is True

    def test_handle_dispatches_via_sender(self):
        sent = []
        ch_reg = ChannelRegistry()
        ch_reg.register(
            ChannelConfig(channel=Channel.FEISHU, enabled=True,
                          app_id="x", app_secret="y"),
            sender=lambda m: sent.append(m),
        )
        router = ChannelRouter(
            channel_registry=ch_reg,
            mcp_adapter=FakeMcpAdapter(),
        )
        msg = InboundMessage(channel=Channel.FEISHU,
                             sender_id="u1", text="service.status")
        router.handle(msg)
        assert len(sent) == 1
        assert sent[0].channel == Channel.FEISHU


# ---------------------------------------------------------------------------
# Channel enum completeness
# ---------------------------------------------------------------------------

class TestChannelEnum:
    def test_all_expected_channels_exist(self):
        expected = {"feishu", "wecom", "telegram", "discord",
                    "dingtalk", "slack", "mqtt"}
        actual = {ch.value for ch in Channel}
        assert expected == actual
