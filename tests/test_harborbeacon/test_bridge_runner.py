"""Tests for harborbeacon.bridge_runner."""
from __future__ import annotations

import json

from harborbeacon.bootstrap import build_harborbeacon_app
from harborbeacon.bridge_runner import (
    build_logging_senders,
    dispatch_webhook_request,
    load_channel_configs_from_admin_state,
    load_channel_configs_from_env,
    load_channel_configs_from_file,
    resolve_channel_configs,
    resolve_runtime_modes,
)
from harborbeacon.channels import Channel, ChannelConfig, OutboundMessage
from harborbeacon.task_api import TaskApiClient


def test_load_channel_configs_from_file_reads_yaml(tmp_path):
    config_path = tmp_path / "channels.yaml"
    config_path.write_text(
        """
channels:
  feishu:
    enabled: true
    app_id: "cli_test"
    app_secret: "secret123"
    transport_mode: "websocket"
  telegram:
    enabled: true
    bot_token: "123456:ABC"
    transport_mode: "long_poll"
        """.strip(),
        encoding="utf-8",
    )

    configs = load_channel_configs_from_file(config_path)

    assert [config.channel for config in configs] == [Channel.FEISHU, Channel.TELEGRAM]
    assert configs[0].transport_mode == "websocket"
    assert configs[1].bot_token == "123456:ABC"


def test_load_channel_configs_from_env_supports_json_blob(monkeypatch):
    monkeypatch.setenv(
        "HARBORBEACON_CHANNELS_JSON",
        json.dumps(
            {
                "channels": {
                    "feishu": {
                        "enabled": True,
                        "app_id": "cli_test",
                        "app_secret": "secret123",
                    }
                }
            }
        ),
    )

    configs = load_channel_configs_from_env()

    assert len(configs) == 1
    assert configs[0].channel == Channel.FEISHU
    assert configs[0].is_configured()


def test_load_channel_configs_from_admin_state_reads_saved_bridge_provider(tmp_path):
    state_path = tmp_path / "admin-console.json"
    state_path.write_text(
        json.dumps(
            {
                "bridge_provider": {
                    "configured": True,
                    "app_id": "cli_saved",
                    "app_secret": "saved-secret",
                    "app_name": "HarborBeacon Bot",
                    "bot_open_id": "ou_bot",
                }
            },
            ensure_ascii=False,
        ),
        encoding="utf-8",
    )

    configs = load_channel_configs_from_admin_state(state_path)

    assert len(configs) == 1
    assert configs[0].channel == Channel.FEISHU
    assert configs[0].app_id == "cli_saved"
    assert configs[0].transport_mode == "websocket"
    assert configs[0].extra["receive_id_type"] == "open_id"
    assert configs[0].extra["app_name"] == "HarborBeacon Bot"


def test_resolve_channel_configs_prefers_yaml_then_admin_state_then_env(tmp_path, monkeypatch):
    monkeypatch.setenv("FEISHU_APP_ID", "cli_env")
    monkeypatch.setenv("FEISHU_APP_SECRET", "env-secret")
    monkeypatch.setenv("TELEGRAM_BOT_TOKEN", "123456:ENV")

    admin_state_path = tmp_path / "admin-console.json"
    admin_state_path.write_text(
        json.dumps(
            {
                "bridge_provider": {
                    "configured": True,
                    "app_id": "cli_saved",
                    "app_secret": "saved-secret",
                }
            }
        ),
        encoding="utf-8",
    )

    config_path = tmp_path / "channels.yaml"
    config_path.write_text(
        """
channels:
  feishu:
    enabled: true
    app_id: "cli_yaml"
    app_secret: "yaml-secret"
  telegram:
    enabled: true
    bot_token: "123456:YAML"
        """.strip(),
        encoding="utf-8",
    )

    configs = resolve_channel_configs(
        config_path=config_path,
        admin_state_path=admin_state_path,
    )

    assert len(configs) == 2
    feishu = next(config for config in configs if config.channel == Channel.FEISHU)
    telegram = next(config for config in configs if config.channel == Channel.TELEGRAM)
    assert feishu.app_id == "cli_yaml"
    assert feishu.app_secret == "yaml-secret"
    assert telegram.bot_token == "123456:YAML"


