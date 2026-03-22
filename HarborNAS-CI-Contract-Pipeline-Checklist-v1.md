# HarborNAS CI Contract Pipeline Checklist v1

## Purpose
This checklist maps contract documents to executable CI stages so governance can be enforced automatically on every merge and release.

## Covered Contracts
1. HarborNAS-Middleware-Endpoint-Contract-v1.md
2. HarborNAS-Files-BatchOps-Contract-v1.md
3. HarborNAS-Planner-TaskDecompose-Contract-v1.md
4. HarborNAS-Contract-E2E-Test-Plan-v1.md

## Pipeline Stages

### Stage 1: Lint and Schema Validation
- Validate JSON schema for planner/action/request/response envelopes.
- Validate route-priority policy ordering.
- Validate error code catalog consistency.

Exit criteria:
- 100% schema validation pass
- 0 unknown fields in normalized action model

### Stage 2: Contract Unit Tests
- system.harbor_ops contract tests
- files.batch_ops contract tests
- planner.task_decompose contract tests

Exit criteria:
- pass rate >= 99%
- no contract-breaking regression

### Stage 3: Fallback Chain Tests
- middleware API unavailable -> midcli fallback
- midcli unavailable -> constrained CLI (where allowed)
- no-route scenario -> deterministic error

Exit criteria:
- fallback behavior matches contract
- no silent route skipping

### Stage 4: Policy and Risk Gates
- High-risk action confirmation enforcement
- Path policy deny rules
- command template allowlist checks

Exit criteria:
- 100% enforcement coverage for HIGH/CRITICAL
- 0 policy bypass

### Stage 5: E2E Scenarios (Nightly)
- Execute scenario set defined in HarborNAS-Contract-E2E-Test-Plan-v1.md
- Run in ENV-A and ENV-B daily

Exit criteria:
- all mandatory scenarios green
- P95 pre-execution orchestration latency <= 2s

### Stage 6: Drift and Compatibility Matrix (Pre-release)
- Run HarborNAS ref vs upstream ref compatibility matrix
- Compare endpoint/method/field changes

Exit criteria:
- no blocking rows in matrix
- fallback tests pass for all drift-affected capabilities

## Suggested CI Job Layout

| Job Name | Trigger | Stage | Required |
|---|---|---|---|
| contract-schema-check | PR, push | Stage 1 | yes |
| contract-unit-tests | PR, push | Stage 2 | yes |
| fallback-chain-tests | PR, push | Stage 3 | yes |
| policy-risk-gates | PR, push | Stage 4 | yes |
| e2e-nightly | schedule | Stage 5 | yes |
| drift-compatibility | release branch | Stage 6 | yes |

## Artifact Requirements
- contract-test-report.json
- fallback-report.json
- policy-enforcement-report.json
- e2e-report.json
- drift-matrix-report.json

All reports must include:
- commit SHA
- timestamp
- environment profile
- pass/fail summary
- blocking failures list

## Blocking Rules
- Any Stage 1-4 failure blocks merge.
- Stage 5 failures open incident ticket and block release promotion.
- Stage 6 blocking rows forbid release tagging.

## Ownership Matrix

| Area | Owner |
|---|---|
| Planner contract tests | AI/backend engineer |
| Execution contract tests | Platform engineer |
| Policy and risk gates | Security/QA engineer |
| Drift compatibility checks | DevOps engineer |

## Implementation Checklist
- [ ] Add workflow file for PR checks.
- [ ] Add scheduled nightly workflow.
- [ ] Add release-branch drift workflow.
- [ ] Upload all required reports as artifacts.
- [ ] Fail pipeline on blocking rules.
- [ ] Publish status summary to release dashboard.
