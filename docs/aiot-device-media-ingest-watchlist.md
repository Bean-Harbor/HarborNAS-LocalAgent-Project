# AIoT Device Media Ingest Watchlist

## Purpose

This watchlist defines the device-domain metadata contract for future
multimodal ingest. It keeps AIoT focused on producing index-friendly media
artifacts, not search or ranking logic.

## Image First

This round is image-first. The device lane should make snapshot and
analyze-derived image artifacts easy to ingest later, while leaving video and
audio as follow-up work.

For the local retrieval demo, the framework should point at the persisted
snapshot image under `.harborbeacon/vision/snapshots/` plus its sibling
`analysis_snapshot` JSON sidecar. The annotated image under
`.harborbeacon/vision/annotated/` is a preview candidate only and should stay
linked back to the source snapshot.

Current image-side signals already available for framework ingestion:

- stable source/annotated linkage
- `tags` and `labels`
- `caption` from the analysis summary
- `derived_text` from summary, detection summary, and detection labels
- capture/device context in `ingest_metadata`

## Indexable Artifacts

- `snapshot`
  - keep: `captured_at_epoch_ms`, `device_id`, `device_name`, `room`, `vendor`,
    `model`, `discovery_source`
  - provenance: `media`
  - ingest disposition: `knowledge_index_candidate`
  - emit a stable `.json` sidecar next to the image file with source linkage
    and capture context
- `vision.analyze_camera` snapshot artifact
  - keep the same media metadata as the source snapshot
  - keep `source_storage`, `byte_size`, and the annotated image path when
    available
  - provenance: `media`
  - ingest disposition: `knowledge_index_candidate`
  - keep the annotated image sidecar aligned with the image file name and carry
    simple labels/tags when they are already available

## Control And Runtime Only

- `open_stream`
  - provenance: `control`
  - ingest disposition: `runtime_only`
  - keep for audit, do not treat as a knowledge-index entry
- `discover`, `connect`, `ptz`
  - these are control/runtime artifacts
  - they may carry device metadata for audit and routing, but they are not
    knowledge-index candidates

## Required Metadata

- `device_id`
- `captured_at_epoch_ms` or equivalent runtime timestamp
- `device_name`
- `room`
- `vendor`
- `model`
- `discovery_source`
- `provenance`
- `ingest_disposition`
- `stream_transport` and `source_requires_auth` when available

## Watchpoints

- keep media capture separate from control execution
- do not widen device-native code into query parsing or ranking
- do not route device control through HarborOS system control by default
- do not change IM seam semantics or `route_key` / `resume_token` behavior

## Suggested Checks

- snapshot result serializes `ingest_metadata`
- open stream result remains `control` / `runtime_only`
- analyze snapshot artifact preserves the source device metadata
- bridge smoke keeps `scan -> connect -> snapshot -> analyze` stable where the
  current codebase supports it

## Still Missing For Full Multimodal Search

- OCR extraction from image artifacts
- video clip ingestion and clip-sidecar generation
- audio transcript or speech segment extraction
- semantic vision summaries that are separate from the file sidecar
- query routing, ranking, and answer synthesis, which stay in the framework
- richer multimodal fusion over more than one image artifact at a time

## Rollback And Reality Limits

- if sidecar generation fails, keep the image artifact and capture metadata;
  do not block the device-control flow
- if annotated output is missing, keep the source snapshot sidecar stable and
  mark only the source image as the citation candidate
- do not use these sidecars to infer retrieval answers in AIoT
- keep control/runtime artifacts separate from citation candidates even when
  they share the same device
- if the framework retrieval path changes, AIoT should continue to emit the
  same stable file names and metadata without taking ownership of ranking or
  answer generation
- if the demo path moves, keep the sidecar shape and linkage stable first;
  the operator can update the pointer without changing AIoT semantics
- do not add OCR or semantic rewriting inside AIoT just to improve citations;
  those belong in the framework retrieval layer
