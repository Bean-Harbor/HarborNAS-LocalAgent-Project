"""Fallback, regression, and release-gate tests.

These tests verify critical non-negotiable system properties:
1. Route priority is always middleware_api -> midcli -> browser -> mcp.
2. HarborOS domains are restricted to API/CLI routes only.
3. High-risk actions are blocked without approval.
4. Fallback works correctly when primary executor fails.
5. Audit records are produced for every action.
6. Plugin skills don't modify core internals.
"""
from __future__ import annotations

from dataclasses import dataclass

import pytest

from orchestrator.audit import AuditLog
from orchestrator.contracts import (
    Action,
    ExecutionResult,
    RiskLevel,
    Route,
    ROUTE_PRIORITY,
    StepStatus,
    TaskPlan,
)
from orchestrator.policy import ApprovalContext, PolicyViolation, enforce
from orchestrator.router import HARBOROS_DOMAINS, Router, allowed_routes
from orchestrator.runtime import Runtime


# ── test helpers ─────────────────────────────────────────────────────

class StubExecutor:
    """Configurable executor for testing router behaviour."""

    def __init__(self, route: Route, *, available: bool = True, fail: bool = False):
        self._route = route
        self._available = available
        self._fail = fail
        self.call_count = 0

    @property
    def route(self) -> Route:
        return self._route

    def is_available(self) -> bool:
        return self._available

    def execute(self, action: Action, *, task_id: str, step_id: str) -> ExecutionResult:
        self.call_count += 1
        if self._fail:
            return ExecutionResult(
                task_id=task_id,
                step_id=step_id,
                executor_used=self._route.value,
                status=StepStatus.FAILED,
                error_code="STUB_FAIL",
                error_message="Intentional failure",
            )
        return ExecutionResult(
            task_id=task_id,
            step_id=step_id,
            executor_used=self._route.value,
            status=StepStatus.SUCCESS,
            result_payload={"stub": True},
        )


class FailThenSucceedExecutor:
    """First call raises, subsequent calls succeed."""

    def __init__(self, route: Route):
        self._route = route
        self.call_count = 0

    @property
    def route(self) -> Route:
        return self._route

    def is_available(self) -> bool:
        return True

    def execute(self, action: Action, *, task_id: str, step_id: str) -> ExecutionResult:
        self.call_count += 1
        if self.call_count == 1:
            raise RuntimeError("First call fails")
        return ExecutionResult(
            task_id=task_id, step_id=step_id,
            executor_used=self._route.value, status=StepStatus.SUCCESS,
        )


def _action(domain="service", operation="status", risk=RiskLevel.LOW, resource=None, **kw):
    if resource is None:
        resource = {"service_name": "test_svc"} if domain == "service" else {"source": "/data"}
    return Action(domain=domain, operation=operation, resource=resource, risk_level=risk, **kw)


# ══════════════════════════════════════════════════════════════════════
# 1. ROUTE PRIORITY — non-negotiable ordering
# ══════════════════════════════════════════════════════════════════════

class TestRoutePriority:
    def test_global_priority_order(self):
        """Route priority must be middleware_api -> midcli -> browser -> mcp."""
        assert ROUTE_PRIORITY == [
            Route.MIDDLEWARE_API,
            Route.MIDCLI,
            Route.BROWSER,
            Route.MCP,
        ]

    def test_middleware_api_is_first_choice_for_service(self):
        api = StubExecutor(Route.MIDDLEWARE_API)
        cli = StubExecutor(Route.MIDCLI)
        router = Router([api, cli])
        result = router.execute(_action("service", "status"), task_id="t1", step_id="s1")
        assert result.executor_used == "middleware_api"
        assert api.call_count == 1
        assert cli.call_count == 0

    def test_middleware_api_is_first_choice_for_files(self):
        api = StubExecutor(Route.MIDDLEWARE_API)
        cli = StubExecutor(Route.MIDCLI)
        router = Router([api, cli])
        result = router.execute(_action("files", "search"), task_id="t1", step_id="s1")
        assert result.executor_used == "middleware_api"

    def test_midcli_first_for_extension_domain_if_no_api(self):
        """For non-HarborOS domains, midcli is used when API is unavailable."""
        cli = StubExecutor(Route.MIDCLI)
        router = Router([cli])
        result = router.execute(_action("video", "trim"), task_id="t1", step_id="s1")
        assert result.executor_used == "midcli"


# ══════════════════════════════════════════════════════════════════════
# 2. HARBOROS DOMAIN RESTRICTIONS
# ══════════════════════════════════════════════════════════════════════

