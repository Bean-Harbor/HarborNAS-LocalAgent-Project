# HarborNAS Local Agent — Detailed Implementation Plan v1

## Overview

This document is the authoritative, English-language implementation plan for building the
HarborNAS Local Agent system. It synthesises all existing design artefacts into a single
actionable guide, maps every design decision to concrete code tasks, and defines the
completion criteria for each phase.

**Governing documents (read first)**

| Document | Role |
|---|---|
| `HarborNAS-LocalAgent-V2-Assistant-Skills-Roadmap.md` | Overall V2 scope and 8-week plan |
| `HarborNAS-Skill-Spec-v1.md` | Skill manifest, runtime contract, routing rules |
| `HarborNAS-Middleware-Endpoint-Contract-v1.md` | `system.harbor_ops` API/CLI mapping |
| `HarborNAS-Files-BatchOps-Contract-v1.md` | `files.batch_ops` path policy and route chain |
| `HarborNAS-Planner-TaskDecompose-Contract-v1.md` | Planner input/output schema and DAG rules |
| `HarborNAS-Contract-E2E-Test-Plan-v1.md` | End-to-end validation scenarios |
| `HarborNAS-CI-Contract-Pipeline-Checklist-v1.md` | CI gate requirements |
| `scripts/harbor_integration.py` | Live integration library (already implemented) |

---

## Current State

The repository already contains:

- **15 planning and contract documents** describing architecture, routing policy, skill
  specs, and release gates.
- **CI scaffold** — three GitHub Actions workflows (`contract-pr-check`,
  `contract-nightly-e2e`, `contract-release-drift`) and four Python scripts that validate
  documentation consistency, probe live HarborNAS systems, run drift matrices, and gate
  releases.
- **pytest suites** for contracts, fallback routing, and policy enforcement.
- **`scripts/harbor_integration.py`** — a complete integration library covering
  `MiddlewareClient`, `MidcliClient`, path policy enforcement, approval gates, and
  capability discovery.

What does **not** yet exist is the `src/` tree — the actual agent runtime code:
planner, router, executor classes, skill registry, skill implementations, API gateway,
session management, and observability layer.

---

## Target Directory Layout

```
src/
  agent/
    __init__.py
    config.py                 # Central configuration (extends IntegrationConfig)
    session.py                # Session and task state machine
    planner/
      __init__.py
      task_decomposer.py      # planner.task_decompose contract implementation
      intent_parser.py        # NL -> normalised action object
      plan_validator.py       # DAG validation, schema checks
    router/
      __init__.py
      skill_router.py         # Route-priority selection (API > CLI > Browser > MCP)
      policy_engine.py        # Risk check, confirmation gate
    executors/
      __init__.py
      base.py                 # Abstract executor interface
      middleware_executor.py  # MiddlewareExecutor (primary)
      midcli_executor.py      # MidCLIExecutor (fallback)
      browser_executor.py     # BrowserExecutor (stub, future)
      mcp_executor.py         # MCPExecutor (stub, future)
    skills/
      __init__.py
      registry.py             # Skill manifest loader and CRUD
      system_harbor_ops/
        skill.yaml
        handler.py            # system.harbor_ops implementation
        tests/
          contract_test.json
      files_batch_ops/
        skill.yaml
        handler.py            # files.batch_ops implementation
        tests/
          contract_test.json
    gateway/
      __init__.py
      api.py                  # FastAPI app (REST + WebSocket streaming)
      auth.py                 # Auth middleware
      session_store.py        # Redis-backed session persistence
    observability/
      __init__.py
      logger.py               # Structured JSON logger
      metrics.py              # Prometheus counters/histograms
      audit.py                # Immutable audit event emitter
tests/
  unit/
    test_task_decomposer.py
    test_skill_router.py
    test_middleware_executor.py
    test_midcli_executor.py
    test_skill_registry.py
    test_policy_engine.py
  integration/
    test_harbor_ops_skill.py
    test_files_batch_ops_skill.py
    test_planner_router_executor.py
  (existing tests/contracts, tests/fallback, tests/policy remain unchanged)
```

---

## Phase 1 — Core Infrastructure (Week 1–2)

