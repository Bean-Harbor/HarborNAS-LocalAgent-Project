"""Tests for assistant.audit"""
from orchestrator.audit import AuditEvent, AuditLog
from orchestrator.contracts import Action, ExecutionResult, RiskLevel, StepStatus


def test_record_start_creates_event():
    log = AuditLog()
    action = Action(domain="service", operation="start", resource={"service_name": "ssh"}, risk_level=RiskLevel.MEDIUM)
    event = log.record_start("task1", "s1", action)
    assert event.task_id == "task1"
    assert event.step_id == "s1"
    assert event.domain == "service"
    assert event.operation == "start"
    assert event.risk_level == "MEDIUM"
    assert len(log.events) == 1


def test_record_complete_updates_event():
    log = AuditLog()
    action = Action(domain="service", operation="status", resource={"service_name": "ssh"})
    event = log.record_start("t1", "s1", action)
    result = ExecutionResult(
        task_id="t1", step_id="s1", executor_used="middleware_api",
        status=StepStatus.SUCCESS, duration_ms=42,
    )
    log.record_complete(event, result)
    assert event.route_selected == "middleware_api"
    assert event.status == "SUCCESS"
    assert event.duration_ms == 42
    assert event.fallback_used is False


def test_record_policy_block():
    log = AuditLog()
    action = Action(domain="service", operation="stop", resource={"service_name": "ssh"}, risk_level=RiskLevel.HIGH)
    event = log.record_start("t1", "s1", action)
    log.record_policy_block(event, "APPROVAL_REQUIRED", "needs approval")
    assert event.status == "BLOCKED"
    assert event.error_code == "APPROVAL_REQUIRED"


def test_find_by_task():
    log = AuditLog()
    a1 = Action(domain="service", operation="start", resource={"service_name": "ssh"})
    a2 = Action(domain="service", operation="stop", resource={"service_name": "smb"}, risk_level=RiskLevel.HIGH)
    log.record_start("t1", "s1", a1)
    log.record_start("t2", "s1", a2)
    assert len(log.find_by_task("t1")) == 1
    assert len(log.find_by_task("t2")) == 1


def test_find_by_ref():
    log = AuditLog()
    action = Action(domain="service", operation="status", resource={"service_name": "ssh"})
    event = log.record_start("t1", "s1", action)
    found = log.find_by_ref(event.audit_ref)
    assert found is event


def test_flush_callback_invoked():
    flushed = []
    log = AuditLog(flush_fn=lambda ev: flushed.append(ev))
    action = Action(domain="service", operation="status", resource={"service_name": "ssh"})
    event = log.record_start("t1", "s1", action)
    result = ExecutionResult(
        task_id="t1", step_id="s1", executor_used="midcli",
        status=StepStatus.SUCCESS, duration_ms=10,
    )
    log.record_complete(event, result)
    assert len(flushed) == 1
    assert flushed[0].route_selected == "midcli"


def test_audit_event_to_dict():
    event = AuditEvent(task_id="t1", step_id="s1", domain="service", operation="start")
    d = event.to_dict()
    assert d["task_id"] == "t1"
    assert "timestamp" in d
