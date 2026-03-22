"""Tests for harborclaw.autonomy — autonomy ↔ risk level mapping."""
import pytest

from orchestrator.contracts import RiskLevel
from orchestrator.policy import ApprovalContext
from harborclaw.autonomy import (
    Autonomy,
    autonomy_to_approval,
    is_read_only_safe,
    risk_to_autonomy,
)


class TestAutonomyToApproval:
    def test_readonly_returns_none(self):
        assert autonomy_to_approval(Autonomy.READ_ONLY) is None

    def test_readonly_string(self):
        assert autonomy_to_approval("ReadOnly") is None

    def test_supervised_no_token(self):
        ctx = autonomy_to_approval(Autonomy.SUPERVISED)
        assert isinstance(ctx, ApprovalContext)
        assert ctx.token is None

    def test_supervised_with_approver(self):
        ctx = autonomy_to_approval(Autonomy.SUPERVISED, approver_id="user-1")
        assert ctx.approver_id == "user-1"
        assert ctx.token is None

    def test_full_with_token(self):
        ctx = autonomy_to_approval(Autonomy.FULL, token="secret-tok")
        assert ctx.token == "secret-tok"

    def test_full_string(self):
        ctx = autonomy_to_approval("Full", token="t")
        assert ctx.token == "t"

    def test_invalid_autonomy_raises(self):
        with pytest.raises(ValueError):
            autonomy_to_approval("InvalidLevel")


class TestRiskToAutonomy:
    def test_low(self):
        assert risk_to_autonomy(RiskLevel.LOW) == Autonomy.SUPERVISED

    def test_medium(self):
        assert risk_to_autonomy(RiskLevel.MEDIUM) == Autonomy.SUPERVISED

    def test_high(self):
        assert risk_to_autonomy(RiskLevel.HIGH) == Autonomy.FULL

    def test_critical(self):
        assert risk_to_autonomy(RiskLevel.CRITICAL) == Autonomy.FULL


class TestReadOnlySafe:
    @pytest.mark.parametrize("op", ["status", "query", "search", "list", "get", "info"])
    def test_safe_operations(self, op):
        assert is_read_only_safe(op) is True

    @pytest.mark.parametrize("op", ["start", "stop", "restart", "copy", "move", "delete", "archive"])
    def test_unsafe_operations(self, op):
        assert is_read_only_safe(op) is False
