# Retrieval Round-Trip Launch Pack

This is the repeatable HarborBeacon-side demo note for document/image retrieval.

## Demo Corpus

- One document: `docs/sakura-notes.md`
- One image: `images/spring-garden.jpg`
- One image sidecar: `images/spring-garden.json`

## Demo Cases

### 1. Explicit `knowledge.search`

- Operator note:
  - explicit `knowledge.search` remains the direct retrieval path even when
    natural-language routing is enabled.

- Input:
  - domain: `knowledge`
  - action: `search`
  - query: `樱花`
- Canary flag state:
  - `HARBORBEACON_ENABLE_LEGACY_KNOWLEDGE_NL_FALLBACK` unset
- Expected HarborBeacon reply:
  - `result.status = completed`
  - `result.message` matches `reply_pack.summary`
  - `reply_pack.citations` includes the document and image
  - `artifacts` mirrors the same citation set

### 2. General Message Can Route To Retrieval

- Input:
  - domain: `general`
  - action: `message`
  - raw text: `帮我找到和樱花有关的文件`
- Expected HarborBeacon reply:
  - `result.status = completed`
  - `result.message` matches `reply_pack.summary`
  - `reply_pack.citations` includes the document and image
  - `artifacts` mirrors the same citation set
  - natural-language retrieval routing is allowed when the planner recognizes
    retrieval intent

## Rollback

- Natural-language retrieval routing can be disabled without removing explicit
  `knowledge.search`.
- If a launch gate fails, HarborBeacon returns `failed` from `task_api` rather
  than silently falling back to another retrieval seam.
- No IM contract changes are required.
- No retrieval semantics move to HarborGate or HarborOS.
- No legacy retrieval fallback exists to toggle during rollback.
