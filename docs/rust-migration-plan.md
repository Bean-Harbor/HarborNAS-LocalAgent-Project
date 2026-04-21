# Rust Migration Plan for HarborBeacon Local Agent

## Goal

Replace Python runtime modules with Rust while preserving HarborOS constraints:

- Core-in-HarborOS and extensions-as-plugins
- Route priority: `middleware_api -> midcli -> browser -> mcp`
- Command-line-first execution and `midcli` preference for HarborOS operations
- High-risk approval and full audit trail

## Current status

Implemented in Rust:

- Orchestrator contracts, runtime loop, router, policy, and audit
- HarborOps route executors (`middleware_api`, `midcli`)
- Skill manifest and registry basics
- Files plugin handler (`files.batch_ops`)
- Browser plugin handler (`browser.automation`)
- Media plugin handler (`media.video_edit`)
- Planner task decomposition module
- Contract script suite migration (`validate_contract_schemas`, `run_e2e_suite`, `run_drift_matrix`, `evaluate_release_gate`)
- CI workflow default script entrypoints switched to Rust binaries
- CLI binary entrypoint (`harborbeacon-agent`)

## Closeout Switches

The Rust migration lane is now aligned to the HarborBeacon cutover package
instead of introducing new northbound contract work:

- inbound contract fields are fixed at the current v1.5 seam; no new
  northbound fields are part of this closeout
- `route_key`, `resume_token`, and `delivery.idempotency_key` remain opaque
  boundary fields, not new business semantics
- legacy recipient fallback is documented as removed from the HarborBeacon-side
  rollback shape
- direct platform delivery and raw credential ownership are decommissioning
  items, not active migration targets

## Module mapping

- `orchestrator/contracts.py` -> `src/orchestrator/contracts.rs`
- `orchestrator/router.py` -> `src/orchestrator/router.rs`
- `orchestrator/policy.py` -> `src/orchestrator/policy.rs`
- `orchestrator/audit.py` -> `src/orchestrator/audit.rs`
- `orchestrator/runtime.py` -> `src/orchestrator/runtime.rs`
- `orchestrator/executors/harbor_ops.py` -> `src/orchestrator/executors/harbor_ops.rs`
- `skills/manifest.py` + `skills/registry.py` -> `src/skills/manifest.rs` + `src/skills/registry.rs`

## Next slices

1. Add HarborBeacon adapters in Rust or isolate Python HarborBeacon behind stable IPC.
2. Add deeper parity tests for live middleware/midcli paths and mutation rollback.
3. Add release-gate parity tests for end-to-end live integration environments.

## Build and run

- `cargo build --release`
- `./target/release/harborbeacon-agent --plan examples/plan_service_status.json`
