"""Tests for harborbeacon.webhook — Webhook receiver."""
import json
import pytest

from harborbeacon.channels import Channel, ChannelConfig
from harborbeacon.webhook import WebhookReceiver, WebhookRequest, WebhookResponse


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------

def _feishu_config(enabled=True) -> ChannelConfig:
    return ChannelConfig(
        channel=Channel.FEISHU,
        enabled=enabled,
        app_id="cli_test",
        app_secret="secret123",
    )


def _telegram_config(enabled=True) -> ChannelConfig:
    return ChannelConfig(
        channel=Channel.TELEGRAM,
        enabled=enabled,
        bot_token="bot_token_123",
    )


def _make_request(
    channel: str = "feishu",
    body: dict | None = None,
    method: str = "POST",
    headers: dict | None = None,
) -> WebhookRequest:
    raw = json.dumps(body or {}).encode("utf-8")
    return WebhookRequest(
        method=method,
        path=f"/webhook/{channel}",
        headers=headers or {},
        body=raw,
    )


# ---------------------------------------------------------------------------
# Basic routing
# ---------------------------------------------------------------------------

class TestRouting:
    def test_get_rejected(self):
        receiver = WebhookReceiver()
        resp = receiver.handle(_make_request(method="GET"))
        assert resp.status_code == 405

    def test_unknown_channel(self):
        receiver = WebhookReceiver()
        resp = receiver.handle(_make_request(channel="whatapp"))
        assert resp.status_code == 404

    def test_unregistered_channel(self):
        receiver = WebhookReceiver()
        resp = receiver.handle(_make_request(channel="feishu"))
        assert resp.status_code == 404

    def test_disabled_channel(self):
        receiver = WebhookReceiver()
        receiver.register_channel(Channel.FEISHU, _feishu_config(enabled=False))
        resp = receiver.handle(_make_request(channel="feishu"))
        assert resp.status_code == 403


# ---------------------------------------------------------------------------
# Feishu challenge
# ---------------------------------------------------------------------------

class TestFeishuChallenge:
    def test_challenge_response(self):
        receiver = WebhookReceiver()
        receiver.register_channel(Channel.FEISHU, _feishu_config())
        req = _make_request(body={
            "type": "url_verification",
            "challenge": "abc123xyz",
        })
        resp = receiver.handle(req)
        assert resp.status_code == 200
        assert resp.json_body["challenge"] == "abc123xyz"


# ---------------------------------------------------------------------------
# Message handling
# ---------------------------------------------------------------------------

class TestMessageHandling:
    def test_message_dispatched_to_handler(self):
        received = []
        receiver = WebhookReceiver()
        receiver.register_channel(
            Channel.FEISHU, _feishu_config(),
            on_message=lambda msg: received.append(msg),
        )
        body = {
            "event": {
                "sender": {"sender_id": {"open_id": "ou_123"}},
                "message": {"content": json.dumps({"text": "查看 plex"})},
            }
        }
        resp = receiver.handle(_make_request(body=body))
        assert resp.status_code == 200
        assert len(received) == 1
        assert received[0].sender_id == "ou_123"
        assert "plex" in received[0].text

    def test_default_handler(self):
        received = []
        receiver = WebhookReceiver()
        receiver.register_channel(Channel.TELEGRAM, _telegram_config())
        receiver.set_default_handler(lambda msg: received.append(msg))
        body = {"message": {"from": {"id": 42}, "text": "hello"}}
        resp = receiver.handle(_make_request(channel="telegram", body=body))
        assert resp.status_code == 200
        assert len(received) == 1

    def test_empty_message_returns_ok(self):
        receiver = WebhookReceiver()
        receiver.register_channel(Channel.FEISHU, _feishu_config())
        body = {
            "event": {
                "sender": {"sender_id": {"open_id": "ou_1"}},
                "message": {"content": json.dumps({"text": ""})},
            }
        }
        resp = receiver.handle(_make_request(body=body))
        assert resp.status_code == 200

    def test_handler_exception_returns_500(self):
        def bad_handler(msg):
            raise RuntimeError("boom")

        receiver = WebhookReceiver()
        receiver.register_channel(
            Channel.FEISHU, _feishu_config(),
            on_message=bad_handler,
        )
        body = {
            "event": {
                "sender": {"sender_id": {"open_id": "ou_1"}},
                "message": {"content": json.dumps({"text": "hello"})},
            }
        }
        resp = receiver.handle(_make_request(body=body))
        assert resp.status_code == 500

    def test_invalid_json_body(self):
        receiver = WebhookReceiver()
        receiver.register_channel(Channel.FEISHU, _feishu_config())
        req = WebhookRequest(
            method="POST",
            path="/webhook/feishu",
            headers={},
            body=b"not json",
        )
        resp = receiver.handle(req)
        assert resp.status_code == 400


# ---------------------------------------------------------------------------
# Slack challenge
# ---------------------------------------------------------------------------

class TestSlackChallenge:
    def test_slack_challenge(self):
        receiver = WebhookReceiver()
        receiver.register_channel(
            Channel.SLACK,
            ChannelConfig(channel=Channel.SLACK, enabled=True, bot_token="xoxb-test"),
        )
        req = _make_request(
            channel="slack",
            body={"type": "url_verification", "challenge": "slack_ch"},
        )
        resp = receiver.handle(req)
        assert resp.status_code == 200
        assert resp.json_body["challenge"] == "slack_ch"


# ---------------------------------------------------------------------------
# Multiple channels
# ---------------------------------------------------------------------------

class TestMultiChannel:
    def test_registered_channels(self):
        receiver = WebhookReceiver()
        receiver.register_channel(Channel.FEISHU, _feishu_config())
        receiver.register_channel(Channel.TELEGRAM, _telegram_config())
        assert set(receiver.registered_channels) == {Channel.FEISHU, Channel.TELEGRAM}

    def test_channel_specific_handler(self):
        feishu_msgs = []
        tg_msgs = []
        receiver = WebhookReceiver()
        receiver.register_channel(
            Channel.FEISHU, _feishu_config(),
            on_message=lambda m: feishu_msgs.append(m),
        )
        receiver.register_channel(
            Channel.TELEGRAM, _telegram_config(),
            on_message=lambda m: tg_msgs.append(m),
        )
        # Send to feishu
        body1 = {
            "event": {
                "sender": {"sender_id": {"open_id": "ou_1"}},
                "message": {"content": json.dumps({"text": "hello"})},
            }
        }
        receiver.handle(_make_request(channel="feishu", body=body1))
        # Send to telegram
        body2 = {"message": {"from": {"id": 1}, "text": "hi"}}
        receiver.handle(_make_request(channel="telegram", body=body2))

        assert len(feishu_msgs) == 1
        assert len(tg_msgs) == 1


# ---------------------------------------------------------------------------
# WebhookResponse
# ---------------------------------------------------------------------------

class TestWebhookResponse:
    def test_to_bytes_json(self):
        r = WebhookResponse(json_body={"ok": True})
        assert b'"ok": true' in r.to_bytes()

    def test_to_bytes_text(self):
        r = WebhookResponse(body="hello")
        assert r.to_bytes() == b"hello"
