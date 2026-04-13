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


def _format_size(size_bytes: Any) -> str:
    if not isinstance(size_bytes, (int, float)):
        return ""
    size = float(size_bytes)
    units = ["B", "KB", "MB", "GB"]
    idx = 0
    while size >= 1024 and idx < len(units) - 1:
        size /= 1024
        idx += 1
    if idx == 0:
        return f"{int(size)} {units[idx]}"
    return f"{size:.1f} {units[idx]}"


def _format_operation_summary(payload: Any, operation: str, *, markdown: bool = False) -> str:
    if not isinstance(payload, dict):
        return _format_payload(payload)

    if operation == "photo.upload_to_nas":
        lines = ["照片已上传到 NAS" if not markdown else "照片已上传到 NAS"]
        if payload.get("target_path"):
            lines.append(f"目标路径: {payload['target_path']}")
        if payload.get("file_name"):
            lines.append(f"文件名: {payload['file_name']}")
        size_text = _format_size(payload.get("size_bytes"))
        if size_text:
            lines.append(f"大小: {size_text}")
        if payload.get("source_message_id"):
            lines.append(f"来源消息: {payload['source_message_id']}")
        return "\n".join(lines)

    if operation == "weather.query":
        city = payload.get("city", "")
        summary = payload.get("summary", "")
        temperature = payload.get("temperature")
        units = payload.get("units", "metric")
        observed_at = payload.get("observed_at", "")
        source = payload.get("source", "")
        temp_unit = "°F" if units == "imperial" else "°C"
        lines = []
        headline = city or "天气查询结果"
        if summary:
            headline = f"{headline}: {summary}"
        lines.append(headline)
        if temperature is not None:
            lines.append(f"温度: {temperature}{temp_unit}")
        if payload.get("wind_speed") is not None:
            lines.append(f"风速: {payload['wind_speed']}")
        if observed_at:
            lines.append(f"更新时间: {observed_at}")
        if source:
            lines.append(f"来源: {source}")
        return "\n".join(lines)

    return _format_payload(payload)


def _format_operation_error(result: ExecutionResult, operation: str) -> str:
    payload = result.result_payload if isinstance(result.result_payload, dict) else {}

    if operation == "photo.upload_to_nas":
        lines = [payload.get("error_title") or "照片上传失败"]
        target_text = payload.get("target_path") or payload.get("target_dir")
        if target_text:
            lines.append(f"目标位置: {target_text}")
        if payload.get("file_name"):
            lines.append(f"文件名: {payload['file_name']}")
        if payload.get("source_message_id"):
            lines.append(f"来源消息: {payload['source_message_id']}")
        if payload.get("error_hint"):
            lines.append(f"建议: {payload['error_hint']}")
        return "\n".join(lines)

    return ""


def _uses_special_summary(operation: str) -> bool:
    return operation in {"photo.upload_to_nas", "weather.query"}


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
        lines.append(_format_operation_summary(result.result_payload, operation))

    if not result.ok and result.error_message:
        operation_error = _format_operation_error(result, operation)
        if operation_error:
            lines.append(operation_error)
            lines.append(f"详细错误: {result.error_message}")
        else:
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
        if _uses_special_summary(operation):
            payload = _format_operation_summary(result.result_payload, operation, markdown=True)
            lines.append(payload)
        else:
            payload = _format_payload(result.result_payload)
            lines.append(f"```\n{payload}\n```")

    if not result.ok and result.error_message:
        operation_error = _format_operation_error(result, operation)
        if operation_error:
            lines.append(operation_error)
            lines.append("")
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
        payload = _format_operation_summary(result.result_payload, operation, markdown=True)
        elements.append({
            "tag": "div",
            "text": {"tag": "lark_md", "content": payload},
        })

    # Error
    if not result.ok and result.error_message:
        operation_error = _format_operation_error(result, operation)
        if operation_error:
            elements.append({
                "tag": "div",
                "text": {"tag": "lark_md", "content": operation_error},
            })
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