**Goal**: A testable skeleton that can load config, hold session state, and route a
single no-op action through the executor chain.

### 1.1 Extended Configuration (`src/agent/config.py`)

- Subclass or extend `IntegrationConfig` from `scripts/harbor_integration.py`.
- Add fields: `session_ttl_s`, `api_host`, `api_port`, `redis_url`, `metrics_port`.
- Load from environment with sensible defaults; expose `AgentConfig.from_env()`.
- **Acceptance test**: `AgentConfig.from_env()` returns a valid object with defaults
  when no environment variables are set.

### 1.2 Task State Machine (`src/agent/session.py`)

Implement the task lifecycle defined in V2 Roadmap §Week 1-2:

```
queued → planned → executing → completed
                             → failed
```

Key types:
- `TaskState` (enum)
- `TaskRecord` (dataclass: `task_id`, `trace_id`, `state`, `created_at`, `plan`,
  `result`, `error`)
- `SessionStore` (in-memory dict for Phase 1; swapped for Redis in Phase 5)

**Acceptance test**: Create a task, transition it through all states, verify final state
is serialisable to JSON.

### 1.3 Abstract Executor Interface (`src/agent/executors/base.py`)

```python
class ExecutorResult:
    ok: bool
    executor_used: str
    route_fallback_used: bool
    result: dict
    artifacts: list[str]
    metrics: dict          # duration_ms, retries
    error: dict | None

class BaseExecutor(ABC):
    @abstractmethod
    async def execute(self, action: dict, *, dry_run: bool,
                      approval_token: str | None) -> ExecutorResult: ...

    @abstractmethod
    def supports(self, domain: str, operation: str) -> bool: ...

    @property
    @abstractmethod
    def name(self) -> str: ...
```

### 1.4 MiddlewareExecutor (`src/agent/executors/middleware_executor.py`)

- Wraps `MiddlewareClient` from `scripts/harbor_integration.py`.
- `supports()` returns `True` when the configured middleware binary is present and the
  requested method is in the method list returned by `middleware.get_methods()`.
- `execute()`:
  1. Validates `action` against the canonical action model (domain, operation, resource,
     args, risk_level).
  2. Calls `ensure_approved()` for HIGH/CRITICAL risk.
  3. Calls `execute_service_action()` or `execute_file_action()` from the integration
     library.
  4. Normalises the raw result into `ExecutorResult`.
- `dry_run=True` must return a preview payload with zero side effects (mirrors the
  integration library behaviour).

**Acceptance tests**:
- Middleware binary absent → `CapabilityUnavailableError` raised, not swallowed.
- HIGH risk without approval token → `ApprovalRequiredError`.
- Denied path → `PathPolicyError`.
- Dry-run → result contains `"dry_run": true`, no real command executed.

### 1.5 MidCLIExecutor (`src/agent/executors/midcli_executor.py`)

- Wraps `MidcliClient` from `scripts/harbor_integration.py`.
- Same `supports()` / `execute()` contract as `MiddlewareExecutor`.
- Sets `route_fallback_used=True` in every `ExecutorResult`.
- Must enforce the midcli command allowlist from `HarborNAS-Files-BatchOps-Contract-v1.md`
  §CLI Template Policy.

**Acceptance tests**: mirror those for `MiddlewareExecutor`, plus verify
`route_fallback_used=True`.

### Phase 1 Done Criteria

- [ ] All unit tests in `tests/unit/test_middleware_executor.py` and
  `tests/unit/test_midcli_executor.py` pass.
- [ ] Existing `tests/contracts`, `tests/fallback`, `tests/policy` suites still pass.
- [ ] `pytest tests/ -q` exits 0.

---

## Phase 2 — Skill Registry and Router (Week 3–4)

**Goal**: Skills can be declared in `skill.yaml`, discovered at startup, and a router
selects the correct executor chain for each action.

### 2.1 Skill Registry (`src/agent/skills/registry.py`)

- `SkillManifest` dataclass (mirrors `skill.yaml` schema from `HarborNAS-Skill-Spec-v1.md`).
- `SkillRegistry.load(skills_dir)` — scans `src/agent/skills/*/skill.yaml` and parses
  each manifest.
