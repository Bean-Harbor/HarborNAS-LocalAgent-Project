"""Tests for assistant.executors.harbor_ops"""
from orchestrator.contracts import Action, RiskLevel, Route, StepStatus
from orchestrator.executors.harbor_ops import (
    MiddlewareExecutor,
    MidcliExecutor,
    _build_midcli_command,
    _map_service_operation,
)


# --- _map_service_operation ---

def test_map_status():
    method, args = _map_service_operation("status", "ssh", {})
    assert method == "service.query"
    assert args == ["ssh"]


def test_map_start():
    method, args = _map_service_operation("start", "ssh", {})
    assert method == "service.control"
    assert args == ["START", "ssh", {}]


def test_map_stop():
    method, args = _map_service_operation("stop", "smb", {})
    assert method == "service.control"
    assert args == ["STOP", "smb", {}]


def test_map_restart():
    method, args = _map_service_operation("restart", "ssh", {})
    assert method == "service.control"
    assert args == ["RESTART", "ssh", {}]


def test_map_enable():
    method, args = _map_service_operation("enable", "ssh", {"enable": True})
    assert method == "service.update"
    assert args == ["ssh", {"enable": True}]


# --- _build_midcli_command ---

def test_midcli_status_command():
    cmd = _build_midcli_command("status", "ssh", {})
    assert cmd == "service ssh show"


def test_midcli_start_command():
    cmd = _build_midcli_command("start", "ssh", {})
    assert cmd == "service start service=ssh"


def test_midcli_stop_command():
    cmd = _build_midcli_command("stop", "smb", {})
    assert cmd == "service stop service=smb"


def test_midcli_restart_command():
    cmd = _build_midcli_command("restart", "ssh", {})
    assert cmd == "service restart service=ssh"


def test_midcli_enable_command():
    cmd = _build_midcli_command("enable", "ssh", {"enable": False})
    assert cmd == "service update id_or_name=ssh enable=false"


# --- MiddlewareExecutor ---

def test_middleware_executor_success():
    def fake_call(method, *args):
        return {"state": "RUNNING"}, 15

    ex = MiddlewareExecutor(call_fn=fake_call)
    assert ex.route == Route.MIDDLEWARE_API
    assert ex.is_available()

    action = Action(domain="service", operation="status", resource={"service_name": "ssh"})
    result = ex.execute(action, task_id="t1", step_id="s1")
    assert result.ok
    assert result.executor_used == "middleware_api"
    assert result.result_payload == {"state": "RUNNING"}


def test_middleware_executor_not_available_without_call_fn():
    ex = MiddlewareExecutor()
    assert not ex.is_available()


def test_middleware_executor_handles_error():
    def fail_call(method, *args):
        raise ConnectionError("timeout")

    ex = MiddlewareExecutor(call_fn=fail_call)
    action = Action(domain="service", operation="start", resource={"service_name": "ssh"}, risk_level=RiskLevel.MEDIUM)
    result = ex.execute(action, task_id="t1", step_id="s1")
    assert not result.ok
    assert result.error_code == "MIDDLEWARE_ERROR"
    assert "timeout" in result.error_message


# --- MidcliExecutor ---

def test_midcli_executor_success():
    def fake_run(command):
        return "service,state\nssh,RUNNING", 8

    ex = MidcliExecutor(run_fn=fake_run)
    assert ex.route == Route.MIDCLI
    assert ex.is_available()

    action = Action(domain="service", operation="status", resource={"service_name": "ssh"})
    result = ex.execute(action, task_id="t1", step_id="s1")
    assert result.ok
    assert result.executor_used == "midcli"


def test_midcli_executor_not_available_without_run_fn():
    ex = MidcliExecutor()
    assert not ex.is_available()


def test_midcli_executor_handles_error():
    def fail_run(command):
        raise OSError("command not found")

    ex = MidcliExecutor(run_fn=fail_run)
    action = Action(domain="service", operation="restart", resource={"service_name": "ssh"}, risk_level=RiskLevel.HIGH)
    result = ex.execute(action, task_id="t1", step_id="s1")
    assert not result.ok
    assert result.error_code == "MIDCLI_ERROR"
    assert "command not found" in result.error_message
