# HarborOS 192.168.3.165 Validation Note

Updated: 2026-04-19

This note records the real HarborOS validation work completed against
`192.168.3.165` from the current Windows host and the Debian13 verifier
`192.168.3.223`.

## Preflight Evidence

The target passed the non-destructive connectivity checks:

- TCP `80` reachable
- TCP `443` reachable
- `GET http://192.168.3.165/` returned `200`
- `GET https://192.168.3.165/` returned `200`
- `ws://192.168.3.165/websocket` accepted the websocket handshake
- `wss://192.168.3.165/websocket` accepted the websocket handshake
- `auth.login` succeeded with the HarborOS admin account over both `ws` and `wss`

Observed pre-auth websocket envelope:

```json
{"msg":"connected","session":"<session-id>"}
```

## Live Smoke Evidence

Repository smoke completed successfully with:

- validate report:
  `.tmp-live/harboros-vm-smoke/validate-contract-20260419-074844.json`
- e2e report:
  `.tmp-live/harboros-vm-smoke/e2e-20260419-074844.json`
- native middleware probe:
  installed `midclt` on the Windows host from the official TrueNAS
  websocket client and verified direct remote calls against
  `ws://192.168.3.165/api/current`

What this proves today:

- `midclt ping` succeeded against `ws://192.168.3.165/api/current`
- `midclt call system.info` returned live system metadata
- middleware method presence for `service.query`, `service.control`,
  `filesystem.listdir`, `filesystem.copy`, and `filesystem.move` was verified
  live through `core.get_methods`
- `service.query` executed live through `middleware_api` and is the
  `route_mode=middleware_first` proof line
- `files.list` executed live through `middleware_api` and is the
  `route_mode=middleware_first` proof line
- system-domain routing stayed reviewable under live conditions
- high-risk actions still stayed behind the policy gate

Current scenario summary:

- `planner-to-harbor-ops`: passed, `executor_used=middleware_api`, `route_mode=middleware_first`
- `planner-to-files-batch-ops`: passed, `executor_used=middleware_api`, `route_mode=middleware_first`
- `guarded-service-restart`: passed in preview, `executor_used=middleware_api`, `route_mode=middleware_first`
- `guarded-files-copy`: passed as policy-gated block
- `guarded-files-move`: passed as policy-gated block

## Approved Mutation Proof

Additional live acceptance was completed against the same target after the
Windows path-normalization fix landed in the repo.

What was approved and executed live:

- `service.control(RESTART, ssh)` completed successfully as job `5798`
- `filesystem.copy` completed successfully as job `5813`
  using source
  `/mnt/software/harborbeacon-agent-ci/copy-source.txt`
  and destination
  `/mnt/software/harborbeacon-agent-ci/accept-copy-live-20260419-accept.txt`
- `filesystem.copy` completed successfully as job `5814`
  to prepare a dedicated move source:
  `/mnt/software/harborbeacon-agent-ci/move-source-20260419-accept.txt`
- `filesystem.move` completed successfully as job `5817`
  moving that file into
  `/mnt/software/harborbeacon-agent-ci/move-destination`

Observed post-run filesystem state:

- `/mnt/software/harborbeacon-agent-ci/accept-copy-live-20260419-accept.txt`
  exists
- `/mnt/software/harborbeacon-agent-ci/move-destination/move-source-20260419-accept.txt`
  exists

What this changes architecturally:

- HarborOS system-domain live proof now includes one approved service mutation
  and real `copy` / `move` mutations, not only preview or gate enforcement
- the validated live sandbox on this target is
  `/mnt/software/harborbeacon-agent-ci`
  rather than the earlier assumed `/mnt/agent-ci`

## Dual-Host Acceptance Proof

Cross-host acceptance completed on `2026-04-19` from Debian13
`192.168.3.223` against the HarborOS target `192.168.3.165`.

Archived local report copies:

- validate report:
  `.tmp-live/debian-dualhost/reports/validate-contract-20260419-095711.json`
- e2e report:
  `.tmp-live/debian-dualhost/reports/e2e-20260419-095711.json`
- latency summary:
  `.tmp-live/debian-dualhost/reports/latency-summary.json`
