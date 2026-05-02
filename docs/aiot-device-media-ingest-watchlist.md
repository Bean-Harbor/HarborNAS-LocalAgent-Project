# AIoT Device Media Ingest Watchlist

## Purpose

This watchlist defines the device-domain metadata contract for multimodal
ingest. It is VLM-first: the first-version input surface is image, snapshot,
still-frame, and local DVR segment content for the HarborDesk device page.
Continuous video is stored as rolling media segments and summarized through
keyframe sidecars, while audio transcript extraction stays follow-up work. It
keeps AIoT focused on producing index-friendly media artifacts, not search or
ranking logic.

Release-v1 reuses existing camera profile, recording policy, admin defaults,
and model policy records. The device lane should not invent a new
use-case-specific profile object just to carry capture folder, clip length, or
keyframe hints.

## Devices & AIoT Admin Summary

- Ownership: the device lane owns snapshot and analyze-derived image evidence,
  while `discover`, `connect`, `inspect`, `control`, `ptz`, and `open_stream`
  stay runtime/control only.
- Watchlist: keep stable source/annotated linkage, preserve `caption`,
  `derived_text`, `tags`, `labels`, and `ingest_metadata`, and keep the sidecar
  shape deterministic for first-class admin page consumption.
- Non-regression: do not widen AIoT into query parsing, ranking, or answer
  synthesis, and do not reinterpret `route_key` or `resume_token` as media
  semantics. HarborOS does not own device control, and retrieval/control
  separation stays explicit.

## VLM First

This round is VLM-first. The device lane should make image, snapshot,
still-frame, and DVR segment artifacts easy to ingest through the existing
knowledge pipeline, while leaving audio transcript extraction as follow-up work.

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
- `clip`
  - keep: `captured_at_epoch_ms`, `clip_length_seconds`, `device_id`,
    `device_name`, `room`, `vendor`, `model`, `discovery_source`
  - provenance: `media`
  - ingest disposition: `knowledge_index_candidate`
  - emit a stable `.json` sidecar next to the clip file with clip timing and
    keyframe hints
- `dvr_segment`
  - keep: `started_at`, `ended_at`, `duration_seconds`, `retention_expires_at`,
    `stream_kind`, `device_id`, `device_name`, `room`, `vendor`, `model`,
    `discovery_source`
  - provenance: `media`
  - ingest disposition: `knowledge_index_candidate`
  - emit a stable `.json` sidecar next to the segment file with source video,
    timing, stream kind, retention, and keyframe hints
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
- `discover`, `connect`, `inspect`, `control`, `ptz`
  - these are control/runtime artifacts
  - they may carry device metadata for audit and routing, but they are not
    knowledge-index candidates

## Clip Hints

- `clip_length_seconds`
- `capture_subdirectory` or `capture_folder_path`
- `keyframe_count`
- `keyframe_interval_seconds`
- `storage_target`
- `reply_policy_id` when the operator wants the post-capture reply path to
  reference a specific summary policy

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
- keep inspection separate from media citation candidates
- keep retrieval evidence separate from runtime control
- do not widen device-native code into query parsing or ranking
- do not route device control through HarborOS system control by default
- do not change IM seam semantics or `route_key` / `resume_token` behavior

## Suggested Checks

- snapshot result serializes `ingest_metadata`
- open stream result remains `control` / `runtime_only`
- inspect and control remain runtime-only control artifacts
- analyze snapshot artifact preserves the source device metadata
- bridge smoke keeps `scan -> connect -> snapshot -> analyze` stable where the
  current codebase supports it

## Still Missing For Full Multimodal Search

- OCR extraction from image artifacts
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

## Closeout Proof Pack

Date: 2026-04-19

Boundary proof:

- VLM-first boundary proof: image, snapshot, still-frame, clip, and DVR segment artifacts remain device-generated inputs for framework ingestion.
- continuous video is first represented as rolling media segments with keyframe sidecars; it does not create a DVR-specific VLM, embedding, reranker, or answer chain.
- `discover`, `connect`, `inspect`, `control`, and `ptz` remain control/runtime artifacts, not citation candidates.
- `inspect` and `control` stay runtime-only and are not promoted into retrieval ownership.
- media capture remains separate from control execution and HarborOS system control.
- HarborOS does not own device control.

Current risk signals:

- the source/annotated linkage must stay stable even if annotated output is missing.
- sidecar generation can fail independently, so the source snapshot path must remain usable.
- the framework demo pointer may move, but AIoT semantics must not change with it.

Non-regression conclusion:

- This closeout confirms the ingest lane remains image-first and does not absorb query, ranking, or answer-generation responsibilities.
