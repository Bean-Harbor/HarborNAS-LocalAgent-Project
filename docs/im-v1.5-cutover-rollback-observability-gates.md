# IM v1.5 Cutover Rollback and Observability Gates

## Purpose

This gate package is for HarborBeacon IM v1.5 cutover operations only.
It is meant to keep HarborOS System Domain work inside its boundary while
making rollback, observability, and boundary drift checks explicit.

## Go / No-Go Gates

Cutover is allowed only when all of the following are true:

- inbound `POST /api/tasks` contract tests pass against the frozen v1.5 shape
- outbound `POST /api/notifications/deliveries` contract tests pass against the frozen v1.5 shape
- redacted gateway status is available for UI or setup flows when status is needed
- same `task_id` replay remains idempotent and does not create a second business transition
- same `delivery.idempotency_key` replay remains idempotent and does not create duplicate user-visible delivery
- `route_key` remains opaque and write-only from the HarborBeacon side
- `resume_token` continues business-flow continuation and is not treated as an idempotency key
- non-200 errors remain reserved for request rejection, while accepted delivery failures remain `HTTP 200`
- HarborBeacon does not directly deliver platform messages after cutover
- HarborBeacon does not own raw platform credentials, credential validation, or platform-provider auth state
- legacy recipient fallback may only be re-enabled via `HARBORBEACON_ENABLE_LEGACY_IM_RECIPIENT_FALLBACK=1` during rollback

## Rollback Gates

Rollback is acceptable only if it keeps HarborBeacon inside the same frozen boundary:

- rollback must preserve the HarborGate-owned delivery path
- rollback must not reintroduce direct platform delivery from HarborBeacon
- rollback must not reintroduce raw platform credential storage or validation in HarborBeacon
- rollback must not broaden HarborOS into IM transport ownership
- rollback must preserve existing HarborOS system-domain fallback order:
  - `Middleware API -> MidCLI -> Browser/MCP fallback`
- rollback notes must say whether legacy recipient fallback is disabled or explicitly re-enabled

If cutover fails, rollback should revert to the previous approved interface path
without moving platform ownership back into HarborBeacon.

## Observability Gates

Each cutover or rollback record should capture the following fields when available:

- `task_id`
- `trace_id`
- `source.route_key`
- `source.conversation_id`
- `message.message_id`
- `notification_id`
- `delivery.idempotency_key`
- `destination.route_key`
- `provider_message_id`
- `gateway_status`
- `contract_version`

## Drift Checks

These checks must be called out explicitly in reports and reviews:

- HarborBeacon accidentally reintroduced direct platform delivery: yes / no
- HarborBeacon accidentally reintroduced raw credential ownership or validation: yes / no
- HarborOS system-domain scope drifted into IM transport ownership: yes / no
- rollback kept the boundary intact: yes / no
- observability fields were available for the seam under review: yes / no

## Daily Reporting Use

When this package is referenced in a daily sync, report:

1. seam status for inbound task and outbound delivery paths
2. rollback safety status and any boundary-preserving limitation
3. whether HarborBeacon still avoids direct platform delivery and raw credential ownership
4. whether the required observability fields were seen in logs, fixtures, or test output
