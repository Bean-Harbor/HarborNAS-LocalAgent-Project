# 2026-04-28 VM Control Plane Closeout

## Scope

- Target: HarborOS VM `192.168.1.5`.
- Builder: `192.168.1.197`.
- Release name: `20260428-vm-control-plane-closeout-r1`.
- Entry point: `http://192.168.1.5/ui/harbordesk`.
- Out of scope: 182, GPU inference, switching the current runtime model, and new model downloads.

## Release Artifacts

- HarborBeacon plus HarborGate release bundle:
  - Builder path: `/home/harbor-innovations/artifacts/harborbeacon-release-bundles/harbor-release-20260428-vm-control-plane-closeout-r1.tar.gz`
  - Local evidence copy: `C:\Users\beanw\OpenSource\_artifacts\harborbeacon-closeout-20260428\release\harbor-release-20260428-vm-control-plane-closeout-r1.tar.gz`
  - SHA256: `9b95d21a14c24e3d6d0aa5cb604d690cfa7ca08ad789571d5ab5bc977bea612c`
- Native HarborOS WebUI bundle:
  - Builder path: `/home/harbor-innovations/artifacts/harbordesk-webui-vm-control-plane-closeout-20260428-080513.tar.gz`
  - Local evidence copy: `C:\Users\beanw\OpenSource\_artifacts\harborbeacon-closeout-20260428\release\harbordesk-webui-vm-control-plane-closeout-20260428-080513.tar.gz`
  - SHA256: `3561fdf1c14f7ca135a4c52022a110b733e6327811968c8eb6927a40977a7c1b`

## VM State

- Active release symlink: `/var/lib/harborbeacon-agent-ci/current -> /var/lib/harborbeacon-agent-ci/releases/20260428-vm-control-plane-closeout-r1`.
- Install root: `/var/lib/harborbeacon-agent-ci`.
- Writable root: `/mnt/software/harborbeacon-agent-ci`.
- HarborDesk WebUI mount source: `/var/lib/harbordesk-webui/releases/vm-control-plane-closeout-20260428-080513`.
- WebUI mount target: `/usr/share/truenas/webui`, read-only bind mount.
- Model download mirror remains `HF_ENDPOINT=https://hf-mirror.com`.
- Qwen3.5 4B is installed at `/mnt/software/harborbeacon-models/qwen-qwen3.5-4b`.
- Qwen3.5 4B manifest exists at `/mnt/software/harborbeacon-models/qwen-qwen3.5-4b/snapshot_manifest.json`.
- Qwen3.5 4B was not set as the current runtime model during closeout.

## Validation

- HarborGate targeted tests: `77 passed, 1 warning`.
  - `python -m pytest tests/test_server.py tests/test_gateway.py tests/test_weixin_adapter.py tests/test_weixin_runner.py -q`
- HarborBeacon targeted tests passed:
  - `cargo test --bin agent-hub-admin-api model_download --quiet`
  - `cargo test --bin agent-hub-admin-api huggingface --quiet`
  - `cargo test --lib image_search --quiet`
  - `cargo test --bin agent-hub-admin-api rag_readiness --quiet`
  - `cargo test --lib rag_answer --quiet`
  - `cargo test --bin assistant-task-api turn --quiet`
- HarborDesk standalone frontend build passed:
  - `npm run build`
- Native HarborOS WebUI checks passed on builder:
  - `corepack yarn jest src/app/pages/harbordesk/services/harbordesk-api.service.spec.ts src/app/pages/harbordesk/utils/harborgate-urls.spec.ts --runInBand`
  - `corepack yarn build:prod`
- VM HTTP checks returned 200:
  - `/ui/harbordesk`
  - `/api/harbordesk/state`
  - `/api/harbordesk/models/local-catalog`
  - `/api/harbordesk/models/local-downloads`
  - `/api/harbordesk/models/endpoints`
  - `/api/harbordesk/rag/readiness`
  - `/api/harbordesk/gateway/status`
  - `/setup/weixin`
  - `/admin/im/weixin`
