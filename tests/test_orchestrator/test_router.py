"""Tests for assistant.router"""
from orchestrator.contracts import Action, ExecutionResult, RiskLevel, Route, StepStatus
from orchestrator.router import Router, allowed_routes


# --- stub executors ---

class StubExecutor:
    def __init__(
        self,
        route: Route,
        available: bool = True,
        fail: bool = False,
        supported_domains: set[str] | None = None,
    ):
        self.route = route
        self._available = available
        self._fail = fail
        self._supported_domains = supported_domains
        self.called = False

    def is_available(self) -> bool:
        return self._available

    def supports(self, action: Action) -> bool:
        if self._supported_domains is None:
            return True
        return action.domain in self._supported_domains

    def execute(self, action: Action, *, task_id: str, step_id: str) -> ExecutionResult:
        self.called = True
        if self._fail:
            raise RuntimeError("executor failed")
        return ExecutionResult(
            task_id=task_id,
            step_id=step_id,
            executor_used=self.route.value,
            status=StepStatus.SUCCESS,
            duration_ms=5,
        )


# --- allowed_routes tests ---

def test_harboros_domain_restricted_to_api_and_midcli():
    a = Action(domain="service", operation="status", resource={"service_name": "ssh"})
    routes = allowed_routes(a)
    assert routes == [Route.MIDDLEWARE_API, Route.MIDCLI]


def test_files_domain_restricted_to_api_and_midcli():
    a = Action(domain="files", operation="copy", resource={})
    routes = allowed_routes(a)
    assert routes == [Route.MIDDLEWARE_API, Route.MIDCLI]


def test_non_harboros_domain_gets_all_routes():
    a = Action(domain="media", operation="transcode", resource={})
    routes = allowed_routes(a)
    assert routes == [Route.MIDDLEWARE_API, Route.MIDCLI, Route.BROWSER, Route.MCP]


# --- Router.resolve tests ---

def test_resolve_picks_primary_when_available():
    mw = StubExecutor(Route.MIDDLEWARE_API)
    cli = StubExecutor(Route.MIDCLI)
    router = Router([mw, cli])
    a = Action(domain="service", operation="status", resource={"service_name": "ssh"})
    ex, fallback = router.resolve(a)
    assert ex is mw
    assert fallback is False


def test_resolve_falls_back_when_primary_unavailable():
    mw = StubExecutor(Route.MIDDLEWARE_API, available=False)
    cli = StubExecutor(Route.MIDCLI)
    router = Router([mw, cli])
    a = Action(domain="service", operation="status", resource={"service_name": "ssh"})
    ex, fallback = router.resolve(a)
    assert ex is cli
    assert fallback is True


def test_resolve_returns_none_when_nothing_available():
    mw = StubExecutor(Route.MIDDLEWARE_API, available=False)
    cli = StubExecutor(Route.MIDCLI, available=False)
    router = Router([mw, cli])
    a = Action(domain="service", operation="status", resource={"service_name": "ssh"})
    ex, fallback = router.resolve(a)
    assert ex is None


def test_resolve_skips_unsupported_executor():
    mw = StubExecutor(Route.MIDDLEWARE_API, supported_domains={"service"})
    mcp = StubExecutor(Route.MCP, supported_domains={"camera"})
    router = Router([mw, mcp])
    a = Action(domain="camera", operation="scan", resource={})
    ex, fallback = router.resolve(a)
    assert ex is mcp
    assert fallback is True


# --- Router.execute tests ---

def test_execute_uses_primary():
    mw = StubExecutor(Route.MIDDLEWARE_API)
    cli = StubExecutor(Route.MIDCLI)
    router = Router([mw, cli])
    a = Action(domain="service", operation="start", resource={"service_name": "ssh"}, risk_level=RiskLevel.MEDIUM)
    result = router.execute(a, task_id="t1", step_id="s1")
    assert result.ok
    assert result.executor_used == "middleware_api"
    assert result.fallback_used is False
    assert mw.called
    assert not cli.called


def test_execute_falls_back_on_primary_failure():
    mw = StubExecutor(Route.MIDDLEWARE_API, fail=True)
    cli = StubExecutor(Route.MIDCLI)
    router = Router([mw, cli])
    a = Action(domain="service", operation="start", resource={"service_name": "ssh"}, risk_level=RiskLevel.MEDIUM)
    result = router.execute(a, task_id="t1", step_id="s1")
    assert result.ok
    assert result.executor_used == "midcli"
    assert result.fallback_used is True
    assert mw.called
    assert cli.called


def test_execute_falls_back_on_primary_unavailable():
    mw = StubExecutor(Route.MIDDLEWARE_API, available=False)
    cli = StubExecutor(Route.MIDCLI)
    router = Router([mw, cli])
    a = Action(domain="service", operation="start", resource={"service_name": "ssh"}, risk_level=RiskLevel.MEDIUM)
    result = router.execute(a, task_id="t1", step_id="s1")
    assert result.ok
    assert result.executor_used == "midcli"
    assert result.fallback_used is True


def test_execute_returns_failure_when_all_fail():
    mw = StubExecutor(Route.MIDDLEWARE_API, fail=True)
    cli = StubExecutor(Route.MIDCLI, fail=True)
    router = Router([mw, cli])
    a = Action(domain="service", operation="start", resource={"service_name": "ssh"}, risk_level=RiskLevel.MEDIUM)
    result = router.execute(a, task_id="t1", step_id="s1")
    assert not result.ok
    assert result.error_code == "NO_EXECUTOR_AVAILABLE"


def test_execute_returns_failure_when_none_registered():
    router = Router()
    a = Action(domain="service", operation="start", resource={"service_name": "ssh"}, risk_level=RiskLevel.MEDIUM)
    result = router.execute(a, task_id="t1", step_id="s1")
    assert not result.ok
    assert result.error_code == "NO_EXECUTOR_AVAILABLE"


def test_register_adds_executor():
    router = Router()
    cli = StubExecutor(Route.MIDCLI)
    router.register(cli)
    a = Action(domain="service", operation="status", resource={"service_name": "ssh"})
    result = router.execute(a, task_id="t1", step_id="s1")
    assert result.ok
    assert result.executor_used == "midcli"
    assert result.fallback_used is True  # midcli is not the first in priority


def test_execute_skips_unsupported_executor():
    mw = StubExecutor(Route.MIDDLEWARE_API, supported_domains={"service"})
    mcp = StubExecutor(Route.MCP, supported_domains={"camera"})
    router = Router([mw, mcp])
    a = Action(domain="camera", operation="scan", resource={})
    result = router.execute(a, task_id="t1", step_id="s1")
    assert result.ok
    assert result.executor_used == "mcp"
    assert result.fallback_used is True
    assert not mw.called
    assert mcp.called
