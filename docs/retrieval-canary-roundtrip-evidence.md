# Retrieval Canary + Round-Trip Evidence

This note covers the HarborBeacon retrieval path for documents and images.

## What Is Covered

- Explicit `knowledge.search` retrieval for indexed documents and images.
- Planner-routed natural-language retrieval for retrieval-intent
  `general.message` requests.
- HarborBeacon-owned `reply_pack` content with summary, citations, and previews.
- Artifact packaging that mirrors the visible citations.

## Current Routing Rule

- Opportunistic natural-language retrieval is now a supported route when the
  planner classifies the message as retrieval-related.
- Explicit `knowledge.search` remains the direct retrieval path.

## Out Of Scope

- Video semantics.
- Audio semantics.
- Any retrieval meaning owned by HarborGate or HarborOS.

## Rollback

- Explicit `knowledge.search` remains available.
- Natural-language retrieval routing can be disabled independently if needed.
- No IM contract or route-key behavior changes are required for rollback.
