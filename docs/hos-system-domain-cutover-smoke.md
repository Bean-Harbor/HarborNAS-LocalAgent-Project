# HarborOS System Domain Cutover Smoke

## Purpose

This pack proves the HarborOS System Domain stays on the frozen route order:

`Middleware API -> MidCLI -> Browser/MCP fallback`

It is a HarborOS-only proof pack. It does not exercise IM ingress, `route_key`
handling, notification delivery, or any device-native adapter stack.
IM dual-channel readiness, source-bound reply routing, and proactive delivery
failures are IM-lane concerns, not HarborOS blockers.
The release install root may live under `/var/lib/harborbeacon-agent-ci`, but
this pack continues to cite `/mnt/software/harborbeacon-agent-ci` as the
HarborOS writable / mutation root.

Scaffold-only read previews such as `files.stat` and `files.read_text` are
reviewed in the substrate pack and are not treated here as live cutover parity
requirements.

During the IM prelaunch rehearsal, HarborOS remains southbound-only support for
the `HarborGate -> HarborBeacon -> HarborOS` chain. Feishu is the stable
baseline surface; Weixin remains on the parity track until ingress is confirmed
and the same rehearsal matrix passes.
HarborOS does not take ownership of route keys, message replay, platform
delivery, or device-native control.

## What This Pack Proves

- HarborOS service/files actions stay on `Middleware API` or `MidCLI`
- Browser and MCP remain fallback-only for non-system domains
- HarborOS executors do not claim device-native domains
- HarborOS executors do not claim device-native domains such as camera/device
- device-native actions such as `discover`, `snapshot`, `share_link`, `inspect`,
  and `control` stay outside HarborOS system ownership
- validation tooling keeps the system-domain boundary reviewable
- the proof pack emits direct quote labels for `service.query`, `files.list`,
  `service.restart`, `files.copy`, and `files.move`
- the proof pack labels HarborOS service/files evidence as
  `route_mode=middleware_first` or `route_mode=midcli_fallback`, so the
  Windows verifier line and the Debian shim line can be cited directly

## Admin Surface Summary

Use these fields when a unified HarborDesk view needs one compact proof row:

- `action_summary`: `HarborOS service query proof`, `HarborOS files list proof`,
  `Approved HarborOS service restart`, `Approved HarborOS file copy`, or
  `Approved HarborOS file move`
- `live_status_summary`: keep route order and writable root in the live status
  lane, separate from proof evidence
- `proof_summary`: keep verifier labels and pause conditions in the proof lane
- `proof_label`: the stable machine label such as `service.query` or
  `files.move`
- `route_mode`: `middleware_first`, `midcli_fallback`, `midcli_primary`, or
  `policy_gate`
- `verifier_line_label`: `Windows verifier line` for middleware-first proof
  lines, `Debian shim line` for midcli fallback lines
- `writable_root`: `/mnt/software/harborbeacon-agent-ci`
- exec-capable install roots such as `/var/lib/harborbeacon-agent-ci` are
  release-layout details, not mutation proof fields
- `pause_conditions`: pause when ordinary HarborOS actions drift to browser or
  MCP, when `route_mode=midcli_fallback` spikes, when supported actions lose an
  executor, or when writes target anything outside the approved writable root
- IM dual-channel parity gaps and proactive delivery failures stay in the IM
  lane; they are not HarborOS blockers

## Architect Quote Pack

Use these as the shortest stable citations:

- `service.query` -> `route_mode=middleware_first` on the Windows verifier line
- `files.list` -> `route_mode=middleware_first` on the Windows verifier line
- `service.restart` -> approved HarborOS service mutation
- `service.query` -> `route_mode=midcli_fallback` on the Debian `192.168.3.223` line
- `files.list` -> `route_mode=midcli_fallback` on the Debian `192.168.3.223` line
- `service.restart` -> approved HarborOS service mutation on the Debian shim line

## Smoke Coverage

Run these reviewable tests before canary:

```bash
cargo test harbor_domains_use_api_then_midcli_only
cargo test non_system_domains_keep_browser_and_mcp_in_priority
cargo test harboros_executors_do_not_claim_device_native_domains
cargo test planner_keeps_control_plane_route_priority_for_service
cargo test planner_keeps_browser_and_mcp_for_non_system_domains
```

What each result should confirm:

- `service` and `files` still resolve to `Middleware API -> MidCLI`
- `device` or other non-system domains still keep `Browser -> MCP` in the
  fallback list
- `camera.snapshot`, `camera.share_link`, `device.discover`,
  `device.inspect`, and `device.control` still stay on device-native paths
- HarborOS middleware and midcli executors reject device-native ownership
- planner output still matches the frozen route priority, not IM or AIoT
  convenience routing
- when the smoke pack reports `route_mode=middleware_first`, that line is the
  primary HarborOS proof; when it reports `route_mode=midcli_fallback`, that
  line is fallback evidence rather than middleware parity

## Canary Watchlist

If HarborOS system actions regress during IM v1.5 cutover, watch these signals:

- `executor_used` unexpectedly becomes `browser` or `mcp` for `service` or
  `files`
- `route_mode=midcli_fallback` spikes for ordinary HarborOS control actions
- `NO_EXECUTOR_AVAILABLE` appears for supported HarborOS system operations
- logs show `unsupported harbor domain` for service/files requests
- any HarborOS smoke references camera, ONVIF, RTSP, or other device-native
  control work
- IM cutover evidence starts depending on Browser/MCP success for supported
  `service` or `files` actions
- Feishu baseline rehearsal logs, or Weixin parity-track logs, show device-native
  verbs such as `discover`, `snapshot`, `share_link`, `inspect`, or `control`
  being routed through HarborOS executors

If any of the above appears, pause HarborOS canary traffic and keep the
boundary fixed instead of broadening HarborOS ownership.

## Independent Verifier Note

Keep two proof lines available for canary day:

- Windows verifier proof for the `route_mode=middleware_first` path
- Debian `192.168.3.223` proof for an independent verifier run that may cite
  `route_mode=midcli_fallback` when the Python shim is used

If Debian still reaches HarborOS through the Python shim plus `midcli`
fallback, describe that explicitly as fallback evidence. Do not present it as
native `midclt` middleware parity.

## Rollback Notes

Rollback on canary day should preserve the same HarborOS system-domain shape:

- keep `Middleware API` first for supported HarborOS service/files actions
- keep `MidCLI` as the deterministic fallback
- keep the route evidence labels aligned with the actual path taken
- keep `Browser/MCP` as fallback only for non-system domains
- do not move device-native control into HarborOS just to make a smoke pass
- HarborOS may support storage/archive/policy for AIoT flows, but it does not
  become the control owner for discovery or camera operations
- do not route IM or notification concerns back into HarborOS system control

Rollback is acceptable only if it restores observability and execution safety
without changing the boundary.
