---
applyTo: "**"
description: "Always enforce HarborNAS assistant architecture constraints: HarborOS core-integration, plugin-based skills, API->midcli->browser->mcp routing, and command-line-first execution."
---

# HarborNAS Assistant Constraints (Always On)

## Product boundary

- Integrate only the orchestrator core into HarborOS: runtime, planner, router, policy, audit, and HarborOS action adapter.
- HarborClaw (ZeroClaw fork) is pre-installed in HarborOS and serves as the user-facing IM access layer.
- Users interact via IM channels (Feishu, WeCom, Telegram, Discord, DingTalk, Slack, MQTT); HarborClaw routes intents to the orchestrator runtime via MCP/CLI/API.
- Keep non-core capabilities as plugins (skills): video editing, browser automation, third-party software control.
- Do not move plugin-specific logic into HarborOS core unless it is required for platform safety or governance.

## HarborClaw IM integration

- HarborClaw runs on the same machine as HarborOS and is part of the pre-installed image.
- IM channel configuration should be one-click on HarborOS boot.
- HarborClaw autonomy levels (ReadOnly / Supervised / Full) must align with assistant risk levels (LOW / MEDIUM-HIGH / admin-only).
- All IM-originated commands flow through the same policy, audit, and routing pipeline as WebUI/API commands.

## Execution policy

- Use deterministic route priority: `middleware_api -> midcli -> browser -> mcp`.
- For HarborOS domain operations, never use browser or MCP if API or midcli route is available.
- Prefer command-line execution for capability expansion.
- For HarborOS CLI route, prefer `midcli` over generic shell commands.

## Safety and governance

- Enforce dry-run for high-risk and destructive operations when preview is supported.
- Enforce explicit approval for `HIGH` and `CRITICAL` risk actions.
- Enforce path/service validation and deny unsafe operations by default.
- Record structured audit events for every task step: selected route, fallback, inputs, outcome, and duration.

## Delivery mode

- Ship in vertical slices: runnable code first, docs second.
- Keep contract tests and fallback tests updated with every capability change.
- Treat `midcli-only` availability as `degraded` where policy allows; do not block release by default.
