"""system.harbor_ops executors: middleware API and midcli adapters.

These implement the router.Executor protocol so the Router can
dispatch service-domain and files-domain actions through the deterministic
route chain.
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

    def supports(self, action: Action) -> bool:
        return action.domain in {"service", "files"}

    def execute(self, action: Action, *, task_id: str, step_id: str) -> ExecutionResult:
        started = time.monotonic()
        try:
            method, args = _map_middleware_action(action)
            payload, _ = self._call_fn(method, *args)
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

    def supports(self, action: Action) -> bool:
        return action.domain in {"service", "files"}

    def execute(self, action: Action, *, task_id: str, step_id: str) -> ExecutionResult:
        started = time.monotonic()
        try:
            command = _build_midcli_command(action)
            stdout, _ = self._run_fn(command)
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


def _map_middleware_action(action: Action) -> tuple[str, list[Any]]:
    if action.domain == "service":
        service_name = action.resource.get("service_name", "")
        return _map_service_operation(action.operation, service_name, action.args)

    if action.domain == "files":
        return _map_files_operation(action.operation, action.resource, action.args)

    raise ValueError(f"Unmapped action domain: {action.domain}")


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


def _map_files_operation(
    operation: str,
    resource: dict[str, Any],
    args: dict[str, Any],
) -> tuple[str, list[Any]]:
    source = resource.get("source", "")
    destination = resource.get("destination", "")
    recursive = bool(args.get("recursive", False))

    if operation == "copy":
        return "filesystem.copy", [source, destination, {"recursive": recursive, "preserve_attrs": False}]
    if operation == "move":
        return "filesystem.move", [[source], destination, {"recursive": recursive}]

    raise ValueError(f"Unmapped files operation: {operation}")


_MIDCLI_SERVICE_MAP: dict[str, str] = {
    "status": "service {name} show",
    "start": "service start service={name}",
    "stop": "service stop service={name}",
    "restart": "service restart service={name}",
    "enable": "service update id_or_name={name} enable={enable}",
}


def _build_midcli_command(action: Action) -> str:
    if action.domain == "service":
        service_name = action.resource.get("service_name", "")
        template = _MIDCLI_SERVICE_MAP.get(action.operation)
        if template is None:
            raise ValueError(f"Unmapped midcli operation: {action.operation}")
        enable_val = str(action.args.get("enable", True)).lower()
        return template.format(name=service_name, enable=enable_val)

    if action.domain == "files":
        return _build_midcli_files_command(action.operation, action.resource, action.args)

    raise ValueError(f"Unmapped action domain: {action.domain}")


def _build_midcli_files_command(
    operation: str,
    resource: dict[str, Any],
    args: dict[str, Any],
) -> str:
    source = json.dumps(resource.get("source", ""))
    destination = json.dumps(resource.get("destination", ""))
    recursive = bool(args.get("recursive", False))

    if operation == "copy":
        command = f"filesystem copy src={source} dst={destination}"
    elif operation == "move":
        command = f"filesystem move src={source} dst={destination}"
    else:
        raise ValueError(f"Unmapped files operation: {operation}")

    if recursive:
        command += " recursive=true"
    return command