def test_resolve_channel_configs_uses_admin_state_when_yaml_missing(tmp_path, monkeypatch):
    monkeypatch.delenv("FEISHU_APP_ID", raising=False)
    monkeypatch.delenv("FEISHU_APP_SECRET", raising=False)
    monkeypatch.delenv("TELEGRAM_BOT_TOKEN", raising=False)

    admin_state_path = tmp_path / "admin-console.json"
    admin_state_path.write_text(
        json.dumps(
            {
                "bridge_provider": {
                    "configured": True,
                    "app_id": "cli_saved",
                    "app_secret": "saved-secret",
                }
            }
        ),
        encoding="utf-8",
    )

    configs = resolve_channel_configs(
        config_path=tmp_path / "missing.yaml",
        admin_state_path=admin_state_path,
    )

    assert len(configs) == 1
    assert configs[0].channel == Channel.FEISHU
    assert configs[0].app_id == "cli_saved"


def test_build_logging_senders_serializes_platform_payload(caplog):
    configs = [
        ChannelConfig(
            channel=Channel.TELEGRAM,
            enabled=True,
            bot_token="123456:ABC",
        )
    ]
    sender = build_logging_senders(configs)[Channel.TELEGRAM]

    with caplog.at_level("INFO"):
        sender(
            OutboundMessage(
                channel=Channel.TELEGRAM,
                recipient_id="42",
                text="hello",
            )
        )

    assert "Outbound telegram reply to 42" in caplog.text
    assert '"chat_id": "42"' in caplog.text


def test_resolve_runtime_modes_auto_splits_webhook_and_gateway():
    configs = [
        ChannelConfig(
            channel=Channel.FEISHU,
            enabled=True,
            app_id="cli_test",
            app_secret="secret123",
            transport_mode="websocket",
        ),
        ChannelConfig(
            channel=Channel.SLACK,
            enabled=True,
            bot_token="xoxb-test",
            transport_mode="webhook",
        ),
    ]

    enable_webhook, enable_gateway = resolve_runtime_modes(configs, "auto")

    assert enable_webhook is True
    assert enable_gateway is True


def test_dispatch_webhook_request_routes_to_camera_task_api():
    captured = {}

    def fake_request(url, payload, timeout_s):
        captured["payload"] = payload
        return 200, {
            "status": "completed",
            "result": {"message": "camera.scan ok"},
        }

    app = build_harborbeacon_app(
        [
            ChannelConfig(
                channel=Channel.FEISHU,
                enabled=True,
                app_id="cli_test",
                app_secret="secret123",
            )
        ],
        channel_senders=build_logging_senders(
            [
                ChannelConfig(
                    channel=Channel.FEISHU,
                    enabled=True,
                    app_id="cli_test",
                    app_secret="secret123",
                )
            ]
        ),
        task_api_client=TaskApiClient(
            base_url="http://127.0.0.1:4175",
            request_fn=fake_request,
        ),
    )

    body = json.dumps(
        {
            "event": {
                "sender": {"sender_id": {"open_id": "ou_123"}},
                "message": {"content": json.dumps({"text": "扫描摄像头"})},
            }
        }
    ).encode("utf-8")
    status_code, headers, response_body = dispatch_webhook_request(
        app,
        "POST",
        "/webhook/feishu",
        {},
        body,
    )

    assert status_code == 200
    assert isinstance(headers, dict)
    assert response_body == b"ok"
    assert captured["payload"]["intent"]["domain"] == "camera"
    assert captured["payload"]["intent"]["action"] == "scan"
