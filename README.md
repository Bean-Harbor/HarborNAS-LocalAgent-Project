# HarborNAS Local Agent Planning Package

This repository contains the completed planning deliverables for a HarborNAS local-first AI agent project, including architecture, roadmap, quick reference, meeting guide, launch checklist, and document index.

## Documents
- HarborNAS-LocalAgent-Plan.md
- HarborNAS-LocalAgent-Roadmap.md
- HarborNAS-LocalAgent-QuickRef.md
- HarborNAS-LocalAgent-MeetingGuide.md
- HarborNAS-LocalAgent-LaunchChecklist.md
- HarborNAS-LocalAgent-DocumentIndex.md
- HarborNAS-LocalAgent-V2-Assistant-Skills-Roadmap.md
- HarborNAS-Skill-Spec-v1.md
- HarborNAS-Middleware-Endpoint-Contract-v1.md
- HarborNAS-Files-BatchOps-Contract-v1.md
- HarborNAS-Planner-TaskDecompose-Contract-v1.md
- HarborNAS-Contract-E2E-Test-Plan-v1.md
- HarborNAS-CI-Contract-Pipeline-Checklist-v1.md
- HarborNAS-GitHub-Actions-Workflow-Draft-v1.md

## V2 Additions

- `HarborNAS-LocalAgent-V2-Assistant-Skills-Roadmap.md`: assistant + skills integration roadmap with HarborOS control-plane-first policy (`middleware API > midcli > browser > MCP`).
- `HarborNAS-Skill-Spec-v1.md`: standard skill contract (manifest schema, runtime envelope, routing, risk and test requirements), including HarborOS `middleware` API binding and `midcli` fallback rules.
- `HarborNAS-Middleware-Endpoint-Contract-v1.md`: executable endpoint contract for `system.harbor_ops`, including action normalization, API/CLI mapping, error model, and compatibility matrix template.
- `HarborNAS-Files-BatchOps-Contract-v1.md`: executable endpoint contract for `files.batch_ops`, including path policy, route fallback chain, CLI template constraints, and compatibility matrix template.
- `HarborNAS-Planner-TaskDecompose-Contract-v1.md`: executable planner contract for `planner.task_decompose`, including step schema, dependency rules, route-candidate policy, and release gates.
- `HarborNAS-Contract-E2E-Test-Plan-v1.md`: end-to-end validation plan across planner + execution contracts, including environment matrix, fallback checks, drift checks, and release exit criteria.
- `HarborNAS-CI-Contract-Pipeline-Checklist-v1.md`: CI job checklist that maps all contract governance to merge, nightly, and pre-release pipeline stages.
- `HarborNAS-GitHub-Actions-Workflow-Draft-v1.md`: initial GitHub Actions workflow draft mapping contract governance into concrete PR, nightly, and release workflows.

## Executable CI Scaffold

- `.github/workflows/contract-pr-check.yml`: PR and branch validation for contract schema checks plus contract, fallback, and policy test suites.
- `.github/workflows/contract-nightly-e2e.yml`: nightly/manual E2E matrix scaffold for `env-a` and `env-b`.
- `.github/workflows/contract-release-drift.yml`: release-branch drift matrix and release gate workflow.
- `scripts/validate_contract_schemas.py`: validates that required contract documents and route-priority rules stay aligned.
- `scripts/run_e2e_suite.py`: emits scaffolded E2E, latency, and audit reports for workflow wiring.
- `scripts/run_drift_matrix.py`: emits the initial drift-matrix artifact for release gating.
- `scripts/evaluate_release_gate.py`: converts drift output into a blocking/non-blocking release decision.
- `tests/contracts`, `tests/fallback`, `tests/policy`: minimal pytest suites that keep the documented routing, fallback, and governance rules from regressing.

Current scope note: this scaffold validates documentation-backed contracts and CI wiring. It does not yet execute against a live HarborNAS middleware or `midcli` environment.