- audit summary:
  `.tmp-live/debian-dualhost/reports/audit-coverage-summary.json`

What this proves in addition to the Windows-host acceptance:

- the current HarborOS smoke pack is reproducible from an independent verifier
  host, not only from the authoring workstation
- `validate-contract-schemas --require-live` passed from Debian with no frozen
  contract changes
- `run-e2e-suite --require-live` returned `ok=true` from Debian
- `service.query`, `files.list`, approved `service.restart`, approved
  `files.copy`, and approved `files.move` all executed successfully from the
  Debian verifier
- the cross-host execution path currently degrades cleanly to `midcli` when
  native `midclt` is not installed on the verifier
- the Debian verifier proof line should be read as
  `route_mode=midcli_fallback` when the Python shim is doing the work

Observed Debian-side scenario summary:

- `planner-to-harbor-ops`: passed, `executor_used=midcli`, `route_mode=midcli_fallback`
- `planner-to-files-batch-ops`: passed, `executor_used=midcli`, `route_mode=midcli_fallback`
- `guarded-service-restart`: passed, `executor_used=midcli`, `route_mode=midcli_fallback`, job `5938`
- `guarded-files-copy`: passed, `executor_used=midcli`, `route_mode=midcli_fallback`
- `guarded-files-move`: passed, `executor_used=midcli`, `route_mode=midcli_fallback`

## Quotable Closeout

The architect-facing closeout can cite these exact statements:

- Windows host verification recorded `proof_label=service.query` and
  `proof_label=files.list` with `route_mode=middleware_first`
- Windows host approval recorded `proof_label=service.restart` for the live
  service mutation
- Debian verifier `192.168.3.223` recorded the same proof labels with
  `route_mode=midcli_fallback`
- the live writable root is fixed to `/mnt/software/harborbeacon-agent-ci`
- no evidence line should collapse that root back to `/mnt/agent-ci` or
  `/data/agent-ci`

## HarborDesk Summary

The admin surface can safely reuse these proof labels without expanding the
HarborOS boundary:

- `service.query` and `files.list` are the live verification action summaries
  for the Windows verifier line
- `service.restart`, `files.copy`, and `files.move` are the approved mutation
  action summaries
- `route_mode=middleware_first` belongs on the Windows verifier line
- `route_mode=midcli_fallback` belongs on the Debian shim line
- `writable_root=/mnt/software/harborbeacon-agent-ci` is the only live
  writable root to surface here
- pause if ordinary HarborOS control actions start routing through browser or
  MCP, if `midcli_fallback` spikes, or if a write target escapes the approved
  writable root
- Feishu/Weixin delivery routing issues belong to the IM lane; they are not
  HarborOS blockers unless HarborOS route order or writable-root policy drifts

Verifier environment note:

- Debian used a local Python venv for the Harbor midcli shim because the system
  Python initially lacked `websocket-client`
- that dependency gap was an environment issue only; once the venv was present,
  the same repository smoke script succeeded unchanged

## Real vs Remaining Gaps

Real today:

- live websocket transport
- live `auth.login`
- live `midclt` install on the Windows host
- live middleware probe via `ws://192.168.3.165/api/current`
- live `service.query`
- live `files.list`
- live method-surface proof for `service.control`, `filesystem.copy`, and
  `filesystem.move`
- approved live `service.restart`
- approved live `files.copy`
- approved live `files.move`
- Debian13 `192.168.3.223 -> 192.168.3.165` dual-host acceptance run

Still missing for fuller parity:

- the current live sandbox root differs from the originally planned
  `/mnt/agent-ci` or `/data/agent-ci`; if we want stricter operator guidance we
  should either provision one of those roots or update the runbook to bless
  `/mnt/software/harborbeacon-agent-ci`
- direct middleware mutation attempts under `/data` still return
  `path not permitted`, so `/data` should not be assumed to be live-writable on
  this target without additional HarborOS policy changes

## Boundary Reminder

The boundary stays fixed:

- `Middleware API -> MidCLI -> Browser/MCP fallback`
- HarborOS remains the system-domain connector owner
- retrieval semantics do not move into HarborOS as a shortcut
