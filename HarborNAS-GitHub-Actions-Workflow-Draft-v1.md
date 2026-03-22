# HarborNAS GitHub Actions Workflow Draft v1

## Purpose
This document translates the CI contract pipeline checklist into an initial GitHub Actions workflow design that engineering can implement with minimal interpretation.

## Workflow Set
1. `contract-pr-check.yml`
2. `contract-nightly-e2e.yml`
3. `contract-release-drift.yml`

## 1) contract-pr-check.yml

### Trigger
- pull_request
- push to active development branches

### Jobs

#### job: schema_check
Purpose:
- validate planner/action/request/response schemas
- validate route-priority ordering

Suggested steps:
```yaml
- uses: actions/checkout@v4
- uses: actions/setup-python@v5
  with:
    python-version: '3.11'
- run: pip install -r requirements-dev.txt
- run: python scripts/validate_contract_schemas.py
```

#### job: contract_unit_tests
Purpose:
- run system.harbor_ops, files.batch_ops, planner.task_decompose unit contract tests

Suggested steps:
```yaml
- uses: actions/checkout@v4
- uses: actions/setup-python@v5
  with:
    python-version: '3.11'
- run: pip install -r requirements-dev.txt
- run: pytest tests/contracts -q --json-report --json-report-file=contract-test-report.json
```

#### job: fallback_chain_tests
Purpose:
- validate middleware -> midcli -> constrained CLI fallback behavior

Suggested steps:
```yaml
- uses: actions/checkout@v4
- uses: actions/setup-python@v5
  with:
    python-version: '3.11'
- run: pip install -r requirements-dev.txt
- run: pytest tests/fallback -q --json-report --json-report-file=fallback-report.json
```

#### job: policy_risk_gates
Purpose:
- enforce high-risk confirmation rules
- enforce path deny rules
- enforce command template allowlist

Suggested steps:
```yaml
- uses: actions/checkout@v4
- uses: actions/setup-python@v5
  with:
    python-version: '3.11'
- run: pip install -r requirements-dev.txt
- run: pytest tests/policy -q --json-report --json-report-file=policy-enforcement-report.json
```

### Required Artifacts
- contract-test-report.json
- fallback-report.json
- policy-enforcement-report.json

## 2) contract-nightly-e2e.yml

### Trigger
- schedule (nightly)
- manual dispatch

### Target Environments
- ENV-A: middleware + midcli available
- ENV-B: middleware degraded + midcli available

### Jobs

#### job: e2e_matrix
Purpose:
- run mandatory scenarios from HarborNAS-Contract-E2E-Test-Plan-v1.md

Suggested strategy:
```yaml
strategy:
  matrix:
    env_profile: [env-a, env-b]
```

Suggested steps:
```yaml
- uses: actions/checkout@v4
- uses: actions/setup-python@v5
  with:
    python-version: '3.11'
- run: pip install -r requirements-dev.txt
- run: python scripts/run_e2e_suite.py --env ${{ matrix.env_profile }} --report e2e-report.json
```

### Required Artifacts
- e2e-report.json
- latency-summary.json
- audit-coverage-summary.json

## 3) contract-release-drift.yml

### Trigger
- push to release branches
- workflow_dispatch

### Jobs

#### job: drift_compatibility
Purpose:
- compare HarborNAS refs against upstream refs
- run compatibility matrix validation

Suggested steps:
```yaml
- uses: actions/checkout@v4
  with:
    fetch-depth: 0
- uses: actions/setup-python@v5
  with:
    python-version: '3.11'
- run: pip install -r requirements-dev.txt
- run: python scripts/run_drift_matrix.py --harbor-ref develop --upstream-ref master --report drift-matrix-report.json
```

#### job: release_gate
Purpose:
- fail release when blocking rows exist or fallback validation fails

Suggested steps:
```yaml
- uses: actions/download-artifact@v4
- run: python scripts/evaluate_release_gate.py drift-matrix-report.json
```

### Required Artifacts
- drift-matrix-report.json
- release-gate-summary.json

## Shared Standards

### Concurrency
```yaml
concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: true
```

### Artifact Upload
```yaml
- uses: actions/upload-artifact@v4
  with:
    name: contract-reports
    path: |
      contract-test-report.json
      fallback-report.json
      policy-enforcement-report.json
```

### Failure Policy
- Any PR-check job failure blocks merge.
- Nightly E2E failures open incident and mark dashboard red.
- Release drift failures block tagging and promotion.

## Recommended Repository Layout
```text
.github/workflows/
  contract-pr-check.yml
  contract-nightly-e2e.yml
  contract-release-drift.yml
scripts/
  validate_contract_schemas.py
  run_e2e_suite.py
  run_drift_matrix.py
  evaluate_release_gate.py
tests/
  contracts/
  fallback/
  policy/
```

## Implementation Order
1. Implement `contract-pr-check.yml` first.
2. Add artifact upload and merge blocking.
3. Add nightly E2E matrix.
4. Add release drift gate last.

## Exit Criteria
This draft is implementation-ready when:
- workflow filenames are accepted by the team,
- required scripts are assigned owners,
- artifact names are wired into dashboard/report consumers.
