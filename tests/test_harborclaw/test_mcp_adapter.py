"""Tests for harborclaw.mcp_adapter — MCP server adapter."""
import json

import pytest

from orchestrator.contracts import Action, ExecutionResult, RiskLevel, Route, StepStatus
from orchestrator.audit import AuditLog
from orchestrator.router import Router
from orchestrator.runtime import Runtime
from skills.manifest import SkillManifest, HarborApiConfig, HarborCliConfig, RiskConfig
from skills.registry import Registry

from harborclaw.autonomy import Autonomy
from harborclaw.mcp_adapter import McpServerAdapter, McpToolSchema, McpToolResult


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------

def _make_manifest(**overrides) -> SkillManifest:
    defaults = dict(
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
    defaults.update(overrides)
    return SkillManifest(**defaults)


class FakeExecutor:
    """Minimal executor that satisfies the Router Executor protocol."""

    def __init__(self, route: Route, *, available: bool = True, payload: str = "ok"):
        self._route = route
        self._available = available
        self._payload = payload

    @property
    def route(self) -> Route:
        return self._route

    def is_available(self) -> bool:
        return self._available

    def execute(self, action: Action, *, task_id: str, step_id: str) -> ExecutionResult:
        return ExecutionResult(
            task_id=task_id,
            step_id=step_id,
            executor_used=self._route.value,
            status=StepStatus.SUCCESS,
            result_payload={"out": self._payload, "op": action.operation},
        )


def _build_adapter(**kw) -> tuple[McpServerAdapter, Registry, Runtime]:
    reg = Registry()
    reg.register(_make_manifest())
    router = Router([FakeExecutor(Route.MIDDLEWARE_API)])
    runtime = Runtime(router=router, audit=AuditLog())
    adapter = McpServerAdapter(reg, runtime, **kw)
    return adapter, reg, runtime


# ---------------------------------------------------------------------------
# list_tools
# ---------------------------------------------------------------------------

class TestListTools:
    def test_returns_tool_per_capability(self):
        adapter, reg, _ = _build_adapter()
        tools = adapter.list_tools()
        names = {t.name for t in tools}
        assert names == {"service.status", "service.start", "service.stop"}

    def test_tool_has_description(self):
        adapter, _, _ = _build_adapter()
        tools = adapter.list_tools()
        for t in tools:
            assert isinstance(t.description, str)
            assert len(t.description) > 0

    def test_tool_has_input_schema(self):
        adapter, _, _ = _build_adapter()
        tools = adapter.list_tools()
        for t in tools:
            assert "properties" in t.inputSchema

    def test_deduplicates_capabilities(self):
        """If two skills provide the same capability, list it once."""
        reg = Registry()
        reg.register(_make_manifest(id="a", capabilities=["service.status"]))
        reg.register(_make_manifest(id="b", capabilities=["service.status", "service.start"]))
        router = Router([FakeExecutor(Route.MIDDLEWARE_API)])
        adapter = McpServerAdapter(reg, Runtime(router=router))
        tools = adapter.list_tools()
        names = [t.name for t in tools]
        assert names.count("service.status") == 1

    def test_empty_registry(self):
        reg = Registry()
        router = Router()
        adapter = McpServerAdapter(reg, Runtime(router=router))
        assert adapter.list_tools() == []


# ---------------------------------------------------------------------------
# call_tool — happy path
# ---------------------------------------------------------------------------

class TestCallTool:
    def test_call_status_supervised(self):
        adapter, _, _ = _build_adapter(default_autonomy=Autonomy.SUPERVISED)
        result = adapter.call_tool("service.status", {"resource": {"service_name": "plex"}})
        assert not result.isError
        payload = json.loads(result.content[0]["text"])
        assert payload["status"] == "SUCCESS"

    def test_result_contains_operation(self):
        adapter, _, _ = _build_adapter()
        result = adapter.call_tool("service.status", {"resource": {"service_name": "plex"}})
        payload = json.loads(result.content[0]["text"])
        assert payload["result_payload"]["op"] == "status"

    def test_call_with_explicit_autonomy(self):
        adapter, _, _ = _build_adapter()
        result = adapter.call_tool(
            "service.status",
            {"resource": {"service_name": "plex"}},
            autonomy="Supervised",
        )
        assert not result.isError

    def test_call_returns_content_list(self):
        adapter, _, _ = _build_adapter()
        result = adapter.call_tool("service.status", {"resource": {"service_name": "plex"}})
        assert isinstance(result.content, list)
        assert result.content[0]["type"] == "text"


# ---------------------------------------------------------------------------
# call_tool — ReadOnly guard
# ---------------------------------------------------------------------------

class TestReadOnlyGuard:
    def test_readonly_blocks_mutation(self):
        adapter, _, _ = _build_adapter()
        result = adapter.call_tool(
            "service.start",
            {"resource": {"service_name": "plex"}},
            autonomy=Autonomy.READ_ONLY,
        )
        assert result.isError
        payload = json.loads(result.content[0]["text"])
        assert payload["error"] == "AUTONOMY_BLOCKED"

    def test_readonly_allows_status(self):
        adapter, _, _ = _build_adapter()
        result = adapter.call_tool(
            "service.status",
            {"resource": {"service_name": "plex"}},
            autonomy=Autonomy.READ_ONLY,
        )
        assert not result.isError

    def test_readonly_blocks_stop(self):
        adapter, _, _ = _build_adapter()
        result = adapter.call_tool(
            "service.stop",
            {"resource": {"service_name": "plex"}},
            autonomy=Autonomy.READ_ONLY,
        )
        assert result.isError


# ---------------------------------------------------------------------------
# call_tool — unknown tool
# ---------------------------------------------------------------------------

class TestUnknownTool:
    def test_unknown_capability(self):
        adapter, _, _ = _build_adapter()
        result = adapter.call_tool("nonexistent.tool", {})
        assert result.isError
        payload = json.loads(result.content[0]["text"])
        assert payload["error"] == "UNKNOWN_TOOL"


# ---------------------------------------------------------------------------
# call_tool — approval / risk gate integration
# ---------------------------------------------------------------------------

class TestApprovalIntegration:
    def test_high_risk_without_token_blocked(self):
        """HIGH risk action under Supervised autonomy (no token) → blocked by policy."""
        adapter, _, _ = _build_adapter(default_autonomy=Autonomy.SUPERVISED)
        result = adapter.call_tool(
            "service.start",
            {
                "resource": {"service_name": "plex"},
                "risk_level": "HIGH",
            },
        )
        # Policy blocks it → ExecutionResult with BLOCKED status
        payload = json.loads(result.content[0]["text"])
        assert payload["status"] == "BLOCKED"

    def test_high_risk_with_full_autonomy(self):
        """HIGH risk under Full autonomy with token → succeeds."""
        adapter, _, _ = _build_adapter(
            default_autonomy=Autonomy.FULL,
            approval_token="valid-tok",
        )
        result = adapter.call_tool(
            "service.start",
            {
                "resource": {"service_name": "plex"},
                "risk_level": "HIGH",
            },
        )
        payload = json.loads(result.content[0]["text"])
        assert payload["status"] == "SUCCESS"

    def test_call_token_override(self):
        """Per-call token overrides adapter default."""
        adapter, _, _ = _build_adapter(default_autonomy=Autonomy.SUPERVISED)
        result = adapter.call_tool(
            "service.start",
            {
                "resource": {"service_name": "plex"},
                "risk_level": "HIGH",
            },
            autonomy=Autonomy.FULL,
            approval_token="per-call-tok",
        )
        payload = json.loads(result.content[0]["text"])
        assert payload["status"] == "SUCCESS"


# ---------------------------------------------------------------------------
# Approval context isolation
# ---------------------------------------------------------------------------

class TestApprovalIsolation:
    def test_runtime_approval_restored_after_call(self):
        """call_tool should restore the runtime's original approval context."""
        adapter, _, runtime = _build_adapter()
        original = runtime.approval
        adapter.call_tool("service.status", {"resource": {"service_name": "plex"}})
        assert runtime.approval is original