- VM services active:
  - `harbor-model-api`
  - `assistant-task-api`
  - `agent-hub-admin-api`
  - `harborgate`
  - `harborgate-weixin-runner`
  - `nginx`
  - `usr-share-truenas-webui.mount`
- Model management:
  - `Qwen/Qwen3.5-4B` catalog state: `installed=true`, `status=ready`.
  - Download status endpoint: `status=ready`, `jobs=3`.
- Weixin:
  - Gateway status reports Weixin `connected=true` and transport `status=polling`.
  - This closeout did not send another Weixin message.
- Browser smoke:
  - Ran from the Windows workstation because the 197 builder cannot route Chromium to `192.168.1.5`.
  - Report path: `C:\Users\beanw\OpenSource\_artifacts\harborbeacon-closeout-20260428\browser-smoke-local-r4\report.json`
  - Result: `issues=0`, HarborDesk loaded, all tabs opened, Weixin setup URL opened, no network failures, no HarborDesk API failures, no direct `:4174` requests.
- Persistence and drift:
  - nginx reload and restart preserved HarborDesk and setup routes.
  - Drift scan: `/api/tasks=0`, `X-Contract-Version: 1.5=0`.
- Redaction:
  - Diff-level scans found no live Weixin token, HF token, camera password, or raw credential in tracked changes.
  - Browser smoke setup URLs were redacted in the saved report.

## Fixes Captured In This Closeout

- HarborBeacon model download status now computes top-level readiness from the latest job per model, so older failed jobs do not keep the downloads endpoint in a blocked state after a later successful retry.
- HarborDesk standalone build warning from an unnecessary optional chain was removed.
- HarborDesk native WebUI smoke tooling was updated for the current long-term Models/RAG layout:
  - `Switch / edit` and `Download` labels.
  - Scrollable HarborDesk content.
  - Read-only device tab checks when no camera devices are configured.
  - Playwright request failure compatibility with string and dict failure payloads.

## Rollback

- Previous usable HarborBeacon/HarborGate baseline: `20260428-vm-admin-download-r2`.
- Previous WebUI mount source before closeout: `/var/lib/harbordesk-webui/releases/vm-cpu-photo-rag-20260428-112535`.
- Rollback steps:
  1. Stop or quiesce live tests.
  2. Repoint `/var/lib/harborbeacon-agent-ci/current` to `/var/lib/harborbeacon-agent-ci/releases/20260428-vm-admin-download-r2`, or reinstall the archived `20260428-vm-admin-download-r2` bundle.
  3. Restart `harbor-model-api`, `assistant-task-api`, `agent-hub-admin-api`, `harborgate`, and `harborgate-weixin-runner`.
  4. Restore `usr-share-truenas-webui.mount` to `What=/var/lib/harbordesk-webui/releases/vm-cpu-photo-rag-20260428-112535`.
  5. Run `systemctl daemon-reload`, restart `usr-share-truenas-webui.mount`, and reload or restart nginx.
  6. Recheck `/ui/harbordesk`, `/api/harbordesk/state`, model catalog/downloads/endpoints, RAG readiness, gateway status, `/setup/weixin`, and `/admin/im/weixin`.

## Residual Risks

- `cargo fmt --check` on the builder still fails on pre-existing formatting drift in `src/scripts/model_benchmark.rs` and `src/scripts/release_gate.rs`.
- HarborDesk standalone `npm run build` still reports npm audit vulnerabilities in the dependency tree.
- Native WebUI production build still reports existing warnings: duplicate icon definitions, CommonJS optimization bailouts, selector warnings, and missing git hash in the synced snapshot.
- The builder host cannot run the browser smoke against `192.168.1.5`; browser smoke evidence for this release was collected from the Windows workstation.
