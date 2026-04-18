# HarborNAS IM Gateway Agent Contract v1.2

## Status

This document consolidates:

- the current `HarborNAS-IM-Gateway-Agent-Contract-v1.md`
- the `HarborNAS-IM-Gateway-Agent-Contract-v1.1-Proposal.md`
- additional v1.2 clarifications needed for stable two-repo implementation

Recommended disposition:

- accept this as the working freeze candidate for first independent implementation
- treat the normative rules below as the baseline for cross-repo development
- keep v1 as historical context only

## Purpose

This contract freezes the v1.2 boundary between:

- external IM Gateway
- HarborNAS backend

The design goal is:

- IM Gateway owns all IM-platform concerns
- HarborNAS owns all business and task concerns
- both sides communicate only through HTTP/JSON contracts

## Frozen Interfaces

v1.2 freezes exactly two cross-repo interfaces:

1. inbound task interface
   - IM Gateway -> HarborNAS
   - based on existing `POST /api/tasks`
2. outbound notification delivery interface
   - HarborNAS -> IM Gateway
   - new interface for all platform-facing notifications

## Hard Boundary Rules

- IM Gateway MUST NOT import `harborbeacon.*`, `orchestrator.*`, or any HarborNAS runtime code.
- HarborNAS MUST NOT import IM Gateway adapter, runtime, or platform client code.
- The repos MUST NOT share `.harbornas/*.json` or any other runtime state files.
- IM platform credentials such as `app_id`, `app_secret`, bot token, websocket ticket, webhook secret, and refresh tokens MUST live only in IM Gateway.
- HarborNAS remains the source of truth for:
  - business session state
  - resumable workflow state
  - approval state
  - artifacts
  - audit trail
- IM Gateway may keep only lightweight transport and runtime state such as:
  - websocket connection state
  - long-poll cursor
  - route registry
  - context token
  - temporary delivery cache
- HarborNAS MUST NOT directly deliver platform messages after cutover of the notification interface.
- HarborNAS MUST NOT remain the long-term owner of platform credential validation. If HarborNAS needs connection status in UI or setup flow, it should obtain a redacted status result from IM Gateway, not store and validate raw platform credentials itself.

## Ownership Split

- IM Gateway owns:
  - adapters
  - webhook, websocket, and long-poll connection mode
  - message normalization
  - ingress session handling
  - outbound delivery
  - platform payload formatting
  - platform credential storage
  - route key generation and lookup
- HarborNAS owns:
  - `assistant_task_api`
  - task execution
  - business state
  - approvals
  - artifacts
  - audit
  - notification intent generation
  - business conversation continuity

## Cross-Cutting Normative Rules

### Contract Versioning

- Both interfaces MUST carry `X-Contract-Version: 1.2`.
- Either side MAY mirror the version into logs or payload metadata for debugging.

### Authentication

- Both interfaces MUST require explicit service-to-service authentication.
- Local single-host deployment MAY use loopback bind plus a shared bearer token.
- Cross-host deployment MUST use authenticated transport and MUST NOT rely on "localhost is trusted" as the only security boundary.

### Timeout and Retry Ownership

- `IM Gateway -> HarborNAS`:
  - request timeout SHOULD default to 15 seconds
  - retry only on transport failure or 5xx
  - do not retry on explicit business failure returned by HarborNAS
- `HarborNAS -> IM Gateway`:
  - request timeout SHOULD default to 10 seconds
  - retry only when the delivery response declares `retryable=true`
- Idempotency key retention TTL in IM Gateway SHOULD be at least 24 hours.

### Observability

Both repos MUST log, when available:

- `task_id`
- `trace_id`
- `source.route_key`
- `source.conversation_id`
- `message.message_id`
- `notification_id`
- `delivery.idempotency_key`
- `destination.route_key`
- `provider_message_id`

### Content Ownership

- HarborNAS owns business semantics.
- IM Gateway MAY adapt formatting for platform constraints, but it MUST NOT reinterpret, summarize, or rewrite business meaning contained in `TaskResponse` or notification payloads.

### Long-Running Task Policy

- `POST /api/tasks` MUST return a user-renderable result synchronously for supported IM turns.
- Long-running background work MAY emit later notifications, but the initial task response still needs a usable reply.
- Async-only turn semantics are out of scope for v1.2 and belong in a later version.

## Interface 1: Inbound Task Interface

### Endpoint

`POST /api/tasks`

### v1.2 Strategy

This interface intentionally reuses the current `TaskRequest` shape instead of inventing a second overlapping turn endpoint.

Existing top-level fields remain:

- `task_id`
- `trace_id`
- `step_id`
- `source`
- `intent`
- `entity_refs`
- `args`
- `autonomy`