- `SkillRegistry.get(skill_id)` / `SkillRegistry.list()`.
- Validate manifest at load time: required fields, semantic version format,
  `harbor_api.allowed_methods` is non-empty if `harbor_api.enabled=true`.

**Acceptance tests** (`tests/unit/test_skill_registry.py`):
- Valid manifest loaded successfully.
- Missing required field raises `ValueError`.
- Unknown executor type raises `ValueError`.

### 2.2 Skill Router (`src/agent/router/skill_router.py`)

Implements the hard routing policy from V2 Roadmap §Control-plane-first Routing Policy:

```
middleware_api → midcli → browser → mcp
```

```python
class SkillRouter:
    def __init__(self, executors: list[BaseExecutor], registry: SkillRegistry): ...

    async def route(self, action: dict) -> BaseExecutor:
        """Return the first executor that supports the action, in priority order."""
        ...
```

Rules (must be enforced, not advisory):
- Never select CLI if middleware API supports the capability and is healthy.
- Never select Browser/MCP if middleware or CLI is available.
- If no executor supports the action, raise `CapabilityUnavailableError`.

**Acceptance tests** (`tests/unit/test_skill_router.py`):
- All executors available → selects middleware.
- Middleware `supports()` returns `False` → selects midcli.
- All executors unavailable → raises `CapabilityUnavailableError`.

### 2.3 Policy Engine (`src/agent/router/policy_engine.py`)

- Extracts from the integration library's `ensure_approved()` and
  `validate_path_policy()` into a clean class.
- `PolicyEngine.check(action)` returns `PolicyResult(allowed, reasons)`.
- Called by the router **before** executor selection so that denied actions never reach
  an executor.

**Acceptance tests** (`tests/unit/test_policy_engine.py`):
- HIGH risk + no token → `PolicyResult(allowed=False, ...)`.
- Denied path → `PolicyResult(allowed=False, ...)`.
- LOW risk + allowed path → `PolicyResult(allowed=True)`.

### Phase 2 Done Criteria

- [ ] `tests/unit/test_skill_registry.py` passes.
- [ ] `tests/unit/test_skill_router.py` passes.
- [ ] `tests/unit/test_policy_engine.py` passes.
- [ ] Existing tests still pass.

---

## Phase 3 — Planner Layer (Week 3–4, parallel with Phase 2)

**Goal**: Natural-language input is converted into a validated execution plan
(`planner.task_decompose` contract).

### 3.1 Intent Parser (`src/agent/planner/intent_parser.py`)

- Parses a free-text user request into a list of `NormalisedAction` objects.
- Phase 1 implementation: keyword-matching rule engine (no LLM dependency) covering the
  action vocabulary in `HarborNAS-Middleware-Endpoint-Contract-v1.md` and
  `HarborNAS-Files-BatchOps-Contract-v1.md`.
- Each `NormalisedAction`: `domain`, `operation`, `resource`, `args`.
- Returns an empty list when no known action is matched (handled upstream as
  `UNKNOWN_CAPABILITY`).

### 3.2 Task Decomposer (`src/agent/planner/task_decomposer.py`)

Implements the `planner.task_decompose` output contract:

```python
class TaskDecomposer:
    def __init__(self, intent_parser, registry: SkillRegistry, policy: PolicyEngine): ...

    def decompose(self, request: dict) -> dict:
        """
        Input:  planner.task_decompose input contract
        Output: planner.task_decompose output contract
        """
```

Step construction rules (from `HarborNAS-Planner-TaskDecompose-Contract-v1.md`):
- Each step has unique `step_id`, `domain`, `operation`, `resource`, `args`.
- `risk_level` is assigned via `service_operation_risk()` / `file_operation_risk()` from
  the integration library.
- `route_candidates` lists only routes for which the skill has an enabled executor, in
  policy priority order.
- `requires_confirmation=True` when `risk_level in ("HIGH", "CRITICAL")`.
- `depends_on` is populated for sequential actions in the same NL request.

### 3.3 Plan Validator (`src/agent/planner/plan_validator.py`)

- Validates the decomposer output against the contract schema.
- Checks: non-empty steps, unique step IDs, valid `depends_on` references, no DAG cycles,
  `requires_confirmation` set for HIGH/CRITICAL.
