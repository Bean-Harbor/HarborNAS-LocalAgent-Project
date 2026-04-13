"""Dispatcher: the central chain that ties IM → Intent → MCP → Response → Reply.

This is the "glue" module that connects:
  1. InboundMessage (from webhook/adapter)
  2. IntentParser (NLU)
  3. McpServerAdapter (tool execution)
  4. ResponseFormatter (result formatting)
  5. ChannelRegistry (reply sending)

The Dispatcher also manages:
  - Session context (per-user conversation state)
  - Approval flow (HIGH risk → confirmation prompt → execute on "确认")
  - Message dedup (same message ID within TTL is skipped)
  - Group chat filtering (only respond when @mentioned / question / request)
  - Thinking indicator ("正在思考…" placeholder updated with final reply)
  - Error handling at each stage

Referenced OpenClaw's proven UX patterns for dedup, group-chat intelligence,
thinking indicator, and rich-media reply.
"""
from __future__ import annotations

import logging
import re
import time
import uuid
from dataclasses import dataclass, field
from typing import Any

from harborbeacon.adapters import ChannelAdapter, get_adapter
from harborbeacon.autonomy import Autonomy
from harborbeacon.channels import (
    Attachment,
    Channel,
    ChannelRegistry,
    ChatType,
    InboundMessage,
    OutboundMessage,
)
from harborbeacon.formatter import OutputFormat, ResponseFormatter
from harborbeacon.intent import IntentError, IntentParser, IntentResult
from harborbeacon.mcp_adapter import McpServerAdapter

logger = logging.getLogger("harborbeacon.dispatcher")


# ---------------------------------------------------------------------------
# Session management (per-user conversation state)
# ---------------------------------------------------------------------------

@dataclass
class SessionEntry:
    """Tracks one user session for multi-turn / approval flows."""
    session_id: str = field(default_factory=lambda: uuid.uuid4().hex[:12])
    user_id: str = ""
    channel: Channel = Channel.FEISHU
    pending_tool: str | None = None
    pending_args: dict[str, Any] | None = None
    pending_risk: str | None = None
    pending_resume_tool: str | None = None
    pending_resume_token: str | None = None
    pending_missing_fields: list[str] = field(default_factory=list)
    last_active: float = field(default_factory=time.time)
    history: list[dict[str, str]] = field(default_factory=list)

    def is_expired(self, timeout_seconds: int = 600) -> bool:
        return (time.time() - self.last_active) > timeout_seconds

    def touch(self) -> None:
        self.last_active = time.time()


class SessionStore:
    """In-memory session store keyed by (channel, user_id)."""

    def __init__(self, timeout: int = 600) -> None:
        self._sessions: dict[tuple[Channel, str], SessionEntry] = {}
        self._timeout = timeout

    def get_or_create(self, channel: Channel, user_id: str) -> SessionEntry:
        key = (channel, user_id)
        session = self._sessions.get(key)
        if session is None or session.is_expired(self._timeout):
            session = SessionEntry(user_id=user_id, channel=channel)
            self._sessions[key] = session
        session.touch()
        return session

    def get(self, channel: Channel, user_id: str) -> SessionEntry | None:
        key = (channel, user_id)
        session = self._sessions.get(key)
        if session and not session.is_expired(self._timeout):
            session.touch()
            return session
        return None

    def clear(self, channel: Channel, user_id: str) -> None:
        self._sessions.pop((channel, user_id), None)

    @property
    def active_count(self) -> int:
        return sum(1 for s in self._sessions.values() if not s.is_expired(self._timeout))


# ---------------------------------------------------------------------------
# Message dedup (OpenClaw pattern)
# ---------------------------------------------------------------------------

class MessageDedup:
    """Reject duplicate messages by platform message_id.

    Feishu (and other platforms) may deliver the same event more than once.
    Keeps a bounded set of seen IDs with TTL.
    """

    def __init__(self, ttl_seconds: int = 600) -> None:
        self._seen: dict[str, float] = {}
        self._ttl = ttl_seconds

    def is_duplicate(self, message_id: str) -> bool:
        if not message_id:
            return False
        now = time.time()
        # Evict expired entries lazily
        expired = [k for k, ts in self._seen.items() if now - ts > self._ttl]
        for k in expired:
            del self._seen[k]

        if message_id in self._seen:
            return True
        self._seen[message_id] = now
        return False

    @property
    def size(self) -> int:
        return len(self._seen)


