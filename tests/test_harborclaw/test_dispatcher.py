"""Tests for harborclaw.dispatcher — Central dispatch chain."""
import json
import pytest

from orchestrator.contracts import Action, ExecutionResult, Route, StepStatus
from orchestrator.audit import AuditLog
from orchestrator.router import Router
from orchestrator.runtime import Runtime
from skills.manifest import SkillManifest, HarborApiConfig, HarborCliConfig, RiskConfig
from skills.registry import Registry

from harborclaw.autonomy import Autonomy
from harborclaw.channels import Channel, ChannelConfig, ChannelRegistry, InboundMessage, OutboundMessage
from harborclaw.dispatcher import Dispatcher, SessionEntry, SessionStore, _MUTATION_OPS
from harborclaw.formatter import ResponseFormatter
from harborclaw.intent import IntentParser
from harborclaw.mcp_adapter import McpServerAdapter


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------

def _make_manifest() -> SkillManifest:
    return SkillManifest(
        id="system.harbor_ops",
        name="HarborOS Service Operations",
        version="1.0.0",
        summary="Manage HarborOS services",
        owner="harbor-team",
        capabilities=["service.status", "service.start", "service.stop"],
        harbor_api=HarborApiConfig(enabled=True, allowed_methods=["query", "start", "stop"]),
        harbor_cli=HarborCliConfig(enabled=True, allowed_subcommands=["status", "start", "stop"]),
        risk=RiskConfig(default_level="LOW"),
    )


class FakeExecutor:
    def __init__(self, route=Route.MIDDLEWARE_API, payload="ok"):
        self._route = route
        self._payload = payload

    @property
    def route(self):
        return self._route

    def is_available(self):
        return True

    def execute(self, action, *, task_id, step_id):
        return ExecutionResult(
            task_id=task_id,
            step_id=step_id,
            executor_used=self._route.value,
            status=StepStatus.SUCCESS,
            result_payload={"out": self._payload, "service": action.resource.get("service_name", "")},
        )


def _build_stack():
    """Build the full dispatcher stack with fake executor."""
    reg = Registry()
    reg.register(_make_manifest())

    router = Router([FakeExecutor()])
    runtime = Runtime(router=router, audit=AuditLog())
    adapter = McpServerAdapter(reg, runtime, default_autonomy=Autonomy.SUPERVISED)
    parser = IntentParser()  # regex only
    channel_reg = ChannelRegistry()

    sent_messages: list[OutboundMessage] = []

    def fake_sender(msg: OutboundMessage):
        sent_messages.append(msg)

    channel_reg.register(
        ChannelConfig(channel=Channel.FEISHU, enabled=True, app_id="test", app_secret="s"),
        sender=fake_sender,
    )
    channel_reg.register(
        ChannelConfig(channel=Channel.TELEGRAM, enabled=True, bot_token="t"),
        sender=fake_sender,
    )

    dispatcher = Dispatcher(
        intent_parser=parser,
        mcp_adapter=adapter,
        channel_registry=channel_reg,
        default_autonomy=Autonomy.SUPERVISED,
    )

    return dispatcher, sent_messages


def _inbound(text: str, channel=Channel.FEISHU, sender="user1") -> InboundMessage:
    return InboundMessage(channel=channel, sender_id=sender, text=text)


# ---------------------------------------------------------------------------
# Session management
# ---------------------------------------------------------------------------

class TestSessionStore:
    def test_create_session(self):
        store = SessionStore()
        s = store.get_or_create(Channel.FEISHU, "u1")
        assert s.user_id == "u1"
        assert s.channel == Channel.FEISHU

    def test_reuse_session(self):
        store = SessionStore()
        s1 = store.get_or_create(Channel.FEISHU, "u1")
        s2 = store.get_or_create(Channel.FEISHU, "u1")
        assert s1.session_id == s2.session_id

    def test_different_users_different_sessions(self):
        store = SessionStore()
        s1 = store.get_or_create(Channel.FEISHU, "u1")
        s2 = store.get_or_create(Channel.FEISHU, "u2")
        assert s1.session_id != s2.session_id

    def test_expired_session_recreated(self):
        store = SessionStore(timeout=0)
        s1 = store.get_or_create(Channel.FEISHU, "u1")
        import time; time.sleep(0.01)
        s2 = store.get_or_create(Channel.FEISHU, "u1")
        assert s1.session_id != s2.session_id

    def test_active_count(self):
        store = SessionStore()
        store.get_or_create(Channel.FEISHU, "u1")
        store.get_or_create(Channel.FEISHU, "u2")
        assert store.active_count == 2

    def test_clear(self):
        store = SessionStore()
        store.get_or_create(Channel.FEISHU, "u1")
        store.clear(Channel.FEISHU, "u1")
        assert store.get(Channel.FEISHU, "u1") is None


