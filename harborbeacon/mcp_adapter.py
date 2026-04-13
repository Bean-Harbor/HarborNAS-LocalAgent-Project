"""MCP server adapter: expose HarborOS skills as MCP tools.

Implements the Model Context Protocol (MCP) tool interface so that
HarborBeacon (running locally inside HarborOS) can:
  1. List available tools  →  ``list_tools()``
  2. Call a tool           →  ``call_tool(name, arguments)``

Each skill capability (e.g. ``service.status``) becomes one MCP tool.
The adapter translates MCP request ↔ Action/ExecutionResult.
"""
from __future__ import annotations

import importlib.util
import json
import os
import uuid
from pathlib import Path
from dataclasses import asdict, dataclass, field
from typing import Any

from orchestrator.contracts import Action, ExecutionResult, RiskLevel, StepStatus
from orchestrator.runtime import Runtime
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

    Runs locally inside HarborOS alongside HarborBeacon.  The ChannelRouter
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
        try:
            action = self._build_action(name, arguments)
        except ValueError as exc:
            if name == "photo.upload_to_nas":
                resource = arguments.get("resource", {}) if isinstance(arguments, dict) else {}
                args = arguments.get("args", {}) if isinstance(arguments, dict) else {}
                result = ExecutionResult(
                    task_id=uuid.uuid4().hex,
                    step_id="s1",
                    executor_used="mcp",
                    status=StepStatus.FAILED,
                    error_code="VALIDATION_ERROR",
                    error_message=str(exc),
                )
                photo_action = Action(
                    domain="files",
                    operation="copy",
                    resource=resource,
                    args=args,
                    risk_level=RiskLevel.MEDIUM,
                )
                result = self._enrich_execution_result(name, photo_action, result)
                payload = result.to_dict()
                payload["error"] = result.error_code
                payload["message"] = result.error_message
                return McpToolResult(
                    content=[{"type": "text", "text": json.dumps(payload, default=str)}],
                    isError=True,
                )
            return McpToolResult(
                content=[{"type": "text", "text": json.dumps({
                    "error": "VALIDATION_ERROR",
                    "message": str(exc),
                })}],
                isError=True,
            )
        manifest = self._resolve_manifest(name)
        if action is None:
            return McpToolResult(
                content=[{"type": "text", "text": json.dumps({
                    "error": "UNKNOWN_TOOL",
                    "message": f"No skill provides capability {name!r}",
                })}],
                isError=True,
            )

        if manifest and self._supports_local_handler(manifest):
            result = self._execute_local_handler(manifest, action)
            return self._result_to_mcp(result)

        # Build approval context from HarborBeacon autonomy
        approval = autonomy_to_approval(autonomy, token=token)

        # Patch runtime approval for this call
        prev_approval = self._runtime.approval
        self._runtime.approval = approval
        try:
            result = self._runtime.execute_single(action)
        finally:
            self._runtime.approval = prev_approval

        result = self._enrich_execution_result(name, action, result)
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
        manifest = self._resolve_manifest(name)
        if manifest is None:
            return None

        resource = arguments.get("resource", {})
        args = arguments.get("args", {})
        risk_str = arguments.get("risk_level", manifest.risk.default_level)
        dry_run = arguments.get("dry_run", False)
        domain, operation = self._normalize_tool_action(name, resource, args)

        return Action(
            domain=domain,
            operation=operation,
            resource=resource,
            args=args,
            risk_level=RiskLevel(risk_str),
            dry_run=bool(dry_run),
        )

    def _resolve_manifest(self, name: str) -> SkillManifest | None:
        skills = self._registry.find_by_capability(name)
        if not skills:
            return None
        return skills[0]

    @staticmethod
    def _normalize_tool_action(
        name: str,
        resource: dict[str, Any],
        args: dict[str, Any],
    ) -> tuple[str, str]:
        if name == "photo.upload_to_nas":
            source_path = resource.get("source_path") or args.get("source_path", "")
            attachment_key = resource.get("attachment_key") or args.get("source_attachment", "")
            target_dir = str(args.get("target_dir") or os.environ.get("HARBOR_IM_UPLOAD_DIR", "")).strip()
            if not target_dir:
                raise ValueError(
                    "photo.upload_to_nas requires args.target_dir or environment variable HARBOR_IM_UPLOAD_DIR"
                )
            file_name = (
                resource.get("file_name")
                or resource.get("local_file_name")
                or f"{attachment_key or 'upload'}.bin"
            )
            normalized_source = source_path or attachment_key or resource.get("source", "")
            normalized_destination = (
                f"{target_dir.rstrip('/')}/{file_name}" if target_dir else file_name
            )

            resource.setdefault("source", normalized_source)
            resource.setdefault("destination", normalized_destination)
            args.setdefault("source_channel", resource.get("source_channel", ""))
            args.setdefault(
                "source_message_id",
                resource.get("source_message_id", resource.get("message_id", "")),
            )
            args.setdefault("target_dir", target_dir)
            if source_path:
                args.setdefault("source_path", source_path)

            return "files", "copy"

        return name.split(".", 1) if "." in name else (name, name)

    @staticmethod
    def _supports_local_handler(manifest: SkillManifest) -> bool:
        if not manifest.source_path:
            return False
        if manifest.harbor_api.enabled or manifest.harbor_cli.enabled:
            return False
        handler_path = Path(manifest.source_path).with_name("handler.py")
        return handler_path.is_file()

    def _execute_local_handler(self, manifest: SkillManifest, action: Action) -> ExecutionResult:
        handler_path = Path(manifest.source_path or "").with_name("handler.py")
        spec = importlib.util.spec_from_file_location(f"{manifest.id}_handler", handler_path)
        if spec is None or spec.loader is None:
            return ExecutionResult(
                task_id=uuid.uuid4().hex,
                step_id="s1",
                executor_used="local_handler",
                status="FAILED",
                error_code="LOCAL_HANDLER_LOAD_ERROR",
                error_message=f"Could not load local handler for {manifest.id}",
            )

        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        handle = getattr(module, "handle", None)
        if handle is None:
            return ExecutionResult(
                task_id=uuid.uuid4().hex,
                step_id="s1",
                executor_used="local_handler",
                status="FAILED",
                error_code="LOCAL_HANDLER_MISSING",
                error_message=f"Local handler missing handle() for {manifest.id}",
            )

        try:
            payload = handle(action.operation, **action.resource, **action.args)
        except Exception as exc:
            return ExecutionResult(
                task_id=uuid.uuid4().hex,
                step_id="s1",
                executor_used="local_handler",
                status="FAILED",
                error_code="LOCAL_HANDLER_ERROR",
                error_message=str(exc),
            )

        if isinstance(payload, dict) and payload.get("error"):
            return ExecutionResult(
                task_id=uuid.uuid4().hex,
                step_id="s1",
                executor_used="local_handler",
                status="FAILED",
                error_code="LOCAL_HANDLER_ERROR",
                error_message=str(payload.get("error")),
                result_payload=payload,
            )

        return ExecutionResult(
            task_id=uuid.uuid4().hex,
            step_id="s1",
            executor_used="local_handler",
            status="SUCCESS",
            result_payload=payload,
        )

    @staticmethod
    def _enrich_execution_result(
        tool_name: str,
        action: Action,
        result: ExecutionResult,
    ) -> ExecutionResult:
        if tool_name != "photo.upload_to_nas":
            return result

        payload = (
            result.result_payload
            if isinstance(result.result_payload, dict)
            else {"result": result.result_payload}
        )
        target_dir = action.args.get("target_dir") or os.environ.get("HARBOR_IM_UPLOAD_DIR", "")
        file_name = action.resource.get("file_name") or action.resource.get("local_file_name") or ""
        target_path = action.resource.get("destination") or (
            f"{str(target_dir).rstrip('/')}/{file_name}" if target_dir and file_name else str(target_dir or "")
        )
        size_bytes = action.resource.get("size_bytes")
        source_message_id = action.args.get("source_message_id") or action.resource.get("source_message_id", "")
        source_channel = action.args.get("source_channel") or action.resource.get("source_channel", "")

        error_category, error_title, error_hint = McpServerAdapter._classify_photo_upload_error(result)
        result.result_payload = {
            "operation": "photo.upload_to_nas",
            "target_path": target_path,
            "target_dir": target_dir,
            "file_name": file_name,
            "size_bytes": size_bytes,
            "source_channel": source_channel,
            "source_message_id": source_message_id,
            "task_id": result.task_id,
            "trace_id": result.audit_ref,
            "executor_result": payload,
        }
        if not result.ok:
            result.result_payload["error_category"] = error_category
            result.result_payload["error_title"] = error_title
            result.result_payload["error_hint"] = error_hint
        return result

    @staticmethod
    def _classify_photo_upload_error(result: ExecutionResult) -> tuple[str, str, str]:
        if result.error_code == "VALIDATION_ERROR":
            return (
                "configuration",
                "照片上传未完成，尚未配置 NAS 目标目录",
                "请设置 HARBOR_IM_UPLOAD_DIR，或在调用时传入 args.target_dir",
            )
        if result.status == StepStatus.BLOCKED or result.error_code == "APPROVAL_REQUIRED":
            return (
                "approval",
                "照片上传已被拦截，等待确认",
                "批准该操作后可继续上传照片",
            )
        if result.error_code == "NO_EXECUTOR_AVAILABLE":
            return (
                "routing",
                "照片上传未完成，当前没有可用执行链路",
                "请检查 HarborOS 路由配置，以及中间件或命令执行器是否可用",
            )
        return (
            "execution",
            "照片上传失败",
            "请检查目标目录权限、文件执行链路与 HarborOS 运行日志",
        )

    @staticmethod
    def _result_to_mcp(result: ExecutionResult) -> McpToolResult:
        """Convert an ExecutionResult to an MCP tool result."""
        payload = result.to_dict()
        return McpToolResult(
            content=[{"type": "text", "text": json.dumps(payload, default=str)}],
            isError=not result.ok,
        )
