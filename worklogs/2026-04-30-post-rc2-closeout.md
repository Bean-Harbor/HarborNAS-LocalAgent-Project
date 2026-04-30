# 2026-04-30 Post-RC2 Closeout

## Completed

- Merged the HarborBeacon, HarborGate, and HarborNAS WebUI release trains into
  their main integration branches.
- HarborBeacon is now on `master` at `a5f6da0`, HarborGate is on `main` at
  `57ff759`, and HarborNAS WebUI is on `develop` at `8e3f04d`.
- Built the post-merge RC2 release bundle on the `.197` builder:
  `harbor-release-20260430-rc2-beacona5f6da0-gate57ff759.tar.gz`.
- Installed RC2 on HarborOS `.82` with
  `HARBOR_RELEASE_VERSION=20260430-rc2-beacona5f6da0-gate57ff759`.
- Kept HarborDesk and HarborBot as native HarborNAS WebUI pages:
  `/ui/harbordesk` is the admin/configuration surface, and `/ui/harborbot` is
  the user-facing multimodal retrieval surface.
- Preserved the frozen v2.0 HarborBeacon and HarborGate boundary. No v1.5
  runtime dual stack, no demo-only API, and no `/api/turns` shortcut page were
  added.
- Verified that local-first is now represented in both the page rehearsal and
  the protected runtime turn path. Cloud remains a policy-controlled fallback;
  SiliconFlow is the current `.82` fallback proof, not the default architecture.

## RC2 Artifact

- Builder: `192.168.1.197`
- Target: `192.168.3.82`
- Bundle:
  `/home/harbor-innovations/artifacts/harborbeacon-release-bundles/harbor-release-20260430-rc2-beacona5f6da0-gate57ff759.tar.gz`
- Target copy:
  `/var/tmp/harbor-release-20260430-rc2-beacona5f6da0-gate57ff759.tar.gz`
- SHA256:
  `7119842506d38aac82c7e236b7f96a054244bb50be07c5e6b001ac7b0683484c`

## Validation

- HarborBeacon local gate:
  - `cargo test --lib general_message`
  - `cargo test --lib handle_rag_answer`
  - `cargo test --lib handle_knowledge_search`
  - `cargo test --bin agent-hub-admin-api knowledge`
  - `pytest tests/contracts/test_rag_harbordesk_admin_contract.py -q`
  - `git diff --check`
- GitHub checks for HarborBeacon PR #11:
  - `schema_check`
  - `contract_unit_tests`
  - `fallback_chain_tests`
  - `policy_risk_gates`
- HarborGate gate:
  - `python -m pytest tests/test_gateway.py -q`
- HarborNAS WebUI gate:
  - HarborDesk and HarborBot targeted Jest specs
  - `corepack yarn tn-icons`
  - `corepack yarn build:prod`
- `.82` smoke:
  - `GET /ui/harbordesk` returned `200`
  - `GET /ui/harborbot` returned `200`
  - sprite config contains HarborDesk, HarborBot, active variants, and
    `mdi-account`
  - RAG readiness, knowledge index status, and model endpoints returned `200`
  - `POST /api/harbordesk/knowledge/search` for `春天的照片` returned one VLM
    content-indexed image with `filename_match_used=false`
  - `GET /api/harbordesk/knowledge/preview` returned `image/jpeg`

## Runtime Turn Evidence

- Protected runtime endpoint:
  `POST http://127.0.0.1:4175/api/turns`
- Required headers:
  - `Authorization: Bearer <task-api token>`
  - `X-Contract-Version: 2.0`
- Content retrieval turn:
  - input: `帮我找春天的照片`
  - result: `turn.status=completed`
  - reply: `已找到与“春天”相关的 1 张图片。`
  - artifacts: `1`
  - delivery hints: `1`
- Architecture explanation turn:
  - input:
    `解释一下 HarborBeacon 和 HarborGate 现在的 local-first 架构，以及云端 fallback 是怎么受控的`
  - result: `turn.status=completed`
  - reply includes `local-first`, `HarborBeacon`, `HarborGate`,
    `privacy/resource policy`, `受控 fallback`, and `SiliconFlow`

## Boundary Check

- Frozen contract changed: no
- `POST /api/turns` wire shape changed: no
- v1.5/v2.0 runtime dual stack introduced: no
- HarborDesk or HarborBot demo-only turn surface introduced: no
- HarborBeacon direct IM platform delivery reintroduced: no
- HarborBeacon IM raw credential ownership reintroduced: no
- HarborOS System Domain and Home Device Domain collapsed: no

## Current Dirtiness

- HarborGate and HarborNAS WebUI are clean.
- HarborBeacon has docs-only untracked assets:
  - `docs/harbornas-iso-packaging-dependencies.md`, now intended for a docs PR.
  - `docs/design/`, intentionally parked and excluded from the RC2 release train.

## Next Exact Order

1. Land a docs-only post-RC2 closeout PR.
2. Promote RC2 toward a GA candidate with release evidence and rollback notes
   only; do not change product behavior.
3. Run the `.82` local model promotion gate on the existing Candle side-lane.
   Keep the default backend unchanged unless the benchmark report says
   `gate.promotable=true`.
4. After GA and local model gate decisions, resume Home Agent Hub / AIoT MVP
   planning around discovery, snapshot, AI detection, and IM delivery.