# ---------------------------------------------------------------------------
# Group chat filtering (OpenClaw pattern)
# ---------------------------------------------------------------------------

# Chinese request verbs that indicate the user wants the bot to act
_REQUEST_VERBS = frozenset([
    "帮", "麻烦", "请", "能否", "可以",
    "解释", "看看", "排查", "分析", "总结",
    "写", "改", "修", "查", "对比", "翻译",
    "启动", "停止", "重启", "检查", "查看",
])

_EN_QUESTION_WORDS = re.compile(
    r"\b(why|how|what|when|where|who|help)\b", re.IGNORECASE,
)


def should_respond_in_group(text: str, mentions: list[str]) -> bool:
    """Decide if the bot should respond in a group chat.

    Follows OpenClaw's low-disturbance pattern: only respond when
    @mentioned, when the message looks like a question, or when it
    contains request-type verbs.  Avoids spamming in casual chat.
    """
    # Always respond if @mentioned
    if mentions:
        return True
    # Question mark at end
    if text.rstrip().endswith(("?", "？")):
        return True
    # English question words
    if _EN_QUESTION_WORDS.search(text):
        return True
    # Chinese request verbs
    if any(v in text for v in _REQUEST_VERBS):
        return True
    return False


# ---------------------------------------------------------------------------
# Channel ↔ OutputFormat mapping
# ---------------------------------------------------------------------------

_CHANNEL_FORMAT: dict[Channel, OutputFormat] = {
    Channel.FEISHU: OutputFormat.FEISHU_CARD,
    Channel.WECOM: OutputFormat.PLAIN,
    Channel.TELEGRAM: OutputFormat.MARKDOWN,
    Channel.DISCORD: OutputFormat.MARKDOWN,
    Channel.DINGTALK: OutputFormat.PLAIN,
    Channel.SLACK: OutputFormat.MARKDOWN,
    Channel.MQTT: OutputFormat.PLAIN,
}


def _format_for_channel(channel: Channel) -> OutputFormat:
    return _CHANNEL_FORMAT.get(channel, OutputFormat.PLAIN)


# ---------------------------------------------------------------------------
# Approval keywords
# ---------------------------------------------------------------------------

_CONFIRM_KEYWORDS = frozenset({"确认", "confirm", "yes", "y", "是"})
_CANCEL_KEYWORDS = frozenset({"取消", "cancel", "no", "n", "否"})


# ---------------------------------------------------------------------------
# Dispatcher
# ---------------------------------------------------------------------------

