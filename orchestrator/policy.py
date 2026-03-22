"""Policy enforcement: risk gates, approval checks, path/service validation.

Reuses validation logic from scripts/harbor_integration.py where possible,
wrapping it in the assistant contract types.
"""
from __future__ import annotations

import re
from dataclasses import dataclass
from typing import Any

from .contracts import Action, RiskLevel, CONFIRMATION_REQUIRED_LEVELS

SERVICE_NAME_RE = re.compile(r"^[a-z0-9_-]{1,64}$")


class PolicyViolation(Exception):
    """Raised when an action violates policy."""

    def __init__(self, code: str, message: str):
        super().__init__(message)
        self.code = code


@dataclass
class ApprovalContext:
    """Carries approval credentials for the current request."""
    token: str | None = None
    required_token: str | None = None
    approver_id: str | None = None


def check_risk_gate(action: Action, approval: ApprovalContext | None) -> None:
    """Block HIGH/CRITICAL actions that lack a valid approval token."""
    if action.risk_level not in CONFIRMATION_REQUIRED_LEVELS:
        return
    if approval is None or not approval.token:
        raise PolicyViolation(
            "APPROVAL_REQUIRED",
            f"{action.domain}.{action.operation} (risk={action.risk_level.value}) requires approval",
        )
    if approval.required_token and approval.token != approval.required_token:
        raise PolicyViolation(
            "APPROVAL_TOKEN_MISMATCH",
            f"Approval token does not match for {action.domain}.{action.operation}",
        )


def check_service_name(action: Action) -> None:
    """Validate service_name in resource dict."""
    if action.domain != "service":
        return
    name = action.resource.get("service_name", "")
    if not SERVICE_NAME_RE.match(name):
        raise PolicyViolation(
            "INVALID_SERVICE_NAME",
            f"Invalid service name: {name!r}",
        )


SUPPORTED_SERVICE_OPS = {"status", "start", "stop", "restart", "enable"}
SUPPORTED_FILE_OPS = {"search", "copy", "move", "archive"}


def check_operation(action: Action) -> None:
    """Reject unknown operations."""
    if action.domain == "service":
        if action.operation not in SUPPORTED_SERVICE_OPS:
            raise PolicyViolation(
                "UNSUPPORTED_OPERATION",
                f"Unsupported service operation: {action.operation}",
            )
    elif action.domain == "files":
        if action.operation not in SUPPORTED_FILE_OPS:
            raise PolicyViolation(
                "UNSUPPORTED_OPERATION",
                f"Unsupported file operation: {action.operation}",
            )


def enforce(action: Action, approval: ApprovalContext | None = None) -> None:
    """Run all policy checks for an action. Raises PolicyViolation on failure."""
    check_operation(action)
    check_service_name(action)
    check_risk_gate(action, approval)