- Returns `(valid: bool, errors: list[str])`.

**Acceptance tests** (`tests/unit/test_task_decomposer.py`):
- "Restart SSH" → one step, `risk_level=HIGH`, `requires_confirmation=True`,
  `route_candidates=["middleware_api","midcli"]`.
- "Enable SSH then restart" → two steps, `s2.depends_on=["s1"]`.
- Unknown capability → `ok=False`, `error.code=UNKNOWN_CAPABILITY`.
- Plan with dependency cycle → validator returns `valid=False`.

### Phase 3 Done Criteria

- [ ] `tests/unit/test_task_decomposer.py` passes.
- [ ] Planner output validates against the JSON schema in the contract document.

---

## Phase 4 — Skill Implementations (Week 5–6)

**Goal**: The two production skills (`system.harbor_ops`, `files.batch_ops`) are
implemented, tested, and registered.

### 4.1 `system.harbor_ops` Skill

**Files**: `src/agent/skills/system_harbor_ops/skill.yaml`, `handler.py`

`skill.yaml` (excerpt):
```yaml
id: system.harbor_ops
version: 1.0.0
harbor_api:
  enabled: true
  provider: middleware
  endpoint_group: service
  allowed_methods: [query, start, stop, restart, update]
harbor_cli:
  enabled: true
  tool: midcli
  command_group: service
  allowed_subcommands: [status, start, stop, restart]
risk:
  default_level: MEDIUM
  requires_confirmation: [HIGH, CRITICAL]
```

`handler.py` maps each operation to the integration library:

| Operation | Library call |
|---|---|
| `status` / `query` | `middleware.call("service.query", service_name)` |
| `start` | `execute_service_action(operation="start", ...)` |
| `stop` | `execute_service_action(operation="stop", ...)` |
| `restart` | `execute_service_action(operation="restart", ...)` |
| `enable` | `middleware.call("service.update", ...)` |

- All operations validate `service_name` against `^[a-z0-9_-]{1,64}$`.
- Unknown operations return `{"ok": false, "error": {"code": "VALIDATION_ERROR"}}`.

**Contract tests** (`src/agent/skills/system_harbor_ops/tests/contract_test.json`):
- Happy-path status query.
- START/STOP/RESTART with and without approval token.
- Invalid service name rejected.
- Middleware unavailable → midcli fallback.

**Integration tests** (`tests/integration/test_harbor_ops_skill.py`):
- Unit-level: mock `MiddlewareClient` and `MidcliClient`.
- Live-mode: skipped unless `HARBOR_MIDDLEWARE_BIN` resolves.

### 4.2 `files.batch_ops` Skill

**Files**: `src/agent/skills/files_batch_ops/skill.yaml`, `handler.py`

`skill.yaml` (excerpt):
```yaml
id: files.batch_ops
version: 1.0.0
harbor_api:
  enabled: true
  provider: middleware
  endpoint_group: filesystem
  allowed_methods: [listdir, copy, move, archive, search]
harbor_cli:
  enabled: true
  tool: midcli
  command_group: filesystem
  allowed_subcommands: [copy, move]
risk:
  default_level: MEDIUM
  requires_confirmation: [HIGH, CRITICAL]
permissions:
  fs_read:  ["/mnt/**", "/data/**"]
  fs_write: ["/mnt/**", "/data/**", "/tmp/agent/**"]
```

`handler.py` maps operations to the integration library:

| Operation | Library call |
|---|---|
| `copy` | `execute_file_action(operation="copy", ...)` |
| `move` | `execute_file_action(operation="move", ...)` |
| `archive` | `execute_file_action(operation="archive", ...)` |
| `search` / `listdir` | `middleware.call("filesystem.listdir", path)` or midcli fallback |

- Path normalisation and `validate_path_policy()` called before execution.
- CLI template mode enforces the template allowlist from the Files contract.
- `dry_run=True` always for HIGH operations unless `HARBOR_ALLOW_MUTATIONS=1`.

**Contract tests** and **integration tests** mirror those for `system.harbor_ops`.

### Phase 4 Done Criteria

