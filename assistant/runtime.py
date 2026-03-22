"""Assistant runtime: the main orchestration loop.

Lifecycle:  plan → (for each step: policy → route → execute → audit)
This module wires planner, router, policy, and audit together.
"""
from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any

from .audit import AuditLog
from .contracts import Action, ExecutionResult, StepStatus, TaskPlan
from .policy import ApprovalContext, PolicyViolation, enforce
from .router import Router


@dataclass
class TaskResult:
    """Aggregate result for a full task."""
    task_id: str
    results: list[ExecutionResult] = field(default_factory=list)

    @property
    def ok(self) -> bool:
        return all(r.ok for r in self.results)

    @property
    def summary(self) -> dict[str, Any]:
        return {
            "task_id": self.task_id,
            "total_steps": len(self.results),
            "succeeded": sum(1 for r in self.results if r.ok),
            "failed": sum(1 for r in self.results if r.status == StepStatus.FAILED),
            "blocked": sum(1 for r in self.results if r.status == StepStatus.BLOCKED),
        }


class Runtime:
    """Orchestrates a TaskPlan through the full pipeline."""

    def __init__(
        self,
        router: Router,
        audit: AuditLog | None = None,
        approval: ApprovalContext | None = None,
    ):
        self.router = router
        self.audit = audit or AuditLog()
        self.approval = approval

    def execute_plan(self, plan: TaskPlan) -> TaskResult:
        """Execute every step in the plan sequentially."""
        task_result = TaskResult(task_id=plan.task_id)

        for idx, action in enumerate(plan.steps):
            step_id = f"s{idx + 1}"
            event = self.audit.record_start(plan.task_id, step_id, action)

            # --- policy gate ---
            try:
                enforce(action, self.approval)
            except PolicyViolation as pv:
                self.audit.record_policy_block(event, pv.code, str(pv))
                task_result.results.append(ExecutionResult(
                    task_id=plan.task_id,
                    step_id=step_id,
                    executor_used="none",
                    status=StepStatus.BLOCKED,
                    error_code=pv.code,
                    error_message=str(pv),
                    audit_ref=event.audit_ref,
                ))
                continue

            # --- dry-run short-circuit ---
            if action.dry_run:
                result = ExecutionResult(
                    task_id=plan.task_id,
                    step_id=step_id,
                    executor_used="dry_run",
                    status=StepStatus.SUCCESS,
                    audit_ref=event.audit_ref,
                    result_payload=action.to_dict(),
                )
                self.audit.record_complete(event, result)
                task_result.results.append(result)
                continue

            # --- route + execute ---
            result = self.router.execute(action, task_id=plan.task_id, step_id=step_id)
            result.audit_ref = event.audit_ref
            self.audit.record_complete(event, result)
            task_result.results.append(result)

        return task_result

    def execute_single(self, action: Action) -> ExecutionResult:
        """Convenience: wrap a single action in a plan and execute."""
        plan = TaskPlan(goal=f"{action.domain}.{action.operation}")
        plan.add(action)
        result = self.execute_plan(plan)
        return result.results[0]
