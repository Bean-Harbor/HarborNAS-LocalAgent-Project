# AIoT Camera Regression Annex - 2026-04-22

## Purpose

This annex freezes the current camera regression baseline for the Home Device
Domain after the HarborOS redeploy and confirms that camera ownership remains
device-domain owned.

It does not widen HarborOS System Domain ownership and does not move camera
control into HarborOS service/files execution.

## Ownership Boundary

- Owner: Home Device Domain
- Governing references:
  - `HarborBeacon-Harbor-Collaboration-Contract-v2.md`
  - `docs/aiot-camera-canary-watchlist.md`
  - `docs/camera-domain-task-contract.md`
- Non-regression rule:
  - `discover`, `snapshot`, `share_link`, `inspect`, and `control` remain AIoT/device-domain actions
  - HarborOS may host storage, archive, and snapshot proxy support, but it does not become the camera control owner

## Device Baseline

- Camera IP: `192.168.3.231`
- Username baseline: `admin`
- Password handling:
  - kept in the local target registry only
  - not repeated in repo docs
- HarborOS snapshot proxy:
  - `http://192.168.3.182:4174/api/cameras/cam-rtsp-192-168-3-231/snapshot.jpg`

## Stream Baseline

- Primary stream:
  - path: `/stream1`
  - result: reachable
  - codec: `hevc`
  - resolution: `2880x1620`
- Secondary stream:
  - path: `/stream2`
  - result: reachable
  - codec: `h264`
  - resolution: `640x480`

## Regression Checklist

1. RTSP primary stream still succeeds with the stored device-domain credential baseline.
2. RTSP secondary stream still succeeds with the stored device-domain credential baseline.
3. HarborOS snapshot proxy returns `200 image/jpeg` and remains a media artifact path, not a control-path substitute.
4. Camera actions remain device-domain owned and are not reclassified into HarborOS system control.
5. `route_key` remains opaque routing metadata and is not reused as camera/device semantics.
6. `resume_token` remains business-flow continuation and is not turned into device-auth state.

## Current Result

- No new camera-domain regression was introduced in the HarborOS redeploy.
- Current live smoke proof:
  - HarborOS root page `200 text/html`
  - camera snapshot proxy `200 image/jpeg`
- Current credential behavior proof:
  - anonymous and password-only RTSP attempts were previously rejected with `401`
  - current working baseline continues to require the stored username

## Next Backlog

- Keep signed share-link output distinct from raw device URLs.
- Keep clip capture as a media artifact only; do not expand to continuous-video ownership.
- Keep device credential handoff and resume behavior reviewable without moving secrets into repo docs.
