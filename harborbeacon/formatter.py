"""Response formatter: ExecutionResult → human-readable messages.

Converts ``ExecutionResult`` into text suitable for different IM channels.
Supports three output formats:

- **plain**:   Simple text (Telegram, MQTT, CLI)
- **markdown**: Markdown (Discord, Slack, WebUI)
- **feishu_card**: Feishu interactive card JSON

Each format includes:  status indicator, operation summary, result payload,
error details (when failed), and audit reference.
"""
from __future__ import annotations

import json
from dataclasses import dataclass, field
from enum import Enum
from typing import Any

from orchestrator.contracts import ExecutionResult, StepStatus


class OutputFormat(str, Enum):
    PLAIN = "plain"
    MARKDOWN = "markdown"
    FEISHU_CARD = "feishu_card"


# ---------------------------------------------------------------------------
# Status indicators
# ---------------------------------------------------------------------------

_STATUS_EMOJI: dict[StepStatus, str] = {
    StepStatus.SUCCESS: "✅",
    StepStatus.FAILED: "❌",
    StepStatus.BLOCKED: "🚫",
    StepStatus.PENDING: "⏳",
    StepStatus.EXECUTING: "⚙️",
    StepStatus.APPROVED: "👍",
    StepStatus.SKIPPED: "⏭️",
}

_STATUS_LABEL_ZH: dict[StepStatus, str] = {
    StepStatus.SUCCESS: "执行成功",
    StepStatus.FAILED: "执行失败",
    StepStatus.BLOCKED: "已拦截",
    StepStatus.PENDING: "等待中",
    StepStatus.EXECUTING: "执行中",
    StepStatus.APPROVED: "已批准",
    StepStatus.SKIPPED: "已跳过",
}


def _status_text(status: StepStatus) -> str:
    emoji = _STATUS_EMOJI.get(status, "❓")
    label = _STATUS_LABEL_ZH.get(status, status.value)
    return f"{emoji} {label}"


# ---------------------------------------------------------------------------
# Payload formatting
# ---------------------------------------------------------------------------

def _format_payload(payload: Any, *, max_len: int = 500) -> str:
    """Convert a result payload to a readable string."""
    if payload is None:
        return ""
    if isinstance(payload, str):
        text = payload
    elif isinstance(payload, dict):
        text = json.dumps(payload, ensure_ascii=False, indent=2)
    else:
        text = str(payload)
    if len(text) > max_len:
        text = text[:max_len] + "…"
    return text


# ---------------------------------------------------------------------------
# Plain text
# ---------------------------------------------------------------------------

def format_plain(result: ExecutionResult, *, operation: str = "") -> str:
    """Format result as plain text."""
    lines: list[str] = []
    header = _status_text(result.status)
    if operation:
        header += f" | {operation}"
    lines.append(header)

    if result.ok and result.result_payload is not None:
        lines.append(_format_payload(result.result_payload))

    if not result.ok and result.error_message:
        lines.append(f"错误: {result.error_message}")
        if result.error_code:
            lines.append(f"错误码: {result.error_code}")

    if result.fallback_used:
        lines.append(f"(使用了备用路由: {result.executor_used})")

    lines.append(f"审计编号: {result.audit_ref}")
    return "\n".join(lines)


# ---------------------------------------------------------------------------
# Markdown
# ---------------------------------------------------------------------------

def format_markdown(result: ExecutionResult, *, operation: str = "") -> str:
    """Format result as Markdown (Discord / Slack / WebUI)."""
    lines: list[str] = []
    header = _status_text(result.status)
    if operation:
        header = f"**{operation}** — {header}"
    lines.append(header)
    lines.append("")

    if result.ok and result.result_payload is not None:
        payload = _format_payload(result.result_payload)
        lines.append(f"```\n{payload}\n```")

    if not result.ok and result.error_message:
        lines.append(f"> **错误**: {result.error_message}")
        if result.error_code:
            lines.append(f"> 错误码: `{result.error_code}`")

    if result.fallback_used:
        lines.append(f"_备用路由: {result.executor_used}_")

    lines.append(f"\n`audit: {result.audit_ref}` | 耗时 {result.duration_ms}ms")
    return "\n".join(lines)


