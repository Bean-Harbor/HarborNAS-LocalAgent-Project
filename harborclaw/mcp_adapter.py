"""MCP server adapter: expose HarborOS skills as MCP tools.

Implements the Model Context Protocol (MCP) tool interface so that
HarborClaw (running locally inside HarborOS) can:
  1. List available tools  →  ``list_tools()``
  2. Call a tool           →  ``call_tool(name, arguments)``

Each skill capability (e.g. ``service.status``) becomes one MCP tool.
The adapter translates MCP request ↔ Action/ExecutionResult.
"""
from __future__ import annotations

import json
from dataclasses import asdict, dataclass, field
from typing import Any

from assistant.contracts import Action, ExecutionResult, RiskLevel
from assistant.runtime import Runtime
from skills.manifest import SkillManifest
from skills.registry import Registry

from .autonomy import Autonomy, autonomy_to_approval, is_read_only_safe


# ---------------------------------------------------------------------------
# MCP tool schema types (subset of the MCP spec)
# ---------------------------------------------------------------------------

@dataclass
class McpToolSchema:
    """MCP tool definition returned by tools/list."""
    name: str
    description: str
    inputSchema: dict[str, Any] = field(default_factory=dict)


@dataclass
class McpToolResult:
    """MCP tool call result returned by tools/call."""
    content: list[dict[str, Any]] = field(default_factory=list)
    isError: bool = False


# ---------------------------------------------------------------------------
# Adapter
# ---------------------------------------------------------------------------

class McpServerAdapter:
    """Bridges our Runtime+Registry to the MCP tool interface.

    Runs locally inside HarborOS alongside HarborClaw.  The ChannelRouter
    calls ``call_tool()`` after parsing user intent from IM messages.

    Usage::

        adapter = McpServerAdapter(registry, runtime)
        tools = adapter.list_tools()
        result = adapter.call_tool("service.status", {"service_name": "plex"})
    """

    def __init__(
        self,
        registry: Registry,
        runtime: Runtime,
        *,
        default_autonomy: Autonomy = Autonomy.SUPERVISED,
        approval_token: str | None = None,
    ):
        self._registry = registry
        self._runtime = runtime
        self._default_autonomy = default_autonomy
        self._approval_token = approval_token

    # ---- MCP tools/list ----

    def list_tools(self) -> list[McpToolSchema]:
        """Return an MCP tool definition for every registered capability."""
        tools: list[McpToolSchema] = []
        seen: set[str] = set()

        for manifest in self._registry.skills:
            for cap in manifest.capabilities:
                if cap in seen:
                    continue
                seen.add(cap)
                tools.append(self._capability_to_tool(manifest, cap))
        return tools

    # ---- MCP tools/call ----

    def call_tool(
        self,
        name: str,
        arguments: dict[str, Any] | None = None,
        *,
        autonomy: Autonomy | str | None = None,
        approval_token: str | None = None,
    ) -> McpToolResult:
        """Execute a tool call from an MCP client.

        ``name`` is a capability string like ``service.status``.
        ``arguments`` is the dict sent by the MCP client.
        """
        arguments = arguments or {}
        autonomy = Autonomy(autonomy) if isinstance(autonomy, str) else (autonomy or self._default_autonomy)
        token = approval_token or self._approval_token

        # ReadOnly guard: reject mutations before even entering the runtime
        if autonomy == Autonomy.READ_ONLY:
            operation = name.rsplit(".", 1)[-1] if "." in name else name
            if not is_read_only_safe(operation):
                return McpToolResult(
                    content=[{"type": "text", "text": json.dumps({
                        "error": "AUTONOMY_BLOCKED",
                        "message": f"Operation {name!r} not allowed in ReadOnly mode",
                    })}],
                    isError=True,
                )

        # Resolve the capability → domain + operation
        action = self._build_action(name, arguments)
        if action is None:
            return McpToolResult(
                content=[{"type": "text", "text": json.dumps({
                    "error": "UNKNOWN_TOOL",
                    "message": f"No skill provides capability {name!r}",
                })}],
                isError=True,
            )

        # Build approval context from HarborClaw autonomy
        approval = autonomy_to_approval(autonomy, token=token)

        # Patch runtime approval for this call
        prev_approval = self._runtime.approval
        self._runtime.approval = approval
        try:
            result = self._runtime.execute_single(action)
        finally:
            self._runtime.approval = prev_approval

        return self._result_to_mcp(result)

    # ---- conversion helpers ----

    def _capability_to_tool(self, manifest: SkillManifest, capability: str) -> McpToolSchema:
        """Convert a skill capability into an MCP tool schema."""
        domain, operation = capability.split(".", 1) if "." in capability else (capability, "")
        description = f"{manifest.name}: {operation or capability}"
        if manifest.summary:
            description = f"{manifest.summary} — {operation}"

        # Build input schema from manifest's input_schema if available
        input_schema: dict[str, Any] = {"type": "object", "properties": {}}
        if manifest.input_schema:
            input_schema = dict(manifest.input_schema)

        # Always accept these top-level fields
        props = input_schema.setdefault("properties", {})
        props.setdefault("resource", {"type": "object", "description": "Target resource"})
        props.setdefault("args", {"type": "object", "description": "Additional arguments"})

        return McpToolSchema(
            name=capability,
            description=description,
            inputSchema=input_schema,
        )

    def _build_action(self, name: str, arguments: dict[str, Any]) -> Action | None:
        """Map an MCP tool name + arguments to an Action."""
        # Check that at least one skill provides this capability
        skills = self._registry.find_by_capability(name)
        if not skills:
            return None

        manifest = skills[0]  # first match

        domain, operation = name.split(".", 1) if "." in name else (name, name)
        resource = arguments.get("resource", {})
        args = arguments.get("args", {})
        risk_str = arguments.get("risk_level", manifest.risk.default_level)
        dry_run = arguments.get("dry_run", False)

        return Action(
            domain=domain,
            operation=operation,
            resource=resource,
            args=args,
            risk_level=RiskLevel(risk_str),
            dry_run=bool(dry_run),
        )

    @staticmethod
    def _result_to_mcp(result: ExecutionResult) -> McpToolResult:
        """Convert an ExecutionResult to an MCP tool result."""
        payload = result.to_dict()
        return McpToolResult(
            content=[{"type": "text", "text": json.dumps(payload, default=str)}],
            isError=not result.ok,
        )
