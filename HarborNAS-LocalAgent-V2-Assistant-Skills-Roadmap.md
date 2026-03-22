# HarborNAS Local Agent V2 Roadmap

## Scope
This V2 roadmap extends the existing HarborNAS local-agent plan with two priorities:
1. Multi-terminal natural-language assistant (mobile/web/desktop).
2. Skills framework with HarborOS control-plane-first execution.
3. Reuse HarborOS existing `middleware` API as primary execution route.
4. Reuse HarborOS existing CLI tool `midcli` as the secondary fallback route.

Execution priority is strict:
1. Middleware API executor
2. MidCLI executor (CLI via `midcli`)
3. Browser executor
4. MCP executor (fallback only)

## V2 Objectives

1. Users can control HarborOS from phone and other terminals using natural language.
2. Skills are hot-pluggable and governed by permissions.
3. Most HarborOS tasks are completed through deterministic API execution.
4. `midcli` is used when API route is unavailable or lacks capability parity.
5. Browser and MCP are used only when API and CLI are unavailable.
5. All actions are auditable and replayable.

## Architecture Delta (V2)

### 0) HarborBeacon IM Access Layer (New)
- HarborBeacon is a ZeroClaw fork pre-installed in HarborOS (same machine).
- Users interact via IM channels: Feishu, WeCom, Telegram, Discord, DingTalk, Slack, MQTT.
- `channels.py` handles channel registration, inbound message parsing, and outbound reply dispatch.
- `mcp_adapter.py` bridges HarborBeacon to assistant runtime via MCP tool calls (ReadOnly guard, per-call approval token).
- `autonomy.py` maps risk levels to autonomy tiers: ReadOnly / Supervised / Full.
- `tool_descriptions.py` converts skill manifests into MCP-compatible tool descriptions.
- One-click IM configuration on HarborOS boot.

### 1) Multi-terminal Access Layer
- Mobile PWA chat client.
- HarborOS WebUI chat panel.
- IM channels via HarborBeacon (飞书 / 企微 / Telegram / Discord / 钉钉 / Slack / MQTT).
- Unified API gateway for auth, session, rate limit, and streaming responses.

### 2) Assistant Orchestration Layer
- Intent parser and task decomposition.
- Planner that converts user intent into execution steps.
- Skill router with policy-based executor selection.
- Confirmation policy for risky operations.

### 2.1) Middleware API Integration Baseline
- HarborOS domain skills must resolve API route first.
- Planner outputs normalized action objects (`domain`, `operation`, `resource`, `args`).
- Router maps normalized actions to middleware endpoints with versioned adapters.
- API responses are normalized into a common result envelope for audit and replay.

### 3) Skills Runtime Layer
- Skill registry (manifest, version, capability tags).
- Skill runtime (sandbox, timeout, retries, output schema).
- Executors:
  - `MiddlewareExecutor` (default for HarborOS operations)
  - `MidCLIExecutor` (secondary fallback)
  - `BrowserExecutor` (secondary)
  - `MCPExecutor` (final fallback)

### 3.1) MidCLI Integration Baseline
- HarborOS domain skills use `midcli` as fallback when API mapping is unavailable.
- Natural-language intents are mapped to approved `midcli` subcommands.
- Command execution should use structured output mode when available (for stable parsing and audit).
- Keep an allowlist of accepted command groups and arguments to prevent unsafe shell expansion.

### 4) Governance & Observability Layer
- Structured logs for every task and substep.
- Command audit trail and replay.
- Success rate, latency, and cost metrics.
- Policy violations and high-risk alerts.

## Control-plane-first Routing Policy

Pseudo policy:

```text
if skill.supports_api and middleware.available:
  route = MIDDLEWARE_API
elif skill.supports_cli and host.cli_available:
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
- Never choose CLI if API route is available for the same capability.
- Never choose Browser/MCP if API or CLI route is available.
- Never execute destructive commands without explicit confirmation.
- Always dry-run when `risk_level >= HIGH` and route supports preview.

## 8-Week Incremental Plan (for current 3-person team)

### Week 1-2: Assistant Entry + Session Backbone
- Build mobile/web chat entry, IM channel integration via HarborBeacon, and unified session API.
- Define task state machine (`queued -> planned -> executing -> completed/failed`).
- Introduce `MiddlewareExecutor` v1, then `MidCLIExecutor` fallback and command audit logging.

Deliverable:
- End-to-end IM -> HarborBeacon -> middleware API -> result loop for basic HarborOS operations.

### Week 3-4: Skills Contract + Router
- Implement skill registry and manifest loader.
- Implement router with fixed priority `API > CLI > Browser > MCP`.
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

- API route ratio >= 70% for HarborOS domain tasks.
- CLI route ratio <= 25% for HarborOS domain tasks (excluding fallback-only capabilities).
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

5. Upstream sync drift between HarborNAS and upstream TrueNAS.
- Control: compatibility matrix tests for endpoint/field changes and fallback validation.

## HarborOS Action Mapping (system.harbor_ops)

| Intent | Primary Route (Middleware API) | Secondary Route (midcli) | Risk |
|---|---|---|---|
| Query service status | `service.query` | `service <name> show` | LOW |
| Start service | `service.start` | `service start service=<name>` | MEDIUM |
| Stop service | `service.stop` | `service stop service=<name>` | HIGH |
| Restart service | `service.restart` | `service restart service=<name>` | HIGH |
| Enable service on boot | `service.update(enable=true)` | `service update id_or_name=<name> enable=true` | MEDIUM |

## Upstream Compatibility Test Matrix (Template)

| Capability | HarborNAS Branch | Upstream Ref | API Contract Test | MidCLI Fallback Test | Result |
|---|---|---|---|---|---|
| service.status | develop | truenas/master | pass/fail | pass/fail | pending |
| service.start | develop | truenas/master | pass/fail | pass/fail | pending |
| service.stop | develop | truenas/master | pass/fail | pass/fail | pending |
| service.restart | develop | truenas/master | pass/fail | pass/fail | pending |

## Immediate Next Tasks

1. Implement skill manifest parser and registry CRUD.
2. Implement `MiddlewareExecutor` with API schema validation and response normalization.
3. Implement `MidCLIExecutor` fallback with dry-run and risk tagging.
3. Add approval API for high-risk actions.
4. Add first two skills and contract tests.
5. Add compatibility matrix CI job for HarborNAS/upstream drift.
