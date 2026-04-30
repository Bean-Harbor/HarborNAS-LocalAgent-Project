# Knowledge Indexing Pack

## Scope

This pack keeps `knowledge.search` inside the HarborBeacon framework lane.
It adds a local manifest-backed index for documents and images so repeated
searches do not reread the full tree on every request, and it keeps retrieval
local-first.

## Why This Belongs To Framework

- It feeds HarborBeacon task and business-state retrieval.
- It does not own IM transport, route keys, delivery semantics, or platform
  credentials.
- It does not move HarborOS system control into the knowledge search path.
- It stays inside the HarborBeacon repo and uses local persistence only.

## Index Convention

- Default index root: `.harborbeacon/knowledge-index`
- Optional override: `HARBOR_KNOWLEDGE_INDEX_ROOT`
- Per-knowledge-root manifest file: `<index_root>/<sha256(root-path)>.json`

## Refresh Rules

- Documents are indexed from file text or normalization output.
- Images are indexed from the image file plus OCR text and the first matching
  sidecar text file.
- Refresh is incremental:
  - unchanged files are reused from the manifest
  - changed files are re-read and rewritten into the manifest
  - deleted files are dropped on the next refresh

## Invalidation Rules

- Rebuild if the manifest is missing or schema version changes.
- The root directory signature is recorded for provenance, but refresh still
  walks the root and reuses unchanged subtrees so deep edits are not missed.
- Reuse cached entries when the directory signature is unchanged.
- Sidecar changes invalidate only the matching image entry.

## Residual Risk

- This is still a metadata walk, not a content-aware filesystem watcher.
- If a NAS does not update directory signatures promptly, a refresh may lag
  until the next visible directory change.
- Audio and video semantic layers are still out of scope for this pack.
- OCR and vector search are part of the indexed document/image loop in this
  round, but they remain local-only and file-backed.