# ---------------------------------------------------------------------------
# Feishu interactive card
# ---------------------------------------------------------------------------

def format_feishu_card(result: ExecutionResult, *, operation: str = "") -> dict[str, Any]:
    """Format result as a Feishu interactive card JSON structure.

    Returns a dict suitable for the Feishu ``interactive`` msg_type.
    """
    status = _status_text(result.status)
    title = operation or "HarborBeacon"

    elements: list[dict[str, Any]] = []

    # Status line
    elements.append({
        "tag": "div",
        "text": {"tag": "lark_md", "content": f"**状态**: {status}"},
    })

    # Payload
    if result.ok and result.result_payload is not None:
        payload = _format_payload(result.result_payload, max_len=800)
        elements.append({
            "tag": "div",
            "text": {"tag": "lark_md", "content": f"```\n{payload}\n```"},
        })

    # Error
    if not result.ok and result.error_message:
        error_text = f"**错误**: {result.error_message}"
        if result.error_code:
            error_text += f"\n错误码: `{result.error_code}`"
        elements.append({
            "tag": "div",
            "text": {"tag": "lark_md", "content": error_text},
        })

    # Footer
    footer_parts = [f"audit: {result.audit_ref}", f"耗时: {result.duration_ms}ms"]
    if result.fallback_used:
        footer_parts.append(f"备用路由: {result.executor_used}")
    elements.append({
        "tag": "note",
        "elements": [{"tag": "plain_text", "content": " | ".join(footer_parts)}],
    })

    return {
        "config": {"wide_screen_mode": True},
        "header": {
            "title": {"tag": "plain_text", "content": title},
            "template": "green" if result.ok else "red",
        },
        "elements": elements,
    }


# ---------------------------------------------------------------------------
# Unified formatter
# ---------------------------------------------------------------------------

class ResponseFormatter:
    """Converts ExecutionResult to a channel-appropriate message string/dict.

    Usage::

        fmt = ResponseFormatter()
        text = fmt.format(result, format=OutputFormat.MARKDOWN, operation="service.status")
    """

    def format(
        self,
        result: ExecutionResult,
        *,
        fmt: OutputFormat = OutputFormat.PLAIN,
        operation: str = "",
    ) -> str | dict[str, Any]:
        if fmt == OutputFormat.PLAIN:
            return format_plain(result, operation=operation)
        if fmt == OutputFormat.MARKDOWN:
            return format_markdown(result, operation=operation)
        if fmt == OutputFormat.FEISHU_CARD:
            return format_feishu_card(result, operation=operation)
        return format_plain(result, operation=operation)

    def format_error(self, message: str, *, fmt: OutputFormat = OutputFormat.PLAIN) -> str:
        """Format a generic error message (not tied to an ExecutionResult)."""
        if fmt == OutputFormat.MARKDOWN:
            return f"❌ **错误**: {message}"
        return f"❌ 错误: {message}"

    def format_approval_request(
        self,
        operation: str,
        risk_level: str,
        *,
        fmt: OutputFormat = OutputFormat.PLAIN,
    ) -> str:
        """Format a confirmation prompt for high-risk operations."""
        if fmt == OutputFormat.MARKDOWN:
            return (
                f"⚠️ **需要确认**\n\n"
                f"操作: `{operation}`\n"
                f"风险等级: **{risk_level}**\n\n"
                f"回复 **确认** 或 **取消**"
            )
        return (
            f"⚠️ 需要确认\n"
            f"操作: {operation}\n"
            f"风险等级: {risk_level}\n"
            f"回复 '确认' 或 '取消'"
        )
