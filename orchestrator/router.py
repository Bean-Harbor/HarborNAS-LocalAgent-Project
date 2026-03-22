"""Deterministic route selection with fallback.

Route priority is fixed:  middleware_api -> midcli -> browser -> mcp
For HarborOS domain operations (service, files), browser/MCP are excluded
unless explicitly enabled.
"""
from __future__ import annotations

from typing import Protocol

from .contracts import Action, ExecutionResult, Route, ROUTE_PRIORITY, StepStatus


class Executor(Protocol):
    """Interface that every route executor must implement."""

    @property
    def route(self) -> Route: ...

    def is_available(self) -> bool: ...

    def execute(self, action: Action, *, task_id: str, step_id: str) -> ExecutionResult: ...


# Domains restricted to API/CLI routes only
HARBOROS_DOMAINS = {"service", "files"}


def allowed_routes(action: Action) -> list[Route]:
    """Return the ordered list of routes allowed for this action."""
    if action.domain in HARBOROS_DOMAINS:
        return [r for r in ROUTE_PRIORITY if r in (Route.MIDDLEWARE_API, Route.MIDCLI)]
    return list(ROUTE_PRIORITY)


class Router:
    """Selects the best available executor for an action and falls back."""

    def __init__(self, executors: list[Executor] | None = None):
        self._executors: dict[Route, Executor] = {}
        for ex in (executors or []):
            self._executors[ex.route] = ex

    def register(self, executor: Executor) -> None:
        self._executors[executor.route] = executor

    def resolve(self, action: Action) -> tuple[Executor | None, bool]:
        """Return (executor, fallback_used). Returns (None, False) if nothing available."""
        routes = allowed_routes(action)
        primary = True
        for r in routes:
            ex = self._executors.get(r)
            if ex and ex.is_available():
                return ex, not primary
            primary = False
        return None, False

    def execute(self, action: Action, *, task_id: str, step_id: str) -> ExecutionResult:
        """Route the action to the best executor; try fallbacks on failure."""
        routes = allowed_routes(action)
        last_error: Exception | None = None
        fallback_used = False

        for idx, r in enumerate(routes):
            ex = self._executors.get(r)
            if not ex or not ex.is_available():
                continue
            if idx > 0:
                fallback_used = True
            try:
                result = ex.execute(action, task_id=task_id, step_id=step_id)
                result.fallback_used = fallback_used
                return result
            except Exception as exc:
                last_error = exc
                continue

        return ExecutionResult(
            task_id=task_id,
            step_id=step_id,
            executor_used="none",
            fallback_used=fallback_used,
            status=StepStatus.FAILED,
            error_code="NO_EXECUTOR_AVAILABLE",
            error_message=str(last_error) if last_error else "No executor available for this action",
        )