class TestSessionEntry:
    def test_not_expired(self):
        s = SessionEntry()
        assert not s.is_expired(600)

    def test_expired(self):
        s = SessionEntry()
        s.last_active = 0  # epoch
        assert s.is_expired(1)


# ---------------------------------------------------------------------------
# Read-only operations (no approval needed)
# ---------------------------------------------------------------------------

class TestReadOnlyOps:
    def test_status_query_executes_directly(self):
        dispatcher, sent = _build_stack()
        dispatcher.handle(_inbound("查看 plex 状态"))
        assert len(sent) == 1
        # Should contain success result, not approval request
        assert "确认" not in sent[0].text

    def test_result_contains_payload(self):
        dispatcher, sent = _build_stack()
        dispatcher.handle(_inbound("查看 plex 状态"))
        assert len(sent) == 1
        # Check that plex appears somewhere in the reply (text or card payload)
        combined = sent[0].text + json.dumps(sent[0].payload, ensure_ascii=False)
        assert "plex" in combined.lower()


# ---------------------------------------------------------------------------
# Mutation operations (approval flow under Supervised)
# ---------------------------------------------------------------------------

class TestApprovalFlow:
    def test_stop_triggers_approval(self):
        dispatcher, sent = _build_stack()
        dispatcher.handle(_inbound("停止 plex"))
        assert len(sent) == 1
        assert "确认" in sent[0].text or "需要确认" in sent[0].text

    def test_confirm_executes(self):
        dispatcher, sent = _build_stack()
        dispatcher.handle(_inbound("停止 plex"))
        sent.clear()
        dispatcher.handle(_inbound("确认"))
        assert len(sent) == 1
        assert "确认" not in sent[0].text  # Should be result, not another prompt

    def test_cancel_aborts(self):
        dispatcher, sent = _build_stack()
        dispatcher.handle(_inbound("停止 plex"))
        sent.clear()
        dispatcher.handle(_inbound("取消"))
        assert len(sent) == 1
        assert "取消" in sent[0].text

    def test_new_command_replaces_pending(self):
        dispatcher, sent = _build_stack()
        dispatcher.handle(_inbound("停止 plex"))
        sent.clear()
        # Instead of confirming, send a new command
        dispatcher.handle(_inbound("查看 nginx 状态"))
        assert len(sent) == 1
        # Should execute the new status query, not the stop
        assert "确认" not in sent[0].text

    def test_start_triggers_approval(self):
        dispatcher, sent = _build_stack()
        dispatcher.handle(_inbound("启动 nginx"))
        assert len(sent) == 1
        assert "确认" in sent[0].text


# ---------------------------------------------------------------------------
# Error handling
# ---------------------------------------------------------------------------

class TestErrorHandling:
    def test_unparseable_intent(self):
        dispatcher, sent = _build_stack()
        dispatcher.handle(_inbound("今天天气怎么样"))
        assert len(sent) == 1
        assert "❌" in sent[0].text or "错误" in sent[0].text

    def test_no_sender_registered(self):
        """If channel has no sender, dispatcher logs warning but doesn't crash."""
        dispatcher, _ = _build_stack()
        msg = _inbound("查看 plex 状态", channel=Channel.MQTT)
        # MQTT is not registered in our test stack → RuntimeError in send
        # Dispatcher should catch and log
        dispatcher.handle(msg)  # should not raise


# ---------------------------------------------------------------------------
# Multi-channel
# ---------------------------------------------------------------------------

class TestMultiChannel:
    def test_different_channels_different_sessions(self):
        dispatcher, sent = _build_stack()
        dispatcher.handle(_inbound("查看 plex 状态", channel=Channel.FEISHU))
        dispatcher.handle(_inbound("查看 plex 状态", channel=Channel.TELEGRAM))
        assert len(sent) == 2
        assert sent[0].channel == Channel.FEISHU
        assert sent[1].channel == Channel.TELEGRAM


# ---------------------------------------------------------------------------
# Mutation ops set
# ---------------------------------------------------------------------------

class TestMutationOps:
    def test_stop_in_mutations(self):
        assert "stop" in _MUTATION_OPS

    def test_start_in_mutations(self):
        assert "start" in _MUTATION_OPS

    def test_status_not_in_mutations(self):
        assert "status" not in _MUTATION_OPS
