# Retrieval Round-Trip Launch Pack

This is the repeatable HarborBeacon-side demo note for document/image retrieval.

## Demo Corpus

- One document: `docs/sakura-notes.md`
- One image: `images/spring-garden.jpg`
- One image sidecar: `images/spring-garden.json`

## Demo Cases

### 1. Explicit `knowledge.search`

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

### 2. General Message Stays Outside Retrieval

- Input:
  - domain: `general`
  - action: `message`
  - raw text: `帮我找到和樱花有关的文件`
- Expected HarborBeacon reply:
  - does not opportunistically route into knowledge retrieval
  - returns `failed` from `task_api`
  - explicit `knowledge.search` remains available for retrieval access

## Rollback

- No legacy retrieval fallback exists to toggle during rollback.
- No IM contract changes are required.
- No retrieval semantics move to HarborGate or HarborOS.
