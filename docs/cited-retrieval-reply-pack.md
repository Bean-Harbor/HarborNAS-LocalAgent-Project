# Cited Retrieval Reply Pack

This note describes the HarborBeacon-owned reply packaging used by
`knowledge.search` and the natural-language retrieval fallback.

## Boundary

- HarborBeacon owns retrieval semantics, ranking, citation packaging, and reply meaning.
- HarborGate only renders the returned task payload and artifacts.
- HarborOS remains a read-only file substrate.
- AIoT remains the producer of image metadata and sidecar content.

## Reply Pack Shape

- `result.message` is a HarborBeacon-authored summary string.
- `result.data.reply_pack.summary` mirrors the same summary for replay and audit.
- `result.data.reply_pack.citations[]` carries citation-ready fields:
  - `title`
  - `path`
  - `modality`
  - `matched_terms`
  - `preview`
  - `score`
- `result.artifacts[]` mirrors the visible citations for downstream rendering.

## Limits

- No OCR.
- No vector search.
- No video semantics.
- No audio semantics.
- Previews are text-only and come from indexed file text or sidecar metadata.
