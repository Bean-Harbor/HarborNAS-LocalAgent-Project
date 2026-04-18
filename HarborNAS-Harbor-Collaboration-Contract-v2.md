# HarborNAS Harbor Collaboration Contract v2

## Status

This document is the working freeze candidate for the current multi-lane
development model across:

- the HarborNAS repo
- the external IM Gateway repo
- the `harbor-*` skill topology used to organize ownership

It supersedes the narrower HarborOS-control-only collaboration model as the
primary coordination document for the current phase.

This document does not replace or reinterpret the external IM contract.

## Normative References

The authoritative cross-repo IM boundary remains:

- `C:\Users\beanw\OpenSource\IM\HarborNAS-IM-Gateway-Agent-Contract-v1.5.md`

Execution planning references:

- `C:\Users\beanw\HarborNAS-LocalAgent-Project-git\HarborNAS-LocalAgent-Roadmap.md`
- `C:\Users\beanw\HarborNAS-LocalAgent-Project-git\HarborNAS-LocalAgent-Plan.md`

Historical same-repo HarborOS-only collaboration context:

- `C:\Users\beanw\HarborNAS-LocalAgent-Project-git\HarborNAS-HarborOS-Control-Collaboration-Contract-v1.md`

If this document conflicts with the IM contract v1.5 on cross-repo interface
semantics, the IM contract v1.5 wins.

## Purpose

Freeze the collaboration boundary so functional lanes can move in parallel
without re-coupling the system.

The intended operating model for this phase is:

- IM remains in a separate repo
- HarborNAS remains the business-core repo
- southbound work is domain-split, not one generic adapter bucket
- each lane owns implementation inside a frozen collaboration boundary

## Team Topology

### `harbor-architect`

Own:

- overall architecture and repo topology
- boundary governance
- milestone sequencing and cutover order
- release, rollback, and acceptance gates
- conflict arbitration between lanes

Do not automatically own:

- day-to-day feature coding inside a single lane

### `harbor-framework`

Own:

- shared runtime and control-plane boundaries
- northbound task ingress and response semantics inside HarborNAS
- task/session lifecycle
- approval, artifact, event, and audit semantics
- local inference runtime abstraction and provider-policy seams
- intelligent orchestration, planner, router, and executor contracts
- account, identity, permission, and workspace management

Do not automatically own:

- IM transport internals
- HarborOS system-domain implementation details
- AIoT device-native protocol stacks

### `harbor-im-gateway`

Own:

- IM adapters and platform SDK/client logic
- webhook, websocket, and long-poll transport
- route registry and `route_key` lifecycle
- outbound delivery, platform payload formatting, and delivery retries
- platform credential storage and validation
- redacted gateway status

Do not automatically own:

- HarborNAS business state
- HarborNAS approval, artifact, audit, or task-session truth

### `harbor-hos-control`

Own:

- HarborOS System Domain implementation
- middleware HTTP/WebSocket integration
- `midcli` fallback
- HarborOS service/files execution mapping
- HarborOS validation and control-path tests

Do not automatically own:

- IM bridge behavior
- AIoT device-native adapters
- notification delivery behavior

### `harbor-aiot`

Own:

- Home Device Domain implementation
- camera and LAN AIoT native adapters
- ONVIF, RTSP, vendor-cloud bridge, and device protocol logic
- discovery, PTZ, snapshot, stream-open, and device-control behavior
- media/control separation for device workflows

Do not automatically own:

- IM transport
- HarborOS system-domain execution
- HarborNAS business-state ownership

## System Boundary

### Cross-Repo Boundary

- IM Gateway and HarborNAS communicate only through HTTP/JSON contracts.
- The repos MUST NOT import each other's runtime code.
- The repos MUST NOT share `.harbornas/*.json` or other runtime state files.

### Business Source Of Truth

HarborNAS remains the source of truth for:

- business session state
- resumable workflow state
- approvals
- artifacts
- audit trail
- business conversation continuity

IM Gateway owns transport and platform concerns only.

### Southbound Domain Split

The runtime has at least two distinct southbound domains and they MUST NOT be
collapsed into one routing policy.

#### 1. HarborOS System Domain

Preferred route:

- `Middleware API -> MidCLI -> Browser/MCP fallback`

#### 2. Home Device Domain

Preferred route:

- `Native Adapter -> LAN Bridge -> HarborOS Connector -> Cloud/MCP`

Meaning:

- device-native work should not default to HarborOS CLI or HarborOS middleware
- HarborOS may still provide storage, archive, policy, or coordination support
- media persistence may be HarborOS-backed while control remains device-domain

## Hard Boundary Rules

- HarborNAS MUST NOT directly deliver IM platform messages after cutover.
- HarborNAS MUST NOT become the long-term owner of IM platform credentials.
- IM Gateway MUST NOT absorb HarborNAS business semantics.
- HarborOS control MUST NOT silently absorb Home Device Domain ownership.
- AIoT work MUST NOT silently collapse device-native control into HarborOS
  system control.
- Shared northbound semantics MUST NOT be widened casually for lane-local
  convenience.

## Frozen Interfaces

The following are frozen by the external IM contract and MUST NOT change
without explicit multi-lane sign-off:

- `POST /api/tasks`
- `TaskRequest` and `TaskResponse` semantics visible to IM callers
- top-level `message` block semantics
- `source.route_key`
- resumed-turn behavior using `args.resume_token`
- outbound notification request and response semantics
- `X-Contract-Version`
- shared HTTP auth and non-200 error-envelope rules

