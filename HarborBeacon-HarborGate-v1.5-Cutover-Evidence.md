# HarborBeacon HarborGate v1.5 Cutover Evidence Checklist

## Historical Status

This document is historical after the 2026-04-26 decision to move the active
HarborBeacon <-> HarborGate seam to Contract v2.0.

Do not use this file as the current release gate. The active control pack is:

- `C:\Users\beanw\OpenSource\HarborGate\HarborBeacon-HarborGate-Agent-Contract-v2.0.md`
- `HarborBeacon-HarborGate-v2.0-Upgrade-Runbook.md`
- `docs/im-v2.0-cutover-rollback-observability-gates.md`

## Purpose

This document is the HarborBeacon-side evidence package for the frozen HarborGate
v1.5 seam.

It exists to show that HarborBeacon can operate within the frozen boundary without
widening semantics, re-owning IM transport, or reintroducing direct platform
delivery.

Current live-gate decision:

- `weixin_on_169_no_camera_readiness`

## Current Prelaunch Scope

The current cross-repo rehearsal is intentionally narrow:

- Weixin `1:1` text on `.169` is the restored primary rehearsal surface; one real private DM still gates source-bound reply and `needs_input -> resume`
- Feishu baseline evidence remains historical reference on the same frozen seam and is not today's gate driver
- HarborBeacon still only accepts `POST /api/tasks` and emits `POST /api/notifications/deliveries`
- Weixin group-chat semantics remain out of scope and must not be smuggled into HarborBeacon types
- HarborOS remains an accepted southbound on `192.168.3.182`; this document does not widen it into IM ownership

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

## Identity Reconciliation

This package does not introduce new northbound fields. It only reconciles the
existing identifiers already carried by the frozen seam:

- `task_id`: HarborBeacon business-task identity and replay anchor
- `trace_id`: end-to-end observability correlation across the cutover
- `source.route_key`: write-only routing intent from HarborBeacon to HarborGate
- `message.message_id`: top-level message identity for inbound idempotency
- `resume_token`: HarborBeacon business-flow continuation token, not an
  idempotency key
- `delivery.idempotency_key`: outbound delivery replay protection

The same field set is the minimum evidence bundle for reruns and rollback.

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
- HarborBeacon direct platform delivery count is `0` after cutover
- Feishu baseline evidence covers ingress, `needs_input -> resume`,
  notification replay idempotency, and replay-stable session pointers
- Weixin `1:1` parity-track evidence either confirms real ingress or records an explicit
  blocker category without widening the seam
- Feishu rollback can replay the same HarborBeacon scenarios without any
  contract or recipient-shape changes
- accepted-request delivery failures remain `HTTP 200` with `ok=false`
- request-rejection failures remain non-200 and use the shared error envelope
- redacted gateway status, when needed, does not reveal raw platform
  credentials or platform auth state

## Lane-Local Sync Snapshot

This is the HarborBeacon-side closeout view for today:

- Merge-ready now: doc-only closeout updates, evidence alignment, and rollback
  notes that stay inside the frozen HarborBeacon boundary
- Pending due to live/platform gates only: Feishu baseline proof, Weixin
  parity confirmation or blocker categorization, and external IM repo
  rerun evidence
- Not a HarborBeacon blocker: no additional northbound fields, no route-key
  shape change, and no direct platform delivery reintroduction

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
- rollback must keep HarborGate-owned `route_key` delivery active and must not
  reintroduce legacy recipient fallback
- rollback must not reintroduce legacy recipient fallback
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

## Rehearsal Command Set

Use this command set when building a cutover evidence pack:

- HarborBeacon HTTP/bin regression:
  - `cargo test --bin assistant-task-api -- --nocapture`
  - `cargo test --bin agent-hub-admin-api -- --nocapture`
- HarborBeacon contract and southbound regression:
  - `cargo test --lib -- --nocapture`
  - `python -m pytest tests/contracts/test_harbor_integration.py tests/contracts/test_run_scripts_regression.py -q`
- HarborGate seam regression:
  - `python -m pytest tests/test_gateway.py tests/test_harborbeacon.py tests/test_platform_registry.py tests/test_weixin_adapter.py -q`
- HarborGate live-gate collector:
  - `python .\tools\run_platform_live_gate.py`
  - optional HarborBeacon-backed rehearsal:
    `python .\tools\run_platform_live_gate.py --task-api-url http://127.0.0.1:4175 --task-api-token <shared-token>`
- HarborBeacon-backed local rehearsal:
  - `cargo run --bin assistant-task-api -- --service-token <shared-token>`
  - then rerun the HarborGate live-gate collector with the matching `--task-api-url` and `--task-api-token`

Treat these as evidence collectors, not as permission to change the frozen seam.

## Rerun and Rollback Notes

Use the same HarborBeacon-side scenarios for every rerun so the evidence stays
comparable across Feishu baseline, Weixin parity, and rollback checks.

- replay the same payloads with the same `task_id`, `trace_id`,
  `source.route_key`, `message.message_id`, `resume_token`, and
  `delivery.idempotency_key`
- keep `X-Contract-Version: 1.5` unchanged for the frozen interfaces
- confirm the HarborBeacon direct platform delivery count stays `0`
- do not use reruns or rollback to reintroduce legacy recipient fallback
- do not use rollback to introduce a direct platform send path or a new
  recipient shape
- compare rerun logs against the previous Feishu baseline evidence bundle, not
  against a widened contract

Recommended rerun order:

1. replay the inbound task scenario
2. replay the `needs_input -> resume` scenario with the same `resume_token`
3. replay the notification delivery intent with the same
   `delivery.idempotency_key`
4. confirm the direct platform delivery count remains `0`
5. archive the logs or fixtures that expose the observability fields listed
   below

## Observability Field Map

The evidence bundle is only complete when the following fields can be traced
from logs, fixtures, or test output:

- `task_id`: inbound task correlation and replay matching
- `trace_id`: end-to-end request tracing across reruns
- `source.route_key`: frozen routing intent on ingress
- `message.message_id`: message-level idempotency and retry correlation
- `resume_token`: business-flow continuation for `needs_input`
- `destination.route_key`: outbound delivery intent
- `delivery.idempotency_key`: notification replay protection
- `notification_id`: delivery record correlation
- `provider_message_id`: external provider delivery proof, when present

## Evidence Checklist

The daily evidence bundle for this seam should include:

- Weixin `1:1` parity evidence for ingress, replay, notification idempotency,
  and `needs_input -> resume` once the same rehearsal matrix passes on the frozen seam
- Weixin blocker evidence when ingress is still pending, including the
  collector's `blocker_category`
- Feishu baseline evidence that reruns the HarborBeacon-side scenarios through
  the real task API path
- Feishu rollback evidence that replays the same HarborBeacon-side scenarios
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
- HarborOS southbound proof from both the Windows verifier and the Debian
  `192.168.3.223` verifier, with any `midcli` fallback explicitly called out as
  fallback evidence rather than middleware parity

## Daily Reporting Use

When this evidence package is referenced in a daily sync, report:

1. whether the HarborBeacon-side frozen interfaces still match the v1.5 contract
2. whether Feishu remains baseline-ready, whether Weixin has reached parity or remains blocked in one of the four fixed categories, and whether Feishu rollback is still immediately runnable
3. whether rollback keeps HarborBeacon out of direct platform delivery and raw
   credential ownership
4. which remaining items still depend on the external IM repo
5. whether the required observability fields were present in tests, fixtures,
   or logs
