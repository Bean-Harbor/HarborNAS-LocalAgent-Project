---
name: harboros-assistant-v2
description: "Use when implementing HarborNAS personal assistant capabilities, HarborOS control operations, skill/plugin architecture, API-to-midcli fallback routing, and command-line-first execution. Keywords: assistant runtime, planner, router, midcli, middleware, skills, HarborOS."
---

# HarborOS Assistant V2 Implementation Skill

## Use this skill when

- Building assistant runtime, planner, routing, policy, or audit pipeline.
- Adding HarborOS operations that must use `middleware_api` first and `midcli` as fallback.
- Adding plugin skills (video, browser, software automation) with CLI-first strategy.
- Designing release-safe execution with approvals, dry-run, and observability.

## Non-negotiable rules

- Core-in-HarborOS, extensions-as-plugins.
- Route priority is fixed: `middleware_api -> midcli -> browser -> mcp`.
- HarborOS domain actions must not skip API/CLI routes.
- Command-line-first for extensions; `midcli` first for HarborOS CLI operations.
- High-risk operations require confirmation and approval gates.

## Required architecture outputs

- `assistant.runtime`: task lifecycle and orchestration loop.
- `assistant.planner`: intent to normalized action list.
- `assistant.router`: deterministic route selection and fallback.
- `assistant.policy`: risk, approval, path/service checks.
- `assistant.audit`: per-step event logging and replay references.
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

1. Build assistant runtime minimum loop for `system.harbor_ops`.
2. Wire planner -> router -> policy -> executor -> audit with tests.
3. Add skill registry + manifest loader.
4. Add plugin skills: `files.batch_ops`, `media.video_edit`, `browser.automation`.
5. Add fallback/regression/release-gate tests and metrics.

## Definition of done

- HarborOS control actions execute through API first, then midcli fallback.
- High-risk actions are blocked without approval.
- Every action produces audit records with route and fallback details.
- Plugin skills run without modifying HarborOS core internals.
- CI includes contract, fallback, and policy regression coverage.
