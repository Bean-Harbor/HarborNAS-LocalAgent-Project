# HarborBeacon Planner TaskDecompose Contract v1

## Purpose
This document defines the executable contract for `planner.task_decompose` so natural-language requests can be transformed into deterministic execution plans aligned with route priority:
1. middleware API
2. midcli
3. browser
4. MCP

## Scope
- Skill: `planner.task_decompose`
- Input: user intent, context, policy, capability registry snapshot
- Output: validated execution plan with step-level route candidates and risk metadata

## Input Contract

```json
{
  "task_id": "uuid",
  "trace_id": "uuid",
  "skill_id": "planner.task_decompose",
  "input": {
    "user_request": "Enable SSH and restart service",
    "context": {
      "user_role": "admin",
      "session_id": "uuid",
      "host_profile": "harboros-node-1"
    },
    "policy": {
      "route_priority": ["middleware_api", "midcli", "browser", "mcp"],
      "require_confirmation_levels": ["HIGH", "CRITICAL"]
    },
    "capability_registry_version": "v1"
  }
}
```

## Output Contract

```json
{
  "task_id": "uuid",
  "trace_id": "uuid",
  "ok": true,
  "result": {
    "goal": "Enable and restart SSH service",
    "plan_version": "1.0",
    "steps": [
      {
        "step_id": "s1",
        "domain": "service",
        "operation": "enable",
        "resource": {
          "service_name": "ssh"
        },
        "args": {
          "enable": true
        },
        "risk_level": "MEDIUM",
        "route_candidates": ["middleware_api", "midcli"],
        "requires_confirmation": false,
        "depends_on": []
      },
      {
        "step_id": "s2",
        "domain": "service",
        "operation": "restart",
        "resource": {
          "service_name": "ssh"
        },
        "args": {},
        "risk_level": "HIGH",
        "route_candidates": ["middleware_api", "midcli"],
        "requires_confirmation": true,
        "depends_on": ["s1"]
      }
    ]
  },
  "error": null
}
```

## Planning Rules
1. Decompose user intent into atomic actions that map to known capabilities.
2. Each step must include normalized fields: `domain`, `operation`, `resource`, `args`.
3. Each step must include `route_candidates` in priority order.
4. Each step must include `risk_level` and `requires_confirmation`.
5. Dependencies must form a DAG (no cycles).
6. If no route candidates are available for a required step, planner returns `ok=false` with `NO_EXECUTABLE_ROUTE`.

## Validation Rules
- `steps` must be non-empty.
- `step_id` values must be unique.
- `depends_on` references must point to existing prior steps.
- Route candidates must be subset of allowed list: `middleware_api`, `midcli`, `browser`, `mcp`.
- HIGH/CRITICAL must set `requires_confirmation=true`.

## Error Model

```json
{
  "ok": false,
  "error": {
    "code": "INVALID_INPUT|UNKNOWN_CAPABILITY|NO_EXECUTABLE_ROUTE|POLICY_VIOLATION|DEPENDENCY_CYCLE",
    "message": "human-readable summary",
    "details": {
      "step_id": "s2"
    }
  }
}
```

## Planner Quality Gates
- Plan validity pass rate >= 99%.
- Route-policy compliance = 100%.
- Risk labeling precision >= 95% on test corpus.
- No unresolved dependencies in final plan.

## Compatibility Matrix Template

| Planner Capability | HarborBeacon policy ref | Upstream policy ref | Plan schema test | Route compliance test | Decision |
|---|---|---|---|---|---|
| service.enable decomposition | develop | master | pass/fail | pass/fail | keep/block |
| service.restart decomposition | develop | master | pass/fail | pass/fail | keep/block |
| files.copy decomposition | develop | master | pass/fail | pass/fail | keep/block |
| mixed-task dependency ordering | develop | master | pass/fail | pass/fail | keep/block |

## Minimum Test Cases
1. Single-step decomposition (read-only request).
2. Multi-step decomposition with dependencies.
3. Risk escalation and confirmation tagging.
4. No-route handling and explicit failure return.
5. Policy override rejection when user intent conflicts with governance.

## Release Gate
Release is allowed only when:
- plan schema tests pass,
- route compliance tests pass,
- dependency-cycle tests pass,
- compatibility matrix has no blocking rows.
