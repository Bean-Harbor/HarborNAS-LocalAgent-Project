# HarborOS System Domain Cutover Smoke

## Purpose

This pack proves the HarborOS System Domain stays on the frozen route order:

`Middleware API -> MidCLI -> Browser/MCP fallback`

It is a HarborOS-only proof pack. It does not exercise IM ingress, `route_key`
handling, notification delivery, or any device-native adapter stack.

Scaffold-only read previews such as `files.stat` and `files.read_text` are
reviewed in the substrate pack and are not treated here as live cutover parity
requirements.

## What This Pack Proves

- HarborOS service/files actions stay on `Middleware API` or `MidCLI`
- Browser and MCP remain fallback-only for non-system domains
- HarborOS executors do not claim device-native domains
- HarborOS executors do not claim device-native domains such as camera/device
- validation tooling keeps the system-domain boundary reviewable

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
- HarborOS middleware and midcli executors reject device-native ownership
- planner output still matches the frozen route priority, not IM or AIoT
  convenience routing

## Canary Watchlist

If HarborOS system actions regress during IM v1.5 cutover, watch these signals:

- `executor_used` unexpectedly becomes `browser` or `mcp` for `service` or
  `files`
- `fallback_used` spikes for ordinary HarborOS control actions
- `NO_EXECUTOR_AVAILABLE` appears for supported HarborOS system operations
- logs show `unsupported harbor domain` for service/files requests
- any HarborOS smoke references camera, ONVIF, RTSP, or other device-native
  control work

If any of the above appears, pause HarborOS canary traffic and keep the
boundary fixed instead of broadening HarborOS ownership.

## Rollback Notes

Rollback on canary day should preserve the same HarborOS system-domain shape:

- keep `Middleware API` first for supported HarborOS service/files actions
- keep `MidCLI` as the deterministic fallback
- keep `Browser/MCP` as fallback only for non-system domains
- do not move device-native control into HarborOS just to make a smoke pass
- do not route IM or notification concerns back into HarborOS system control

Rollback is acceptable only if it restores observability and execution safety
without changing the boundary.