- [ ] `tests/integration/test_harbor_ops_skill.py` passes (mock mode).
- [ ] `tests/integration/test_files_batch_ops_skill.py` passes (mock mode).
- [ ] Skill manifests validate against `HarborNAS-Skill-Spec-v1.md` schema.
- [ ] `scripts/run_e2e_suite.py --env env-a` exits 0 in documentation-only mode.

---

## Phase 5 — API Gateway and Session Persistence (Week 7–8)

**Goal**: External clients (mobile PWA, WebUI panel, REST callers) can submit requests
and receive streaming responses.

### 5.1 FastAPI Application (`src/agent/gateway/api.py`)

Endpoints:

| Method | Path | Description |
|---|---|---|
| `POST` | `/v1/tasks` | Submit a natural-language request |
| `GET` | `/v1/tasks/{task_id}` | Poll task status |
| `GET` | `/v1/tasks/{task_id}/stream` | Server-Sent Events stream of task events |
| `POST` | `/v1/tasks/{task_id}/confirm` | Provide approval token for HIGH/CRITICAL step |
| `GET` | `/v1/skills` | List registered skills |
| `GET` | `/v1/health` | Liveness + readiness probe |

Request / response bodies follow the unified envelope defined in
`HarborNAS-Skill-Spec-v1.md` §Request Envelope / Response Envelope.

### 5.2 Auth Middleware (`src/agent/gateway/auth.py`)

- Phase 1: static API-key check via `Authorization: Bearer <key>` header.
- Key loaded from `AGENT_API_KEY` environment variable.
- Requests without a valid key return `403`.

### 5.3 Redis Session Store (`src/agent/gateway/session_store.py`)

- Replaces the in-memory `SessionStore` from Phase 1.
- `TaskRecord` serialised to JSON, stored under `task:{task_id}`.
- TTL set to `AgentConfig.session_ttl_s` (default 86400 s).
- Graceful degradation: if Redis is unavailable, fall back to in-memory store with a
  warning log.

### 5.4 Full Pipeline Wiring

`POST /v1/tasks` triggers this sequence:

```
1. Auth check
2. Create TaskRecord (state=queued)
3. TaskDecomposer.decompose(request)      → plan or UNKNOWN_CAPABILITY error
4. TaskRecord(state=planned, plan=plan)
5. For each step in plan (in topological order):
   a. PolicyEngine.check(step)            → deny immediately if failed
   b. If requires_confirmation and no token → pause, SSE event: "awaiting_confirmation"
   c. SkillRouter.route(step)             → selected executor
   d. executor.execute(step)              → ExecutorResult
   e. TaskRecord updated, SSE event emitted
6. TaskRecord(state=completed|failed)
```

### Phase 5 Done Criteria

- [ ] `POST /v1/tasks` returns `task_id` within 200 ms (p95) for mock executor.
- [ ] SSE stream delivers events in order.
- [ ] `GET /v1/health` returns 200 with middleware/midcli availability status.

---

## Phase 6 — Observability and Security Hardening (Week 7–8, parallel)

### 6.1 Structured Logger (`src/agent/observability/logger.py`)

- JSON lines format on stdout.
- Every log record includes: `timestamp`, `level`, `task_id`, `trace_id`, `component`.
- Replace all `print()` calls across `src/`.

### 6.2 Audit Events (`src/agent/observability/audit.py`)

Required audit fields (from E2E test plan §Observability Checks):

```json
{
  "event":       "task.step.executed",
  "task_id":     "uuid",
  "trace_id":    "uuid",
  "step_id":     "s1",
  "executor_used": "middleware_api",
  "route_fallback_used": false,
  "risk_level":  "HIGH",
  "approved_by": "approver-id-or-null",
  "ok":          true,
  "duration_ms": 120,
  "timestamp":   "iso8601"
}
```

Audit events are append-only and written to the structured logger with `level=AUDIT`.
They are also forwarded to the metrics layer.

### 6.3 Prometheus Metrics (`src/agent/observability/metrics.py`)

Key metrics (from V2 Roadmap §KPIs):

| Metric | Type | Labels |
|---|---|---|
| `agent_tasks_total` | Counter | `status` (completed/failed) |
| `agent_executor_route_total` | Counter | `executor`, `fallback` |
| `agent_task_duration_ms` | Histogram | `executor` |
| `agent_policy_violations_total` | Counter | `violation_type` |
| `agent_confirmation_required_total` | Counter | `risk_level` |

