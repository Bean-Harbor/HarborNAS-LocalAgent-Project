# AIoT Camera Canary Watchlist

## Purpose

This watchlist is for the Home Device Domain cutover day. It keeps camera
journeys observable without widening into IM transport or HarborOS system
control.

This round is VLM-first for the HarborDesk device page: image, snapshot, and
still-frame inputs are first-version surfaces, while continuous video stays
follow-up work.

Release-v1 can add short-clip capture as a media artifact, but the device lane
must continue to express it through the existing recording policy and camera
profile records rather than a new use-case profile object.

## Devices & AIoT Admin Summary

- Ownership: the Home Device Domain owns `camera.scan`, `camera.connect`,
  `camera.snapshot`, `camera.share_link` (`camera.live_view` stays a
  compatibility alias), `device.inspect`, and `device.control`.
- Watchlist: discovery stays normalized, connect continues through
  `needs_input` and `resume_token`, snapshot stays media-only, share output
  stays a signed link artifact, inspect stays read-only, and control stays
  device-native.
- Clip watch: if release-v1 enables short clips, the device lane only needs to
  surface clip length and keyframe hints; HarborDesk should continue to use the
  existing recording policy and camera profile records.
- Non-regression: `route_key` stays opaque routing metadata, `resume_token`
  stays business-flow continuation, camera control never becomes HarborOS
  system control, and retrieval/control separation stays explicit.

## In Scope

- device discovery and scan validation
- camera connect continuation through `needs_input` and `resume_token`
- explicit coverage for `discover`, `snapshot`, `share_link`, `inspect`, and
  `control` while staying inside the device domain
- snapshot, live view share (`camera.share_link`, with `camera.live_view` accepted as a compatibility alias), and analyze flows where the current codebase supports them
- image, snapshot, and still-frame inputs are the first-version VLM surface for HarborDesk; continuous video stays follow-up work
- media/control separation in the device runtime
- legacy fallback when `route_key` is absent

## Camera Journey Canary

1. `camera.scan`
   - expected signal: camera candidates are discovered and normalized
   - watch fields: `device_id`, `discovery_source`, `ip_address`, `rtsp_paths`
2. `camera.connect`
   - expected signal: a device can be added or continued after password prompt
   - watch fields: `requires_auth`, `pending_missing_fields`, `resume_token`
3. `camera.snapshot`
   - expected signal: snapshot capture returns a media artifact only
   - watch fields: `storage.target`, `mime_type`, `byte_size`, `relative_path`
4. `camera.share_link`
   - legacy alias: `camera.live_view`
   - expected signal: share output is a signed link artifact, not a raw device URL
   - watch fields: `device_id`, `expires_at`, `scope`, `token_hash`
5. `camera.analyze`
   - expected signal: analysis returns text plus artifact references
   - watch fields: `analysis.text`, `artifacts[]`, `source`
6. `camera.record_clip`
   - expected signal: clip capture returns a media artifact plus keyframe hints
   - watch fields: `clip_length_seconds`, `storage.target`, `mime_type`, `byte_size`
7. `device.inspect`
   - expected signal: device inspection returns read-only device state and
     metadata without mutating ownership
   - watch fields: `device_id`, `discovery_source`, `room`, `vendor`, `model`
8. `device.control`
   - expected signal: device-native control remains in the device lane and does
     not collapse into HarborOS system control
   - watch fields: `device_id`, `control_mode`, `operation`, `result`

## Cutover Watchpoints

- keep device control inside the device domain
- keep device inspection read-only and device-owned
- keep stream storage and PTZ/control execution separate
- do not treat `route_key` as a device or media semantic
- do not treat `resume_token` as anything other than business-flow continuation
- do not widen HarborOS system control to absorb camera-native behavior

## Failure Signs

- scan discovers devices but connect cannot resume after password
- snapshot returns a control-path artifact or mutates registry state
- live view exposes raw device URLs instead of a share artifact
- analyze loses the device hint or drops the snapshot/media reference
- absence of `route_key` breaks legacy HarborBeacon payload construction

## Recommended Checks

- `python -m pytest tests/test_harborbeacon/test_bootstrap.py`
- `python -m pytest tests/test_harborbeacon/test_dispatcher.py`
- `python -m pytest tests/test_harborbeacon/test_task_api.py`
- `cargo test --lib discovery_service_delegates_snapshot_capture`
- `cargo test --lib snapshot_and_open_stream_keep_media_and_control_paths_separate`

## Closeout Proof Pack

Date: 2026-04-19

Boundary proof:

- `discover`, `snapshot`, `share_link`, `inspect`, and `control` stay owned by the Home Device Domain.
- HarborDesk's device page treats image, snapshot, and still-frame inputs as the first-version VLM surface, while continuous video remains follow-up work.
- short clips stay media artifacts with keyframe-derived retrieval evidence, not full continuous-video semantics.
- `camera.share_link` remains the canonical device-lane action; `camera.live_view` stays a compatibility alias only.
- `inspect` stays read-only and device-owned.
- `control` stays device-native and is not claimed by HarborOS executors or HarborOS system control.
- media capture and PTZ/control execution remain separated.
- retrieval evidence stays separate from runtime control.
- HarborOS does not own device control.

Current risk signals:

- discovery still depends on stable LAN reachability and device credential handoff.
- `connect` resume remains sensitive to password-prompt replay and `resume_token` reuse.
- share-link proof still needs to keep signed-link output distinct from raw device URLs.

Non-regression conclusion:

- This closeout confirms the camera canary remains device-native, keeps retrieval/control separation intact, and does not expand HarborOS system control ownership.
