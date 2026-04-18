# HarborBeacon Contract E2E Test Plan v1

## Purpose
This document defines end-to-end validation across three executable contracts:
- system.harbor_ops
- files.batch_ops
- planner.task_decompose

The goal is to ensure planning, routing, execution, fallback, and governance remain consistent under real operational conditions.

## Covered Contract Documents
1. HarborBeacon-Middleware-Endpoint-Contract-v1.md
2. HarborBeacon-Files-BatchOps-Contract-v1.md
3. HarborBeacon-Planner-TaskDecompose-Contract-v1.md

## E2E Pipeline Under Test
1. User request ingestion
2. Planner decomposition
3. Route selection (middleware API -> midcli -> browser -> MCP)
4. Executor invocation
5. Result normalization and audit logging
6. Policy checks and final response

## Test Environments
- ENV-A: HarborBeacon middleware available + midcli available
- ENV-B: middleware degraded + midcli available
- ENV-C: middleware unavailable + midcli unavailable + constrained CLI available
- ENV-D: all routes unavailable (negative path)

## Core E2E Scenarios

### Scenario 1: Service control happy path
- Input: "Enable SSH and restart service"
- Expected:
  - Planner outputs two ordered steps (enable -> restart)
  - Route chosen: middleware API for both steps
  - HIGH-risk restart requires confirmation token
  - Unified response contains task_id, trace_id, executor_used

### Scenario 2: Service control fallback path
- Input: "Restart SMB service"
- Precondition: middleware service.restart unavailable
- Expected:
  - Planner step remains valid
  - Router falls back to midcli
  - route_fallback_used=true in response
  - Audit log records fallback reason

### Scenario 3: Files copy within policy
- Input: "Copy /mnt/data/a.txt to /mnt/backup/"
- Expected:
  - Path policy check passes
  - Preferred route: middleware API (or midcli if method missing)
  - Result includes items_processed=1

### Scenario 4: Files operation denied by policy
- Input: "Move /etc/passwd to /mnt/backup/"
- Expected:
  - Request rejected before execution
  - Error code: PATH_POLICY_DENIED
  - No executor side effects

### Scenario 5: Planner no-executable-route failure
- Input: request requiring unsupported capability
- Precondition: no mapped route candidates
- Expected:
  - Planner returns NO_EXECUTABLE_ROUTE
  - No execution stage invoked

### Scenario 6: Mixed multi-step task
- Input: "Search logs then archive results"
- Expected:
  - Planner outputs DAG-valid ordered steps
  - Route candidates adhere to policy priority
  - Final artifacts include archive path

## Contract Assertions
- Every step has domain, operation, resource, args, risk_level
- Route candidates are ordered by policy
- HIGH/CRITICAL operations always require confirmation
- Response envelope includes executor_used and route_fallback_used
- Errors use standardized error model codes from contract docs

## Non-Functional Assertions
- P95 pre-execution orchestration latency <= 2s
- Plan schema validation success >= 99%
- Route-policy compliance = 100%
- Audit event completeness = 100%

## Observability Checks
- Correlate task_id + trace_id across planner/router/executor logs
- Verify fallback events include root cause
- Verify risk confirmation events include approver identity

## Compatibility and Drift Matrix

| Scenario | HarborBeacon ref | Upstream ref | Planner schema | Route compliance | Executor contract | Status |
|---|---|---|---|---|---|---|
| service enable/restart | develop | master | pass/fail | pass/fail | pass/fail | pending |
| files copy/move | develop | master | pass/fail | pass/fail | pass/fail | pending |
| mixed task DAG | develop | master | pass/fail | pass/fail | pass/fail | pending |

## Exit Criteria
Release candidate is accepted only if:
1. All core scenarios pass in ENV-A and ENV-B.
2. Policy-denied and no-route failures match expected error codes.
3. No schema-breaking drift in compatibility matrix.
4. Audit coverage reaches 100% for tested scenarios.

## Execution Cadence
- Per-commit: schema + unit-level contract tests
- Nightly: full E2E scenarios in ENV-A/ENV-B
- Pre-release: full matrix including ENV-C/ENV-D
