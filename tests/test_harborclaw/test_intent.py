"""Tests for harborclaw.intent — Intent parser (LLM + regex fallback)."""
import json
import pytest

from harborclaw.intent import (
    IntentError,
    IntentParser,
    IntentResult,
    parse_intent_llm,
    parse_intent_regex,
    _build_system_prompt,
)
from harborclaw.mcp_adapter import McpToolSchema


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------

def _tools() -> list[McpToolSchema]:
    return [
        McpToolSchema(name="service.status", description="Check service status"),
        McpToolSchema(name="service.start", description="Start a service"),
        McpToolSchema(name="service.stop", description="Stop a service"),
        McpToolSchema(name="files.search", description="Search files"),
    ]


def _fake_llm_ok(messages, model):
    """Fake LLM that returns valid JSON."""
    return json.dumps({
        "tool": "service.status",
        "arguments": {"resource": {"service_name": "plex"}, "args": {}},
    })


def _fake_llm_markdown(messages, model):
    """Fake LLM that wraps JSON in markdown fences."""
    return '```json\n{"tool": "service.start", "arguments": {"resource": {"service_name": "nginx"}}}\n```'


def _fake_llm_no_match(messages, model):
    return json.dumps({"tool": "", "arguments": {}})


def _fake_llm_garbage(messages, model):
    return "I don't understand what you mean..."


def _fake_llm_error(messages, model):
    raise ConnectionError("LLM service unavailable")


# ---------------------------------------------------------------------------
# Regex parser tests
# ---------------------------------------------------------------------------

class TestRegexParser:
    def test_chinese_status(self):
        r = parse_intent_regex("查看 plex 状态")
        assert isinstance(r, IntentResult)
        assert r.tool == "service.status"
        assert r.arguments["resource"]["service_name"] == "plex"

    def test_chinese_start(self):
        r = parse_intent_regex("启动 nginx 服务")
        assert isinstance(r, IntentResult)
        assert r.tool == "service.start"
        assert r.arguments["resource"]["service_name"] == "nginx"

    def test_chinese_stop(self):
        r = parse_intent_regex("停止 plex")
        assert isinstance(r, IntentResult)
        assert r.tool == "service.stop"

    def test_chinese_restart(self):
        r = parse_intent_regex("重启 samba 服务")
        assert isinstance(r, IntentResult)
        assert r.tool == "service.restart"
        assert r.arguments["resource"]["service_name"] == "samba"

    def test_english_status(self):
        r = parse_intent_regex("status plex")
        assert isinstance(r, IntentResult)
        assert r.tool == "service.status"

    def test_english_check(self):
        r = parse_intent_regex("check nginx")
        assert isinstance(r, IntentResult)
        assert r.tool == "service.status"

    def test_english_start(self):
        r = parse_intent_regex("start the plex service")
        assert isinstance(r, IntentResult)
        assert r.tool == "service.start"

    def test_english_stop(self):
        r = parse_intent_regex("stop nginx")
        assert isinstance(r, IntentResult)
        assert r.tool == "service.stop"

    def test_english_restart(self):
        r = parse_intent_regex("restart samba")
        assert isinstance(r, IntentResult)
        assert r.tool == "service.restart"

    def test_file_search(self):
        r = parse_intent_regex("搜索 *.mp4")
        assert isinstance(r, IntentResult)
        assert r.tool == "files.search"

    def test_no_match(self):
        r = parse_intent_regex("今天天气怎么样")
        assert isinstance(r, IntentError)
        assert "No pattern matched" in r.message

    def test_confidence_lower_than_llm(self):
        r = parse_intent_regex("查看 plex 状态")
        assert isinstance(r, IntentResult)
        assert r.confidence < 1.0

    def test_service_name_lowercase(self):
        r = parse_intent_regex("查看 Plex 状态")
        assert isinstance(r, IntentResult)
        assert r.arguments["resource"]["service_name"] == "plex"


# ---------------------------------------------------------------------------
# LLM parser tests
# ---------------------------------------------------------------------------

class TestLlmParser:
    def test_successful_parse(self):
        r = parse_intent_llm("查看 plex 状态", _tools(), _fake_llm_ok)
        assert isinstance(r, IntentResult)
        assert r.tool == "service.status"
        assert r.confidence == 0.9
        assert r.raw_llm_response is not None

    def test_markdown_wrapped_json(self):
        r = parse_intent_llm("start nginx", _tools(), _fake_llm_markdown)
        assert isinstance(r, IntentResult)
        assert r.tool == "service.start"

    def test_no_match_returns_error(self):
        r = parse_intent_llm("hello", _tools(), _fake_llm_no_match)
        assert isinstance(r, IntentError)

    def test_garbage_returns_error(self):
        r = parse_intent_llm("hello", _tools(), _fake_llm_garbage)
        assert isinstance(r, IntentError)

    def test_connection_error(self):
        r = parse_intent_llm("status plex", _tools(), _fake_llm_error)
        assert isinstance(r, IntentError)
        assert "LLM call failed" in r.message


# ---------------------------------------------------------------------------
# System prompt tests
# ---------------------------------------------------------------------------

class TestSystemPrompt:
    def test_includes_tool_names(self):
        prompt = _build_system_prompt(_tools())
        assert "service.status" in prompt
        assert "service.start" in prompt

    def test_includes_json_format(self):
        prompt = _build_system_prompt(_tools())
        assert '"tool"' in prompt


# ---------------------------------------------------------------------------
# IntentParser unified interface
# ---------------------------------------------------------------------------

class TestIntentParser:
    def test_llm_first_then_regex_fallback(self):
        parser = IntentParser(tools=_tools(), llm_call=_fake_llm_error)
        # LLM fails → falls back to regex
        r = parser.parse("查看 plex 状态")
        assert isinstance(r, IntentResult)
        assert r.tool == "service.status"

    def test_llm_success(self):
        parser = IntentParser(tools=_tools(), llm_call=_fake_llm_ok)
        r = parser.parse("查看 plex 状态")
        assert isinstance(r, IntentResult)
        assert r.confidence == 0.9  # LLM confidence

    def test_regex_only_no_llm(self):
        parser = IntentParser()  # no LLM configured
        r = parser.parse("查看 plex 状态")
        assert isinstance(r, IntentResult)
        assert r.confidence < 1.0

    def test_neither_works(self):
        parser = IntentParser()
        r = parser.parse("随便聊聊")
        assert isinstance(r, IntentError)


# ---------------------------------------------------------------------------
# IntentResult data type
# ---------------------------------------------------------------------------

class TestIntentResult:
    def test_is_valid(self):
        r = IntentResult(tool="service.status", confidence=0.9)
        assert r.is_valid

    def test_not_valid_empty_tool(self):
        r = IntentResult(tool="", confidence=0.9)
        assert not r.is_valid

    def test_not_valid_zero_confidence(self):
        r = IntentResult(tool="service.status", confidence=0.0)
        assert not r.is_valid
