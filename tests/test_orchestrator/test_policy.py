"""Tests for assistant.policy"""
import pytest
from orchestrator.contracts import Action, RiskLevel
from orchestrator.policy import (
    ApprovalContext,
    PolicyViolation,
    check_operation,
    check_risk_gate,
    check_service_name,
    enforce,
)


# --- risk gate tests ---

def test_low_risk_passes_without_approval():
    a = Action(domain="service", operation="status", resource={"service_name": "ssh"}, risk_level=RiskLevel.LOW)
    check_risk_gate(a, None)  # no exception


def test_high_risk_blocked_without_token():
    a = Action(domain="service", operation="stop", resource={"service_name": "ssh"}, risk_level=RiskLevel.HIGH)
    with pytest.raises(PolicyViolation) as exc_info:
        check_risk_gate(a, None)
    assert exc_info.value.code == "APPROVAL_REQUIRED"


def test_high_risk_passes_with_valid_token():
    a = Action(domain="service", operation="stop", resource={"service_name": "ssh"}, risk_level=RiskLevel.HIGH)
    ctx = ApprovalContext(token="abc123", required_token="abc123")
    check_risk_gate(a, ctx)  # no exception


def test_high_risk_blocked_with_wrong_token():
    a = Action(domain="service", operation="stop", resource={"service_name": "ssh"}, risk_level=RiskLevel.HIGH)
    ctx = ApprovalContext(token="wrong", required_token="abc123")
    with pytest.raises(PolicyViolation) as exc_info:
        check_risk_gate(a, ctx)
    assert exc_info.value.code == "APPROVAL_TOKEN_MISMATCH"


def test_critical_risk_blocked_without_token():
    a = Action(domain="service", operation="stop", resource={"service_name": "ssh"}, risk_level=RiskLevel.CRITICAL)
    with pytest.raises(PolicyViolation) as exc_info:
        check_risk_gate(a, None)
    assert exc_info.value.code == "APPROVAL_REQUIRED"


# --- service name validation ---

def test_valid_service_name():
    a = Action(domain="service", operation="status", resource={"service_name": "ssh"})
    check_service_name(a)  # no exception


def test_invalid_service_name_rejected():
    a = Action(domain="service", operation="status", resource={"service_name": "../etc/passwd"})
    with pytest.raises(PolicyViolation) as exc_info:
        check_service_name(a)
    assert exc_info.value.code == "INVALID_SERVICE_NAME"


def test_empty_service_name_rejected():
    a = Action(domain="service", operation="status", resource={"service_name": ""})
    with pytest.raises(PolicyViolation) as exc_info:
        check_service_name(a)
    assert exc_info.value.code == "INVALID_SERVICE_NAME"


def test_non_service_domain_skips_name_check():
    a = Action(domain="files", operation="copy", resource={"src": "/mnt/a", "dst": "/mnt/b"})
    check_service_name(a)  # no exception


# --- operation validation ---

def test_supported_service_operation():
    for op in ("status", "start", "stop", "restart", "enable"):
        a = Action(domain="service", operation=op, resource={"service_name": "ssh"})
        check_operation(a)  # no exception


def test_unsupported_service_operation():
    a = Action(domain="service", operation="destroy", resource={"service_name": "ssh"})
    with pytest.raises(PolicyViolation) as exc_info:
        check_operation(a)
    assert exc_info.value.code == "UNSUPPORTED_OPERATION"


def test_unsupported_file_operation():
    a = Action(domain="files", operation="delete", resource={})
    with pytest.raises(PolicyViolation) as exc_info:
        check_operation(a)
    assert exc_info.value.code == "UNSUPPORTED_OPERATION"


# --- enforce (full pipeline) ---

def test_enforce_passes_low_risk_valid_action():
    a = Action(domain="service", operation="status", resource={"service_name": "ssh"}, risk_level=RiskLevel.LOW)
    enforce(a)  # no exception


def test_enforce_blocks_high_risk_without_approval():
    a = Action(domain="service", operation="stop", resource={"service_name": "ssh"}, risk_level=RiskLevel.HIGH)
    with pytest.raises(PolicyViolation) as exc_info:
        enforce(a)
    assert exc_info.value.code == "APPROVAL_REQUIRED"


def test_enforce_blocks_bad_service_name_before_risk_check():
    a = Action(domain="service", operation="status", resource={"service_name": "!!!"})
    with pytest.raises(PolicyViolation) as exc_info:
        enforce(a)
    assert exc_info.value.code == "INVALID_SERVICE_NAME"
