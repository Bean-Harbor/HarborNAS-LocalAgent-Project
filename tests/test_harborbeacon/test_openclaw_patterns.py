"""Tests for OpenClaw-inspired E2E improvements.

Covers the features added after comparing HarborBeacon with OpenClaw:
  - Enhanced data models (Attachment, ChatType, etc.)
  - Message dedup
  - Group chat filtering
  - Thinking indicator
  - Rich media adapter parsing (Feishu post, image, file, audio)
  - Improved session key (group uses chat_id)
"""
import time

import pytest

from harborbeacon.channels import (
    Attachment,
    AttachmentType,
    Channel,
    ChannelConfig,
    ChannelRegistry,
    ChatType,
    InboundMessage,
    OutboundMessage,
)
from harborbeacon.adapters import (
    FeishuAdapter,
    TelegramAdapter,
    DiscordAdapter,
    DingTalkAdapter,
    SlackAdapter,
    _extract_post_text,
)
from harborbeacon.dispatcher import (
    Dispatcher,
    MessageDedup,
    should_respond_in_group,
)
from harborbeacon.autonomy import Autonomy


# ===================================================================
# AttachmentType & Attachment
# ===================================================================

class TestAttachmentType:
    def test_values(self):
        assert AttachmentType.IMAGE == "image"
        assert AttachmentType.VIDEO == "video"
        assert AttachmentType.AUDIO == "audio"
        assert AttachmentType.FILE == "file"


class TestAttachment:
    def test_construction(self):
        a = Attachment(type=AttachmentType.IMAGE, content="data:image/png;base64,...")
        assert a.type == AttachmentType.IMAGE
        assert a.mime_type == "application/octet-stream"
        assert a.file_name == ""

    def test_with_fields(self):
        a = Attachment(
            type=AttachmentType.FILE,
            content="/tmp/doc.pdf",
            mime_type="application/pdf",
            file_name="doc.pdf",
        )
        assert a.file_name == "doc.pdf"
        assert a.mime_type == "application/pdf"


# ===================================================================
# ChatType
# ===================================================================

class TestChatType:
    def test_values(self):
        assert ChatType.P2P == "p2p"
        assert ChatType.GROUP == "group"
        assert ChatType.UNKNOWN == "unknown"


# ===================================================================
# Enhanced InboundMessage
# ===================================================================

class TestInboundMessageEnhanced:
    def test_backwards_compatible(self):
        """Old code creating InboundMessage with positional args still works."""
        msg = InboundMessage(channel=Channel.FEISHU, sender_id="u1", text="hi")
        assert msg.message_id == ""
        assert msg.chat_type == ChatType.UNKNOWN
        assert msg.chat_id == ""
        assert msg.mentions == []
        assert msg.attachments == []

    def test_new_fields(self):
        msg = InboundMessage(
            channel=Channel.FEISHU,
            sender_id="u1",
            text="hello",
            message_id="msg_123",
            chat_type=ChatType.GROUP,
            chat_id="chat_456",
            mentions=["@bot"],
            attachments=[Attachment(type=AttachmentType.IMAGE, content="key123")],
        )
        assert msg.message_id == "msg_123"
        assert msg.chat_type == ChatType.GROUP
        assert msg.chat_id == "chat_456"
        assert len(msg.mentions) == 1
        assert len(msg.attachments) == 1


# ===================================================================
# Enhanced OutboundMessage
# ===================================================================

class TestOutboundMessageEnhanced:
    def test_backwards_compatible(self):
        msg = OutboundMessage(channel=Channel.FEISHU, recipient_id="u1", text="hi")
        assert msg.attachments == []
        assert msg.reply_to_message_id == ""
        assert msg.update_message_id == ""

    def test_with_attachments(self):
        msg = OutboundMessage(
            channel=Channel.FEISHU,
            recipient_id="u1",
            text="Here is the image",
            attachments=[Attachment(type=AttachmentType.IMAGE, content="/tmp/x.png")],
            update_message_id="msg_old",
        )
        assert len(msg.attachments) == 1
        assert msg.update_message_id == "msg_old"


# ===================================================================
# MessageDedup
# ===================================================================

