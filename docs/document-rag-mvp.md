# Document RAG MVP

This note describes the HarborBeacon-owned document retrieval path after chunking
was added to the local knowledge index.

## What Is Supported

- Text documents are chunked during indexing and retrieved at chunk/snippet level.
- Image retrieval still uses the existing sidecar/index path, but citation fields
  stay consistent with document retrieval.
- `knowledge.search` returns HarborBeacon-owned `reply_pack`, `documents`,
  `images`, and artifacts that carry citation-ready metadata.
- Citations include source-grounded fields such as `title`, `path`, `modality`,
  `chunk_id`, `line_start`, `line_end`, `matched_terms`, and `preview`.

## What The MVP Does Not Do

- No OCR.
- No vector search.
- No audio or video semantics.
- No IM-side retrieval meaning.
- No HarborOS ownership of business retrieval.

## Indexing Model

- Documents are indexed into a local manifest under the HarborBeacon knowledge
  index root.
- Chunks are line-bounded and stable across refreshes when the source text stays
  unchanged.
- Incremental refresh reuses unchanged files and only rebuilds changed sources.

## Operator Meaning

- Explicit `knowledge.search` remains the supported, direct retrieval entry.
- The gated natural-language fallback can still be disabled independently of
  explicit retrieval.
- `result.message`, `result.data`, `result.artifacts`, and `reply_pack` should
  tell the same retrieval story.

