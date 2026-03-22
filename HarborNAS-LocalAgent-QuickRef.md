# HarborNAS Local Agent V2 快速参考

## 1. 一句话目标

在 HarborOS 控制平面上落地个人助手，提供多模态 RAG 与智能编排能力。

---

## 2. V2 总体框图

```text
┌─────────────────────────────────────────────────────────────────┐
│ Multi-terminal Access                                           │
│ WebUI Chat | Mobile PWA | Desktop Client | API Gateway         │
└───────────────────────────────┬─────────────────────────────────┘
                                │
┌───────────────────────────────▼─────────────────────────────────┐
│ Assistant Orchestration                                          │
│ Intent Parse | Planner(DAG) | Skill Router | Policy Gate        │
└───────────────┬───────────────────────┬─────────────────────────┘
                │                       │
                │ route priority        │ audit/events
                │                       │
┌───────────────▼─────────────────────────────────────────────────┐
│ Executors (strict order)                                         │
│ 1) MiddlewareExecutor 2) MidCLIExecutor 3) Browser 4) MCP        │
└───────────────┬─────────────────────────────────────────────────┘
                │
┌───────────────▼─────────────────────────────────────────────────┐
│ HarborOS Capabilities + RAG Runtime                              │
│ system/file ops | multimodal retrieval | vector store | models   │
└───────────────┬─────────────────────────────────────────────────┘
                │
┌───────────────▼─────────────────────────────────────────────────┐
│ Governance & Observability                                       │
│ contract tests | e2e | drift matrix | release gate | replay      │
└─────────────────────────────────────────────────────────────────┘
```

---

## 3. 路由规则（必须遵守）

```text
if API route available: use API
elif MidCLI route available: use MidCLI
elif Browser route available: use Browser
elif MCP route available: use MCP
else: fail with explicit reason

if risk_level in [HIGH, CRITICAL]:
  require approval
  prefer dry-run preview first
```

---

## 4. 多模态 RAG 框图

```text
Sources
  ├─ Text/Docs
  ├─ Image
  ├─ Audio
  └─ Video
       │
       ▼
Ingestion Pipeline
  ├─ parse/chunk
  ├─ modality embedding
  ├─ metadata normalization
  └─ index write
       │
       ▼
Hybrid Retrieval
  ├─ dense vector search
  ├─ sparse lexical search
  ├─ metadata filter
  └─ rerank
       │
       ▼
Context Builder -> Assistant Answer + Citation + Audit
```

---

## 5. 核心任务对象（统一）

```json
{
  "task_id": "uuid",
  "trace_id": "uuid",
  "intent": "user natural language intent",
  "plan": [{"step_id": "s1", "domain": "system", "operation": "restart"}],
  "route_priority": ["middleware_api", "midcli", "browser", "mcp"],
  "executor_used": "middleware_api",
  "risk_level": "HIGH",
  "requires_confirmation": true,
  "status": "executing"
}
```

---

## 6. 团队分工速记

- A（编排/架构）：Planner、路由、策略门禁
- B（平台/数据）：Session、Skills Runtime、RAG 管道
- C（可靠性/安全）：审计、观测、发布门禁、回归保障

---

## 7. 每周最低交付要求

1. 至少一条端到端可运行链路
2. 新增能力必须有 contract + regression test
3. 关键指标快照（成功率、P95、fallback ratio）
4. 风险项明确负责人和截止时间