class TestMessageDedup:
    def test_first_message_not_dup(self):
        d = MessageDedup()
        assert d.is_duplicate("msg_1") is False

    def test_same_message_is_dup(self):
        d = MessageDedup()
        d.is_duplicate("msg_1")
        assert d.is_duplicate("msg_1") is True

    def test_different_messages_not_dup(self):
        d = MessageDedup()
        d.is_duplicate("msg_1")
        assert d.is_duplicate("msg_2") is False

    def test_empty_id_never_dup(self):
        d = MessageDedup()
        assert d.is_duplicate("") is False
        assert d.is_duplicate("") is False  # Still not dup

    def test_expired_entries_evicted(self):
        d = MessageDedup(ttl_seconds=0)  # Immediate expiry
        d.is_duplicate("msg_1")
        time.sleep(0.01)
        assert d.is_duplicate("msg_1") is False  # Evicted

    def test_size(self):
        d = MessageDedup()
        d.is_duplicate("a")
        d.is_duplicate("b")
        d.is_duplicate("c")
        assert d.size == 3


# ===================================================================
# Group chat filtering
# ===================================================================

class TestShouldRespondInGroup:
    def test_with_mentions(self):
        assert should_respond_in_group("random text", ["@bot"]) is True

    def test_question_mark_zh(self):
        assert should_respond_in_group("服务状态怎么样？", []) is True

    def test_question_mark_en(self):
        assert should_respond_in_group("what is going on?", []) is True

    def test_english_question_word(self):
        assert should_respond_in_group("how to restart plex", []) is True

    def test_chinese_request_verb(self):
        assert should_respond_in_group("帮我查一下 plex 状态", []) is True
        assert should_respond_in_group("请重启 jellyfin", []) is True
        assert should_respond_in_group("分析一下网络问题", []) is True

    def test_casual_chat_ignored(self):
        assert should_respond_in_group("今天天气不错", []) is False
        assert should_respond_in_group("哈哈好的", []) is False
        assert should_respond_in_group("ok cool", []) is False

    def test_empty_mentions_and_text(self):
        assert should_respond_in_group("", []) is False


# ===================================================================
# Feishu adapter — enhanced parsing
# ===================================================================

class TestFeishuAdapterEnhanced:
    def _adapter(self):
        return FeishuAdapter()

    def test_text_message_with_metadata(self):
        raw = {
            "event": {
                "sender": {"sender_id": {"open_id": "ou_xxx"}},
                "message": {
                    "message_id": "om_abc",
                    "chat_id": "oc_123",
                    "chat_type": "group",
                    "message_type": "text",
                    "content": '{"text": "查看 plex 状态"}',
                    "mentions": [{"key": "@_user_1"}],
                },
            }
        }
        msg = self._adapter().parse_inbound(raw)
        assert msg.message_id == "om_abc"
        assert msg.chat_type == ChatType.GROUP
        assert msg.chat_id == "oc_123"
        assert msg.mentions == ["@_user_1"]
        assert "plex" in msg.text

    def test_p2p_chat_type(self):
        raw = {
            "event": {
                "sender": {"sender_id": {"open_id": "ou_xxx"}},
                "message": {
                    "message_id": "om_1",
                    "chat_id": "oc_1",
                    "chat_type": "p2p",
                    "message_type": "text",
                    "content": '{"text": "hi"}',
                },
            }
        }
        msg = self._adapter().parse_inbound(raw)
        assert msg.chat_type == ChatType.P2P

    def test_image_message(self):
        raw = {
            "event": {
                "sender": {"sender_id": {"open_id": "ou_xxx"}},
                "message": {
                    "message_id": "om_img",
                    "chat_id": "oc_1",
                    "chat_type": "p2p",
                    "message_type": "image",
                    "content": '{"image_key": "img_abc123"}',
                },
            }
        }
        msg = self._adapter().parse_inbound(raw)
        assert msg.text == "[图片]"
        assert len(msg.attachments) == 1
        assert msg.attachments[0].type == AttachmentType.IMAGE
        assert msg.attachments[0].content == "img_abc123"

    def test_file_message(self):
        raw = {
            "event": {
                "sender": {"sender_id": {"open_id": "ou_xxx"}},
                "message": {
                    "message_id": "om_file",
                    "chat_id": "oc_1",
                    "chat_type": "p2p",
                    "message_type": "file",
                    "content": '{"file_key": "file_abc", "file_name": "report.pdf"}',
                },
            }
        }
        msg = self._adapter().parse_inbound(raw)
        assert "[文件]" in msg.text
        assert "report.pdf" in msg.text
        assert len(msg.attachments) == 1
        assert msg.attachments[0].type == AttachmentType.FILE
        assert msg.attachments[0].file_name == "report.pdf"

    def test_video_message(self):
        raw = {
            "event": {
                "sender": {"sender_id": {"open_id": "ou_xxx"}},
                "message": {
                    "message_id": "om_vid",
                    "chat_id": "oc_1",
                    "chat_type": "p2p",
                    "message_type": "media",
                    "content": '{"file_key": "vid_abc", "file_name": "demo.mp4"}',
                },
            }
        }
        msg = self._adapter().parse_inbound(raw)
        assert "[视频]" in msg.text
        assert len(msg.attachments) == 1
        assert msg.attachments[0].type == AttachmentType.VIDEO

    def test_audio_message(self):
        raw = {
            "event": {
                "sender": {"sender_id": {"open_id": "ou_xxx"}},
                "message": {
                    "message_id": "om_aud",
                    "chat_id": "oc_1",
                    "chat_type": "p2p",
                    "message_type": "audio",
                    "content": '{"file_key": "aud_abc", "file_name": "voice.opus"}',
                },
            }
        }
        msg = self._adapter().parse_inbound(raw)
        assert "[语音]" in msg.text
        assert len(msg.attachments) == 1
        assert msg.attachments[0].type == AttachmentType.AUDIO

    def test_post_message_extracts_text_and_images(self):
        raw = {
            "event": {
                "sender": {"sender_id": {"open_id": "ou_xxx"}},
                "message": {
                    "message_id": "om_post",
                    "chat_id": "oc_1",
                    "chat_type": "p2p",
                    "message_type": "post",
                    "content": '{"title": "报告标题", "content": [[{"tag": "text", "text": "第一段"}, {"tag": "img", "image_key": "img_k1"}], [{"tag": "text", "text": "第二段"}]]}',
                },
            }
        }
        msg = self._adapter().parse_inbound(raw)
        assert "报告标题" in msg.text
        assert "第一段" in msg.text
        assert "第二段" in msg.text
        assert len(msg.attachments) == 1
        assert msg.attachments[0].content == "img_k1"