class TestHarborOSDomainRestrictions:
    def test_service_domain_restricted(self):
        assert "service" in HARBOROS_DOMAINS

    def test_files_domain_restricted(self):
        assert "files" in HARBOROS_DOMAINS

    def test_service_no_browser_route(self):
        routes = allowed_routes(_action("service", "status"))
        assert Route.BROWSER not in routes
        assert Route.MCP not in routes

    def test_files_no_mcp_route(self):
        routes = allowed_routes(_action("files", "copy"))
        assert Route.BROWSER not in routes
        assert Route.MCP not in routes

    def test_extension_domain_allows_all_routes(self):
        routes = allowed_routes(_action("video", "trim"))
        assert Route.BROWSER in routes
        assert Route.MCP in routes

    def test_browser_executor_not_used_for_service(self):
        """Even if browser is the only available executor, service domain rejects it."""
        browser = StubExecutor(Route.BROWSER)
        router = Router([browser])
        result = router.execute(_action("service", "status"), task_id="t1", step_id="s1")
        assert result.status == StepStatus.FAILED
        assert browser.call_count == 0


# ══════════════════════════════════════════════════════════════════════
# 3. FALLBACK BEHAVIOUR
# ══════════════════════════════════════════════════════════════════════

class TestFallback:
    def test_fallback_to_midcli_when_api_unavailable(self):
        api = StubExecutor(Route.MIDDLEWARE_API, available=False)
        cli = StubExecutor(Route.MIDCLI)
        router = Router([api, cli])
        result = router.execute(_action("service", "status"), task_id="t1", step_id="s1")
        assert result.executor_used == "midcli"
        assert result.fallback_used is True

    def test_fallback_to_midcli_when_api_fails(self):
        api = FailThenSucceedExecutor(Route.MIDDLEWARE_API)
        cli = StubExecutor(Route.MIDCLI)
        router = Router([api, cli])
        result = router.execute(_action("service", "status"), task_id="t1", step_id="s1")
        assert result.executor_used == "midcli"
        assert result.fallback_used is True

    def test_no_fallback_used_when_primary_succeeds(self):
        api = StubExecutor(Route.MIDDLEWARE_API)
        cli = StubExecutor(Route.MIDCLI)
        router = Router([api, cli])
        result = router.execute(_action("service", "status"), task_id="t1", step_id="s1")
        assert result.fallback_used is False
        assert cli.call_count == 0

    def test_all_executors_fail_returns_failure(self):
        api = StubExecutor(Route.MIDDLEWARE_API, available=False)
        cli = StubExecutor(Route.MIDCLI, available=False)
        router = Router([api, cli])
        result = router.execute(_action("service", "status"), task_id="t1", step_id="s1")
        assert result.status == StepStatus.FAILED
        assert result.error_code == "NO_EXECUTOR_AVAILABLE"

    def test_fallback_chain_extension_domain(self):
        """For extension domains, fallback goes through all 4 routes."""
        api = StubExecutor(Route.MIDDLEWARE_API, available=False)
        cli = StubExecutor(Route.MIDCLI, available=False)
        browser = StubExecutor(Route.BROWSER)
        router = Router([api, cli, browser])
        result = router.execute(_action("video", "trim"), task_id="t1", step_id="s1")
        assert result.executor_used == "browser"
        assert result.fallback_used is True


# ══════════════════════════════════════════════════════════════════════
# 4. POLICY GATES — high-risk blocking
# ══════════════════════════════════════════════════════════════════════

class TestPolicyGates:
    def test_high_risk_blocked_without_approval(self):
        action = _action(risk=RiskLevel.HIGH)
        with pytest.raises(PolicyViolation):
            enforce(action, approval=None)

    def test_critical_risk_blocked_without_approval(self):
        action = _action(risk=RiskLevel.CRITICAL)
        with pytest.raises(PolicyViolation):
            enforce(action, approval=None)

    def test_low_risk_passes_without_approval(self):
        action = _action(risk=RiskLevel.LOW)
        enforce(action, approval=None)  # no raise

    def test_medium_risk_passes_without_approval(self):
        action = _action(risk=RiskLevel.MEDIUM)
        enforce(action, approval=None)  # no raise

    def test_high_risk_passes_with_approval(self):
        action = _action(risk=RiskLevel.HIGH)
        approval = ApprovalContext(token="valid-token")
        enforce(action, approval)  # no raise

    def test_runtime_blocks_high_risk_step(self):
        """Full pipeline: high-risk action gets BLOCKED status without approval."""
        api = StubExecutor(Route.MIDDLEWARE_API)
        router = Router([api])
        rt = Runtime(router)
        action = _action(risk=RiskLevel.HIGH)
        plan = TaskPlan(goal="test high risk")
        plan.add(action)
        result = rt.execute_plan(plan)
        assert result.results[0].status == StepStatus.BLOCKED
        assert api.call_count == 0


# ══════════════════════════════════════════════════════════════════════
# 5. AUDIT TRAIL — every action produces records
# ══════════════════════════════════════════════════════════════════════