Exposed at `GET /metrics` (Prometheus scrape endpoint).

### 6.4 Security Controls

From `HarborNAS-Skill-Spec-v1.md` §Security Controls:

1. **Command policy** — Argument tokenizer in `MidCLIExecutor` must never join
   arguments into a raw shell string. Use `subprocess` list form only.
   Deny shell metacharacters (`;`, `&&`, `||`, `` ` ``, `$()`, `>`, `<`).
2. **Sandbox** — Skills run inside an isolated working directory
   (`HARBOR_MUTATION_ROOT`). No symlink traversal outside sandbox.
3. **Approval audit** — Every HIGH/CRITICAL execution must record `approved_by`.
4. **Secret hygiene** — `AGENT_API_KEY`, `HARBOR_MIDCLI_PASSWORD`, approval tokens
   must never appear in logs or structured output.

### Phase 6 Done Criteria

- [ ] Every task execution emits an audit event with all required fields.
- [ ] Prometheus endpoint returns `agent_executor_route_total` with correct labels.
- [ ] No secrets appear in log output (manual review + grep for token values).

---

## Phase 7 — Integration Testing and Release Gate (Week 7–8)

### 7.1 Full Pipeline Integration Tests

`tests/integration/test_planner_router_executor.py`:

Covers all 6 E2E scenarios from `HarborNAS-Contract-E2E-Test-Plan-v1.md`:

| Scenario | Environment | Expected outcome |
|---|---|---|
| Service control happy path | ENV-A (mock middleware + midcli) | middleware_api used, HIGH step requires token |
| Service control fallback | ENV-B (middleware degraded) | midcli used, `route_fallback_used=True` |
| Files copy within policy | ENV-A | path policy passes, items_processed=1 |
| Files operation policy denied | any | `PATH_POLICY_DENIED` before execution |
| Planner no-executable-route | any | `NO_EXECUTABLE_ROUTE`, no executor invoked |
| Mixed multi-step task | ENV-A | DAG-valid plan, ordered execution |

### 7.2 KPI Validation

Verify all KPIs from V2 Roadmap §KPIs during integration test run:

| KPI | Target | Measured by |
|---|---|---|
| API route ratio | >= 70% | `agent_executor_route_total` |
| CLI fallback ratio | <= 25% | `agent_executor_route_total` |
| Task success rate | >= 95% | `agent_tasks_total` |
| P95 orchestration latency | <= 2 s | `agent_task_duration_ms` |
| HIGH-risk confirmation coverage | 100% | `agent_confirmation_required_total` vs executions |
| Skill regression pass rate | >= 98% | pytest exit code |

### 7.3 CI Integration

The existing three GitHub Actions workflows gate on:

1. **PR check** (`contract-pr-check.yml`):
   - Schema validation → contract tests → fallback tests → policy tests → **new**: unit tests.

2. **Nightly E2E** (`contract-nightly-e2e.yml`):
   - Add `tests/integration/` suite to the nightly matrix.

3. **Release drift** (`contract-release-drift.yml`):
   - Drift matrix → release gate → **new**: integration test suite against the release tag.

No existing workflow files need to be deleted; only additional `pytest` steps are added.

### Phase 7 Done Criteria

- [ ] All six integration scenarios pass.
- [ ] `scripts/run_e2e_suite.py --env env-a` passes in documentation-only mode.
- [ ] `scripts/run_drift_matrix.py` exits 0.
- [ ] `scripts/evaluate_release_gate.py` returns `"allowed": true`.
- [ ] All existing tests still pass.

---

## Risk Register

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Middleware API changes between HarborNAS and upstream | MEDIUM | HIGH | Drift matrix CI job; fallback to midcli |
| Unsafe command injection via midcli | LOW | CRITICAL | Argument tokenizer allowlist; subprocess list form; no shell expansion |
| Approval token leakage in logs | LOW | HIGH | Token scrubbing in logger; test with known token values |
| LLM-based intent parser inaccuracy (future) | MEDIUM | MEDIUM | Start with rule-based parser; add LLM layer behind feature flag |
| Redis unavailable in production | LOW | MEDIUM | Graceful fallback to in-memory store; health endpoint reports degraded |
| Browser/MCP executor overuse | LOW | MEDIUM | Hard routing policy; route-ratio alerting in Prometheus |

---

## Responsibility Matrix (3-person team)

| Role | Owns |
|---|---|
| Engineer A (AI/Backend) | Intent parser, task decomposer, plan validator, policy engine, skill router |
| Engineer B (Platform/Data) | Skill registry, executor classes, API gateway, session store, state machine |
| Engineer C (DevOps/Security/QA) | Observability layer, security controls, CI workflow updates, integration tests |

---

## Milestone Summary

| Milestone | End of Week | Key Deliverable |
|---|---|---|
| M1 — Core infra | 2 | Config, state machine, executor interface, MiddlewareExecutor + MidCLIExecutor unit-tested |
| M2 — Router + Registry | 4 | Skill registry, router with policy, planner with DAG validator |
| M3 — First skills | 6 | `system.harbor_ops` + `files.batch_ops` passing integration tests |
| M4 — API + Observability | 8 | FastAPI gateway, SSE streaming, audit log, Prometheus metrics |
| M5 — Beta | 8 | All 6 E2E scenarios passing, CI gates green, KPIs measured |

---

## Quick-start for Implementers

```bash
# 1. Install dev dependencies
pip install -r requirements-dev.txt

# 2. Run existing contract tests to confirm clean baseline
pytest tests/ -q

# 3. Create src package structure
mkdir -p src/agent/{planner,router,executors,skills,gateway,observability}
touch src/__init__.py src/agent/__init__.py ...

# 4. Start with Phase 1: config.py → session.py → executors/base.py

# 5. Validate each file with the existing CI script
python scripts/validate_contract_schemas.py --report /tmp/check.json

# 6. Live integration (when midclt and cli are available on the target host)
HARBOR_MIDDLEWARE_BIN=midclt HARBOR_MIDCLI_BIN=cli \
  python scripts/validate_contract_schemas.py --require-live
```

---

## Appendix: Environment Variable Reference

Inherited from `scripts/harbor_integration.py` (all remain valid):

| Variable | Default | Purpose |
|---|---|---|
| `HARBOR_MIDDLEWARE_BIN` | `midclt` | Middleware CLI binary |
| `HARBOR_MIDCLI_BIN` | `cli` | MidCLI binary |
| `HARBOR_MIDCLI_URL` | — | Remote midcli WebSocket URL |
| `HARBOR_MIDCLI_USER` | — | MidCLI auth username |
| `HARBOR_MIDCLI_PASSWORD` | — | MidCLI auth password |
| `HARBOR_PROBE_SERVICE` | `ssh` | Service used for live health probes |
| `HARBOR_FILESYSTEM_PATH` | `/mnt` | Path used for filesystem probes |
| `HARBOR_ALLOW_MUTATIONS` | `0` | Set to `1` to allow write operations |
| `HARBOR_APPROVAL_TOKEN` | — | Token passed for HIGH/CRITICAL operations |
| `HARBOR_REQUIRED_APPROVAL_TOKEN` | — | Expected token value for local gate |
| `HARBOR_APPROVER_ID` | — | Approver identity for audit trail |
| `HARBOR_MUTATION_ROOT` | `/mnt/agent-ci` | Sandbox root for mutation fixtures |
| `HARBOR_SOURCE_REPO_PATH` | — | Local HarborNAS source tree for drift check |
| `UPSTREAM_SOURCE_REPO_PATH` | — | Upstream source tree for drift check |

New variables introduced by this plan:

| Variable | Default | Purpose |
|---|---|---|
| `AGENT_API_KEY` | — | Static API key for gateway auth |
| `AGENT_API_HOST` | `0.0.0.0` | Gateway bind host |
| `AGENT_API_PORT` | `8000` | Gateway bind port |
| `AGENT_REDIS_URL` | — | Redis URL for session store |
| `AGENT_SESSION_TTL_S` | `86400` | Session TTL in seconds |
| `AGENT_METRICS_PORT` | `9090` | Prometheus metrics port |

---

*Version: 1.0 — Created: 2026-03-22*
