# Local Model Backend Benchmark Gate

更新时间：2026-04-23

## 1. Purpose

这份文档只定义一件事：

- 在 HarborOS 单机上，什么时候允许把 `harbor-model-api` 的 backend
  从当前默认实现切到另一个本地 backend
- 当前权威 runtime gate 是 HarborOS `.182`
- Builder `.223` 只负责 build / prefetch / spawned benchmark 证据，不作为 live gate

这里冻结的不是具体推理框架，而是 **本地 OpenAI-compatible API seam**。

## 2. Frozen Boundary

- local OpenAI-compatible API seam is frozen
- HarborBeacon 继续只依赖：
  - `GET /healthz`
  - `POST /v1/chat/completions`
  - `POST /v1/embeddings`
- HarborBeacon 不因为 backend 变化而改 northbound seam：
  - `POST /api/tasks`
  - `POST /api/notifications/deliveries`
- HarborGate 不拥有模型选择权，也不承接 retrieval semantics

## 3. Backend Policy

- Candle is a preferred backend candidate, not the frozen contract.
- Mistral, sidecar, Candle, and future backends compete behind the same local OpenAI-compatible seam; none of them is the business-domain architecture.
- `4176` 继续运行默认 `openai_proxy` backend。
- 在没有通过 benchmark gate 之前，必须 keep `openai_proxy` as the default backend。
- 只有当某个 backend 通过同一套 benchmark gate，才允许把它升成默认实现。
- Cloud fallback is not backend promotion evidence. The product remains local-first; SiliconFlow may be used only as controlled fallback for `semantic.router` and `retrieval.answer`, and it must not be cited as the default local backend.
- Model prefetch/download jobs should record the Hugging Face endpoint used. Mirror priority is HarborDesk input -> `HF_ENDPOINT` -> `https://hf-mirror.com`.

## 4. Required Evidence

backend promotion 的证据固定来自 `benchmark-local-model-backend` 产出的
JSON 报告，而不是口头判断。

报告至少要覆盖：

- cold start
- healthz readiness
- Chinese chat probes
- embedding retrieval quality relative to a lexical baseline

## 5. How To Run

### Attached observation

这个模式用于观察一个已经在跑的模型服务，但它 **不构成 promotion 证据**，
因为不会测 cold start。

```bash
cargo run --bin benchmark-local-model-backend -- \
  --base-url http://127.0.0.1:4176/v1 \
  --healthz-url http://127.0.0.1:4176/healthz \
  --backend openai_proxy \
  --output /tmp/local-model-benchmark-openai-proxy.json
```

### Spawned promotion gate

这个模式才是正式 gate。它会自己拉起 `harbor-model-api`，测 cold start，
再跑 chat 和 embedding probes。

```bash
cargo run --bin benchmark-local-model-backend -- \
  --spawn-binary ./target/x86_64-unknown-linux-musl/release/harbor-model-api \
  --backend candle \
  --bind 127.0.0.1:4186 \
  --chat-model harbor-local-chat \
  --embedding-model harbor-local-embed \
  --output /tmp/local-model-benchmark-candle.json
```

如果 backend 依赖上游兼容服务，也可以显式传：

```bash
--upstream-base-url http://127.0.0.1:11434/v1
```

## 6. Promotion Rules

只有在下列条件全部满足时，报告里的 `gate.promotable` 才能为 `true`：

- cold start 在门限内完成
- `/healthz` 报告 ready
- `health.backend.kind` 与期望 backend 一致
- Chinese chat probes 全部通过
- embedding probes 维度稳定、非零
- embedding retrieval quality relative to a lexical baseline 有可见提升

如果任一项不满足：

- 不切默认 backend
- 不改 HarborBeacon 的模型 endpoint contract
- 不改 HarborGate 或 IM seam

### 6.1 Candle Side Lane On HarborOS

- HarborOS `.182` 上可以临时拉起 `candle` 旁路实验实例，例如 `127.0.0.1:4186`，
  但这条 lane 只用于候选验证，不直接改 live 默认 backend。
- 当前 Candle 候选基线固定为：
  - chat: `Qwen/Qwen3-1.7B`
  - embeddings: `jinaai/jina-embeddings-v2-base-zh`
- `.223` 上任何 spawned benchmark 红灯都只算兼容性 / 冷启动历史证据；
  是否允许进入 cutover rehearsal，只看 `.182` 的 target-runtime report。
- `Qwen3.5` 继续延期到后续 loader / backend 轮次，不进入本轮 gate。
- 这条 lane 的最小通过口径是：
  - `/healthz` 报告 `ok` / `ready=true`
  - `/v1/chat/completions` 通过中文探针
  - `/v1/embeddings` 返回稳定、非零、可归一化向量
- `4186` 只算 Candle candidate gate；`4176` 才是 live default `openai_proxy`
  通路。
- 只要 `4186` 没有同时通过 chat 和 embeddings gate，就不能把它算作
  `gate.promotable=true` 的证据，也不能据此把默认 backend 从 `openai_proxy`
  切到 `candle`。
- 一旦 `.182` 的 spawned report 通过，上线动作也仍然只允许进入
  `Candle cutover rehearsal`，不允许在同一轮直接改 `4176` 默认 backend。

## 7. Release Gate Integration

`evaluate-release-gate` 可以可选接收 benchmark 报告：

```bash
cargo run --bin evaluate-release-gate -- \
  drift-report.json \
  --require-live \
  --model-benchmark-report /tmp/local-model-benchmark-candle.json \
  --require-model-benchmark \
  --output release-gate-summary.json
```

这条门禁只在“要切默认 backend”的那一波 release 使用。
普通 release 继续可以不要求 model benchmark evidence。

## 8. What This Lane Does Not Change

- 不把 Candle 冻成唯一实现
- 不把 retrieval semantics、ranking、citation、answer ownership 下放到 HarborOS
- 不把 AIoT 设备 evidence contract 变成模型语义 contract
- 不把多模态 RAG 退化成“只换一个推理框架”