Before freezing v1.2, add one explicit top-level `message` block so IM-specific metadata stops leaking into `args`.

### Canonical Request

```json
{
  "task_id": "task_01JABC...",
  "trace_id": "trace_01JABC...",
  "step_id": "step_01",
  "source": {
    "channel": "feishu",
    "surface": "im_gateway",
    "conversation_id": "oc_xxx",
    "user_id": "ou_xxx",
    "session_id": "gw_sess_01JABC...",
    "route_key": "gw_route_01JABC..."
  },
  "intent": {
    "domain": "camera",
    "action": "scan",
    "raw_text": "扫描摄像头"
  },
  "entity_refs": {},
  "args": {},
  "autonomy": {
    "level": "supervised"
  },
  "message": {
    "message_id": "om_xxx",
    "chat_type": "p2p",
    "mentions": [
      {
        "id": "ou_bot_xxx",
        "name": "HarborNAS Bot"
      }
    ],
    "attachments": []
  }
}
```

### `source` Block Rules

- `source.channel`, `source.surface`, `source.conversation_id`, `source.user_id`, and `source.route_key` are required for IM Gateway callers.
- `source.route_key` is an opaque IM Gateway-owned route handle.
- HarborNAS MUST treat `source.route_key` as write-only routing metadata.
- HarborNAS SHOULD persist `source.route_key` in its business conversation state so later notifications can target the same IM route without HarborNAS storing platform-native recipient semantics.
- `source.session_id` is ingress runtime metadata only. It MUST NOT be treated as the source of truth for HarborNAS business workflow state.

### `message` Block Contract

```json
{
  "message_id": "platform_message_id",
  "chat_type": "p2p",
  "mentions": [
    {
      "id": "platform_user_id",
      "name": "Display Name"
    }
  ],
  "attachments": [
    {
      "attachment_id": "att_01JABC...",
      "type": "image",
      "name": "front-door.jpg",
      "mime_type": "image/jpeg",
      "size_bytes": 183920,
      "download": {
        "mode": "gateway_proxy",
        "url": "http://127.0.0.1:8787/files/att_01JABC...",
        "method": "GET",
        "headers": {
          "Authorization": "Bearer attachment_token"
        },
        "auth": {
          "type": "bearer"
        },
        "expires_at": "2026-04-18T14:10:00Z",
        "max_size_bytes": 20971520
      },
      "metadata": {
        "platform_file_key": "file_xxx"
      }
    }
  ]
}
```

### Attachment Rules

- `message` is required for IM Gateway callers in v1.2.
- `message.message_id` is required when the platform exposes one.
- If the platform truly does not expose a stable message id, IM Gateway MUST still keep retry identity stable using the same `task_id` and `trace_id`.
- `message.chat_type` MUST be one of:
  - `p2p`
  - `group`
  - `channel`
  - `unknown`
- `message.attachments` may be empty.
- HarborNAS MUST treat `download.url` and access metadata as opaque transport contract details.
- HarborNAS MUST NOT assume local filesystem access or platform-native file identifiers.

### Intent Ownership Rule

v1.2 adopts the transitional compatibility model:

- IM Gateway MUST populate `intent.domain` and `intent.action`
- HarborNAS remains the owner of execution semantics, state, approvals, artifacts, and audit
- this does not transfer business semantics ownership to IM Gateway
- moving business intent resolution fully into HarborNAS is a future-version concern

### Inbound Idempotency Rule

- For retries of the same inbound IM event, IM Gateway MUST reuse all of:
  - `task_id`
  - `trace_id`
  - `message.message_id`, when the platform exposes one
  - `source.route_key`
- A new `task_id` means a new business task, even if `trace_id` is unchanged.
- IM Gateway SHOULD derive or persist `task_id` from stable inbound event identity, such as platform plus conversation plus message id.
- If no platform message id exists, IM Gateway MUST generate the `task_id` once and persist the mapping so retries reuse the same value.
- HarborNAS MUST treat repeated `POST /api/tasks` calls with the same `task_id` as idempotent replays of the same task intent, not as a new business turn.

### Backward Compatibility

- Legacy non-IM callers may omit `message` and `source.route_key`.
- HarborNAS may initially treat those fields as optional during rollout for legacy callers only.
- Once IM Gateway is the primary IM caller, `message` and `source.route_key` should be treated as required for IM surfaces.

### Response Contract

This interface keeps the existing `TaskResponse` envelope and adds an optional machine-readable `error` block.