# ===================================================================
# _extract_post_text utility
# ===================================================================

class TestExtractPostText:
    def test_simple_post(self):
        post = {
            "title": "Hello",
            "content": [
                [{"tag": "text", "text": "World"}],
            ],
        }
        text, imgs = _extract_post_text(post)
        assert "Hello" in text
        assert "World" in text
        assert imgs == []

    def test_post_with_image(self):
        post = {
            "content": [
                [
                    {"tag": "text", "text": "Look:"},
                    {"tag": "img", "image_key": "img_xyz"},
                ],
            ],
        }
        text, imgs = _extract_post_text(post)
        assert "Look:" in text
        assert imgs == ["img_xyz"]

    def test_post_with_link(self):
        post = {
            "content": [
                [{"tag": "a", "text": "click here", "href": "https://example.com"}],
            ],
        }
        text, imgs = _extract_post_text(post)
        assert "click here" in text

    def test_empty_post(self):
        text, imgs = _extract_post_text({})
        assert text == ""
        assert imgs == []


# ===================================================================
# Telegram adapter — enhanced parsing
# ===================================================================

class TestTelegramAdapterEnhanced:
    def _adapter(self):
        return TelegramAdapter()

    def test_message_with_chat_metadata(self):
        raw = {
            "message": {
                "message_id": 42,
                "from": {"id": 123},
                "chat": {"id": -100, "type": "supergroup"},
                "text": "hello bot?",
                "entities": [{"type": "mention", "offset": 0, "length": 5}],
            }
        }
        msg = self._adapter().parse_inbound(raw)
        assert msg.message_id == "42"
        assert msg.chat_type == ChatType.GROUP
        assert msg.chat_id == "-100"
        assert len(msg.mentions) == 1

    def test_private_chat(self):
        raw = {
            "message": {
                "message_id": 1,
                "from": {"id": 99},
                "chat": {"id": 99, "type": "private"},
                "text": "hi",
            }
        }
        msg = self._adapter().parse_inbound(raw)
        assert msg.chat_type == ChatType.P2P

    def test_photo_attachment(self):
        raw = {
            "message": {
                "message_id": 2,
                "from": {"id": 99},
                "chat": {"id": 99, "type": "private"},
                "text": "",
                "photo": [
                    {"file_id": "small", "width": 100, "height": 100},
                    {"file_id": "large", "width": 800, "height": 600},
                ],
            }
        }
        msg = self._adapter().parse_inbound(raw)
        assert len(msg.attachments) == 1
        assert msg.attachments[0].content == "large"  # largest picked

    def test_document_attachment(self):
        raw = {
            "message": {
                "message_id": 3,
                "from": {"id": 99},
                "chat": {"id": 99, "type": "private"},
                "text": "",
                "document": {
                    "file_id": "doc123",
                    "file_name": "report.pdf",
                    "mime_type": "application/pdf",
                },
            }
        }
        msg = self._adapter().parse_inbound(raw)
        assert len(msg.attachments) == 1
        assert msg.attachments[0].type == AttachmentType.FILE
        assert msg.attachments[0].file_name == "report.pdf"


