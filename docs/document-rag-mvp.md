# Document RAG MVP

This note describes the HarborBeacon-owned document retrieval path after chunking
was added to the local knowledge index.

## What Is Supported

- Text documents are chunked during indexing and retrieved at chunk/snippet level.
- Image retrieval uses OCR-derived text plus sidecar/index metadata, and citation
  fields stay consistent with document retrieval.
- `knowledge.search` returns HarborBeacon-owned `reply_pack`, `documents`,
  `images`, and artifacts that carry citation-ready metadata.
- Natural-language `general.message` requests may also route into retrieval when
  the planner classifies them as retrieval-intent; explicit `knowledge.search`
  remains the direct entry.
- Citations include source-grounded fields such as `title`, `path`, `modality`,
  `chunk_id`, `line_start`, `line_end`, `matched_terms`, and `preview`.

## What The MVP Does Not Do

- No audio or video semantics.
- No IM-side retrieval meaning.
- No HarborOS ownership of business retrieval.

## Indexing Model

- Documents are indexed into a local manifest under the HarborBeacon knowledge
  index root.
- Images are normalized through OCR text and sidecar metadata before indexing.
- Chunks are line-bounded and stable across refreshes when the source text stays
  unchanged.
- Retrieval combines lexical and vector signals inside the local index so repeat
  lookups do not reread the full tree on every request.
- Incremental refresh reuses unchanged files and only rebuilds changed sources.

## Operator Meaning

- Explicit `knowledge.search` remains the supported, direct retrieval entry.
- Natural-language `general.message` can also enter retrieval when the planner
  decides the request is retrieval-related.
- `result.message`, `result.data`, `result.artifacts`, and `reply_pack` should
  tell the same retrieval story.