```json
{
  "task_id": "task_01JABC...",
  "trace_id": "trace_01JABC...",
  "status": "completed",
  "executor_used": "camera_hub_service",
  "risk_level": "LOW",
  "result": {
    "message": "已按后台默认策略扫描 192.168.31.0/24，但当前没有发现可确认的摄像头候选设备。",
    "data": {},
    "artifacts": [],
    "events": [],
    "next_actions": [
      "分析客厅摄像头"
    ]
  },
  "audit_ref": "audit_01JABC...",
  "missing_fields": [],
  "prompt": null,
  "resume_token": null,
  "error": null
}
```

Failure shape:

```json
{
  "task_id": "task_01JABC...",
  "trace_id": "trace_01JABC...",
  "status": "failed",
  "result": {
    "message": "当前无法完成该操作，请稍后重试。",
    "data": {},
    "artifacts": [],
    "events": [],
    "next_actions": []
  },
  "error": {
    "code": "VALIDATION_ERROR|UNSUPPORTED_ACTION|TEMPORARY_UNAVAILABLE|ATTACHMENT_UNAVAILABLE|INTERNAL_ERROR",
    "message": "human-readable summary"
  }
}
```

### Response Rules

- For supported turns, HarborNAS SHOULD return HTTP 200 with a business `TaskResponse`, even when `status=failed`.
- 4xx and 5xx are reserved for contract, auth, or infrastructure failures.
- `status=needs_input` with `prompt` and `resume_token` continues the same HarborNAS-owned business flow.

### Gateway Reply Mapping

IM Gateway should map `TaskResponse` to user-visible replies as follows:

- `result.message`
  - primary reply body
- `result.artifacts`
  - attachment or link rendering source
- `result.next_actions`
  - optional suggestion chips or appended text
- `status=needs_input` with `prompt` and `resume_token`
  - continue the same HarborNAS-owned business flow
- `status=failed`
  - render failure message without reinterpreting HarborNAS business semantics

## Interface 2: Outbound Notification Delivery Interface

### Why This Interface Exists

HarborNAS currently still contains direct IM delivery logic. If IM Gateway is meant to fully replace the current IM layer, HarborNAS core must stop sending directly to Feishu, Telegram, or any other IM platform.

### Endpoint

`POST /api/notifications/deliveries`

This endpoint is hosted by IM Gateway.

### Canonical Request

```json
{
  "notification_id": "notif_01JABC...",
  "trace_id": "trace_01JABC...",
  "source": {
    "service": "harbornas",
    "module": "task_api",
    "event_type": "task.completed"
  },
  "destination": {
    "kind": "conversation",
    "route_key": "gw_route_01JABC...",
    "id": "optional-legacy-value",
    "platform": "optional",
    "recipient": {
      "recipient_id": "ou_xxx",
      "recipient_type": "open_id"
    }
  },
  "content": {
    "title": "Front Door AI 分析",
    "body": "检测到 1 人，已生成摘要。",
    "payload_format": "plain_text",
    "structured_payload": {},
    "attachments": []
  },
  "delivery": {
    "mode": "send",
    "reply_to_message_id": "",
    "update_message_id": "",
    "idempotency_key": "idem_01JABC..."
  },
  "metadata": {
    "correlation_id": "trace_01JABC..."
  }
}
```

### Destination Routing Rules

- `destination.route_key` is the preferred outbound routing identifier in v1.2.
- `route_key` is opaque and owned only by IM Gateway.
- HarborNAS MUST treat `route_key` as write-only routing metadata and MUST NOT infer platform semantics from it.
- If HarborNAS is replying into the same IM conversation that produced an inbound task request, it SHOULD reuse the previously persisted `source.route_key` as `destination.route_key`.
- Routing fallback priority SHOULD be:
  1. `destination.route_key`
  2. `{destination.platform, destination.id}`
  3. explicit `destination.recipient`
- HarborNAS SHOULD NOT depend on platform-native identifiers once `route_key` is available.

### Route Lifetime Rule

- IM Gateway owns route key lifecycle and registry.
- If a `route_key` is no longer usable, IM Gateway MUST return a machine-readable failure such as `ROUTE_NOT_FOUND` or `ROUTE_EXPIRED`.
- HarborNAS MAY fall back to explicit legacy recipient fields only when they are present and only during migration.

### Notification Rules

- HarborNAS produces notification intent only.
- IM Gateway performs actual platform delivery.
- HarborNAS MUST NOT attach platform credentials to this request.
- `delivery.mode` MUST be one of:
  - `send`
  - `reply`
  - `update`

### Response Contract

```json
{
  "delivery_id": "delivery_01JABC...",
  "notification_id": "notif_01JABC...",
  "trace_id": "trace_01JABC...",
  "ok": true,
  "status": "sent",
  "platform": "feishu",
  "provider_message_id": "om_xxx",
  "retryable": false,
  "error": null
}
```

Failure response:

```json
{
  "delivery_id": "delivery_01JABC...",
  "notification_id": "notif_01JABC...",
  "trace_id": "trace_01JABC...",
  "ok": false,
  "status": "failed",
  "platform": "feishu",
  "provider_message_id": null,
  "retryable": true,
  "error": {
    "code": "RATE_LIMIT|AUTH_FAILED|INVALID_RECIPIENT|ROUTE_NOT_FOUND|ROUTE_EXPIRED|PLATFORM_UNAVAILABLE|UNSUPPORTED_CONTENT",
    "message": "human-readable summary"
  }
}
```

### Delivery Idempotency Rule

- `delivery.idempotency_key` MUST stay stable for notification retries.
- IM Gateway MUST avoid duplicate user-visible sends when the same idempotency key is retried.

## Business Session Truth

HarborNAS already persists business conversation state in `.harbornas/task-api-conversations.json`.

That means:

- resumable workflow truth stays in HarborNAS
- IM Gateway may keep only transport session helpers
- IM Gateway MUST NOT become the source of truth for workflow or business state

## Credential and Setup Boundary

- HarborNAS direct platform delivery is transitional and must be removed after rollout of the notification interface.
- HarborNAS direct platform credential validation is also transitional and should be removed from long-term architecture.
- If HarborNAS needs to show "connected" or "credential verified" in UI, the preferred long-term model is:
  - IM Gateway stores and validates platform credentials
  - IM Gateway exposes a redacted status result or admin API
  - HarborNAS consumes only connection status, app display name, or route capability metadata
- HarborNAS SHOULD NOT remain the owner of raw `app_id`, `app_secret`, bot token, or equivalent platform credentials after full cutover.

## Recommended Private Models

These are private implementation details, not shared cross-repo contracts.

- IM Gateway private models:
  - internal `InboundMessage`
  - internal `OutboundMessage`
  - adapter runtime state
  - route registry records
- HarborNAS private models:
  - `TaskRequest`
  - `TaskResponse`
  - `NotificationRequest`
  - task session state
  - approval, artifact, and audit models

## Recommended Implementation Split

- Engineer A: IM Gateway repo
  - adapters
  - gateway runtime
  - route registry
  - session ingress
  - platform delivery
  - platform credential and config management
  - message normalization and reply formatting
- Engineer B: HarborNAS repo
  - `assistant_task_api`
  - business and task state machine
  - approval flow
  - artifact and audit persistence
  - notification intent generation
  - replacing direct IM send with IM Gateway delivery calls

## Minimum Test Cases

1. IM Gateway -> `POST /api/tasks` happy path with `source.route_key`, `message.message_id`, `chat_type`, `mentions`, and `attachments`.
2. HarborNAS task resume path with `status=needs_input`, `prompt`, and `resume_token`.
3. Same inbound message retried with the same `task_id` does not create a second business task transition.
4. Same inbound message retried with a different `task_id` is treated as a new task.
5. HarborNAS -> IM Gateway notification send using `destination.route_key` succeeds without HarborNAS providing platform-native recipient fields.
6. Notification retry with the same `idempotency_key` does not duplicate end-user delivery.
7. Expired or invalid attachment download metadata is rejected with a machine-readable error.
8. Expired or missing `route_key` returns `ROUTE_NOT_FOUND` or `ROUTE_EXPIRED`.
9. HarborNAS build or contract test fails if direct IM platform credential usage remains in the notification delivery path after full cutover.

## JSON Schema and Fixtures

- There SHOULD be one JSON Schema per request and response type.
- Both repos SHOULD validate against the same golden fixture set.
- CI SHOULD cover:
  - schema conformance
  - inbound replay and idempotency behavior
  - outbound notification idempotency behavior
  - route key happy path and expiry path

## Rollout Order

1. First, make IM Gateway call `POST /api/tasks` and map `TaskResponse` back to user replies.
2. In the same phase, ensure IM Gateway always provides stable `task_id`, `trace_id`, and `source.route_key` for retries of the same inbound event.
3. Then, extract HarborNAS notification delivery behind the new HTTP notification interface.
4. Then, persist and reuse `route_key` from HarborNAS business conversation state for follow-up notifications.
5. Finally, remove HarborNAS direct IM platform delivery and direct platform credential validation so IM Gateway fully owns the IM layer.

## Release Gate

A release is allowed only when:

- both frozen interfaces have contract tests
- one real IM round-trip passes through `IM Gateway -> /api/tasks -> TaskResponse -> user reply`
- one real notification round-trip passes through `HarborNAS -> IM Gateway -> platform delivery`
- same-message retry with the same `task_id` is proven idempotent
- HarborNAS no longer depends on platform credentials for IM notification delivery
- the intended migration plan away from HarborNAS-owned platform credential validation is agreed and tracked
