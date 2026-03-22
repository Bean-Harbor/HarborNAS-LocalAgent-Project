"""Tests for assistant.contracts"""
from orchestrator.contracts import (
    Action,
    CONFIRMATION_REQUIRED_LEVELS,
    ExecutionResult,
    RiskLevel,
    Route,
    ROUTE_PRIORITY,
    StepStatus,
    TaskPlan,
)


def test_action_auto_sets_requires_approval_for_high():
    a = Action(domain="service", operation="stop", resource={"service_name": "ssh"}, risk_level=RiskLevel.HIGH)
    assert a.requires_approval is True


def test_action_auto_sets_requires_approval_for_critical():
    a = Action(domain="service", operation="stop", resource={"service_name": "ssh"}, risk_level=RiskLevel.CRITICAL)
    assert a.requires_approval is True


def test_action_does_not_require_approval_for_low():
    a = Action(domain="service", operation="status", resource={"service_name": "ssh"}, risk_level=RiskLevel.LOW)
    assert a.requires_approval is False


def test_action_accepts_string_risk_level():
    a = Action(domain="service", operation="start", resource={"service_name": "ssh"}, risk_level="MEDIUM")
    assert a.risk_level == RiskLevel.MEDIUM
    assert a.requires_approval is False


def test_action_to_dict_round_trips():
    a = Action(domain="service", operation="restart", resource={"service_name": "ssh"}, risk_level=RiskLevel.HIGH)
    d = a.to_dict()
    assert d["risk_level"] == "HIGH"
    assert d["requires_approval"] is True
    assert d["domain"] == "service"


def test_execution_result_ok_only_on_success():
    r = ExecutionResult(task_id="t1", step_id="s1", executor_used="middleware_api", status=StepStatus.SUCCESS)
    assert r.ok is True
    r2 = ExecutionResult(task_id="t1", step_id="s1", executor_used="middleware_api", status=StepStatus.FAILED)
    assert r2.ok is False


def test_execution_result_to_dict():
    r = ExecutionResult(task_id="t1", step_id="s1", executor_used="midcli", status=StepStatus.BLOCKED)
    d = r.to_dict()
    assert d["status"] == "BLOCKED"
    assert d["executor_used"] == "midcli"


def test_route_priority_order():
    assert ROUTE_PRIORITY == [Route.MIDDLEWARE_API, Route.MIDCLI, Route.BROWSER, Route.MCP]


def test_task_plan_add():
    plan = TaskPlan(goal="test")
    a = Action(domain="service", operation="status", resource={"service_name": "ssh"})
    sid = plan.add(a)
    assert sid == "s1"
    assert len(plan.steps) == 1
    plan.add(Action(domain="service", operation="start", resource={"service_name": "ssh"}, risk_level="MEDIUM"))
    assert len(plan.steps) == 2
