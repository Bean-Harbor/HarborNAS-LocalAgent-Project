# HarborNAS IM Gateway Agent Contract v1

## Purpose
This document freezes the v1 boundary between:
- external IM Gateway
- HarborNAS backend

The design goal is:
- IM Gateway owns all IM-platform concerns
- HarborNAS owns all business/task concerns
- both sides communicate only through HTTP/JSON contracts

## Frozen Interfaces
v1 freezes exactly two cross-repo interfaces:
1. inbound task interface
   - IM Gateway -> HarborNAS
   - based on existing `POST /api/tasks`
2. outbound notification delivery interface
   - HarborNAS -> IM Gateway
   - new interface for all platform-facing notifications

## Hard Boundary Rules
- IM Gateway must not import `harborbeacon.*`, `orchestrator.*`, or any HarborNAS runtime code.
- HarborNAS must not import IM Gateway adapter/runtime code.
- Repos must not share `.harbornas/*.json` or any other runtime state files.
- IM platform credentials such as `app_id`, `app_secret`, bot token, websocket ticket, webhook secret must live only in IM Gateway.
- HarborNAS remains the source of truth for:
  - business session state
  - resumable workflow state
  - approval state
  - artifacts
  - audit trail
- IM Gateway may keep only lightweight transport/runtime state such as:
  - websocket connection state
  - long-poll cursor
  - context token
  - temporary delivery cache

## Ownership Split
- IM Gateway owns:
  - adapters
  - webhook/websocket/long-poll connection mode
  - message normalization
  - session ingress
  - reply delivery
  - platform payload formatting
  - platform credential storage
- HarborNAS owns:
  - `assistant_task_api`
  - task execution
  - business state
  - approvals
  - artifacts
  - audit
  - notification intent generation

## Interface 1: Inbound Task Interface

### Endpoint
`POST /api/tasks`

### v1 Strategy
This interface intentionally reuses the current `TaskRequest` shape instead of inventing a new IM-only turn endpoint.

Existing top-level fields remain:
- `task_id`
- `trace_id`
- `step_id`
- `source`
- `intent`
- `entity_refs`
- `args`
- `autonomy`

Before freezing v1, add one explicit top-level `message` block so IM-specific metadata stops leaking into `args`.

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
    "session_id": "sess_01JABC..."
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
        "expires_at": "2026-04-18T14:10:00Z"
      },
      "metadata": {
        "platform_file_key": "file_xxx"
      }
    }
  ]
}
```

### Request Rules
- `source.channel`, `source.surface`, `source.conversation_id`, and `source.user_id` are required for IM callers.
- `intent.raw_text` is required.
- `message` is required for IM Gateway callers in v1.
- `message.message_id` is strongly recommended. If platform truly does not expose one, IM Gateway must still keep `trace_id` stable across retries.
- `message.chat_type` must be one of:
  - `p2p`
  - `group`
  - `channel`
  - `unknown`
- `message.attachments` may be empty.
- IM metadata such as `message_id`, `chat_type`, `mentions`, `attachments` must not be hidden inside `args`.

### Backward Compatibility
- legacy non-IM callers may omit `message`
- HarborNAS may initially treat `message` as optional during rollout
- once IM Gateway is the primary caller, `message` should be treated as required for IM surfaces

### Response Contract
This interface keeps the existing `TaskResponse` envelope.

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
  "resume_token": null
}
```

### Gateway Reply Mapping
IM Gateway should map `TaskResponse` to user-visible replies as follows:
- `result.message`
  - primary reply body
- `result.artifacts`
  - attachment/link rendering source
- `result.next_actions`
  - optional suggestion chips or appended text
- `status=needs_input` with `prompt` and `resume_token`
  - continue the same HarborNAS-owned business flow
- `status=failed`
  - render failure message without reinterpreting HarborNAS business semantics

### Why HarborNAS Keeps Business Session State
HarborNAS already persists business conversation state in `.harbornas/task-api-conversations.json`.
That means:
- resumable workflow truth stays in HarborNAS
- IM Gateway may keep only transport session helpers
- IM Gateway must not become the source of truth for workflow/business state

## Interface 2: Outbound Notification Delivery Interface

