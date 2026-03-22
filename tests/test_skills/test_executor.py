"""Tests for skills.executor — BaseExecutor, concrete executors, factory."""
import pytest

from orchestrator.contracts import Action, Route, StepStatus
from skills.executor import (
    BaseExecutor,
    BrowserExecutor,
    CliExecutor,
    McpExecutor,
    MiddlewareApiExecutor,
    MidcliSkillExecutor,
    executors_from_manifest,
)
from skills.manifest import parse_manifest


# ── helpers ─────────────────────────────────────────────────────────

def _manifest(harbor_api=None, harbor_cli=None, executors=None):
    data = {"id": "test.exec", "capabilities": ["t.one"]}
    if harbor_api:
        data["harbor_api"] = harbor_api
    if harbor_cli:
        data["harbor_cli"] = harbor_cli
    if executors:
        data["executors"] = executors
    return parse_manifest(data)


def _action(domain="service", operation="status", resource=None, args=None):
    return Action(
        domain=domain,
        operation=operation,
        resource=resource or {"name": "test_svc"},
        args=args or {},
    )


# ── MiddlewareApiExecutor ────────────────────────────────────────────

class TestMiddlewareApiExecutor:
    def test_is_available_with_call_fn(self):
        e = MiddlewareApiExecutor("test.exec", call_fn=lambda m, r, a: ({}, 0))
        assert e.is_available() is True

    def test_is_available_without_call_fn(self):
        e = MiddlewareApiExecutor("test.exec")
        assert e.is_available() is False

    def test_route(self):
        e = MiddlewareApiExecutor("test.exec", call_fn=lambda m, r, a: ({}, 0))
        assert e.route == Route.MIDDLEWARE_API

    def test_execute_allowed_method(self):
        captured = {}
        def fake_call(method, resource, args):
            captured["method"] = method
            return {"ok": True}, 0

        e = MiddlewareApiExecutor(
            "test.exec",
            call_fn=fake_call,
            allowed_methods=["query", "start"],
        )
        result = e.execute(_action(operation="query"), task_id="t1", step_id="s1")
        assert result.status == StepStatus.SUCCESS
        assert captured["method"] == "service.query"

    def test_execute_blocked_method(self):
        e = MiddlewareApiExecutor(
            "test.exec",
            call_fn=lambda m, r, a: ({}, 0),
            allowed_methods=["query"],
        )
        result = e.execute(_action(operation="delete"), task_id="t1", step_id="s1")
        assert result.status == StepStatus.FAILED
        assert "not in allowed_methods" in result.error_message


# ── MidcliSkillExecutor ─────────────────────────────────────────────

class TestMidcliSkillExecutor:
    def test_route(self):
        e = MidcliSkillExecutor("test.exec", run_fn=lambda c: ("", 0))
        assert e.route == Route.MIDCLI

    def test_is_available_with_run_fn(self):
        e = MidcliSkillExecutor("test.exec", run_fn=lambda c: ("", 0))
        assert e.is_available() is True

    def test_is_available_without_run_fn(self):
        e = MidcliSkillExecutor("test.exec")
        assert e.is_available() is False

    def test_execute_allowed_subcommand(self):
        captured = {}
        def fake_run(cmd):
            captured["cmd"] = cmd
            return "OK", 0

        e = MidcliSkillExecutor(
            "test.exec",
            run_fn=fake_run,
            allowed_subcommands=["status", "start"],
        )
        result = e.execute(_action(operation="status"), task_id="t1", step_id="s1")
        assert result.status == StepStatus.SUCCESS
        assert "status" in captured["cmd"]

    def test_execute_blocked_subcommand(self):
        e = MidcliSkillExecutor(
            "test.exec",
            run_fn=lambda c: ("", 0),
            allowed_subcommands=["status"],
        )
        result = e.execute(_action(operation="delete"), task_id="t1", step_id="s1")
        assert result.status == StepStatus.FAILED
        assert "not in allowed_subcommands" in result.error_message


# ── CliExecutor ──────────────────────────────────────────────────────

class TestCliExecutor:
    def test_route(self):
        e = CliExecutor("test.exec", run_fn=lambda c: ("", 0))
        assert e.route == Route.MIDCLI  # generic CLI reuses MIDCLI route

    def test_is_available_with_run_fn(self):
        e = CliExecutor("test.exec", run_fn=lambda c: ("", 0))
        assert e.is_available() is True

    def test_is_available_without_run_fn(self):
        e = CliExecutor("test.exec")
        assert e.is_available() is False


# ── BrowserExecutor / McpExecutor ────────────────────────────────────

class TestPlaceholderExecutors:
    def test_browser_not_available(self):
        e = BrowserExecutor("test.exec")
        assert e.is_available() is False

    def test_mcp_not_available(self):
        e = McpExecutor("test.exec")
        assert e.is_available() is False

    def test_browser_available_when_set(self):
        e = BrowserExecutor("test.exec", available=True)
        assert e.is_available() is True

    def test_mcp_available_when_set(self):
        e = McpExecutor("test.exec", available=True)
        assert e.is_available() is True


# ── executors_from_manifest ──────────────────────────────────────────

class TestFactory:
    def _dummy_call(self, m, r, a):
        return {}, 0

    def _dummy_run(self, cmd):
        return "", 0

    def test_api_and_cli_enabled(self):
        m = _manifest(
            harbor_api={"enabled": True, "provider": "middleware", "allowed_methods": ["query"]},
            harbor_cli={"enabled": True, "tool": "midcli", "allowed_subcommands": ["status"]},
        )
        execs = executors_from_manifest(m, api_call_fn=self._dummy_call, cli_run_fn=self._dummy_run)
        routes = {e.route for e in execs}
        assert Route.MIDDLEWARE_API in routes
        assert Route.MIDCLI in routes

    def test_only_api(self):
        m = _manifest(
            harbor_api={"enabled": True, "provider": "middleware", "allowed_methods": ["query"]},
        )
        execs = executors_from_manifest(m, api_call_fn=self._dummy_call)
        assert len(execs) == 1
        assert execs[0].route == Route.MIDDLEWARE_API

    def test_only_cli(self):
        m = _manifest(
            harbor_cli={"enabled": True, "tool": "midcli", "allowed_subcommands": ["status"]},
        )
        execs = executors_from_manifest(m, cli_run_fn=self._dummy_run)
        assert len(execs) == 1
        assert execs[0].route == Route.MIDCLI

    def test_neither_returns_empty(self):
        m = _manifest()
        execs = executors_from_manifest(m)
        assert execs == []

    def test_custom_cli_executor(self):
        m = _manifest(executors={"cli": {"enabled": True, "command": "python run.py"}})
        execs = executors_from_manifest(m, cli_run_fn=self._dummy_run)
        assert len(execs) >= 1
        assert any(isinstance(e, CliExecutor) for e in execs)


# ── BaseExecutor contract ───────────────────────────────────────────

class TestBaseContract:
    def test_abstract_cannot_instantiate(self):
        with pytest.raises(TypeError):
            BaseExecutor("x", Route.MIDCLI)  # type: ignore[abstract]
