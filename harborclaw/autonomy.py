"""Autonomy level mapping for HarborClaw.

HarborClaw uses three autonomy levels (inherited from ZeroClaw):
  - ReadOnly:   observe only, no mutations
  - Supervised: needs user confirmation for risky ops
  - Full:       autonomous execution, all ops allowed

Our assistant uses four risk levels + ApprovalContext:
  - LOW / MEDIUM:  no approval needed
  - HIGH:          approval token required
  - CRITICAL:      approval token required
"""
from __future__ import annotations

from enum import Enum
from typing import Any

from orchestrator.contracts import RiskLevel
from orchestrator.policy import ApprovalContext


class Autonomy(str, Enum):
    """HarborClaw autonomy levels (ReadOnly / Supervised / Full)."""
    READ_ONLY = "ReadOnly"
    SUPERVISED = "Supervised"
    FULL = "Full"


def autonomy_to_approval(
    autonomy: Autonomy | str,
    token: str | None = None,
    approver_id: str | None = None,
) -> ApprovalContext | None:
    """Convert a HarborClaw autonomy level to an ApprovalContext.

    - ReadOnly:   returns None (no approval, but mutations will be blocked by policy)
    - Supervised: returns ApprovalContext without token (will trigger approval prompt)
    - Full:       returns ApprovalContext with the provided token
    """
    if isinstance(autonomy, str):
        autonomy = Autonomy(autonomy)

    if autonomy == Autonomy.READ_ONLY:
        return None
    if autonomy == Autonomy.SUPERVISED:
        return ApprovalContext(token=None, approver_id=approver_id)
    # Full
    return ApprovalContext(token=token, approver_id=approver_id)


def risk_to_autonomy(risk: RiskLevel) -> Autonomy:
    """Suggest the minimum HarborClaw autonomy level needed for a risk level."""
    if risk in (RiskLevel.LOW, RiskLevel.MEDIUM):
        return Autonomy.SUPERVISED
    return Autonomy.FULL


def is_read_only_safe(operation: str) -> bool:
    """Return True if an operation is safe under ReadOnly autonomy."""
    return operation in _READ_ONLY_OPS


_READ_ONLY_OPS = frozenset({
    "status", "query", "search", "list", "get", "info",
    "health", "stats", "version", "ping",
})
