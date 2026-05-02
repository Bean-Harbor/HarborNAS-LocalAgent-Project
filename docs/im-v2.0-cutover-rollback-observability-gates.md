# IM v2.0 Cutover Rollback And Observability Gates

## Purpose

This document replaces the v1.5 cutover gate package for current HarborBeacon
work. It keeps the v2.0 upgrade controlled while the code is being moved from
task requests to conversation turns.

## Go / No-Go Gates

Cutover is allowed only when all are true:

- HarborGate sends `POST /api/web/turns` for inbound IM turns.
- `POST /api/turns` appears only as the deprecated HarborBeacon single-port
  compatibility alias.
- Active service-to-service requests use `X-Contract-Version: 2.0`.
- No active HarborGate path posts `/api/tasks`.
- HarborBeacon notification intents still target
  `POST /api/notifications/deliveries` on HarborGate.
- HarborBeacon admin/status probes still consume HarborGate redacted status via
  `GET /api/gateway/status`.
- No active request builder emits `args.resume_token`.
- HarborBeacon keys business conversation state by Beacon-owned
  `conversation.handle`.
- HarborGate stores `conversation.handle` and continuation values opaquely.
- HarborGate does not route on Beacon business `active_frame.kind`.
- Pending Beacon active frames persist across no-tool conversation acts and
  clear only on explicit resolve, cancel, or superseding tool intent.
- Notification delivery stays in HarborGate.
- HarborBeacon still owns approvals, artifacts, audit, and business state.
- Group chat remains out of scope.

## Rollback Shape

Rollback is an artifact-level rollback of both repos.

Allowed rollback:

- revert both HarborBeacon and HarborGate to the last approved v1.5 artifacts.
- keep platform credentials in HarborGate.
- keep HarborBeacon direct IM delivery disabled.

Disallowed rollback:

- enabling in-process v1.5/v2.0 compatibility.
- reintroducing direct platform sends in HarborBeacon.
- moving platform credential validation into HarborBeacon.
- adding group-chat semantics to get through the release.

## Observability Fields

Each v2.0 evidence bundle should capture:

- `turn.turn_id`
- `turn.trace_id`
- `conversation.handle`
- `transport.route_key`
- `transport.message_id`
- `active_frame.frame_id`
- `active_frame.kind`
- `continuation_present`
- `reply.kind`
- `artifact_count`
- `delivery.idempotency_key`
- `provider_message_id`
- `contract_version`

## Drift Checks

Daily reports must answer:

- active `X-Contract-Version: 1.5` present: yes / no
- active HarborGate `/api/tasks` request present: yes / no
- active `args.resume_token` emission present: yes / no
- Beacon business state keyed by transport session: yes / no
- Gate interpreting business active frames: yes / no
- group chat introduced: yes / no

Any `yes` is not release-ready.
