# AIoT Image Retrieval Canary Evidence

## What AIoT Produces

- a stable snapshot image path under `.harborbeacon/vision/snapshots/<device>-<timestamp>.jpg`
- a stable JSON sidecar next to that image file
- matching annotated image output under `.harborbeacon/vision/annotated/` when analysis produces one
- deterministic source linkage and capture context in the sidecar
- simple `tags` and `labels` when they are already known

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
- AIoT does not own the framework retrieval index.
- AIoT does not decide which citation wins; it only emits stable candidates.
- AIoT does not turn the caption or derived text into ranking logic.
- If a sidecar is missing, retrieval should fall back to the image path and
  available capture metadata only.
