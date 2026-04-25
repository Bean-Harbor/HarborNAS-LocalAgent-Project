# HarborDesk Runtime Truth Closeout - 2026-04-25

## Scope

- Owner lane: `harbor-framework`
- Collaborating lanes:
  - `harbor-im-gateway` for gateway-status blocker semantics only
  - `harbor-architect` for release-gate and ownership closeout
- Frozen seams that did **not** change:
  - `POST /api/tasks`
  - `POST /api/notifications/deliveries`
  - `GET /api/gateway/status`

## What Closed In HarborBeacon

- `GET /api/feature-availability` is now the HarborBeacon-owned grouped read-model for:
  - runtime truth from local model `/healthz`
  - route policy status and fallback order
  - account-management delivery/binding projection
  - HarborGate status blockers
- Runtime truth now has higher precedence than stale stored admin projection for built-in local LLM/embedder rows.
- The admin plane keeps `projection_mismatch` explicit instead of silently rewriting stored endpoint state.
- HarborDesk Angular and `docs/webui` both surface:
  - `Runtime alignment`
  - `Feature availability`
  - grouped blocker/source-of-truth rows
- `4176=candle` remains the accepted local-model default, and the read-model reports that runtime truth instead of the old stored placeholder state.

## Blocker Ownership

- The remaining live blocker is still `weixin_dns_resolution`.
- This blocker stays in the `harbor-im-gateway` lane plus environment/network ownership.
- HarborGate now exports two layers of Weixin blocker state:
  - `weixin.blocker_category` for the specific redacted transport blocker
  - `weixin.ingress_blocker_category` and `release_v1.weixin_blocker_category` for the coarse release-v1 parity bucket
- HarborBeacon must only project the blocker as an IM delivery/binding issue.
- HarborBeacon must not reinterpret `weixin_dns_resolution` as:
  - a task/business-core failure
  - a model-runtime failure
  - a HarborOS system-domain failure
- HarborBeacon reads the real `GET /api/gateway/status` payload in this order:
  - `weixin.blocker_category`
  - `release_v1.weixin_blocker_category`
  - legacy top-level `weixin_blocker_category` if present

## Verification Matrix

Run exactly these commands for this closeout:

```powershell
cargo test --bin agent-hub-admin-api --quiet
cargo test --bin harbor-model-api --quiet
python -m pytest tests/contracts/test_contract_docs.py tests/contracts/test_release_packaging_install_lane.py -q
npm run build
```

The expected acceptance signal is:

- model runtime rows can show `projection_mismatch` while remaining readable and reviewable
- `retrieval.embed` / `retrieval.answer` stay green when `4176 /healthz` is healthy
- `weixin_dns_resolution` stays isolated as an IM blocker in feature availability and system settings
- `weixin.blocker_category` and `release_v1.weixin_blocker_category` stay semantically distinct in docs and UI copy
- no frozen northbound or cross-repo contract semantics are widened

## Explicit Non-Scope For This Code Closeout

- `docs/harborgate-to-harborbeacon-walkthrough.md`
- `docs/HarborGate-to-HarborBeacon-overview.pptx`
- `tools/generate_harborgate_overview_ppt.py`
- `tools/sync_build_host.ps1`

These artifacts can ship later as docs/tooling follow-up, but they are not part of the HarborBeacon code closeout for runtime truth and feature availability.

## Track Split

- Thread A: HarborBeacon runtime-truth code closeout
  - owner: `harbor-framework`
  - goal: land the current runtime overlay, feature-availability, HarborDesk, docs/webui, and release-portability diff as one reviewable HarborBeacon changeset
- Thread B: docs/tooling walkthrough follow-up
  - owner: docs/tooling follow-up
  - goal: carry the walkthrough, PPT, and helper scripts without reopening HarborBeacon runtime-truth scope
- Thread C: live `weixin_dns_resolution` investigation
  - owner: `harbor-im-gateway` plus environment/network
  - goal: restore live IM reachability and run real gateway-side verification after DNS/platform recovery

See `docs/harbordesk-runtime-truth-handoff-2026-04-25.md` for the exact changeset boundary and handoff conditions.
