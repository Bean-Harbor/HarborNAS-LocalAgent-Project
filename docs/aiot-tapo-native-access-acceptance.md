# TP-Link/Tapo Native Access Acceptance Note

Date: 2026-04-19

Session:
- `019da4df-a03d-7d01-9aa8-57b52547013e`

Scope:
- Home Device Domain only
- RTSP discovery, stream candidate ordering, and camera snapshot selection
- no HarborOS ownership transfer

Accepted device-domain behavior:
- TP-Link/Tapo candidates now prefer `/stream1` and `/stream2` before the generic RTSP defaults.
- Camera registry profile metadata now carries a vendor preset and can carry a `native_snapshot_url` when one is present.
- native snapshot capture is preferred when the profile or binding provides a URL.
- ffmpeg remains the fallback when no native snapshot URL is confirmed.
- release-v1 clip capture should reuse the existing recording policy and media metadata surface; do not introduce a TP-Link-specific profile object.
- HarborDesk should display the selected camera, storage subdirectory, clip length, and keyframe hints from the existing camera/profile and recording-policy records.

Context check:
- no native snapshot URL was confirmed from the current session context
- the device lane therefore keeps the media path safe by retaining the ffmpeg fallback

Validation:
- `cargo test preferred_rtsp_paths_prioritize_tapo_vendor_presets`
- `cargo test snapshot_round_trip_uses_profile_native_snapshot_url_when_binding_missing`
- `python -m pytest tests/contracts/test_aiot_boundary_guard.py`

Boundary note:
- HarborOS does not take over device-native RTSP or snapshot ownership.
- Home Device Domain remains the source of truth for camera discovery and media access.
- short clip capture, if enabled, should remain device-domain media execution that writes into the HarborOS writable root subtree configured by the operator.
