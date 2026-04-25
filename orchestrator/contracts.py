"""Canonical data contracts for the assistant runtime.

Action envelope and ExecutionResult follow the schema defined in
HarborBeacon-Middleware-Endpoint-Contract-v1.md and the SKILL.md
harboros-assistant-v2 spec.
"""
from __future__ import annotations

import uuid
from dataclasses import asdict, dataclass, field
from enum import Enum
from typing import Any


class RiskLevel(str, Enum):
    LOW = "LOW"
    MEDIUM = "MEDIUM"
    HIGH = "HIGH"
    CRITICAL = "CRITICAL"


class StepStatus(str, Enum):
    PENDING = "PENDING"
    APPROVED = "APPROVED"
    EXECUTING = "EXECUTING"
    SUCCESS = "SUCCESS"
    FAILED = "FAILED"
    SKIPPED = "SKIPPED"
    BLOCKED = "BLOCKED"


class Route(str, Enum):
    MIDDLEWARE_API = "middleware_api"
    MIDCLI = "midcli"
    BROWSER = "browser"
    MCP = "mcp"


ROUTE_PRIORITY: list[Route] = [
    Route.MIDDLEWARE_API,
    Route.MIDCLI,
    Route.BROWSER,
    Route.MCP,
]

CONFIRMATION_REQUIRED_LEVELS: set[RiskLevel] = {RiskLevel.HIGH, RiskLevel.CRITICAL}


@dataclass
class Action:
    """Normalised action envelope."""
    domain: str
    operation: str
    resource: dict[str, Any]
    args: dict[str, Any] = field(default_factory=dict)
    risk_level: RiskLevel = RiskLevel.LOW
    requires_approval: bool = False
    dry_run: bool = False

    def __post_init__(self) -> None:
        if isinstance(self.risk_level, str):
            self.risk_level = RiskLevel(self.risk_level)
        if self.risk_level in CONFIRMATION_REQUIRED_LEVELS:
            self.requires_approval = True

    def to_dict(self) -> dict[str, Any]:
        d = asdict(self)
        d["risk_level"] = self.risk_level.value
        return d


@dataclass
class ExecutionResult:
    """Unified result from any executor."""
    task_id: str
    step_id: str
    executor_used: str
    fallback_used: bool = False
    status: StepStatus = StepStatus.PENDING
    duration_ms: int = 0
    error_code: str | None = None
    error_message: str | None = None
    audit_ref: str = field(default_factory=lambda: uuid.uuid4().hex[:12])
    result_payload: Any = None

    def __post_init__(self) -> None:
        if isinstance(self.status, str):
            self.status = StepStatus(self.status)

    @property
    def ok(self) -> bool:
        return self.status == StepStatus.SUCCESS

    def to_dict(self) -> dict[str, Any]:
        d = asdict(self)
        d["status"] = self.status.value
        return d


@dataclass
class TaskPlan:
    """A plan is a sequence of actions with dependencies."""
    task_id: str = field(default_factory=lambda: uuid.uuid4().hex)
    goal: str = ""
    steps: list[Action] = field(default_factory=list)

    def add(self, action: Action) -> str:
        step_id = f"s{len(self.steps) + 1}"
        self.steps.append(action)
        return step_id