## Default Ownership Rules

Unless explicitly reassigned, the following belong to `harbor-framework`:

- local inference orchestration and provider abstraction
- planner, router, and intelligent task orchestration
- audit/event persistence model
- approval model semantics
- account management, identity binding, permissions, and workspace state
- shared task/session persistence

Unless explicitly reassigned, the following belong to `harbor-im-gateway`:

- IM transport behavior
- route key generation and lookup
- platform credentials
- platform delivery formatting

Unless explicitly reassigned, the following belong to `harbor-hos-control`:

- HarborOS middleware and `midcli` execution behavior
- HarborOS service/files mapping and validation

Unless explicitly reassigned, the following belong to `harbor-aiot`:

- camera and AIoT protocol adapters
- device discovery and control execution
- device-media/control split inside the Home Device Domain

Unless explicitly reassigned, the following belong to `harbor-architect`:

- boundary arbitration
- cutover sequencing
- release and rollback gates
- final acceptance of cross-lane changes

## Write Scope Defaults

These are default ownership examples, not a complete file ACL.

### `harbor-framework`

Usually owns first-change rights in areas such as:

- `src/runtime/task_api.rs`
- `src/runtime/task_session.rs`
- `src/control_plane/*`
- `src/orchestrator/router.rs`
- `src/orchestrator/policy.rs`
- `src/connectors/ai_provider.rs`

### `harbor-im-gateway`

Usually owns first-change rights in the external IM repo for:

- adapters
- transport entrypoints
- route registry
- delivery pipeline
- platform credential handling

### `harbor-hos-control`

Usually owns first-change rights in areas such as:

- `src/connectors/harboros.rs`
- `src/orchestrator/executors/harbor_ops.rs`
- `src/domains/system.rs`
- HarborOS-specific tests, plans, and runbooks

### `harbor-aiot`

Usually owns first-change rights in areas such as:

- device/camera discovery and media-control paths
- device-native adapters and registry-facing device logic
- camera snapshot/stream/PTZ execution paths
- device-domain tests, fixtures, and runbooks

## Change Control

### Lane-Local Changes

A lane may land changes independently when:

- the change stays within its domain boundary
- no frozen interface changes
- no shared semantic reinterpretation

### Shared Runtime Changes

Changes touching shared runtime or business semantics require
`harbor-framework` sign-off.

### Cross-Lane Routing Changes

Changes that move work between HarborOS System Domain and Home Device Domain
require:

- `harbor-framework`
- `harbor-hos-control`
- `harbor-aiot`

### Frozen Contract Changes

Changes to frozen IM-facing interfaces require:

- `harbor-architect`
- `harbor-framework`
- `harbor-im-gateway`

### Release Or Cutover Changes

Changes that alter rollout order, rollback shape, or acceptance criteria require
`harbor-architect` sign-off.

## Collaboration Workflow

When a request arrives:

1. classify whether it is framework, IM, HarborOS system, AIoT device, or
   cross-cutting work
2. assign the owning lane
3. name required collaborators only if a shared seam is touched
4. restate what is frozen before implementation starts
5. prefer adapter-local or lane-local changes before editing shared models
6. run the highest-signal validation for the affected lane plus seam tests

## Daily GitHub Sync Rule

Every working day should end with both lane-local sync and architecture-level
closeout.

### Lane-Local Sync Responsibility

- each lane owner syncs their own repo or lane changes to GitHub before ending
  the workday
- `harbor-framework` is the default daily sync owner for HarborNAS-repo core
  work
- `harbor-im-gateway` is the daily sync owner for the external IM Gateway repo
- `harbor-hos-control` syncs HarborOS System Domain changes
- `harbor-aiot` syncs AIoT and Home Device Domain changes

At minimum, the lane owner should leave behind:

- a pushed branch or updated pull request for the day's work
- a short change summary
- current validation status
- blockers, known risks, and rollback notes if the change is risky

The default reporting template lives at:

- `C:\Users\beanw\HarborNAS-LocalAgent-Project-git\docs\daily\harbor-daily-sync-template.md`

Lane owners should not wait for `harbor-architect` to do basic commit, push, or
pull-request hygiene on their behalf.

### Architecture Closeout Responsibility

`harbor-architect` owns the end-of-day integration closeout across lanes.

This means:

- checking which lane updates are ready to merge and which must wait
- confirming whether cross-lane seams remain inside the frozen boundary
- identifying cutover, rollback, or release risks introduced that day
- publishing the daily integration view: merged, pending, blocked, and next
  actions

`harbor-architect` governs the daily closeout decision, but does not replace
lane-local GitHub ownership.

### Default Working Rule

In plain terms:

- each lane owner is responsible for pushing their own work
- `harbor-architect` is responsible for deciding whether the system is safe to
  close, merge, or carry forward to the next day

## Observability Rule

All lanes should preserve and log, when available:

- `task_id`
- `trace_id`
- `source.route_key`
- `source.conversation_id`
- `message.message_id`
- `notification_id`
- `delivery.idempotency_key`
- `destination.route_key`

## Release Gate

A cross-lane release is allowed only when:

- lane-local tests pass for the touched areas
- frozen contract tests pass when applicable
- rollback shape is documented for boundary-moving changes
- no repo import or runtime-state sharing violation was introduced
- IM credential ownership did not leak into HarborNAS
- device-native ownership did not collapse into HarborOS system control

## Working Principle

Move each lane fast, but keep the boundary still.
