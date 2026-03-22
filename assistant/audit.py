"""Structured audit logging for every task step.

Each step in the runtime loop emits an AuditEvent that captures
the route selected, whether fallback was used, inputs, outcome, and duration.
Events are stored in-memory with an optional flush callback for external sinks.
"""
from __future__ import annotations

import time
import uuid
from dataclasses import asdict, dataclass, field
from typing import Any, Callable

from .contracts import Action, ExecutionResult, StepStatus


@dataclass
class AuditEvent:
    """One step in the audit trail."""
    audit_ref: str = field(default_factory=lambda: uuid.uuid4().hex[:12])
    task_id: str = ""
    step_id: str = ""
    domain: str = ""
    operation: str = ""
    route_selected: str = ""
    fallback_used: bool = False
    risk_level: str = ""
    status: str = "PENDING"
    duration_ms: int = 0
    error_code: str | None = None
    error_message: str | None = None
    dry_run: bool = False
    timestamp: float = field(default_factory=time.time)
    inputs: dict[str, Any] = field(default_factory=dict)
    outputs: dict[str, Any] = field(default_factory=dict)

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)


FlushCallback = Callable[[AuditEvent], None]


class AuditLog:
    """Append-only audit trail for one task."""

    def __init__(self, flush_fn: FlushCallback | None = None):
        self._events: list[AuditEvent] = []
        self._flush_fn = flush_fn

    @property
    def events(self) -> list[AuditEvent]:
        return list(self._events)

    def record_start(self, task_id: str, step_id: str, action: Action) -> AuditEvent:
        event = AuditEvent(
            task_id=task_id,
            step_id=step_id,
            domain=action.domain,
            operation=action.operation,
            risk_level=action.risk_level.value,
            dry_run=action.dry_run,
            inputs=action.to_dict(),
        )
        self._events.append(event)
        return event

    def record_complete(
        self,
        event: AuditEvent,
        result: ExecutionResult,
    ) -> None:
        event.route_selected = result.executor_used
        event.fallback_used = result.fallback_used
        event.status = result.status.value
        event.duration_ms = result.duration_ms
        event.error_code = result.error_code
        event.error_message = result.error_message
        event.outputs = result.to_dict()
        if self._flush_fn:
            self._flush_fn(event)

    def record_policy_block(
        self,
        event: AuditEvent,
        code: str,
        message: str,
    ) -> None:
        event.status = StepStatus.BLOCKED.value
        event.error_code = code
        event.error_message = message
        if self._flush_fn:
            self._flush_fn(event)

    def find_by_task(self, task_id: str) -> list[AuditEvent]:
        return [e for e in self._events if e.task_id == task_id]

    def find_by_ref(self, audit_ref: str) -> AuditEvent | None:
        for e in self._events:
            if e.audit_ref == audit_ref:
                return e
        return None