# ===================================================================
# Discord adapter — enhanced parsing
# ===================================================================

class TestDiscordAdapterEnhanced:
    def _adapter(self):
        return DiscordAdapter()

    def test_guild_message(self):
        raw = {
            "id": "msg_d1",
            "channel_id": "ch_1",
            "guild_id": "guild_1",
            "author": {"id": "u1"},
            "content": "hello",
            "mentions": [{"id": "bot_id"}],
        }
        msg = self._adapter().parse_inbound(raw)
        assert msg.message_id == "msg_d1"
        assert msg.chat_type == ChatType.GROUP
        assert msg.chat_id == "ch_1"
        assert msg.mentions == ["bot_id"]

    def test_dm_message(self):
        raw = {
            "id": "msg_d2",
            "channel_id": "dm_1",
            "author": {"id": "u1"},
            "content": "hi",
        }
        msg = self._adapter().parse_inbound(raw)
        assert msg.chat_type == ChatType.P2P


# ===================================================================
# DingTalk adapter — enhanced parsing
# ===================================================================

class TestDingTalkAdapterEnhanced:
    def _adapter(self):
        return DingTalkAdapter()

    def test_group_message(self):
        raw = {
            "msgId": "dt_msg_1",
            "senderStaffId": "staff_1",
            "conversationId": "conv_1",
            "conversationType": "2",
            "text": {"content": "查看服务"},
            "atUsers": [{"dingtalkId": "bot_dt"}],
        }
        msg = self._adapter().parse_inbound(raw)
        assert msg.message_id == "dt_msg_1"
        assert msg.chat_type == ChatType.GROUP
        assert msg.chat_id == "conv_1"
        assert msg.mentions == ["bot_dt"]

    def test_p2p_message(self):
        raw = {
            "senderStaffId": "staff_1",
            "conversationType": "1",
            "text": {"content": "hello"},
        }
        msg = self._adapter().parse_inbound(raw)
        assert msg.chat_type == ChatType.P2P


# ===================================================================
# Slack adapter — enhanced parsing
# ===================================================================

class TestSlackAdapterEnhanced:
    def _adapter(self):
        return SlackAdapter()

    def test_channel_message(self):
        raw = {
            "event": {
                "user": "U123",
                "text": "check status",
                "ts": "1234567890.123456",
                "channel": "C_GEN",
                "channel_type": "channel",
            }
        }
        msg = self._adapter().parse_inbound(raw)
        assert msg.message_id == "1234567890.123456"
        assert msg.chat_type == ChatType.GROUP
        assert msg.chat_id == "C_GEN"

    def test_im_message(self):
        raw = {
            "event": {
                "user": "U456",
                "text": "hi",
                "ts": "111.222",
                "channel": "D_DM",
                "channel_type": "im",
            }
        }
        msg = self._adapter().parse_inbound(raw)
        assert msg.chat_type == ChatType.P2P


# ===================================================================
# Dispatcher — dedup integration
# ===================================================================

