# AIoT Camera Canary Watchlist

## Purpose

This watchlist is for the Home Device Domain cutover day. It keeps camera
journeys observable without widening into IM transport or HarborOS system
control.

## In Scope

- device discovery and scan validation
- camera connect continuation through `needs_input` and `resume_token`
- snapshot, live view share (`camera.share_link`, with `camera.live_view` accepted as a compatibility alias), and analyze flows where the current codebase supports them
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

## Cutover Watchpoints

- keep device control inside the device domain
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
