---
name: harboros-assistant-v2
description: "Use when implementing HarborBeacon personal assistant capabilities, HarborOS control operations, skill/plugin architecture, API-to-midcli fallback routing, and command-line-first execution. Keywords: assistant runtime, planner, router, midcli, middleware, skills, HarborOS."
---

# HarborOS Assistant V2 Implementation Skill

## Use this skill when

- Building assistant runtime, planner, routing, policy, or audit pipeline.
- Adding HarborOS operations that must use `middleware_api` first and `midcli` as fallback.
- Adding plugin skills (video, browser, software automation) with CLI-first strategy.
- Designing release-safe execution with approvals, dry-run, and observability.
- Integrating HarborBeacon IM channels (Feishu, WeCom, Telegram, Discord, DingTalk, Slack, MQTT) with the assistant runtime.
- Configuring HarborBeacon autonomy levels (ReadOnly / Supervised / Full) and MCP adapter bridging.

## Non-negotiable rules

- **Rust-only implementation**: All new code must be written in Rust. Do not use Python for new features. Existing Python code is legacy reference only — all active development, binaries, and executors must be Rust.
- Core-in-HarborOS, extensions-as-plugins.
- HarborBeacon (ZeroClaw fork) is pre-installed in HarborOS; users interact via IM channels.
- Route priority is fixed: `middleware_api -> midcli -> browser -> mcp`.
- HarborOS domain actions must not skip API/CLI routes.
- Command-line-first for extensions; `midcli` first for HarborOS CLI operations.
- High-risk operations require confirmation and approval gates.
- HarborBeacon autonomy levels must align with assistant risk levels.

## Project north star

Treat the project as:

`一个以 IM 为统一入口、以设备协同和媒体数据流为核心、以本地优先与云端补能为原则、通过智能编排、数据脱敏与统一账号凭据治理，统一编排家庭 AIoT 设备与 NAS/HarborOS 的本地优先家庭智能平台。`

This definition is fixed unless the user explicitly redefines the product direction.

## Top-level system frame

Reason about the whole project in four layers only, top to bottom:

1. `IM entry layer`
   Feishu / Slack / Discord / WeChat / WeCom / Telegram and similar channels.
2. `HarborBeacon interaction layer`
   IM access, session management, natural-language understanding, intent parsing, follow-up questions, rich-media replies.
3. `HarborOS Runtime / Control Plane`
   Unified task entry, orchestration, automation and device collaboration, device registry, event bus, media pipeline, AI analysis, local account system, credential governance, desensitization, and cloud augmentation.
4. `Hardware and system layer`
   NAS, HarborOS services, cameras, and other AIoT devices.

HarborBeacon must not bypass the Runtime / Control Plane to reach cloud providers, credentials, or device backends directly.

## Required architecture outputs

- `harborbeacon.channels`: IM channel registration, message routing, intent parsing.
- `harborbeacon.mcp_adapter`: MCP tool bridge with ReadOnly guard and approval tokens.
- `harborbeacon.autonomy`: autonomy level mapping (ReadOnly/Supervised/Full).
- `harborbeacon.tool_descriptions`: skill manifest to MCP/TOML conversion.
- `orchestrator.runtime`: task lifecycle and orchestration loop.
- `orchestrator.planner`: intent to normalized action list.
- `orchestrator.router`: deterministic route selection and fallback.
- `orchestrator.policy`: risk, approval, path/service checks.
- `orchestrator.audit`: per-step event logging and replay references.
- `skills.registry`: manifest loading and versioning.
- `skills.executors`: API/CLI/browser/MCP executor adapters.

## Canonical contracts

Action envelope fields:

- `domain`
- `operation`
- `resource`
- `args`
- `risk_level`
- `requires_approval`

Execution result fields:

- `task_id`
- `step_id`
- `executor_used`
- `fallback_used`
- `status`
- `duration_ms`
- `error_code`
- `audit_ref`

## Implementation sequence

1. Build HarborBeacon IM channel integration and one-click configuration.
2. Build assistant runtime minimum loop for `system.harbor_ops`.
3. Wire planner -> router -> policy -> executor -> audit with tests.
4. Add skill registry + manifest loader.
5. Add plugin skills: `files.batch_ops`, `media.video_edit`, `browser.automation`.
6. Add fallback/regression/release-gate tests and metrics.

## Definition of done

- HarborOS control actions execute through API first, then midcli fallback.
- High-risk actions are blocked without approval.
- Every action produces audit records with route and fallback details.
- Plugin skills run without modifying HarborOS core internals.
- CI includes contract, fallback, and policy regression coverage.