### Why This Interface Exists
HarborNAS currently still contains direct IM delivery logic in:
- [src/connectors/notifications.rs](/C:/Users/beanw/HarborNAS-LocalAgent-Project-git/src/connectors/notifications.rs:128)
- [src/runtime/task_api.rs](/C:/Users/beanw/HarborNAS-LocalAgent-Project-git/src/runtime/task_api.rs:1272)

If IM Gateway is meant to fully replace the current IM layer, HarborNAS core must stop sending directly to Feishu/Telegram/other IM platforms.

### Endpoint
`POST /api/notifications/deliveries`

This endpoint is hosted by IM Gateway.

### Request Contract

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
    "id": "oc_xxx",
    "platform": "feishu",
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

### Notification Rules
- HarborNAS produces notification intent only.
- IM Gateway performs actual platform delivery.
- HarborNAS must not attach platform credentials to this request.
- `destination.platform` is optional if IM Gateway can resolve destination routing from its own mapping.
- `destination.recipient` is optional when `kind` and `id` are enough for IM Gateway resolution.
- `delivery.mode` must be one of:
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
    "code": "RATE_LIMIT|AUTH_FAILED|INVALID_RECIPIENT|PLATFORM_UNAVAILABLE|UNSUPPORTED_CONTENT",
    "message": "human-readable summary"
  }
}
```

## Cross-Interface Idempotency Rules
- IM Gateway -> HarborNAS:
  - `trace_id` must stay stable for retried delivery of the same inbound user message
  - `message.message_id` should be used by HarborNAS for dedup where possible
- HarborNAS -> IM Gateway:
  - `delivery.idempotency_key` must stay stable for notification retries
  - IM Gateway must avoid duplicate user-visible sends when the same idempotency key is retried

## Risk List
### Risk 1
New IM project accidentally depends on HarborNAS runtime modules.

Mitigation:
- only communicate via HTTP/JSON
- private gateway models stay private to the IM repo

### Risk 2
HarborNAS keeps platform-level credentials and continues direct delivery.

Mitigation:
- platform credentials live only in IM Gateway
- HarborNAS direct IM delivery code is transitional and must be removed after notification interface rollout

## Recommended Private Models
These are private implementation details, not shared cross-repo contracts.

- IM Gateway private models:
  - internal `InboundMessage`
  - internal `OutboundMessage`
  - adapter runtime/session state
- HarborNAS private models:
  - `TaskRequest`
  - `TaskResponse`
  - `NotificationRequest`
  - task session state
  - approval/artifact/audit models

## Recommended Implementation Split
- Engineer A: IM Gateway repo
  - adapters
  - gateway runtime
  - session ingress
  - platform delivery
  - platform credential/config management
  - message normalization and reply formatting
- Engineer B: HarborNAS repo
  - `assistant_task_api`
  - business/task state machine
  - approval flow
  - artifact and audit persistence
  - notification intent generation
  - replacing direct IM send with IM Gateway delivery call

## Rollout Order
1. First, make IM Gateway call `POST /api/tasks` and map `TaskResponse` back to user replies.
2. Then, extract HarborNAS notification delivery behind the new HTTP notification interface.
3. Finally, remove HarborNAS direct IM platform delivery code so IM Gateway fully owns the IM layer.

## Minimum Test Cases
1. IM Gateway -> `POST /api/tasks` happy path with `message.message_id`, `chat_type`, `mentions`, `attachments`.
2. HarborNAS task resume path with `status=needs_input`, `prompt`, and `resume_token`.
3. Duplicate inbound retry with stable `trace_id` does not create duplicate business state transitions.
4. HarborNAS -> IM Gateway notification send happy path returns `provider_message_id`.
5. Notification retry with same `idempotency_key` does not duplicate end-user delivery.
6. HarborNAS build fails if direct platform credential usage remains in notification delivery path after full cutover.

## Release Gate
A release is allowed only when:
- both frozen interfaces have contract tests
- one real IM round-trip passes through `IM Gateway -> /api/tasks -> TaskResponse -> user reply`
- one real notification round-trip passes through `HarborNAS -> IM Gateway -> platform delivery`
- HarborNAS no longer depends on platform credentials for IM notification delivery
