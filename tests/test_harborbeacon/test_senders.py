"""Tests for harborbeacon.senders."""
from __future__ import annotations

import json
from unittest.mock import MagicMock, patch

from harborbeacon.channels import Channel, ChannelConfig, OutboundMessage
from harborbeacon.senders import (
    FeishuMessageSender,
    TelegramMessageSender,
    build_channel_sender,
)


def _json_response(payload: dict) -> MagicMock:
    response = MagicMock()
    response.read.return_value = json.dumps(payload, ensure_ascii=False).encode("utf-8")
    response.__enter__ = MagicMock(return_value=response)
    response.__exit__ = MagicMock(return_value=False)
    return response


def test_feishu_sender_caches_token_and_infers_receive_id_type():
    sender = FeishuMessageSender(
        ChannelConfig(
            channel=Channel.FEISHU,
            enabled=True,
            app_id="cli_test",
            app_secret="secret123",
        )
    )
    first_send = _json_response({"code": 0, "data": {"message_id": "om_sent_1"}})
    second_send = _json_response({"code": 0, "data": {"message_id": "om_sent_2"}})

    with patch(
        "urllib.request.urlopen",
        side_effect=[
            _json_response(
                {
                    "code": 0,
                    "tenant_access_token": "tenant-token",
                    "expire": 7200,
                }
            ),
            first_send,
            second_send,
        ],
    ) as mock_urlopen:
        direct_message = OutboundMessage(
            channel=Channel.FEISHU,
            recipient_id="ou_user_1",
            text="hello",
        )
        sender(direct_message)

        group_message = OutboundMessage(
            channel=Channel.FEISHU,
            recipient_id="oc_group_1",
            text="hello group",
        )
        sender(group_message)

    assert direct_message.payload["sent_message_id"] == "om_sent_1"
    assert group_message.payload["sent_message_id"] == "om_sent_2"
    assert mock_urlopen.call_count == 3

    token_request = mock_urlopen.call_args_list[0].args[0]
    direct_request = mock_urlopen.call_args_list[1].args[0]
    group_request = mock_urlopen.call_args_list[2].args[0]

    assert token_request.full_url.endswith("/open-apis/auth/v3/tenant_access_token/internal")
    assert "receive_id_type=open_id" in direct_request.full_url
    assert "receive_id_type=chat_id" in group_request.full_url

    direct_body = json.loads(direct_request.data.decode("utf-8"))
    assert direct_body["receive_id"] == "ou_user_1"
    assert direct_body["msg_type"] == "text"
    assert json.loads(direct_body["content"]) == {"text": "hello"}


def test_feishu_sender_serializes_card_payload_as_interactive_content():
    sender = FeishuMessageSender(
        ChannelConfig(
            channel=Channel.FEISHU,
            enabled=True,
            app_id="cli_test",
            app_secret="secret123",
        )
    )
    card_payload = {
        "header": {
            "title": {"tag": "plain_text", "content": "HarborBeacon"},
            "template": "green",
        },
        "elements": [],
    }

    with patch(
        "urllib.request.urlopen",
        side_effect=[
            _json_response(
                {
                    "code": 0,
                    "tenant_access_token": "tenant-token",
                    "expire": 7200,
                }
            ),
            _json_response({"code": 0, "data": {"message_id": "om_card_1"}}),
        ],
    ) as mock_urlopen:
        msg = OutboundMessage(
            channel=Channel.FEISHU,
            recipient_id="ou_user_1",
            text="执行完成",
            payload={"card": card_payload},
        )
        sender(msg)

    request = mock_urlopen.call_args_list[1].args[0]
    body = json.loads(request.data.decode("utf-8"))
    assert body["msg_type"] == "interactive"
    assert json.loads(body["content"]) == card_payload
    assert msg.payload["sent_message_id"] == "om_card_1"


def test_feishu_sender_uses_reply_and_patch_endpoints():
    sender = FeishuMessageSender(
        ChannelConfig(
            channel=Channel.FEISHU,
            enabled=True,
            app_id="cli_test",
            app_secret="secret123",
        )
    )

    with patch(
        "urllib.request.urlopen",
        side_effect=[
            _json_response(
                {
                    "code": 0,
                    "tenant_access_token": "tenant-token",
                    "expire": 7200,
                }
            ),
            _json_response({"code": 0, "data": {"message_id": "om_reply_1"}}),
            _json_response({"code": 0, "data": {"message_id": "om_update_1"}}),
        ],
    ) as mock_urlopen:
        sender(
            OutboundMessage(
                channel=Channel.FEISHU,
                recipient_id="ou_user_1",
                text="reply",
                reply_to_message_id="om_source_1",
            )
        )
        sender(
            OutboundMessage(
                channel=Channel.FEISHU,
                recipient_id="ou_user_1",
                text="updated",
                update_message_id="om_source_2",
            )
        )

    reply_request = mock_urlopen.call_args_list[1].args[0]
    patch_request = mock_urlopen.call_args_list[2].args[0]
    assert reply_request.full_url.endswith("/open-apis/im/v1/messages/om_source_1/reply")
    assert reply_request.get_method() == "POST"
    assert patch_request.full_url.endswith("/open-apis/im/v1/messages/om_source_2")
    assert patch_request.get_method() == "PATCH"


def test_telegram_sender_sends_message_and_tracks_message_id():
    sender = TelegramMessageSender(
        ChannelConfig(
            channel=Channel.TELEGRAM,
            enabled=True,
            bot_token="123456:ABC",
        )
    )

    with patch(
        "urllib.request.urlopen",
        return_value=_json_response({"ok": True, "result": {"message_id": 42}}),
    ) as mock_urlopen:
        msg = OutboundMessage(
            channel=Channel.TELEGRAM,
            recipient_id="-1001",
            text="hello",
        )
        sender(msg)

    request = mock_urlopen.call_args.args[0]
    body = json.loads(request.data.decode("utf-8"))
    assert request.full_url == "https://api.telegram.org/bot123456:ABC/sendMessage"
    assert body == {
        "chat_id": "-1001",
        "text": "hello",
        "parse_mode": "Markdown",
    }
    assert msg.payload["sent_message_id"] == "42"


def test_telegram_sender_edits_existing_message():
    sender = TelegramMessageSender(
        ChannelConfig(
            channel=Channel.TELEGRAM,
            enabled=True,
            bot_token="123456:ABC",
        )
    )

    with patch(
        "urllib.request.urlopen",
        return_value=_json_response({"ok": True, "result": {"message_id": 77}}),
    ) as mock_urlopen:
        msg = OutboundMessage(
            channel=Channel.TELEGRAM,
            recipient_id="-1001",
            text="updated",
            update_message_id="77",
        )
        sender(msg)

    request = mock_urlopen.call_args.args[0]
    body = json.loads(request.data.decode("utf-8"))
    assert request.full_url == "https://api.telegram.org/bot123456:ABC/editMessageText"
    assert body["message_id"] == 77
    assert msg.payload["sent_message_id"] == "77"


def test_build_channel_sender_falls_back_to_logging_for_unsupported_channel(caplog):
    sender = build_channel_sender(
        ChannelConfig(
            channel=Channel.DISCORD,
            enabled=True,
            bot_token="discord-token",
        )
    )

    with caplog.at_level("INFO"):
        sender(
            OutboundMessage(
                channel=Channel.DISCORD,
                recipient_id="channel-1",
                text="hello discord",
            )
        )

    assert "Outbound discord reply to channel-1" in caplog.text
