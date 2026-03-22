# HarborNAS Local Agent V2 Roadmap

## Scope
This V2 roadmap extends the existing HarborNAS local-agent plan with two priorities:
1. Multi-terminal natural-language assistant (mobile/web/desktop).
2. Skills framework with CLI-first execution.
3. Reuse HarborOS existing CLI tool `midcli` as the default command gateway.

Execution priority is strict:
1. MidCLI executor (CLI via `midcli`)
2. Browser executor
3. MCP executor (fallback only)

## V2 Objectives

1. Users can control HarborOS from phone and other terminals using natural language.
2. Skills are hot-pluggable and governed by permissions.
3. Most tasks are completed through deterministic command-line execution.
4. Browser and MCP are used only when CLI is unavailable.
5. All actions are auditable and replayable.

## Architecture Delta (V2)

### 1) Multi-terminal Access Layer
- Mobile PWA chat client.
- HarborOS WebUI chat panel.
- Unified API gateway for auth, session, rate limit, and streaming responses.

### 2) Assistant Orchestration Layer
- Intent parser and task decomposition.
- Planner that converts user intent into execution steps.
- Skill router with policy-based executor selection.
- Confirmation policy for risky operations.

### 3) Skills Runtime Layer
- Skill registry (manifest, version, capability tags).
- Skill runtime (sandbox, timeout, retries, output schema).
- Executors:
  - `MidCLIExecutor` (default for HarborOS operations)
  - `BrowserExecutor` (secondary)
  - `MCPExecutor` (final fallback)

### 3.1) MidCLI Integration Baseline
- HarborOS domain skills must execute through `midcli` first.
- Natural-language intents are mapped to approved `midcli` subcommands.
- Command execution should use structured output mode when available (for stable parsing and audit).
- Keep an allowlist of accepted command groups and arguments to prevent unsafe shell expansion.

### 4) Governance & Observability Layer
- Structured logs for every task and substep.
- Command audit trail and replay.
- Success rate, latency, and cost metrics.
- Policy violations and high-risk alerts.

## CLI-first Routing Policy

Pseudo policy:

```text
if skill.supports_cli and host.cli_available:
  route = MIDCLI
elif skill.supports_browser and browser.available:
  route = BROWSER
elif skill.supports_mcp and mcp.available:
  route = MCP
else:
  fail("no executable route")

if command.risk_level in [HIGH, CRITICAL]:
  require_user_confirmation()
```

Hard rules:
- Never choose MCP if CLI route is available.
- Never execute destructive commands without explicit confirmation.
- Always dry-run when `risk_level >= HIGH` and command allows preview.

## 8-Week Incremental Plan (for current 3-person team)

### Week 1-2: Assistant Entry + Session Backbone
- Build mobile/web chat entry and unified session API.
- Define task state machine (`queued -> planned -> executing -> completed/failed`).
- Introduce `MidCLIExecutor` v1 and command audit logging.

Deliverable:
- End-to-end NL -> `midcli` -> result loop for basic HarborOS operations.

### Week 3-4: Skills Contract + Router
- Implement skill registry and manifest loader.
- Implement router with fixed priority `CLI > Browser > MCP`.
- Add approval flow for high-risk commands.

Deliverable:
- Two production-ready skills: system-management, file-ops.

### Week 5-6: Capability Expansion
- Add media skill (`ffmpeg`-based video editing templates).
- Add browser automation skill for sites without CLI surfaces.
- Add sandbox and timeout boundaries.

Deliverable:
- Four+ skills available with governed execution.

### Week 7-8: Reliability + Beta
- Add retries, circuit breaker, and fallback policy.
- Add dashboards for route ratio and failure categories.
- Run beta with real terminal/mobile usage.

Deliverable:
- V2 beta release with measurable SLA.

## Ownership (3 People)

- Engineer A (AI/backend): planner, intent parser, skill router, policy engine.
- Engineer B (platform/data): registry, runtime, API, state machine, persistence.
- Engineer C (DevOps/security/QA): sandbox, observability, audit, security checks, release.

## KPIs for V2

- CLI route ratio >= 80% for automatable tasks.
- Task success rate >= 95% (excluding external dependency failures).
- P95 orchestration latency <= 2s before execution start.
- High-risk actions with confirmation coverage = 100%.
- Skill regression pass rate >= 98% before release.

## Risks and Controls

1. Unsafe command generation.
- Control: allowlist/denylist, argument validators, mandatory confirmation.

2. Skill quality inconsistency.
- Control: shared schema, contract tests, semantic versioning, rollback support.

3. Cross-terminal context drift.
- Control: centralized session store and immutable task events.

4. Browser/MCP overuse.
- Control: hard routing policy and route-ratio alerting.

## Immediate Next Tasks

1. Implement skill manifest parser and registry CRUD.
2. Implement `MidCLIExecutor` with dry-run and risk tagging.
3. Add approval API for high-risk actions.
4. Add first two skills and contract tests.