class Dispatcher:
    """Central dispatch chain: IM message → tool execution → reply.

    Implements OpenClaw-inspired UX patterns:
    - Message dedup (skip duplicate platform events)
    - Group chat filtering (low-disturbance mode)
    - Thinking indicator ("正在思考…" → replaced with result)
    - Session per chat (p2p by sender, group by chat_id)

    Usage::

        dispatcher = Dispatcher(
            intent_parser=parser,
            mcp_adapter=adapter,
            channel_registry=registry,
        )
        # Called by WebhookReceiver or LongConnectionTransport:
        dispatcher.handle(inbound_message)
    """

    def __init__(
        self,
        intent_parser: IntentParser,
        mcp_adapter: McpServerAdapter,
        channel_registry: ChannelRegistry,
        *,
        formatter: ResponseFormatter | None = None,
        session_timeout: int = 600,
        default_autonomy: Autonomy = Autonomy.SUPERVISED,
        thinking_threshold_ms: int = 2500,
    ):
        self._parser = intent_parser
        self._mcp = mcp_adapter
        self._channels = channel_registry
        self._formatter = formatter or ResponseFormatter()
        self._sessions = SessionStore(timeout=session_timeout)
        self._default_autonomy = default_autonomy
        self._dedup = MessageDedup(ttl_seconds=session_timeout)
        self._thinking_threshold_ms = thinking_threshold_ms

    # ---- main entry point ----

    def handle(self, inbound: InboundMessage) -> None:
        """Process an inbound IM message through the full chain."""
        # --- Dedup (OpenClaw pattern) ---
        if self._dedup.is_duplicate(inbound.message_id):
            logger.debug("Skipping duplicate message: %s", inbound.message_id)
            return

        # --- Group chat filter (OpenClaw pattern) ---
        if inbound.chat_type == ChatType.GROUP:
            if not should_respond_in_group(inbound.text, inbound.mentions):
                logger.debug("Skipping group message (not addressed to bot)")
                return

        # Session key: p2p by sender, group by chat_id (OpenClaw pattern)
        session_key = (
            inbound.chat_id if inbound.chat_type == ChatType.GROUP and inbound.chat_id
            else inbound.sender_id
        )
        session = self._sessions.get_or_create(inbound.channel, session_key)
        fmt = _format_for_channel(inbound.channel)

        try:
            self._process(inbound, session, fmt)
        except Exception as exc:
            logger.error("Dispatcher error: %s", exc, exc_info=True)
            self._reply(
                inbound,
                self._formatter.format_error(f"内部错误: {exc}", fmt=fmt),
            )

    def _process(
        self,
        inbound: InboundMessage,
        session: SessionEntry,
        fmt: OutputFormat,
    ) -> None:
        text = inbound.text.strip()
        lower_text = text.lower()

        # ------- Check for approval response -------
        if session.pending_tool:
            if lower_text in _CONFIRM_KEYWORDS:
                self._execute_pending(inbound, session, fmt)
                return
            if lower_text in _CANCEL_KEYWORDS:
                session.pending_tool = None
                session.pending_args = None
                session.pending_risk = None
                self._reply(inbound, "✅ 已取消操作")
                return
            # Not a confirm/cancel → treat as new intent (clear pending)
            session.pending_tool = None
            session.pending_args = None
            session.pending_risk = None

        if session.pending_resume_tool:
            if lower_text in _CANCEL_KEYWORDS:
                self._clear_resume_state(session)
                self._reply(inbound, "✅ 已取消当前接入流程")
                return
            resume_args = _extract_resume_arguments(inbound.text, session)
            if resume_args is not None:
                self._execute_tool(
                    inbound,
                    session.pending_resume_tool,
                    {"resource": {}, "args": resume_args},
                    fmt,
                    session=session,
                )
                return
            self._clear_resume_state(session)

        # ------- Parse intent -------
        result = self._parser.parse(inbound.text)

        if isinstance(result, IntentError):
            self._reply(
                inbound,
                self._formatter.format_error(
                    f"无法理解指令: {result.message}", fmt=fmt,
                ),
            )
            return

        assert isinstance(result, IntentResult)

        # ------- Check if operation needs approval -------
        if self._needs_approval(result):
            session.pending_tool = result.tool
            session.pending_args = result.arguments
            session.pending_risk = "HIGH"
            approval_msg = self._formatter.format_approval_request(
                result.tool, "HIGH", fmt=fmt,
            )
            self._reply(inbound, approval_msg)
            return

        # ------- Execute via MCP -------
        self._execute_tool(inbound, result.tool, result.arguments, fmt, session=session)

    def _execute_pending(
        self,
        inbound: InboundMessage,
        session: SessionEntry,
        fmt: OutputFormat,
    ) -> None:
        """Execute a previously-pending (approved) operation."""
        tool = session.pending_tool
        args = session.pending_args or {}
        session.pending_tool = None
        session.pending_args = None
        session.pending_risk = None

        if tool:
            self._execute_tool(
                inbound, tool, args, fmt,
                autonomy=Autonomy.FULL,
                approval_token="user_confirmed",
                session=session,
            )

    def _execute_tool(
        self,
        inbound: InboundMessage,
        tool: str,
        arguments: dict[str, Any],
        fmt: OutputFormat,
        *,
        autonomy: Autonomy | None = None,
        approval_token: str | None = None,
        session: SessionEntry | None = None,
    ) -> None:
        """Call the MCP adapter and send the formatted result."""
        arguments = self._augment_arguments(arguments, inbound, session)
        mcp_result = self._mcp.call_tool(
            tool,
            arguments,
            autonomy=autonomy or self._default_autonomy,
            approval_token=approval_token,
        )

        # Convert MCP result → ExecutionResult for formatting
        # The MCP result content is a JSON string inside content[0].text
        from orchestrator.contracts import ExecutionResult, StepStatus
        import json

        if mcp_result.isError:
            error_text = ""
            for item in mcp_result.content:
                error_text += item.get("text", "")
            try:
                err_data = json.loads(error_text)
                error_msg = err_data.get("error_message", err_data.get("message", error_text))
                error_code = err_data.get("error_code", err_data.get("error", ""))
            except (json.JSONDecodeError, TypeError):
                error_msg = error_text
                error_code = ""

            # Build a synthetic ExecutionResult for formatting
            exec_result = ExecutionResult(
                task_id="dispatch",
                step_id="s1",
                executor_used="mcp",
                status=StepStatus.FAILED,
                error_code=error_code,
                error_message=error_msg,
            )
        else:
            payload_text = ""
            for item in mcp_result.content:
                payload_text += item.get("text", "")
            try:
                payload = json.loads(payload_text)
            except (json.JSONDecodeError, TypeError):
                payload = payload_text

            if isinstance(payload, dict):
                exec_result = ExecutionResult(
                    task_id=payload.get("task_id", "dispatch"),
                    step_id=payload.get("step_id", "s1"),
                    executor_used=payload.get("executor_used", "mcp"),
                    status=StepStatus(payload.get("status", "SUCCESS")),
                    duration_ms=payload.get("duration_ms", 0),
                    result_payload=payload.get("result_payload"),
                    fallback_used=payload.get("fallback_used", False),
                    audit_ref=payload.get("audit_ref", ""),
                )
            else:
                exec_result = ExecutionResult(
                    task_id="dispatch",
                    step_id="s1",
                    executor_used="mcp",
                    status=StepStatus.SUCCESS,
                    result_payload=payload,
                )

        custom_message = None
        if session is not None and isinstance(exec_result.result_payload, dict):
            payload = exec_result.result_payload
            if payload.get("status") == "needs_input":
                session.pending_resume_tool = tool
                session.pending_resume_token = str(payload.get("resume_token") or "")
                session.pending_missing_fields = list(payload.get("missing_fields") or [])
                prompt = payload.get("prompt") or payload.get("result", {}).get("message") or "需要继续补充信息"
                self._reply(inbound, str(prompt))
                return

            self._clear_resume_state(session)
            custom_message = _format_task_api_message(payload)

        if custom_message:
            self._reply(inbound, custom_message)
            return

        formatted = self._formatter.format(exec_result, fmt=fmt, operation=tool)

        # For feishu card, wrap in payload
        if fmt == OutputFormat.FEISHU_CARD and isinstance(formatted, dict):
            self._reply(inbound, "执行完成", payload={"card": formatted})
        else:
            self._reply(inbound, str(formatted))

    def _needs_approval(self, intent: IntentResult) -> bool:
        """Check if a tool call requires user confirmation.

        HIGH/CRITICAL risk operations under Supervised autonomy need approval.
        """
        if self._default_autonomy == Autonomy.FULL:
            return False
        if self._default_autonomy == Autonomy.READ_ONLY:
            return False  # will be blocked by MCP adapter anyway
        # Supervised: check operation risk
        op = intent.tool.rsplit(".", 1)[-1] if "." in intent.tool else intent.tool
        return op in _MUTATION_OPS

    def _reply(
        self,
        inbound: InboundMessage,
        text: str,
        *,
        payload: dict[str, Any] | None = None,
        attachments: list[Attachment] | None = None,
        update_message_id: str = "",
    ) -> None:
        """Send a reply back to the originating channel."""
        outbound = OutboundMessage(
            channel=inbound.channel,
            recipient_id=inbound.chat_id or inbound.sender_id,
            text=text,
            payload=payload or {},
            attachments=attachments or [],
            update_message_id=update_message_id,
        )
        try:
            self._channels.send(outbound)
        except RuntimeError as exc:
            logger.warning("Cannot send reply to %s: %s", inbound.channel.value, exc)

    def _augment_arguments(
        self,
        arguments: dict[str, Any],
        inbound: InboundMessage,
        session: SessionEntry | None,
    ) -> dict[str, Any]:
        resource = dict(arguments.get("resource") or {})
        args = dict(arguments.get("args") or {})
        args["_source"] = {
            "channel": inbound.channel.value,
            "surface": "harborbeacon",
            "conversation_id": inbound.chat_id or inbound.sender_id,
            "user_id": inbound.sender_id,
            "session_id": session.session_id if session else "",
            "chat_type": inbound.chat_type.value,
            "raw_text": inbound.text,
            "trace_id": inbound.message_id or uuid.uuid4().hex,
            "autonomy_level": self._default_autonomy.value.lower(),
        }
        return {"resource": resource, "args": args}

    @staticmethod
    def _clear_resume_state(session: SessionEntry) -> None:
        session.pending_resume_tool = None
        session.pending_resume_token = None
        session.pending_missing_fields = []

    def _send_thinking_placeholder(self, inbound: InboundMessage) -> str:
        """Send a '正在思考…' placeholder message (OpenClaw pattern).

        Returns the placeholder message ID (empty string if sending failed
        or if the channel doesn't support message updates).
        """
        outbound = OutboundMessage(
            channel=inbound.channel,
            recipient_id=inbound.chat_id or inbound.sender_id,
            text="正在思考…",
        )
        try:
            self._channels.send(outbound)
            # The sender callback may set the message_id on the outbound.
            return outbound.payload.get("sent_message_id", "")
        except RuntimeError:
            return ""

    @property
    def sessions(self) -> SessionStore:
        return self._sessions

    @property
    def dedup(self) -> MessageDedup:
        return self._dedup


