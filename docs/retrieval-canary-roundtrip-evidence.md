# Retrieval Canary + Round-Trip Evidence

This note covers the HarborBeacon retrieval path for documents and images.

## What Is Covered

- Explicit `knowledge.search` retrieval for indexed documents and images.
- `general.message` stays outside retrieval and does not opportunistically
  route into `knowledge.search`.
- HarborBeacon-owned `reply_pack` content with summary, citations, and previews.
- Artifact packaging that mirrors the visible citations.

## Current Routing Rule

- Opportunistic natural-language retrieval fallback has been removed.
- Retrieval stays available through explicit `knowledge.search`.

## Out Of Scope

- OCR.
- Vector search.
- Video semantics.
- Audio semantics.
- Any retrieval meaning owned by HarborGate or HarborOS.

## Rollback

- Explicit `knowledge.search` remains available.
- No IM contract or route-key behavior changes are required for rollback.
