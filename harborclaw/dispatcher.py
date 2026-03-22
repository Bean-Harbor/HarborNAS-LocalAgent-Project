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
  - Error handling at each stage
"""
from __future__ import annotations

import logging
import time
import uuid
from dataclasses import dataclass, field
from typing import Any

from harborclaw.adapters import ChannelAdapter, get_adapter
from harborclaw.autonomy import Autonomy
from harborclaw.channels import (
    Channel,
    ChannelRegistry,
    InboundMessage,
    OutboundMessage,
)
from harborclaw.formatter import OutputFormat, ResponseFormatter
from harborclaw.intent import IntentError, IntentParser, IntentResult
from harborclaw.mcp_adapter import McpServerAdapter

logger = logging.getLogger("harborclaw.dispatcher")


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

    Usage::

        dispatcher = Dispatcher(
            intent_parser=parser,
            mcp_adapter=adapter,
            channel_registry=registry,
        )
        # Called by WebhookReceiver when a message arrives:
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
    ):
        self._parser = intent_parser
        self._mcp = mcp_adapter
        self._channels = channel_registry
        self._formatter = formatter or ResponseFormatter()
        self._sessions = SessionStore(timeout=session_timeout)
        self._default_autonomy = default_autonomy

    # ---- main entry point ----

    def handle(self, inbound: InboundMessage) -> None:
        """Process an inbound IM message through the full chain."""
        session = self._sessions.get_or_create(inbound.channel, inbound.sender_id)
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
        text = inbound.text.strip().lower()

        # ------- Check for approval response -------
        if session.pending_tool:
            if text in _CONFIRM_KEYWORDS:
                self._execute_pending(inbound, session, fmt)
                return
            if text in _CANCEL_KEYWORDS:
                session.pending_tool = None
                session.pending_args = None
                session.pending_risk = None
                self._reply(inbound, "✅ 已取消操作")
                return
            # Not a confirm/cancel → treat as new intent (clear pending)
            session.pending_tool = None
            session.pending_args = None
            session.pending_risk = None

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
        self._execute_tool(inbound, result.tool, result.arguments, fmt)

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
    ) -> None:
        """Call the MCP adapter and send the formatted result."""
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
    ) -> None:
        """Send a reply back to the originating channel."""
        outbound = OutboundMessage(
            channel=inbound.channel,
            recipient_id=inbound.sender_id,
            text=text,
            payload=payload or {},
        )
        try:
            self._channels.send(outbound)
        except RuntimeError as exc:
            logger.warning("Cannot send reply to %s: %s", inbound.channel.value, exc)

    @property
    def sessions(self) -> SessionStore:
        return self._sessions


# Mutation operations that require approval under Supervised mode
_MUTATION_OPS = frozenset({
    "start", "stop", "restart", "enable", "disable",
    "delete", "move", "archive", "copy",
})
