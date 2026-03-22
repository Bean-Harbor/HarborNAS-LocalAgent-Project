"""Executor abstraction layer.

Provides a common base and factory for building route-specific executors
(middleware API, midcli, browser, MCP) from skill manifests.  The Router
in assistant.router already defines the Executor protocol; this module
provides concrete adapter bases and a factory to wire them up.
"""
from __future__ import annotations

import time
from abc import ABC, abstractmethod
from typing import Any, Callable

from orchestrator.contracts import Action, ExecutionResult, Route, StepStatus
from .manifest import SkillManifest


class BaseExecutor(ABC):
    """Common base for all skill executors."""

    def __init__(self, skill_id: str, route: Route):
        self._skill_id = skill_id
        self._route = route

    @property
    def route(self) -> Route:
        return self._route

    @property
    def skill_id(self) -> str:
        return self._skill_id

    @abstractmethod
    def is_available(self) -> bool: ...

    @abstractmethod
    def _do_execute(self, action: Action, *, task_id: str, step_id: str) -> Any: ...

    def execute(self, action: Action, *, task_id: str, step_id: str) -> ExecutionResult:
        started = time.monotonic()
        try:
            payload = self._do_execute(action, task_id=task_id, step_id=step_id)
            duration_ms = int((time.monotonic() - started) * 1000)
            return ExecutionResult(
                task_id=task_id,
                step_id=step_id,
                executor_used=self._route.value,
                status=StepStatus.SUCCESS,
                duration_ms=duration_ms,
                result_payload=payload,
            )
        except Exception as exc:
            duration_ms = int((time.monotonic() - started) * 1000)
            return ExecutionResult(
                task_id=task_id,
                step_id=step_id,
                executor_used=self._route.value,
                status=StepStatus.FAILED,
                duration_ms=duration_ms,
                error_code=f"{self._route.value.upper()}_ERROR",
                error_message=str(exc),
            )


class CliExecutor(BaseExecutor):
    """Executes a skill via CLI command (generic shell, not midcli)."""

    def __init__(
        self,
        skill_id: str,
        *,
        run_fn: Callable[[str], tuple[str, int]] | None = None,
        command_template: str | None = None,
    ):
        super().__init__(skill_id, Route.BROWSER)  # placeholder route for generic CLI
        self._run_fn = run_fn
        self._command_template = command_template
        # Override route to a more appropriate value if needed
        self._route = Route.MIDCLI

    def is_available(self) -> bool:
        return self._run_fn is not None

    def _do_execute(self, action: Action, *, task_id: str, step_id: str) -> Any:
        cmd = self._command_template or f"{action.domain}.{action.operation}"
        stdout, _ = self._run_fn(cmd)
        return stdout


class MiddlewareApiExecutor(BaseExecutor):
    """Executes a skill via HarborOS middleware API calls."""

    def __init__(
        self,
        skill_id: str,
        *,
        call_fn: Callable[..., tuple[Any, int]] | None = None,
        allowed_methods: list[str] | None = None,
    ):
        super().__init__(skill_id, Route.MIDDLEWARE_API)
        self._call_fn = call_fn
        self._allowed_methods = set(allowed_methods or [])

    def is_available(self) -> bool:
        return self._call_fn is not None

    def _do_execute(self, action: Action, *, task_id: str, step_id: str) -> Any:
        method = f"{action.domain}.{action.operation}"
        if self._allowed_methods and action.operation not in self._allowed_methods:
            raise PermissionError(f"Operation {action.operation!r} not in allowed_methods")
        payload, _ = self._call_fn(method, action.resource, action.args)
        return payload


class MidcliSkillExecutor(BaseExecutor):
    """Executes a skill via midcli command line."""

    def __init__(
        self,
        skill_id: str,
        *,
        run_fn: Callable[[str], tuple[str, int]] | None = None,
        allowed_subcommands: list[str] | None = None,
    ):
        super().__init__(skill_id, Route.MIDCLI)
        self._run_fn = run_fn
        self._allowed_subcommands = set(allowed_subcommands or [])

    def is_available(self) -> bool:
        return self._run_fn is not None

    def _do_execute(self, action: Action, *, task_id: str, step_id: str) -> Any:
        if self._allowed_subcommands and action.operation not in self._allowed_subcommands:
            raise PermissionError(f"Subcommand {action.operation!r} not in allowed_subcommands")
        cmd = f"{action.domain} {action.operation}"
        for k, v in action.resource.items():
            cmd += f" {k}={v}"
        stdout, _ = self._run_fn(cmd)
        return stdout


class BrowserExecutor(BaseExecutor):
    """Placeholder executor for browser-based automation."""

    def __init__(self, skill_id: str, *, available: bool = False):
        super().__init__(skill_id, Route.BROWSER)
        self._available = available

    def is_available(self) -> bool:
        return self._available

    def _do_execute(self, action: Action, *, task_id: str, step_id: str) -> Any:
        raise NotImplementedError("Browser executor not yet implemented")


class McpExecutor(BaseExecutor):
    """Placeholder executor for MCP-based execution."""

    def __init__(self, skill_id: str, *, available: bool = False):
        super().__init__(skill_id, Route.MCP)
        self._available = available

    def is_available(self) -> bool:
        return self._available

    def _do_execute(self, action: Action, *, task_id: str, step_id: str) -> Any:
        raise NotImplementedError("MCP executor not yet implemented")


def executors_from_manifest(
    manifest: SkillManifest,
    *,
    api_call_fn: Callable[..., tuple[Any, int]] | None = None,
    cli_run_fn: Callable[[str], tuple[str, int]] | None = None,
) -> list[BaseExecutor]:
    """Build executor instances from a skill manifest's config.

    Returns a list of executors that are enabled in the manifest.
    The caller provides the actual call_fn / run_fn backends.
    """
    result: list[BaseExecutor] = []

    # Middleware API executor
    if manifest.harbor_api.enabled and api_call_fn:
        result.append(MiddlewareApiExecutor(
            manifest.id,
            call_fn=api_call_fn,
            allowed_methods=manifest.harbor_api.allowed_methods,
        ))

    # Midcli executor
    if manifest.harbor_cli.enabled and cli_run_fn:
        result.append(MidcliSkillExecutor(
            manifest.id,
            run_fn=cli_run_fn,
            allowed_subcommands=manifest.harbor_cli.allowed_subcommands,
        ))

    # Generic CLI executor from executors.cli
    cli_cfg = manifest.executors.get("cli")
    if cli_cfg and cli_cfg.enabled and cli_run_fn:
        result.append(CliExecutor(
            manifest.id,
            run_fn=cli_run_fn,
            command_template=cli_cfg.command,
        ))

    # Browser (placeholder)
    browser_cfg = manifest.executors.get("browser")
    if browser_cfg and browser_cfg.enabled:
        result.append(BrowserExecutor(manifest.id, available=True))

    # MCP (placeholder)
    mcp_cfg = manifest.executors.get("mcp")
    if mcp_cfg and mcp_cfg.enabled:
        result.append(McpExecutor(manifest.id, available=True))

    return result
