# HarborNAS Middleware Endpoint Contract v1

## Purpose
This document defines the executable contract for HarborOS-domain skills that use middleware API as primary route and midcli as fallback.

## Scope
- Skill: `system.harbor_ops`
- Domain: service lifecycle operations
- Primary route: middleware API
- Fallback route: midcli

## Canonical Action Model
All NL intents must normalize into this action object before routing.

```json
{
  "domain": "service",
  "operation": "status|start|stop|restart|enable",
  "resource": {
    "service_name": "ssh"
  },
  "args": {
    "enable": true
  },
  "risk_level": "LOW|MEDIUM|HIGH|CRITICAL",
  "dry_run": false
}
```

## Endpoint Mapping Table

| Operation | Middleware Method | Required Fields | Expected Result | MidCLI Fallback | Risk |
|---|---|---|---|---|---|
| status | `service.query` | `service_name` | service state object | `service <name> show` | LOW |
| start | `service.start` | `service_name` | started=true | `service start service=<name>` | MEDIUM |
| stop | `service.stop` | `service_name` | stopped=true | `service stop service=<name>` | HIGH |
| restart | `service.restart` | `service_name` | restarted=true | `service restart service=<name>` | HIGH |
| enable | `service.update` | `service_name`, `enable` | persisted enable state | `service update id_or_name=<name> enable=true/false` | MEDIUM |

## Request Contract (Normalized -> Middleware)

```json
{
  "task_id": "uuid",
  "trace_id": "uuid",
  "skill_id": "system.harbor_ops",
  "executor": "middleware_api",
  "action": {
    "domain": "service",
    "operation": "start",
    "resource": {
      "service_name": "ssh"
    },
    "args": {},
    "risk_level": "MEDIUM",
    "dry_run": false
  }
}
```

## Response Contract (Middleware -> Unified Envelope)

```json
{
  "task_id": "uuid",
  "trace_id": "uuid",
  "ok": true,
  "executor_used": "middleware_api",
  "route_fallback_used": false,
  "result": {
    "service_name": "ssh",
    "operation": "start",
    "state": "RUNNING"
  },
  "artifacts": [],
  "metrics": {
    "duration_ms": 120,
    "retries": 0
  },
  "error": null
}
```

## Route Selection Rules
1. Use middleware API when method mapping exists and health check is green.
2. Use midcli fallback when middleware method is unavailable, incompatible, or unhealthy.
3. Browser/MCP are not valid for `system.harbor_ops` unless explicitly expanded.
4. HIGH/CRITICAL operations require approval token before execution.

## Validation Rules
- `service_name` must match allowlist pattern: `^[a-z0-9_-]{1,64}$`
- Unknown operations are rejected before routing.
- `enable` is required for `operation=enable`.
- If `dry_run=true`, return preview payload without side effects.

## Error Model

```json
{
  "ok": false,
  "error": {
    "code": "METHOD_UNAVAILABLE|VALIDATION_ERROR|TIMEOUT|NON_ZERO_EXIT|AUTH_REQUIRED",
    "message": "human-readable summary",
    "details": {
      "operation": "start",
      "service_name": "ssh"
    }
  }
}
```

## Compatibility Matrix Template

| Capability | HarborNAS middleware ref | Upstream middleware ref | Contract test | MidCLI fallback test | Decision |
|---|---|---|---|---|---|
| service.query | develop | master | pass/fail | pass/fail | keep/block |
| service.start | develop | master | pass/fail | pass/fail | keep/block |
| service.stop | develop | master | pass/fail | pass/fail | keep/block |
| service.restart | develop | master | pass/fail | pass/fail | keep/block |
| service.update(enable) | develop | master | pass/fail | pass/fail | keep/block |

## Minimum Test Cases
1. Happy path: start/stop/restart each service in allowlist test fixture.
2. Validation: reject illegal service names and unsupported operations.
3. Approval: enforce token for HIGH operations.
4. Fallback: force middleware failure and verify midcli route.
5. Drift: run compatibility matrix against HarborNAS + upstream refs.

## Release Gate
A release is allowed only when:
- all contract tests pass,
- fallback tests pass,
- compatibility matrix has no blocking rows,
- audit fields are present in every response.
