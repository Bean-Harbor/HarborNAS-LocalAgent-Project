"""Tests for harborbeacon.formatter — Response formatter."""
import json
import pytest

from orchestrator.contracts import ExecutionResult, StepStatus

from harborbeacon.formatter import (
    OutputFormat,
    ResponseFormatter,
    format_feishu_card,
    format_markdown,
    format_plain,
)


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------

def _ok_result(**kw) -> ExecutionResult:
    defaults = dict(
        task_id="t1",
        step_id="s1",
        executor_used="middleware_api",
        status=StepStatus.SUCCESS,
        duration_ms=42,
        result_payload={"state": "RUNNING", "service": "plex"},
        audit_ref="abc123",
    )
    defaults.update(kw)
    return ExecutionResult(**defaults)


def _failed_result(**kw) -> ExecutionResult:
    defaults = dict(
        task_id="t1",
        step_id="s1",
        executor_used="midcli",
        status=StepStatus.FAILED,
        duration_ms=100,
        error_code="MIDDLEWARE_ERROR",
        error_message="Connection refused",
        audit_ref="def456",
    )
    defaults.update(kw)
    return ExecutionResult(**defaults)


def _blocked_result(**kw) -> ExecutionResult:
    defaults = dict(
        task_id="t1",
        step_id="s1",
        executor_used="none",
        status=StepStatus.BLOCKED,
        error_code="APPROVAL_REQUIRED",
        error_message="service.stop requires approval",
        audit_ref="ghi789",
    )
    defaults.update(kw)
    return ExecutionResult(**defaults)


# ---------------------------------------------------------------------------
# Plain text
# ---------------------------------------------------------------------------

class TestPlainFormat:
    def test_success_includes_emoji(self):
        text = format_plain(_ok_result(), operation="service.status")
        assert "✅" in text
        assert "service.status" in text

    def test_success_includes_payload(self):
        text = format_plain(_ok_result())
        assert "RUNNING" in text

    def test_failed_includes_error(self):
        text = format_plain(_failed_result())
        assert "❌" in text
        assert "Connection refused" in text
        assert "MIDDLEWARE_ERROR" in text

    def test_blocked_includes_indicator(self):
        text = format_plain(_blocked_result())
        assert "🚫" in text

    def test_includes_audit_ref(self):
        text = format_plain(_ok_result())
        assert "abc123" in text

    def test_fallback_noted(self):
        text = format_plain(_ok_result(fallback_used=True, executor_used="midcli"))
        assert "备用路由" in text
        assert "midcli" in text

    def test_no_payload_no_crash(self):
        text = format_plain(_ok_result(result_payload=None))
        assert "✅" in text


# ---------------------------------------------------------------------------
# Markdown
# ---------------------------------------------------------------------------

class TestMarkdownFormat:
    def test_success_bold_operation(self):
        text = format_markdown(_ok_result(), operation="service.status")
        assert "**service.status**" in text
        assert "✅" in text

    def test_code_block_payload(self):
        text = format_markdown(_ok_result())
        assert "```" in text

    def test_error_quoted(self):
        text = format_markdown(_failed_result())
        assert "> **错误**" in text

    def test_includes_duration(self):
        text = format_markdown(_ok_result())
        assert "42ms" in text


# ---------------------------------------------------------------------------
# Feishu card
# ---------------------------------------------------------------------------

class TestFeishuCard:
    def test_returns_dict(self):
        card = format_feishu_card(_ok_result(), operation="service.status")
        assert isinstance(card, dict)
        assert "header" in card
        assert "elements" in card

    def test_green_header_on_success(self):
        card = format_feishu_card(_ok_result())
        assert card["header"]["template"] == "green"

    def test_red_header_on_failure(self):
        card = format_feishu_card(_failed_result())
        assert card["header"]["template"] == "red"

    def test_elements_contain_status(self):
        card = format_feishu_card(_ok_result())
        texts = [e.get("text", {}).get("content", "") for e in card["elements"]]
        found = any("✅" in t for t in texts)
        assert found

    def test_title_is_operation(self):
        card = format_feishu_card(_ok_result(), operation="service.status")
        assert card["header"]["title"]["content"] == "service.status"

    def test_note_has_audit(self):
        card = format_feishu_card(_ok_result())
        notes = [e for e in card["elements"] if e.get("tag") == "note"]
        assert len(notes) == 1
        note_text = notes[0]["elements"][0]["content"]
        assert "abc123" in note_text


# ---------------------------------------------------------------------------
# ResponseFormatter unified
# ---------------------------------------------------------------------------

class TestResponseFormatter:
    def setup_method(self):
        self.fmt = ResponseFormatter()

    def test_format_plain(self):
        result = self.fmt.format(_ok_result(), fmt=OutputFormat.PLAIN)
        assert isinstance(result, str)
        assert "✅" in result

    def test_format_markdown(self):
        result = self.fmt.format(_ok_result(), fmt=OutputFormat.MARKDOWN)
        assert isinstance(result, str)
        assert "```" in result

    def test_format_feishu_card(self):
        result = self.fmt.format(_ok_result(), fmt=OutputFormat.FEISHU_CARD)
        assert isinstance(result, dict)

    def test_format_error(self):
        text = self.fmt.format_error("something broke")
        assert "❌" in text

    def test_format_error_markdown(self):
        text = self.fmt.format_error("something broke", fmt=OutputFormat.MARKDOWN)
        assert "**错误**" in text

    def test_format_approval_request(self):
        text = self.fmt.format_approval_request("service.stop", "HIGH")
        assert "确认" in text
        assert "HIGH" in text

    def test_format_approval_markdown(self):
        text = self.fmt.format_approval_request("service.stop", "HIGH", fmt=OutputFormat.MARKDOWN)
        assert "**需要确认**" in text

    def test_payload_truncation(self):
        big_payload = {"data": "x" * 1000}
        r = _ok_result(result_payload=big_payload)
        text = format_plain(r)
        assert "…" in text
