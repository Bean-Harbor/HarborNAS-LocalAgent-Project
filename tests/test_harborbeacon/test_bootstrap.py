"""Tests for harborbeacon.bootstrap."""
from __future__ import annotations

import json

from harborbeacon.bootstrap import build_harborbeacon_app
from harborbeacon.channels import Channel, ChannelConfig
from harborbeacon.task_api import TaskApiClient
from harborbeacon.webhook import WebhookRequest


def test_build_harborbeacon_app_wires_camera_domain_into_webhook_dispatch():
    sent_messages = []
    captured = {}

    def fake_request(url, payload, timeout_s):
        captured["url"] = url
        captured["payload"] = payload
        captured["timeout_s"] = timeout_s
        return 200, {
            "status": "completed",
            "result": {"message": "已进入 camera.scan 主链路。"},
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
        channel_senders={Channel.FEISHU: lambda msg: sent_messages.append(msg)},
        task_api_client=TaskApiClient(
            base_url="http://127.0.0.1:4175",
            request_fn=fake_request,
        ),
    )

    body = {
        "event": {
            "sender": {"sender_id": {"open_id": "ou_123"}},
            "message": {"content": json.dumps({"text": "扫描摄像头"})},
        }
    }
    response = app.webhook_receiver.handle(
        WebhookRequest(
            method="POST",
            path="/webhook/feishu",
            headers={},
            body=json.dumps(body).encode("utf-8"),
        )
    )

    assert response.status_code == 200
    assert app.registry.has_capability("camera.scan")
    assert len(sent_messages) == 1
    assert any(tool.name == "camera.scan" for tool in app.mcp_adapter.list_tools())
    assert "camera" == captured["payload"]["intent"]["domain"]
    assert "scan" == captured["payload"]["intent"]["action"]
    assert captured["payload"]["source"]["surface"] == "harborbeacon"


def test_build_harborbeacon_app_registers_webhook_and_long_connection_channels():
    app = build_harborbeacon_app(
        [
            ChannelConfig(
                channel=Channel.FEISHU,
                enabled=True,
                app_id="cli_test",
                app_secret="secret123",
            ),
            ChannelConfig(
                channel=Channel.TELEGRAM,
                enabled=True,
                bot_token="123456:ABC",
            ),
        ]
    )

    assert set(app.webhook_receiver.registered_channels) == {Channel.FEISHU, Channel.TELEGRAM}
    assert app.gateway.get_status(Channel.FEISHU) is not None
    assert app.gateway.get_status(Channel.TELEGRAM) is not None
