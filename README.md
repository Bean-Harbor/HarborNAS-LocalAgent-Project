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

## V2 Additions

- `HarborNAS-LocalAgent-V2-Assistant-Skills-Roadmap.md`: assistant + skills integration roadmap with HarborOS control-plane-first policy (`middleware API > midcli > browser > MCP`).
- `HarborNAS-Skill-Spec-v1.md`: standard skill contract (manifest schema, runtime envelope, routing, risk and test requirements), including HarborOS `middleware` API binding and `midcli` fallback rules.
- `HarborNAS-Middleware-Endpoint-Contract-v1.md`: executable endpoint contract for `system.harbor_ops`, including action normalization, API/CLI mapping, error model, and compatibility matrix template.
- `HarborNAS-Files-BatchOps-Contract-v1.md`: executable endpoint contract for `files.batch_ops`, including path policy, route fallback chain, CLI template constraints, and compatibility matrix template.
- `HarborNAS-Planner-TaskDecompose-Contract-v1.md`: executable planner contract for `planner.task_decompose`, including step schema, dependency rules, route-candidate policy, and release gates.
- `HarborNAS-Contract-E2E-Test-Plan-v1.md`: end-to-end validation plan across planner + execution contracts, including environment matrix, fallback checks, drift checks, and release exit criteria.
- `HarborNAS-CI-Contract-Pipeline-Checklist-v1.md`: CI job checklist that maps all contract governance to merge, nightly, and pre-release pipeline stages.
