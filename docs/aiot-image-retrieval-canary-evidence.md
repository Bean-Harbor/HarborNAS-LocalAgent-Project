# AIoT Image Retrieval Canary Evidence

## What AIoT Produces

- This evidence pack is VLM-first: it treats image, snapshot, still-frame, clip, and local DVR segment artifacts as the first-version input surface for HarborDesk retrieval.
- release-v1 extends that same file-oriented evidence model to rolling video segments, but retrieval still uses keyframes and sidecars rather than a DVR-specific model chain.
- a stable snapshot image path under `.harborbeacon/vision/snapshots/<device>-<timestamp>.jpg`
- a stable JSON sidecar next to that image file
- matching annotated image output under `.harborbeacon/vision/annotated/` when analysis produces one
- deterministic source linkage and capture context in the sidecar
- simple `tags` and `labels` when they are already known

## Devices & AIoT Admin Summary

- Ownership: the device lane emits the persisted snapshot image plus sidecar as
  the primary evidence candidate, with an annotated image as an optional
  preview candidate when present.
- Watchlist: keep file-oriented retrieval fields stable, preserve source and
  annotated linkage, and keep the device context attached to the evidence.
- Non-regression: `inspect` and `control` remain runtime-only, and AIoT does
  not claim ranking, citation choice, or answer synthesis. HarborOS does not
  own device control, and retrieval/control separation stays explicit.

## Retrieval-Friendly Fields

- `image_path`
- `source_image_path`
- `annotated_image_path`
- `caption`
- `derived_text`
- `captured_at_epoch_ms`
- `source_storage`
- `device_id`
- `device_name`
- `room`
- `vendor`
- `model`
- `discovery_source`
- `tags`
- `labels`
- `ingest_metadata.provenance`
- `ingest_metadata.ingest_disposition`
- `clip_length_seconds`
- `keyframe_count`
- `keyframe_interval_seconds`

## Current Image-Side Signals

- `caption` is the human-facing scene summary already produced by the vision
  lane
- `derived_text` is the deterministic retrieval hint assembled from summary,
  detection summary, labels, and detections
- `tags` and `labels` stay file-oriented and stable so the framework can cite
  them directly
- source and annotated linkage stay explicit so the framework can build a
  primary citation plus a preview candidate

## Canary Expectation

Framework retrieval should treat the snapshot image and its sidecar as the
primary citation candidate, and the annotated image as a secondary preview or
derived citation candidate when present.

For HarborDesk, this stays the first-version VLM input path: image, snapshot,
still-frame, clip, and DVR segment evidence are in scope. Rolling video is
represented as media artifacts with keyframe-derived retrieval evidence, not as
a separate continuous-video model stack.

## Local Demo Path

Use the persisted snapshot image at
`.harborbeacon/vision/snapshots/<device>-<timestamp>.jpg` plus the sibling JSON
sidecar written for the `analysis_snapshot` role as the canonical round-trip
demo input. The annotated image, when present, should remain a secondary
preview candidate linked from the same source snapshot.

The demo should rely on stable file-oriented fields only:

- source image path
- annotated image path when present
- captured timestamp
- device and room/vendor/model context
- simple tags and labels
- ingest provenance and disposition
- stable sidecar file name derived from the image path

If the annotated file is missing, the framework demo should still be able to
point at the source snapshot image and sidecar without changing retrieval
behavior.

## Reality Limits

- AIoT does not rank, score, or answer retrieval queries.
- AIoT does not perform OCR, semantic search, or multimodal fusion.
- AIoT owns DVR segment capture metadata, but does not own semantic video
  retrieval or answer generation.
- AIoT does not own the framework retrieval index.
- AIoT does not decide which citation wins; it only emits stable candidates.
- AIoT does not turn the caption or derived text into ranking logic.
- `inspect` and `control` remain runtime-only surfaces and are not retrieval
  candidates.
- `inspect` and `control` stay separate from retrieval evidence handling.
- If a sidecar is missing, retrieval should fall back to the image path and
  available capture metadata only.

## Closeout Proof Pack

Date: 2026-04-19

Boundary proof:

- the primary retrieval candidate is the persisted snapshot image plus its sidecar.
- the first-version VLM input surface is image, snapshot, still-frame, clip, and DVR segment content with keyframe sidecars.
- the annotated image stays a secondary preview or derived citation candidate.
- `inspect` and `control` remain runtime-only and are not claimed by HarborOS executors or retrieval logic.
- HarborOS does not own device control.
- the evidence pack stays media-first and does not own ranking or semantic fusion.

Current risk signals:

- source-image and sidecar linkage must stay deterministic across reruns.
- annotated output remains optional and should not gate retrieval continuity.
- the framework path may evolve, but the AIoT file contract must remain stable.

Non-regression conclusion:

- This closeout confirms the retrieval canary still emits device-native media evidence, not HarborOS-owned execution or ranking semantics.