# Mutation operations that require approval under Supervised mode
_MUTATION_OPS = frozenset({
    "start", "stop", "restart", "enable", "disable",
    "delete", "move", "archive", "copy",
})


def _extract_password_reply(text: str) -> str | None:
    trimmed = text.strip()
    patterns = ("摄像头密码", "rtsp密码", "password", "密码")
    for pattern in patterns:
        if trimmed.lower().startswith(pattern.lower()):
            password = trimmed[len(pattern):].strip().lstrip(":：").strip()
            if password:
                return password
    if 4 <= len(trimmed) <= 64 and not any(ch.isspace() for ch in trimmed):
        return trimmed
    return None


def _extract_resume_arguments(text: str, session: SessionEntry) -> dict[str, Any] | None:
    password = _extract_password_reply(text)
    if password is None or not session.pending_resume_token:
        return None
    return {
        "resume_token": session.pending_resume_token,
        "password": password,
    }


def _format_task_api_message(payload: dict[str, Any]) -> str | None:
    if "status" not in payload or "result" not in payload:
        return None

    result = payload.get("result") or {}
    if not isinstance(result, dict):
        result = {}

    lines: list[str] = []
    message = str(result.get("message") or "").strip()
    if message:
        lines.append(message)

    artifacts = result.get("artifacts") or []
    for artifact in artifacts:
        if not isinstance(artifact, dict):
            continue
        label = artifact.get("label") or artifact.get("kind") or "artifact"
        target = artifact.get("url") or artifact.get("path")
        if target:
            lines.append(f"{label}: {target}")

    next_actions = result.get("next_actions") or []
    if next_actions:
        rendered = " / ".join(str(item) for item in next_actions if str(item).strip())
        if rendered:
            lines.append(f"你可以继续说：{rendered}")

    if not lines and payload.get("status") == "failed":
        error = payload.get("error") or payload.get("message")
        if error:
            lines.append(str(error))

    return "\n".join(lines).strip() or None