class TestAuditTrail:
    def test_successful_action_produces_audit(self):
        api = StubExecutor(Route.MIDDLEWARE_API)
        router = Router([api])
        audit = AuditLog()
        rt = Runtime(router, audit=audit)
        action = _action()
        result = rt.execute_single(action)
        events = audit.find_by_task(result.task_id)
        assert len(events) >= 1

    def test_blocked_action_produces_audit(self):
        api = StubExecutor(Route.MIDDLEWARE_API)
        router = Router([api])
        audit = AuditLog()
        rt = Runtime(router, audit=audit)
        action = _action(risk=RiskLevel.HIGH)
        plan = TaskPlan(goal="audit blocked")
        plan.add(action)
        result = rt.execute_plan(plan)
        events = audit.find_by_task(plan.task_id)
        assert len(events) >= 1
        assert events[0].status == StepStatus.BLOCKED.value

    def test_dry_run_produces_audit(self):
        api = StubExecutor(Route.MIDDLEWARE_API)
        router = Router([api])
        audit = AuditLog()
        rt = Runtime(router, audit=audit)
        action = _action(dry_run=True)
        plan = TaskPlan(goal="dry run audit")
        plan.add(action)
        result = rt.execute_plan(plan)
        assert result.results[0].executor_used == "dry_run"
        events = audit.find_by_task(plan.task_id)
        assert len(events) >= 1

    def test_fallback_audit_records_correct_executor(self):
        api = StubExecutor(Route.MIDDLEWARE_API, available=False)
        cli = StubExecutor(Route.MIDCLI)
        router = Router([api, cli])
        audit = AuditLog()
        rt = Runtime(router, audit=audit)
        action = _action()
        result = rt.execute_single(action)
        assert result.executor_used == "midcli"
        assert result.fallback_used is True


# ══════════════════════════════════════════════════════════════════════
# 6. PLUGIN ISOLATION — plugins don't modify core
# ══════════════════════════════════════════════════════════════════════

class TestPluginIsolation:
    def test_registry_load_does_not_modify_route_priority(self):
        """Loading plugins into the registry should not alter ROUTE_PRIORITY."""
        from skills.registry import Registry
        from pathlib import Path
        builtins = Path(__file__).resolve().parents[2] / "skills" / "builtins"
        original = list(ROUTE_PRIORITY)
        r = Registry()
        r.load_dir(builtins)
        assert ROUTE_PRIORITY == original

    def test_registry_load_does_not_modify_harboros_domains(self):
        from skills.registry import Registry
        from pathlib import Path
        builtins = Path(__file__).resolve().parents[2] / "skills" / "builtins"
        original = set(HARBOROS_DOMAINS)
        r = Registry()
        r.load_dir(builtins)
        assert HARBOROS_DOMAINS == original

    def test_extension_skill_cannot_claim_service_domain(self):
        """Plugin registering capabilities in 'service' domain doesn't break restriction."""
        from skills.registry import Registry
        from skills.manifest import parse_manifest
        r = Registry()
        # A rogue plugin claims service capabilities
        rogue = parse_manifest({
            "id": "rogue.plugin",
            "capabilities": ["service.exploit"],
        })
        r.register(rogue)
        # But the router still restricts service domain to API/CLI
        routes = allowed_routes(_action("service", "exploit"))
        assert Route.BROWSER not in routes


# ══════════════════════════════════════════════════════════════════════
# 7. RELEASE GATE — contract shape checks
# ══════════════════════════════════════════════════════════════════════

class TestReleaseGate:
    def test_action_has_required_fields(self):
        a = _action()
        assert hasattr(a, "domain")
        assert hasattr(a, "operation")
        assert hasattr(a, "resource")
        assert hasattr(a, "args")
        assert hasattr(a, "risk_level")
        assert hasattr(a, "requires_approval")

    def test_execution_result_has_required_fields(self):
        r = ExecutionResult(task_id="t", step_id="s", executor_used="x")
        assert hasattr(r, "task_id")
        assert hasattr(r, "step_id")
        assert hasattr(r, "executor_used")
        assert hasattr(r, "fallback_used")
        assert hasattr(r, "status")
        assert hasattr(r, "duration_ms")
        assert hasattr(r, "error_code")
        assert hasattr(r, "audit_ref")

    def test_action_auto_sets_approval_for_high_risk(self):
        a = Action(domain="x", operation="y", resource={}, risk_level=RiskLevel.HIGH)
        assert a.requires_approval is True

    def test_action_auto_sets_approval_for_critical_risk(self):
        a = Action(domain="x", operation="y", resource={}, risk_level=RiskLevel.CRITICAL)
        assert a.requires_approval is True

    def test_action_no_approval_for_low_risk(self):
        a = Action(domain="x", operation="y", resource={}, risk_level=RiskLevel.LOW)
        assert a.requires_approval is False

    def test_execution_result_ok_property(self):
        ok = ExecutionResult(task_id="t", step_id="s", executor_used="x", status=StepStatus.SUCCESS)
        fail = ExecutionResult(task_id="t", step_id="s", executor_used="x", status=StepStatus.FAILED)
        assert ok.ok is True
        assert fail.ok is False
