"""Tests for harborbeacon.dispatcher — Central dispatch chain."""
import json
from pathlib import Path
from unittest.mock import MagicMock, patch
import pytest

from orchestrator.contracts import Action, ExecutionResult, Route, StepStatus
from orchestrator.audit import AuditLog
from orchestrator.router import Router
from orchestrator.runtime import Runtime
from skills.executor import TaskApiExecutor
from skills.manifest import SkillManifest, HarborApiConfig, HarborCliConfig, RiskConfig
from skills.registry import Registry

from harborbeacon.autonomy import Autonomy
from harborbeacon.attachments import AttachmentResolver, ResolvedAttachment
from harborbeacon.camera_domain import build_camera_domain_manifest
from harborbeacon.channels import Attachment, AttachmentType, Channel, ChannelConfig, ChannelRegistry, InboundMessage, OutboundMessage
from harborbeacon.dispatcher import Dispatcher, SessionEntry, SessionStore, _MUTATION_OPS
from harborbeacon.formatter import ResponseFormatter
from harborbeacon.intent import IntentParser
from harborbeacon.mcp_adapter import McpServerAdapter


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
    def __init__(self, route=Route.MIDDLEWARE_API, payload="ok", supported_domains=None):
        self._route = route
        self._payload = payload
        self._supported_domains = set(supported_domains or {"service"})

    @property
    def route(self):
        return self._route

    def is_available(self):
        return True

    def supports(self, action):
        return action.domain in self._supported_domains

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


def _build_extended_stack(*, attachment_resolver: AttachmentResolver | None = None):
    reg = Registry()
    reg.register(_make_manifest())
    builtins_dir = Path(__file__).resolve().parents[2] / "skills" / "builtins"
    reg.load_dir(builtins_dir)

    router = Router([FakeExecutor(supported_domains={"service", "files"})])
    runtime = Runtime(router=router, audit=AuditLog())
    adapter = McpServerAdapter(reg, runtime, default_autonomy=Autonomy.SUPERVISED)
    parser = IntentParser()
    channel_reg = ChannelRegistry()

    sent_messages: list[OutboundMessage] = []

    def fake_sender(msg: OutboundMessage):
        sent_messages.append(msg)

    channel_reg.register(
        ChannelConfig(channel=Channel.FEISHU, enabled=True, app_id="test", app_secret="s"),
        sender=fake_sender,
    )

    dispatcher = Dispatcher(
        intent_parser=parser,
        mcp_adapter=adapter,
        channel_registry=channel_reg,
        attachment_resolver=attachment_resolver,
        default_autonomy=Autonomy.SUPERVISED,
    )
    return dispatcher, sent_messages


class FakeTaskApi:
    def __init__(self):
        self.calls: list[Action] = []

    def execute_action(self, action: Action, task_id: str, step_id: str):
        self.calls.append(action)
        if action.operation == "scan":
            return {
                "status": "completed",
                "result": {
                    "message": "已按后台默认策略扫描 192.168.3.0/24，还剩 1 台待你确认：\n1. Living Room Cam（192.168.3.73，需要密码）\n请直接回复：接入 1。",
                    "next_actions": ["接入 1"],
                },
            }
        if action.operation == "connect" and action.args.get("resume_token"):
            return {
                "status": "completed",
                "result": {"message": "密码已收到。\n已接入摄像头 192.168.3.73，设备库现在共有 1 台。"},
            }
        if action.operation == "connect":
            return {
                "status": "needs_input",
                "missing_fields": ["password"],
                "prompt": "这台摄像头需要密码，请回复：密码 xxxxxx",
                "resume_token": "resume-1",
                "result": {"message": "这台摄像头需要密码，请回复：密码 xxxxxx"},
            }
        if action.operation == "analyze":
            return {
                "status": "completed",
                "result": {"message": "客厅摄像头分析完成：当前画面检测到 1 人。"},
            }
        return {"status": "failed", "result": {"message": "unsupported action"}}


def _build_camera_stack():
    reg = Registry()
    reg.register(_make_manifest())
    camera_manifest = build_camera_domain_manifest()
    reg.register(camera_manifest)

    fake_task_api = FakeTaskApi()
    router = Router([
        FakeExecutor(),
        TaskApiExecutor(
            camera_manifest.id,
            call_fn=fake_task_api.execute_action,
            supported_capabilities=camera_manifest.capabilities,
        ),
    ])
    runtime = Runtime(router=router, audit=AuditLog())
    adapter = McpServerAdapter(reg, runtime, default_autonomy=Autonomy.SUPERVISED)
    parser = IntentParser()
    channel_reg = ChannelRegistry()

    sent_messages: list[OutboundMessage] = []

    def fake_sender(msg: OutboundMessage):
        sent_messages.append(msg)

    channel_reg.register(
        ChannelConfig(channel=Channel.FEISHU, enabled=True, app_id="test", app_secret="s"),
        sender=fake_sender,
    )

    dispatcher = Dispatcher(
        intent_parser=parser,
        mcp_adapter=adapter,
        channel_registry=channel_reg,
        default_autonomy=Autonomy.SUPERVISED,
    )

    return dispatcher, sent_messages, fake_task_api


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
        dispatcher.handle(_inbound("随便聊聊"))
        assert len(sent) == 1
        assert "❌" in sent[0].text or "错误" in sent[0].text

    def test_no_sender_registered(self):
        """If channel has no sender, dispatcher logs warning but doesn't crash."""
        dispatcher, _ = _build_stack()
        msg = _inbound("查看 plex 状态", channel=Channel.MQTT)
        # MQTT is not registered in our test stack → RuntimeError in send
        # Dispatcher should catch and log
        dispatcher.handle(msg)  # should not raise


