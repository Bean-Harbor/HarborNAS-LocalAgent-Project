# HarborBeacon HarborGate v1.5 Cutover Evidence Checklist

## Purpose

This document is the HarborBeacon-side evidence package for the frozen HarborGate
v1.5 seam.

It exists to show that HarborBeacon can operate within the frozen boundary without
widening semantics, re-owning IM transport, or reintroducing direct platform
delivery.

## Frozen Endpoints

The HarborBeacon-side seam under review is anchored on these endpoints:

- `POST /api/tasks`
- `POST /api/notifications/deliveries`
- `GET /api/gateway/status`

Boundary notes:

- `POST /api/tasks` is the frozen inbound task interface between HarborGate and
  HarborBeacon.
- `POST /api/notifications/deliveries` is the frozen outbound notification
  interface hosted by HarborGate.
- `GET /api/gateway/status` is a supporting redacted status interface only; it
  is not one of the two frozen cross-repo interfaces.

## Acceptance Gates

HarborBeacon-side cutover evidence is only complete when all of the following are
true:

- inbound `POST /api/tasks` contract coverage passes against the frozen v1.5
  shape
- `X-Contract-Version: 1.5` remains in place for frozen interface traffic
- `task_id`, `trace_id`, `source.route_key`, and `message.message_id` remain
  observable and idempotent for inbound retries
- `resume_token` continues HarborBeacon business-flow continuation and is not
  treated as an idempotency key
- outbound delivery intent uses `destination.route_key` and
  `delivery.idempotency_key` without requiring HarborBeacon to own platform
  credentials
- HarborBeacon direct platform delivery count is `0` after cutover unless the
  explicit legacy rollback flag is enabled
- accepted-request delivery failures remain `HTTP 200` with `ok=false`
- request-rejection failures remain non-200 and use the shared error envelope
- redacted gateway status, when needed, does not reveal raw platform
  credentials or platform auth state

## Rollback Constraints

Rollback must preserve the frozen boundary:

- HarborBeacon must not directly deliver platform messages after cutover
- HarborBeacon must not store or validate raw platform credentials as the long-term
  owner
- HarborBeacon must treat `route_key` as write-only routing metadata, not as a
  platform recipient model
- HarborBeacon must keep business state, approvals, artifacts, and audit as the
  source of truth
- rollback must keep the HarborGate delivery path in place rather than
  reintroducing a direct platform send path
- rollback may only re-enable legacy recipient fallback through the explicit
  `HARBORBEACON_ENABLE_LEGACY_IM_RECIPIENT_FALLBACK=1` rollback switch
- rollback must keep the HarborOS System Domain fallback order unchanged:
  `Middleware API -> MidCLI -> Browser/MCP fallback`

## External IM Repo Dependencies

The HarborBeacon-side package still depends on the external IM repo for these
pieces of ownership and validation:

- route key lifecycle and route registry behavior
- platform credential storage and validation
- outbound delivery execution and provider-specific payload formatting
- redacted gateway status for setup or UI flows
- transport retries and platform-provider auth state

These are external dependencies, not HarborBeacon-owned semantics.

## Evidence Checklist

The daily evidence bundle for this seam should include:

- contract test results for inbound task handling
- contract test results for outbound notification delivery intent
- replay evidence for same `task_id` idempotency
- replay evidence for same `delivery.idempotency_key` idempotency
- resume evidence for `needs_input` plus `resume_token`
- proof that HarborBeacon direct platform delivery count remains `0` on the
  canary path
- rollback evidence that preserves the frozen boundary
- log or fixture evidence for `task_id`, `trace_id`, `source.route_key`,
  `message.message_id`, `notification_id`, `delivery.idempotency_key`, and
  `provider_message_id`

## Daily Reporting Use

When this evidence package is referenced in a daily sync, report:

1. whether the HarborBeacon-side frozen interfaces still match the v1.5 contract
2. whether rollback keeps HarborBeacon out of direct platform delivery and raw
   credential ownership
3. which remaining items still depend on the external IM repo
4. whether the required observability fields were present in tests, fixtures,
   or logs
