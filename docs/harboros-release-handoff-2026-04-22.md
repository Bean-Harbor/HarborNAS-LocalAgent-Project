# HarborOS Release Handoff - 2026-04-22

## Status

- Scope: release handoff for the validated HarborBeacon / HarborGate cutover bundle
- Boundary note: no frozen v1.5 interface change; this handoff covers release hygiene only
- Governing contracts:
  - `HarborBeacon-Harbor-Collaboration-Contract-v2.md`
  - `C:\Users\beanw\OpenSource\HarborGate\HarborBeacon-HarborGate-Agent-Contract-v1.5.md`

## Artifact

- Builder host: `192.168.3.223`
- HarborBeacon commit: `a4e6d61`
- HarborGate commit: `5a5cc56`
- Bundle root:
  - `/home/r86s/artifacts/harborbeacon-release-bundles/harbor-release-20260422-143454-a4e6d61`
- Tarball:
  - `/home/r86s/artifacts/harborbeacon-release-bundles/harbor-release-20260422-143454-a4e6d61.tar.gz`
- SHA256:
  - `7f67bd08ca72034539c334b8375e45b03a162d78e58e7662eac50b0372b133fc`

## Deploy Target

- HarborOS host: `192.168.3.182`
- Service user: `harboros_admin`
- Install root:
  - `/var/lib/harborbeacon-agent-ci`
- Writable root:
  - `/mnt/software/harborbeacon-agent-ci`
- Live release after redeploy:
  - `20260422-143454-a4e6d61`
- Current symlink:
  - `/var/lib/harborbeacon-agent-ci/current -> /var/lib/harborbeacon-agent-ci/releases/20260422-143454-a4e6d61`

## Install Command

Upload the tarball to HarborOS first, then install as root:

```bash
sudo bash /tmp/harbor-release-upload/install_harboros_release.sh \
  --bundle /tmp/harbor-release-upload/harbor-release-20260422-143454-a4e6d61.tar.gz \
  --install-root /var/lib/harborbeacon-agent-ci \
  --writable-root /mnt/software/harborbeacon-agent-ci \
  --public-origin http://192.168.3.182:4174 \
  --gateway-public-origin http://192.168.3.182:8787
```

## Required Runtime Values

The redeployed host was validated with:

```text
HARBOR_RELEASE_VERSION=20260422-143454-a4e6d61
HARBOR_PUBLIC_ORIGIN=http://192.168.3.182:4174
IM_AGENT_PUBLIC_ORIGIN=http://192.168.3.182:8787
HARBORBEACON_ADMIN_API_URL=http://127.0.0.1:4174
HARBORBEACON_ADMIN_API_TOKEN=<service-token>
HARBOR_FFMPEG_BIN=/var/lib/harborbeacon-agent-ci/runtime/media-tools/bin/ffmpeg
```

- HarborGate admin sync depends on the admin API loopback at `:4174`; do not rely on task API fallback for `/api/admin/notification-targets`.

## Service State

The following units were `active/running` after install:

- `assistant-task-api.service`
- `agent-hub-admin-api.service`
- `harborgate.service`
- `harborgate-weixin-runner.service`

## Post-Install Smoke

Validated on `192.168.3.182` after redeploy:

- `GET http://127.0.0.1:4174/`
  - `200 text/html`
- `GET http://127.0.0.1:4174/api/cameras/cam-rtsp-192-168-3-231/snapshot.jpg`
  - `200 image/jpeg`
- Real Weixin DM `帮我抓拍一下当前摄像头画面`
  - HarborGate observed `event=inbound_task_handled`
  - HarborOS archived `/mnt/software/harborbeacon-agent-ci/camera-archive/cam-rtsp-192-168-3-231-1776868605106.jpg`
  - Weixin runtime persisted `last_send_status=sent`, `last_send_attachment_count=1`, `last_send_content_kind=text+image`

## Noexec Note

- `/mnt/software` is mounted `noexec` on HarborOS.
- Do not point `HARBOR_FFMPEG_BIN` at `/mnt/software/harborbeacon-agent-ci/media-tools/bin/ffmpeg`.
- The validated executable path is:
  - `/var/lib/harborbeacon-agent-ci/runtime/media-tools/bin/ffmpeg`
- The patched installer now preserves or auto-detects an executable ffmpeg path and prefers install-root runtime media tools over writable-root media tools.

## Rollback

- Rollback entrypoint:
  - `/var/lib/harborbeacon-agent-ci/current/install/rollback_harboros_release.sh`
- Validated previous release target:
  - `20260422-085316-5076679`

Example rollback command:

```bash
sudo bash /var/lib/harborbeacon-agent-ci/current/install/rollback_harboros_release.sh \
  --install-root /var/lib/harborbeacon-agent-ci \
  --version 20260422-085316-5076679
```

## Known Non-Blocking Risk

- `frontend/harbordesk` still reports npm audit warnings during builder `npm ci`.
- These warnings did not block:
  - bundle creation
  - HarborOS install
  - HarborDesk root page smoke
  - camera snapshot proxy smoke
- The current native Weixin image reply implementation is validated for HarborOS single-host deployment where HarborGate can read HarborBeacon artifact paths locally.
