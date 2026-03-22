"""Tests for assistant.runtime — the full planner→router→policy→executor→audit loop."""
from assistant.audit import AuditLog
from assistant.contracts import Action, ExecutionResult, RiskLevel, Route, StepStatus, TaskPlan
from assistant.policy import ApprovalContext
from assistant.router import Router
from assistant.runtime import Runtime


# --- stub executor (reusable) ---

class FakeExecutor:
    def __init__(self, route: Route, available: bool = True):
        self.route = route
        self._available = available
        self.executed_actions: list[Action] = []

    def is_available(self) -> bool:
        return self._available

    def execute(self, action: Action, *, task_id: str, step_id: str) -> ExecutionResult:
        self.executed_actions.append(action)
        return ExecutionResult(
            task_id=task_id,
            step_id=step_id,
            executor_used=self.route.value,
            status=StepStatus.SUCCESS,
            duration_ms=10,
            result_payload={"state": "RUNNING"},
        )


def _make_runtime(approval=None, mw_available=True, cli_available=True):
    mw = FakeExecutor(Route.MIDDLEWARE_API, available=mw_available)
    cli = FakeExecutor(Route.MIDCLI, available=cli_available)
    router = Router([mw, cli])
    audit = AuditLog()
    rt = Runtime(router=router, audit=audit, approval=approval)
    return rt, mw, cli, audit


# --- happy path ---

def test_single_low_risk_action_succeeds():
    rt, mw, cli, audit = _make_runtime()
    action = Action(domain="service", operation="status", resource={"service_name": "ssh"})
    result = rt.execute_single(action)
    assert result.ok
    assert result.executor_used == "middleware_api"
    assert len(audit.events) == 1
    assert audit.events[0].status == "SUCCESS"


def test_multi_step_plan():
    rt, mw, cli, audit = _make_runtime(
        approval=ApprovalContext(token="tok", required_token="tok")
    )
    plan = TaskPlan(goal="enable and restart ssh")
    plan.add(Action(domain="service", operation="enable", resource={"service_name": "ssh"}, args={"enable": True}, risk_level=RiskLevel.MEDIUM))
    plan.add(Action(domain="service", operation="restart", resource={"service_name": "ssh"}, risk_level=RiskLevel.HIGH))
    task_result = rt.execute_plan(plan)
    assert task_result.ok
    assert len(task_result.results) == 2
    assert task_result.results[0].executor_used == "middleware_api"
    assert task_result.results[1].executor_used == "middleware_api"
    assert len(audit.events) == 2


# --- policy blocks ---

def test_high_risk_blocked_without_approval():
    rt, mw, cli, audit = _make_runtime()  # no approval
    action = Action(domain="service", operation="stop", resource={"service_name": "ssh"}, risk_level=RiskLevel.HIGH)
    result = rt.execute_single(action)
    assert not result.ok
    assert result.status == StepStatus.BLOCKED
    assert result.error_code == "APPROVAL_REQUIRED"
    assert audit.events[0].status == "BLOCKED"


def test_invalid_service_name_blocked():
    rt, mw, cli, audit = _make_runtime()
    action = Action(domain="service", operation="status", resource={"service_name": "../../etc"})
    result = rt.execute_single(action)
    assert not result.ok
    assert result.status == StepStatus.BLOCKED
    assert result.error_code == "INVALID_SERVICE_NAME"


def test_unsupported_operation_blocked():
    rt, mw, cli, audit = _make_runtime()
    action = Action(domain="service", operation="destroy", resource={"service_name": "ssh"})
    result = rt.execute_single(action)
    assert not result.ok
    assert result.error_code == "UNSUPPORTED_OPERATION"


# --- fallback ---

def test_fallback_to_midcli_when_middleware_unavailable():
    rt, mw, cli, audit = _make_runtime(mw_available=False)
    action = Action(domain="service", operation="status", resource={"service_name": "ssh"})
    result = rt.execute_single(action)
    assert result.ok
    assert result.executor_used == "midcli"
    assert result.fallback_used is True
    assert audit.events[0].fallback_used is True


# --- dry-run ---

def test_dry_run_does_not_execute():
    rt, mw, cli, audit = _make_runtime()
    action = Action(domain="service", operation="start", resource={"service_name": "ssh"}, risk_level=RiskLevel.MEDIUM, dry_run=True)
    result = rt.execute_single(action)
    assert result.ok
    assert result.executor_used == "dry_run"
    assert len(mw.executed_actions) == 0
    assert audit.events[0].status == "SUCCESS"


# --- audit completeness ---

def test_every_step_produces_audit_with_route_and_fallback():
    rt, mw, cli, audit = _make_runtime(
        approval=ApprovalContext(token="t", required_token="t"),
        mw_available=False,
    )
    plan = TaskPlan(goal="start and stop")
    plan.add(Action(domain="service", operation="start", resource={"service_name": "ssh"}, risk_level=RiskLevel.MEDIUM))
    plan.add(Action(domain="service", operation="stop", resource={"service_name": "ssh"}, risk_level=RiskLevel.HIGH))
    rt.execute_plan(plan)
    assert len(audit.events) == 2
    for ev in audit.events:
        assert ev.route_selected in ("midcli", "none")
        assert ev.audit_ref  # non-empty


def test_task_result_summary():
    rt, mw, cli, audit = _make_runtime()
    plan = TaskPlan(goal="mixed")
    plan.add(Action(domain="service", operation="status", resource={"service_name": "ssh"}))  # will succeed
    plan.add(Action(domain="service", operation="stop", resource={"service_name": "ssh"}, risk_level=RiskLevel.HIGH))  # will be blocked
    task_result = rt.execute_plan(plan)
    s = task_result.summary
    assert s["total_steps"] == 2
    assert s["succeeded"] == 1
    assert s["blocked"] == 1
