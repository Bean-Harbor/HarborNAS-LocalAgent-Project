"""Intent parsing: natural language → structured tool call.

Converts user text from IM channels into an MCP tool name + arguments.
Supports two backends:

1. **LLM backend** (default):  Calls a local Ollama or remote OpenAI-compatible
   endpoint. Sends a system prompt that lists available tools and expects a
   JSON response with ``tool`` and ``arguments``.

2. **Regex fallback**:  A deterministic, zero-dependency parser that matches
   common Chinese/English patterns for HarborOS operations.  Used when no LLM
   is configured or when the LLM response is unparseable.

Both backends produce an ``IntentResult``.
"""
from __future__ import annotations

import json
import re
from dataclasses import dataclass, field
from typing import Any, Callable

from harborclaw.mcp_adapter import McpToolSchema


# ---------------------------------------------------------------------------
# Data types
# ---------------------------------------------------------------------------

@dataclass
class IntentResult:
    """Parsed user intent."""
    tool: str                               # MCP tool name, e.g. "service.status"
    arguments: dict[str, Any] = field(default_factory=dict)
    confidence: float = 1.0                 # 0.0–1.0
    raw_llm_response: str | None = None     # LLM raw text (for debugging)

    @property
    def is_valid(self) -> bool:
        return bool(self.tool) and self.confidence > 0.0


@dataclass
class IntentError:
    """Returned when intent cannot be resolved."""
    message: str
    original_text: str


# ---------------------------------------------------------------------------
# LLM backend
# ---------------------------------------------------------------------------

# Type alias for the actual HTTP call function.
# Signature:  (messages: list[dict], model: str) -> str
LlmCallFn = Callable[[list[dict[str, str]], str], str]

_SYSTEM_PROMPT_TEMPLATE = """\
You are HarborClaw, the AI assistant built into HarborOS.
The user will give you a natural-language command.  Your job is to decide
which tool to call and with what arguments.

Available tools:
{tool_list}

Respond with a JSON object ONLY (no markdown, no explanation):
{{"tool": "<tool_name>", "arguments": {{"resource": {{}}, "args": {{}}}}}}

If you cannot match a tool, respond:
{{"tool": "", "arguments": {{}}}}
"""


def _build_system_prompt(tools: list[McpToolSchema]) -> str:
    lines = []
    for t in tools:
        lines.append(f"- {t.name}: {t.description}")
    return _SYSTEM_PROMPT_TEMPLATE.format(tool_list="\n".join(lines))


def parse_intent_llm(
    text: str,
    tools: list[McpToolSchema],
    llm_call: LlmCallFn,
    *,
    model: str = "llama3",
) -> IntentResult | IntentError:
    """Parse intent via an LLM call."""
    system = _build_system_prompt(tools)
    messages = [
        {"role": "system", "content": system},
        {"role": "user", "content": text},
    ]
    try:
        raw = llm_call(messages, model)
    except Exception as exc:
        return IntentError(message=f"LLM call failed: {exc}", original_text=text)

    try:
        data = json.loads(raw)
    except json.JSONDecodeError:
        # Try to extract JSON from markdown fences
        m = re.search(r"\{.*\}", raw, re.DOTALL)
        if m:
            try:
                data = json.loads(m.group())
            except json.JSONDecodeError:
                return IntentError(message="LLM returned unparseable JSON", original_text=text)
        else:
            return IntentError(message="LLM returned unparseable JSON", original_text=text)

    tool_name = data.get("tool", "")
    arguments = data.get("arguments", {})
    if not tool_name:
        return IntentError(message="LLM could not match a tool", original_text=text)

    return IntentResult(tool=tool_name, arguments=arguments, confidence=0.9, raw_llm_response=raw)


# ---------------------------------------------------------------------------
# Regex fallback
# ---------------------------------------------------------------------------

# Pattern table:  (compiled regex, tool_name, resource_extractor)
_ServicePattern = tuple[re.Pattern[str], str, str]

_SERVICE_PATTERNS: list[_ServicePattern] = [
    # Chinese patterns
    (re.compile(r"(?:查看|查询|检查|看看)\s+(\S+?)\s+(?:服务\s*)?(?:状态|运行)(?:情况)?", re.I),
     "service.status", "service_name"),
    (re.compile(r"(?:查看|查询|检查|看看)\s+(\S+?)\s*(?:服务)?$", re.I),
     "service.status", "service_name"),
    (re.compile(r"(?:启动|开启|打开)\s+(\S+?)\s*(?:服务)?$", re.I),
     "service.start", "service_name"),
    (re.compile(r"(?:停止|关闭|停掉)\s+(\S+?)\s*(?:服务)?$", re.I),
     "service.stop", "service_name"),
    (re.compile(r"(?:重启|重新启动)\s+(\S+?)\s*(?:服务)?$", re.I),
     "service.restart", "service_name"),
    # English patterns (restart before start to avoid prefix match)
    (re.compile(r"(?:status|check|show|get)\s+(?:of\s+)?(\S+?)(?:\s+service)?$", re.I),
     "service.status", "service_name"),
    (re.compile(r"restart\s+(?:the\s+)?(\S+?)(?:\s+service)?$", re.I),
     "service.restart", "service_name"),
    (re.compile(r"start\s+(?:the\s+)?(\S+?)(?:\s+service)?$", re.I),
     "service.start", "service_name"),
    (re.compile(r"stop\s+(?:the\s+)?(\S+?)(?:\s+service)?$", re.I),
     "service.stop", "service_name"),
]

_FILE_PATTERNS: list[_ServicePattern] = [
    (re.compile(r"(?:搜索|查找|找|search)\s+(?:文件\s+)?(.+)", re.I),
     "files.search", "pattern"),
    (re.compile(r"(?:复制|拷贝|copy)\s+(.+?)\s+(?:到|to)\s+(.+)", re.I),
     "files.copy", "source"),
]


def parse_intent_regex(text: str) -> IntentResult | IntentError:
    """Deterministic regex-based intent parser (zero dependencies)."""
    text = text.strip()

    for pattern, tool, resource_key in _SERVICE_PATTERNS:
        m = pattern.search(text)
        if m:
            service_name = m.group(1).strip().lower()
            return IntentResult(
                tool=tool,
                arguments={"resource": {resource_key: service_name}, "args": {}},
                confidence=0.7,
            )

    for pattern, tool, resource_key in _FILE_PATTERNS:
        m = pattern.search(text)
        if m:
            return IntentResult(
                tool=tool,
                arguments={"resource": {resource_key: m.group(1).strip()}, "args": {}},
                confidence=0.6,
            )

    return IntentError(message="No pattern matched", original_text=text)


# ---------------------------------------------------------------------------
# Unified entry point
# ---------------------------------------------------------------------------

class IntentParser:
    """Unified intent parser: tries LLM first, falls back to regex.

    Usage::

        parser = IntentParser(tools=adapter.list_tools(), llm_call=my_fn)
        result = parser.parse("查看 plex 状态")
    """

    def __init__(
        self,
        tools: list[McpToolSchema] | None = None,
        llm_call: LlmCallFn | None = None,
        model: str = "llama3",
    ):
        self._tools = tools or []
        self._llm_call = llm_call
        self._model = model

    def parse(self, text: str) -> IntentResult | IntentError:
        """Parse user text into a tool call intent."""
        # Try LLM first if available
        if self._llm_call and self._tools:
            result = parse_intent_llm(text, self._tools, self._llm_call, model=self._model)
            if isinstance(result, IntentResult):
                return result
            # LLM failed → fall through to regex

        # Regex fallback
        return parse_intent_regex(text)