class TestDispatcherDedup:
    """Verify the Dispatcher skips duplicate messages."""

    def _make_dispatcher(self):
        from harborbeacon.intent import IntentParser
        from harborbeacon.mcp_adapter import McpServerAdapter
        from skills.manifest import SkillManifest, HarborApiConfig, HarborCliConfig, RiskConfig
        from skills.registry import Registry
        from orchestrator.audit import AuditLog
        from orchestrator.router import Router
        from orchestrator.runtime import Runtime
        from orchestrator.contracts import Route, ExecutionResult, StepStatus

        manifest = SkillManifest(
            id="test.ops", name="Test", version="1.0.0",
            summary="test", owner="test",
            capabilities=["service.status"],
            harbor_api=HarborApiConfig(enabled=True, allowed_methods=["query"]),
            harbor_cli=HarborCliConfig(enabled=True, allowed_subcommands=["status"]),
            risk=RiskConfig(default_level="LOW"),
        )

        class FakeExec:
            @property
            def route(self):
                return Route.MIDDLEWARE_API
            def is_available(self):
                return True
            def execute(self, action, *, task_id, step_id):
                return ExecutionResult(
                    task_id=task_id, step_id=step_id,
                    executor_used="api", status=StepStatus.SUCCESS,
                    result_payload={"ok": True},
                )

        registry = Registry()
        registry.register(manifest)
        audit = AuditLog()
        router = Router([FakeExec()])
        runtime = Runtime(router=router, audit=audit)
        adapter = McpServerAdapter(registry, runtime, default_autonomy=Autonomy.SUPERVISED)
        parser = IntentParser()

        sent: list[OutboundMessage] = []
        ch_reg = ChannelRegistry()
        ch_reg.register(
            ChannelConfig(channel=Channel.FEISHU, enabled=True,
                          app_id="id", app_secret="sec"),
            sender=lambda msg: sent.append(msg),
        )
        dispatcher = Dispatcher(
            intent_parser=parser, mcp_adapter=adapter,
            channel_registry=ch_reg, default_autonomy=Autonomy.SUPERVISED,
        )
        return dispatcher, sent

    def test_duplicate_message_skipped(self):
        dispatcher, sent = self._make_dispatcher()
        msg = InboundMessage(
            channel=Channel.FEISHU, sender_id="u1",
            text="查看 plex 状态", message_id="om_dup_1",
        )
        dispatcher.handle(msg)
        first_count = len(sent)
        assert first_count >= 1

        # Same message_id again
        dispatcher.handle(msg)
        assert len(sent) == first_count  # No additional reply

    def test_different_message_id_not_skipped(self):
        dispatcher, sent = self._make_dispatcher()
        msg1 = InboundMessage(
            channel=Channel.FEISHU, sender_id="u1",
            text="查看 plex 状态", message_id="om_a",
        )
        msg2 = InboundMessage(
            channel=Channel.FEISHU, sender_id="u1",
            text="查看 plex 状态", message_id="om_b",
        )
        dispatcher.handle(msg1)
        count1 = len(sent)
        dispatcher.handle(msg2)
        assert len(sent) > count1


# ===================================================================
# Dispatcher — group chat filtering integration
# ===================================================================

class TestDispatcherGroupChat:
    def _make_dispatcher(self):
        # Reuse from above
        return TestDispatcherDedup()._make_dispatcher()

    def test_group_casual_chat_ignored(self):
        dispatcher, sent = self._make_dispatcher()
        msg = InboundMessage(
            channel=Channel.FEISHU, sender_id="u1",
            text="今天天气不错", message_id="grp_1",
            chat_type=ChatType.GROUP, chat_id="oc_grp",
        )
        dispatcher.handle(msg)
        assert len(sent) == 0  # Casual chat → no response

    def test_group_mentioned_responds(self):
        dispatcher, sent = self._make_dispatcher()
        msg = InboundMessage(
            channel=Channel.FEISHU, sender_id="u1",
            text="查看 plex 状态", message_id="grp_2",
            chat_type=ChatType.GROUP, chat_id="oc_grp",
            mentions=["@bot"],
        )
        dispatcher.handle(msg)
        assert len(sent) >= 1  # Has mention → responds

    def test_group_question_responds(self):
        dispatcher, sent = self._make_dispatcher()
        msg = InboundMessage(
            channel=Channel.FEISHU, sender_id="u1",
            text="plex 是什么状态？", message_id="grp_3",
            chat_type=ChatType.GROUP, chat_id="oc_grp",
        )
        dispatcher.handle(msg)
        assert len(sent) >= 1  # Question → responds

    def test_p2p_always_responds(self):
        dispatcher, sent = self._make_dispatcher()
        msg = InboundMessage(
            channel=Channel.FEISHU, sender_id="u1",
            text="今天天气不错", message_id="p2p_1",
            chat_type=ChatType.P2P,
        )
        dispatcher.handle(msg)
        assert len(sent) >= 1  # P2P → always responds


# ===================================================================
# Dispatcher — session key model
# ===================================================================