class FakeAttachmentResolver(AttachmentResolver):
    def __init__(self):
        super().__init__(download_root=Path("."))

    def resolve_message_attachment(self, inbound, config, attachment):
        return ResolvedAttachment(
            local_path="/tmp/photo.png",
            file_name="photo.png",
            size_bytes=2048,
        )


class TestWeatherAndAttachments:
    def test_weather_without_city_prompts_for_city(self):
        dispatcher, sent = _build_extended_stack()
        dispatcher.handle(_inbound("今天天气怎么样"))
        assert len(sent) == 1
        assert "请告诉我要查询的城市" in sent[0].text

    def test_weather_remembers_city(self):
        dispatcher, sent = _build_extended_stack()
        geo_response = MagicMock()
        geo_response.read.return_value = json.dumps({
            "results": [{"name": "Shanghai", "country": "China", "latitude": 31.23, "longitude": 121.47}],
        }).encode("utf-8")
        geo_response.__enter__ = MagicMock(return_value=geo_response)
        geo_response.__exit__ = MagicMock(return_value=False)

        weather_response = MagicMock()
        weather_response.read.return_value = json.dumps({
            "current": {
                "temperature_2m": 23.5,
                "wind_speed_10m": 8.1,
                "weather_code": 1,
                "time": "2026-04-06T09:00",
            }
        }).encode("utf-8")
        weather_response.__enter__ = MagicMock(return_value=weather_response)
        weather_response.__exit__ = MagicMock(return_value=False)

        with patch("urllib.request.urlopen", side_effect=[geo_response, weather_response, geo_response, weather_response]):
            dispatcher.handle(_inbound("上海天气怎么样"))
            assert len(sent) == 1
            sent.clear()
            dispatcher.handle(_inbound("今天天气呢"))

        assert len(sent) == 1
        combined = sent[0].text + json.dumps(sent[0].payload, ensure_ascii=False)
        assert "Shanghai" in combined or "上海" in combined

    def test_attachment_message_routes_to_photo_upload(self):
        dispatcher, sent = _build_extended_stack(attachment_resolver=FakeAttachmentResolver())
        dispatcher.handle(
            InboundMessage(
                channel=Channel.FEISHU,
                sender_id="user1",
                text="[图片]",
                message_id="om_1",
                attachments=[
                    Attachment(
                        type=AttachmentType.IMAGE,
                        content="img_abc",
                        file_name="camera.png",
                    )
                ],
            )
        )
        assert len(sent) == 1
        combined = sent[0].text + json.dumps(sent[0].payload, ensure_ascii=False)
        assert "photo.upload_to_nas" in combined or "NAS" in combined


class TestCameraDomainFlow:
    def test_camera_scan_routes_to_task_api(self):
        dispatcher, sent, fake_task_api = _build_camera_stack()
        dispatcher.handle(_inbound("扫描摄像头"))
        assert len(sent) == 1
        assert "待你确认" in sent[0].text
        assert fake_task_api.calls[0].domain == "camera"
        assert fake_task_api.calls[0].operation == "scan"
        assert fake_task_api.calls[0].args["_source"]["surface"] == "harborbeacon"

    def test_camera_connect_resumes_with_password(self):
        dispatcher, sent, fake_task_api = _build_camera_stack()
        dispatcher.handle(_inbound("接入 1"))
        assert len(sent) == 1
        assert "需要密码" in sent[0].text

        sent.clear()
        dispatcher.handle(_inbound("密码 hunter2"))
        assert len(sent) == 1
        assert "密码已收到" in sent[0].text
        assert len(fake_task_api.calls) == 2
        assert fake_task_api.calls[1].args["resume_token"] == "resume-1"
        assert fake_task_api.calls[1].args["password"] == "hunter2"

    def test_camera_analyze_uses_device_hint(self):
        dispatcher, sent, fake_task_api = _build_camera_stack()
        dispatcher.handle(_inbound("分析客厅摄像头"))
        assert len(sent) == 1
        assert "分析完成" in sent[0].text
        assert fake_task_api.calls[0].resource["device_hint"] == "客厅"

    def test_camera_canary_journey_covers_scan_connect_resume_and_analyze(self):
        dispatcher, sent, fake_task_api = _build_camera_stack()

        dispatcher.handle(_inbound("扫描摄像头"))
        assert len(sent) == 1
        assert "待你确认" in sent[0].text

        sent.clear()
        dispatcher.handle(_inbound("接入 1"))
        assert len(sent) == 1
        assert "需要密码" in sent[0].text

        sent.clear()
        dispatcher.handle(_inbound("密码 hunter2"))
        assert len(sent) == 1
        assert "密码已收到" in sent[0].text

        sent.clear()
        dispatcher.handle(_inbound("分析客厅摄像头"))
        assert len(sent) == 1
        assert "分析完成" in sent[0].text

        assert [call.operation for call in fake_task_api.calls] == [
            "scan",
            "connect",
            "connect",
            "analyze",
        ]
        assert fake_task_api.calls[2].args["resume_token"] == "resume-1"
        assert fake_task_api.calls[2].args["password"] == "hunter2"
        assert fake_task_api.calls[3].resource["device_hint"] == "客厅"
        assert all(call.args["_source"]["surface"] == "harborbeacon" for call in fake_task_api.calls)


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
