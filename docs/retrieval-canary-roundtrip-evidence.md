# Retrieval Canary + Round-Trip Evidence

This note covers the HarborBeacon retrieval path for documents and images.

## What Is Covered

- Explicit `knowledge.search` retrieval for indexed documents and images.
- Natural-language fallback routing from `general.message` when the canary
  gate is enabled.
- HarborBeacon-owned `reply_pack` content with summary, citations, and previews.
- Artifact packaging that mirrors the visible citations.

## Canary Gate

- The opportunistic natural-language fallback is disabled by default.
- Enable it with `HARBORBEACON_ENABLE_LEGACY_KNOWLEDGE_NL_FALLBACK=1`.
- Disabling that flag rolls the system back to explicit `knowledge.search`
  only, without removing the retrieval capability itself.

## Out Of Scope

- OCR.
- Vector search.
- Video semantics.
- Audio semantics.
- Any retrieval meaning owned by HarborGate or HarborOS.

## Rollback

- Turn off `HARBORBEACON_ENABLE_LEGACY_KNOWLEDGE_NL_FALLBACK`.
- Explicit `knowledge.search` remains available.
- No IM contract or route-key behavior changes are required for rollback.