class TestDispatcherSessionKey:
    def _make_dispatcher(self):
        return TestDispatcherDedup()._make_dispatcher()

    def test_group_session_uses_chat_id(self):
        dispatcher, sent = self._make_dispatcher()
        # Two different users in same group should share session
        msg1 = InboundMessage(
            channel=Channel.FEISHU, sender_id="u1",
            text="帮我查看 plex", message_id="sk_1",
            chat_type=ChatType.GROUP, chat_id="oc_shared",
            mentions=["@bot"],
        )
        msg2 = InboundMessage(
            channel=Channel.FEISHU, sender_id="u2",
            text="帮我查看 jellyfin", message_id="sk_2",
            chat_type=ChatType.GROUP, chat_id="oc_shared",
            mentions=["@bot"],
        )
        dispatcher.handle(msg1)
        dispatcher.handle(msg2)
        # Both use "oc_shared" as session key
        session = dispatcher.sessions.get(Channel.FEISHU, "oc_shared")
        assert session is not None

    def test_p2p_session_uses_sender_id(self):
        dispatcher, sent = self._make_dispatcher()
        msg = InboundMessage(
            channel=Channel.FEISHU, sender_id="u_private",
            text="帮我查看 plex", message_id="sk_3",
            chat_type=ChatType.P2P,
        )
        dispatcher.handle(msg)
        session = dispatcher.sessions.get(Channel.FEISHU, "u_private")
        assert session is not None


# ===================================================================
# Dispatcher — thinking indicator
# ===================================================================

class TestDispatcherThinkingIndicator:
    def _make_dispatcher(self):
        from harborbeacon.intent import IntentParser
        from harborbeacon.mcp_adapter import McpServerAdapter
        from skills.manifest import SkillManifest, HarborApiConfig, HarborCliConfig, RiskConfig
        from skills.registry import Registry
        from orchestrator.audit import AuditLog
        from orchestrator.router import Router
        from orchestrator.runtime import Runtime
        from orchestrator.contracts import Route

        manifest = SkillManifest(
            id="test.ops", name="Test", version="1.0.0",
            summary="test", owner="test", capabilities=["service.status"],
            harbor_api=HarborApiConfig(enabled=True, allowed_methods=["query"]),
            harbor_cli=HarborCliConfig(enabled=True, allowed_subcommands=["status"]),
            risk=RiskConfig(default_level="LOW"),
        )
        registry = Registry()
        registry.register(manifest)
        audit = AuditLog()
        router = Router([])
        runtime = Runtime(router=router, audit=audit)
        adapter = McpServerAdapter(registry, runtime, default_autonomy=Autonomy.SUPERVISED)
        parser = IntentParser()

        sent = []
        ch_reg = ChannelRegistry()
        ch_reg.register(
            ChannelConfig(channel=Channel.FEISHU, enabled=True,
                          app_id="id", app_secret="sec"),
            sender=lambda msg: sent.append(msg),
        )
        dispatcher = Dispatcher(
            intent_parser=parser, mcp_adapter=adapter,
            channel_registry=ch_reg, thinking_threshold_ms=1000,
            default_autonomy=Autonomy.SUPERVISED,
        )
        return dispatcher, sent

    def test_thinking_method_exists(self):
        """Verify the _send_thinking_placeholder method exists."""
        dispatcher, sent = self._make_dispatcher()

        assert hasattr(dispatcher, "_send_thinking_placeholder")
        msg = InboundMessage(
            channel=Channel.FEISHU, sender_id="u1", text="hi",
        )
        result = dispatcher._send_thinking_placeholder(msg)
        # It sends "正在思考…" via the channel
        assert len(sent) == 1
        assert "正在思考" in sent[0].text


# ===================================================================
# Dispatcher — reply with recipient_id from chat_id
# ===================================================================

class TestDispatcherReplyTarget:
    def test_reply_uses_chat_id_when_available(self):
        dispatcher, sent = TestDispatcherDedup()._make_dispatcher()

        # Group message with chat_id — reply should go to chat_id
        msg = InboundMessage(
            channel=Channel.FEISHU, sender_id="u1",
            text="查看 plex 状态", message_id="rt_1",
            chat_type=ChatType.GROUP, chat_id="oc_group_chat",
            mentions=["@bot"],
        )
        dispatcher.handle(msg)
        assert len(sent) >= 1
        assert sent[0].recipient_id == "oc_group_chat"
