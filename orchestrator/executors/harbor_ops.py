"""system.harbor_ops executors: middleware API and midcli adapters.

These implement the router.Executor protocol so the Router can
dispatch service-domain actions through the deterministic route chain.
"""
from __future__ import annotations

import json
import time
from typing import Any

from ..contracts import Action, ExecutionResult, Route, StepStatus


# ---------------------------------------------------------------------------
# Middleware API executor
# ---------------------------------------------------------------------------

class MiddlewareExecutor:
    """Executes service actions via HarborOS middleware API (midclt call)."""

    route = Route.MIDDLEWARE_API

    def __init__(
        self,
        *,
        call_fn: Any | None = None,
        available: bool = True,
    ):
        # call_fn signature: (method: str, *args) -> (payload, duration_ms)
        # When not provided, executor reports unavailable.
        self._call_fn = call_fn
        self._available = available

    def is_available(self) -> bool:
        return self._available and self._call_fn is not None

    def execute(self, action: Action, *, task_id: str, step_id: str) -> ExecutionResult:
        started = time.monotonic()
        service_name = action.resource.get("service_name", "")
        try:
            method, args = _map_service_operation(action.operation, service_name, action.args)
            payload, api_duration = self._call_fn(method, *args)
            duration_ms = int((time.monotonic() - started) * 1000)
            return ExecutionResult(
                task_id=task_id,
                step_id=step_id,
                executor_used=self.route.value,
                status=StepStatus.SUCCESS,
                duration_ms=duration_ms,
                result_payload=payload,
            )
        except Exception as exc:
            duration_ms = int((time.monotonic() - started) * 1000)
            return ExecutionResult(
                task_id=task_id,
                step_id=step_id,
                executor_used=self.route.value,
                status=StepStatus.FAILED,
                duration_ms=duration_ms,
                error_code="MIDDLEWARE_ERROR",
                error_message=str(exc),
            )


# ---------------------------------------------------------------------------
# MidCLI executor
# ---------------------------------------------------------------------------

class MidcliExecutor:
    """Executes service actions via midcli command line."""

    route = Route.MIDCLI

    def __init__(
        self,
        *,
        run_fn: Any | None = None,
        available: bool = True,
    ):
        # run_fn signature: (command: str) -> (stdout: str, duration_ms: int)
        self._run_fn = run_fn
        self._available = available

    def is_available(self) -> bool:
        return self._available and self._run_fn is not None

    def execute(self, action: Action, *, task_id: str, step_id: str) -> ExecutionResult:
        started = time.monotonic()
        service_name = action.resource.get("service_name", "")
        try:
            command = _build_midcli_command(action.operation, service_name, action.args)
            stdout, cli_duration = self._run_fn(command)
            duration_ms = int((time.monotonic() - started) * 1000)
            return ExecutionResult(
                task_id=task_id,
                step_id=step_id,
                executor_used=self.route.value,
                status=StepStatus.SUCCESS,
                duration_ms=duration_ms,
                result_payload=stdout,
            )
        except Exception as exc:
            duration_ms = int((time.monotonic() - started) * 1000)
            return ExecutionResult(
                task_id=task_id,
                step_id=step_id,
                executor_used=self.route.value,
                status=StepStatus.FAILED,
                duration_ms=duration_ms,
                error_code="MIDCLI_ERROR",
                error_message=str(exc),
            )


# ---------------------------------------------------------------------------
# Helpers — mapping operations to middleware methods / midcli commands
# ---------------------------------------------------------------------------

_MIDDLEWARE_SERVICE_MAP: dict[str, str] = {
    "status": "service.query",
    "start": "service.start",
    "stop": "service.stop",
    "restart": "service.restart",
    "enable": "service.update",
}


def _map_service_operation(
    operation: str, service_name: str, args: dict[str, Any]
) -> tuple[str, list[Any]]:
    method = _MIDDLEWARE_SERVICE_MAP.get(operation)
    if method is None:
        raise ValueError(f"Unmapped service operation: {operation}")

    if operation == "status":
        return method, [service_name]
    if operation in ("start", "stop", "restart"):
        return f"service.control", [operation.upper(), service_name, {}]
    if operation == "enable":
        enable_val = args.get("enable", True)
        return method, [service_name, {"enable": enable_val}]
    raise ValueError(f"Unmapped service operation: {operation}")


_MIDCLI_SERVICE_MAP: dict[str, str] = {
    "status": "service {name} show",
    "start": "service start service={name}",
    "stop": "service stop service={name}",
    "restart": "service restart service={name}",
    "enable": "service update id_or_name={name} enable={enable}",
}


def _build_midcli_command(
    operation: str, service_name: str, args: dict[str, Any]
) -> str:
    template = _MIDCLI_SERVICE_MAP.get(operation)
    if template is None:
        raise ValueError(f"Unmapped midcli operation: {operation}")
    enable_val = str(args.get("enable", True)).lower()
    return template.format(name=service_name, enable=enable_val)
