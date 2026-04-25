# HarborDesk Runtime Truth Handoff - 2026-04-25

## Thread A - HarborBeacon Runtime-Truth Code Closeout

- Owner lane: `harbor-framework`
- Collaborators:
  - `harbor-architect` for boundary and release-gate review
  - `harbor-im-gateway` for blocker ownership acknowledgment only
- Goal: ship the current HarborBeacon runtime-truth/read-model work as one reviewable code-closeout changeset.

### In-Scope Changeset

- `src/bin/agent_hub_admin_api.rs`
- `frontend/harbordesk/src/app/core/admin-api.service.ts`
- `frontend/harbordesk/src/app/core/admin-api.types.ts`
- `frontend/harbordesk/src/app/shared/page-state-panel.component.html`
- `frontend/harbordesk/src/app/shared/page-state-panel.component.ts`
- `frontend/harbordesk/src/app/shared/page-state-panel.component.css`
- `docs/webui/index.html`
- `docs/webui/app.js`
- `README.md`
- `tests/contracts/test_contract_docs.py`
- `docs/harbordesk-runtime-truth-closeout-2026-04-25.md`
- `Cargo.toml`
- `Cargo.lock`
- `tools/bootstrap_release_builder.sh`

### Acceptance Gate

- The verification matrix from `docs/harbordesk-runtime-truth-closeout-2026-04-25.md` stays green.
- `projection_mismatch` remains visible in backend and UI surfaces.
- `weixin_dns_resolution` remains isolated as an IM blocker.
- No frozen cross-repo seam is widened.

## Thread B - Docs/Tooling Walkthrough Follow-Up

- Owner: docs/tooling follow-up thread
- Goal: ship presentation and helper materials without changing the HarborBeacon runtime-truth code-closeout scope.

### Out-Of-Scope For Thread A

- `docs/harborgate-to-harborbeacon-walkthrough.md`
- `docs/HarborGate-to-HarborBeacon-overview.pptx`
- `tools/generate_harborgate_overview_ppt.py`
- `tools/sync_build_host.ps1`

### Rule

- These files may move forward in a separate PR or handoff package.
- They must not be used to justify reopening `GET /api/feature-availability`, runtime overlay logic, or release-portability code in HarborBeacon.

## Thread C - Live `weixin_dns_resolution` Investigation

- Owner lane: `harbor-im-gateway`
- Collaborators: environment/network owners
- HarborBeacon role: read-only consumer of blocker projection

### Handoff Condition

- HarborBeacon already projects `weixin_dns_resolution` as an IM blocker in:
  - `GET /api/feature-availability`
  - HarborDesk System Settings / Feature availability
  - the runtime-truth closeout notes
- No additional HarborBeacon business-core changes are required before DNS/platform recovery.

### Expected Next Action After Recovery

- Run one real gateway-side IM round-trip that proves:
  - inbound task handoff still works
  - proactive delivery is no longer blocked by `weixin_dns_resolution`
  - HarborBeacon does not regain IM credential or delivery ownership
