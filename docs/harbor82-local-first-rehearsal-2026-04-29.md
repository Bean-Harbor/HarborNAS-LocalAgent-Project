# Harbor 82 Local-First Rehearsal - 2026-04-29

## Summary

Tomorrow's live path should present HarborBeacon as `local first`, with cloud
models acting as a controlled fallback layer when local capacity is not yet
sufficient. The current HarborOS host `192.168.3.82` is therefore a strategy
rehearsal, not a claim that the final architecture is cloud-first.

2026-05-01 update: the current model architecture decision supersedes any
broader fallback phrasing in this rehearsal. SiliconFlow remains an
OpenAI-compatible cloud fallback preset for `semantic.router` and
`retrieval.answer` only. HarborOS commands, AIoT control, OCR, VLM, and
embedding defaults stay local/sidecar unless a later architecture decision
expands the policy.

Current verified baseline on `2026-04-30`:

- target host: `192.168.3.82`
- current backend release:
  `20260430-rc2-beacona5f6da0-gate57ff759`
- current WebUI release source: HarborNAS WebUI `develop` at `8e3f04d`
- HarborDesk page entry: `http://192.168.3.82/ui/harbordesk`
- HarborBot user retrieval entry: `http://192.168.3.82/ui/harborbot`
- HarborDesk public admin API proxy: `http://192.168.3.82/api/harbordesk/*`
- local admin API still runs on host-local `http://127.0.0.1:4174/api/*`
- task runtime still runs on host-local `http://127.0.0.1:4175/api/turns`
- GPU detected: `NVIDIA GeForce RTX 3070`, `8192 MiB`
- cloud fallback is connected through `https://api.siliconflow.cn/v1`
- RAG smoke source root is configured under
  `/var/lib/harborbeacon-agent-ci/writable/knowledge-mmrag-smoke`

Important current truth:

- HarborDesk does not currently expose a page-level `/api/turns` rehearsal
  surface.
- HarborBot is the recovered northbound user retrieval page from the
  `HarborNAS-webui-182-baseline` VM line. It is now merged into the current
  `HarborNAS-webui` checkout as a native WebUI page, but it still depends on the
  same real `/api/harbordesk/knowledge/search` and
  `/api/harbordesk/knowledge/preview` routes.
- Runtime `POST /api/turns` remains a protected task API and is not the same as
  the HarborDesk admin proxy.
- HarborDesk page rehearsal must therefore use the real page plus the real
  admin proxy, while runtime turn rehearsal must use the protected runtime
  entry directly.
- External `.82` reads confirmed on `2026-04-30`:
  - `GET /api/harbordesk/state` works
  - `GET /api/harbordesk/models/policies` works
  - `GET /api/harbordesk/models/endpoints` works
  - `GET /api/harbordesk/rag/readiness` works
  - `GET /api/harbordesk/knowledge/settings` works
- External `.82` HarborBot recovery deploy on `2026-04-30`:
  - superseded by backend release
    `20260430-rc2-beacona5f6da0-gate57ff759`
  - deployed native WebUI release
    based on HarborNAS WebUI `8e3f04d`
  - `POST /api/harbordesk/knowledge/search` returns `status=completed`
  - `GET /api/harbordesk/knowledge/preview` returns text and image previews
  - `春天的照片` returns one real VLM content-indexed image with
    `content_match_used=true` and `filename_match_used=false`

## What To Say In The Demo

Use this exact framing:

1. HarborBeacon defaults to local-first routing.
2. Workspace privacy and resource profiles decide whether cloud execution is
   allowed at all.
3. Route policy still carries `local_preferred` and `fallback_order`, so the
   system can move back toward local execution without changing the product
   contract.
4. On `.82`, local GPU capacity is present, but a production local model stack
   is not yet promoted, so SiliconFlow is used as the fallback completion
   layer to keep the full product loop available.

Avoid saying:

- "the system is cloud-based"
- "SiliconFlow is the default architecture"
- "the 3070 is already the active main inference backend"

## Current Rehearsal Surfaces

HarborDesk page rehearsal uses:

- `GET /api/harbordesk/state`
- `GET /api/harbordesk/models/endpoints`
- `GET /api/harbordesk/models/policies`
- `GET /api/harbordesk/rag/readiness`
- `GET /api/harbordesk/knowledge/settings`
- `POST /api/harbordesk/knowledge/search`
- `GET /api/harbordesk/knowledge/preview`

HarborBot page rehearsal uses:

- `GET /ui/harborbot`
- `POST /api/harbordesk/knowledge/search`
- `GET /api/harbordesk/knowledge/preview`
- waterfall evidence fields: `content_source_kinds`, `content_indexed`,
  `content_match_used`, and `filename_match_used`

Runtime turn rehearsal uses:

- host-local `POST http://127.0.0.1:4175/api/turns`
- `Authorization: Bearer <task-api token>`
- `X-Contract-Version: 2.0`

This split is intentional for now. Do not claim that HarborDesk already carries
the runtime turn surface.

The HarborDesk page should be treated as:

- ready for `readiness / policy / endpoint / settings` explanation
- ready for public `knowledge/search` / `knowledge/preview` rehearsal
- not a replacement for protected runtime `POST /api/turns`

HarborBot is now deployed on `.82` and should be treated as the live user-facing
multimodal retrieval page for waterfall rehearsal.

RC2 runtime turn proof:

- `帮我找春天的照片` returns `turn.status=completed`, one image artifact, and
  one delivery hint.
- `解释一下 HarborBeacon 和 HarborGate 现在的 local-first 架构，以及云端 fallback 是怎么受控的`
  returns `turn.status=completed` and the local-first / policy-controlled
  fallback explanation.

## Current Fallback Baseline

The currently verified fallback-capable models are:

- `LLM`: `deepseek-ai/DeepSeek-V4-Flash`
- `Embedding`: `Qwen/Qwen3-Embedding-0.6B`
- `VLM`: `Qwen/Qwen3-VL-8B-Instruct`

These are fallback-capable proofs, not the long-term default runtime claim.

## Rehearsal Order

Run the demo in this order:

1. Open `http://192.168.3.82/ui/harbordesk`
2. Show `RAG readiness`
3. Show `privacy_level` and `default_resource_profile`
4. Show model endpoints, route policy, `local_preferred`, and
   `fallback_order`
5. Show `knowledge/search` and `knowledge/preview` from the HarborDesk page
6. Open `http://192.168.3.82/ui/harborbot` for the user-facing multimodal
   waterfall retrieval flow
7. If search returns no results, use the empty state as the true index signal;
   do not add a shortcut or bypass the real API
8. Run protected runtime `POST /api/turns` knowledge answer using the real task
   API

## Acceptance Signals

The rehearsal is good enough for tomorrow if:

- HarborDesk page loads from `http://192.168.3.82/ui/harbordesk`
- HarborBot page loads from `http://192.168.3.82/ui/harborbot`
- HarborDesk admin reads succeed through `/api/harbordesk/*`
- `privacy_policy` is readable and explains cloud allowance clearly
- `resource_profiles` show why `cloud_allowed` is gated by privacy
- model routing shows `local_preferred` plus `fallback_order`
- HarborDesk `knowledge/search` returns usable evidence
- HarborDesk `knowledge/preview` can open at least one text asset and one image
  asset
- HarborBot shows the same real search results in its waterfall, including
  image inline preview and evidence proving content-indexed retrieval
- protected runtime `POST /api/turns` returns `turn.status=completed`
- one content question and one architecture question both answer cleanly

If a live search returns empty or slow results, the correct callout is that the
page is using the real indexed knowledge API and should be debugged through the
source root / index / modality readiness path, not through a demo-only page
fallback.

## Known Non-Goals

- Weixin is not part of the main rehearsal path.
- OCR and ASR do not need to be made green tonight.
- This host does not need to prove fully local LLM execution tomorrow.
- Do not add a HarborDesk shortcut, debug card, or demo-only page for
  `/api/turns`.
- Do not claim HarborDesk already owns runtime turn interaction when the current
  page only owns admin-plane search and preview.

## Recommended Runtime Questions

Use at least one from each bucket.

Content retrieval:

- "帮我找春天的照片"
- "搜索已有内容：根据当前知识库，总结 Harbor 82 的演示环境。"

Architecture explanation:

- "解释一下 HarborBeacon 和 HarborGate 现在的 local-first 架构，以及云端 fallback 是怎么受控的"
- "当前 82 的本地能力和云端 fallback 分别是什么？"

The `搜索已有内容：` prefix is still useful when the intent must force RAG over
general conversation, but RC2 no longer requires a prefix for the local-first
architecture explanation case.

## Follow-Up After The Rehearsal

After tomorrow's live session, the next engineering step should be to promote a
true local execution path for at least one small model class so that `local
first` is backed by both policy and active runtime, not only by fallback-ready
control-plane semantics.
