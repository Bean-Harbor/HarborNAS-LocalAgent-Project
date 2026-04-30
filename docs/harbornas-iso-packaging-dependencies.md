# HarborBeacon / HarborGate / HarborDesk / HarborBot for HarborNAS ISO 依赖清单

面向：HarborNAS ISO 集成与打包维护者
日期：2026-04-30
状态：基于当前三个仓库的打包准备清单

当前参考 baseline：

- HarborBeacon: `a5f6da0`
- HarborGate: `57ff759`
- HarborNAS WebUI: `8e3f04d`
- `.82` RC2 release:
  `20260430-rc2-beacona5f6da0-gate57ff759`
- RC2 bundle sha256:
  `7119842506d38aac82c7e236b7f96a054244bb50be07c5e6b001ac7b0683484c`

## 1. 集成目标与边界

本清单用于把 HarborBeacon、HarborGate，以及 HarborNAS WebUI 中的 HarborDesk / HarborBot 产品面合并进 HarborNAS ISO。

当前按三个仓库集成：

| 仓库 | ISO 里承载的产品面 | 主要职责 |
|---|---|---|
| `HarborBeacon` | HarborBeacon 后端 runtime | 业务核心、Admin API、Task API、模型代理、审批、artifact、audit、知识库、设备配置状态 |
| `HarborGate` | HarborGate IM gateway | IM 适配器、平台 transport、route key、平台凭据、通知投递、绑定/配置入口 |
| `HarborNAS-webui` | HarborDesk / HarborBot UI | HarborNAS WebUI 内的原生 `/ui/harbordesk` 和 `/ui/harborbot` 页面 |

需要特别注意：

- HarborDesk 是 HarborNAS WebUI 内的原生运维/配置 UI，路径是 `/ui/harbordesk`。
- HarborBot 是 HarborNAS WebUI 内的原生用户检索 UI，路径是 `/ui/harborbot`。
- `HarborBeacon/frontend/harbordesk` 只是历史/过渡阶段的 API 校验壳，不应作为最终 HarborDesk 产品 UI 交付。
- HarborBeacon 与 HarborGate 当前 active contract 是 v2.0。服务间 active 请求必须使用：

```text
X-Contract-Version: 2.0
```

## 2. 运行时硬边界

ISO 打包可以把服务安装在同一系统内，但运行时边界必须保持独立：

- HarborBeacon 与 HarborGate 只通过 HTTP/JSON 合约通信。
- 两个仓库不能互相 import runtime code。
- 两个仓库不能共享 runtime state 文件。
- HarborBeacon 拥有 business state、conversation、continuation、approval、artifact、audit。
- HarborGate 拥有 IM adapter、transport、route key lifecycle、平台凭据、outbound delivery。
- HarborBeacon 不保存 IM 平台原始凭据。
- HarborBeacon 不直接投递 IM 消息。
- HarborGate 不解释 HarborBeacon business conversation 的内部语义。
- HarborGate 只能 opaque 保存 conversation handle 和 continuation values。
- HarborNAS WebUI 只通过 `/api/harbordesk/**` proxy 调 HarborBeacon Admin API。
- AIoT camera / LAN device 管理属于 Home Device Domain，不并入 HarborOS System Domain。

v2.0 禁止回退项：

- 不恢复 HarborGate active 调用 `/api/tasks`。
- 不恢复 public `args.resume_token` continuation。
- 不用 transport `session_id` 作为 HarborBeacon business conversation truth。
- 不把 IM 凭据长期归属放回 HarborBeacon。
- 不在本轮加入 group chat。
- 不引入 v1.5/v2.0 runtime dual-stack，除非架构决策明确反转。

## 3. 服务拓扑、端口与 systemd 单元

建议 ISO 预置以下服务：

| 服务 | 默认监听 | 必需性 | 说明 |
|---|---:|---|---|
| `agent-hub-admin-api.service` | `0.0.0.0:4174` | 必需 | HarborDesk / HarborBot / 管理面 API |
| `assistant-task-api.service` | `127.0.0.1:4175` | 必需 | HarborBeacon v2.0 turn / conversation / continuation API |
| `harbor-model-api.service` | `127.0.0.1:4176` | 建议 | OpenAI-compatible 本地模型代理 |
| `harborgate.service` | `127.0.0.1:8787` | 必需 | HarborGate IM Gateway |
| `harborgate-weixin-runner.service` | 无固定 HTTP | 可选启用 | Weixin long-poll runner |
| `harbor-vlm-sidecar.service` | `127.0.0.1:4196` | 可选 | VLM 图像描述 sidecar，默认可禁用 |

建议启动顺序：

```text
harbor-model-api
assistant-task-api
agent-hub-admin-api
harborgate
harborgate-weixin-runner
harbor-vlm-sidecar
```

HarborNAS WebUI 本身不新增 HarborDesk / HarborBot 后端服务；两者作为 HarborNAS WebUI 页面发布。

## 4. HarborNAS WebUI 集成

HarborDesk / HarborBot 的最终 UI 在 `HarborNAS-webui` 仓库：

| 页面 | WebUI 路径 | 说明 |
|---|---|---|
| HarborDesk | `/ui/harbordesk` | 管理员/运维配置台 |
| HarborBot | `/ui/harborbot` | 用户侧多模态检索入口 |

WebUI 构建要求：

```text
Node.js >= 24.13.1
Yarn 4.9.2
Angular 21.x
@truenas/ui-components ~0.1.12
```

生产构建命令：

```bash
yarn install --immutable
yarn build:prod
```

HarborNAS WebUI 需要为 HarborDesk 预留同源 proxy：

```text
/api/harbordesk/**
```

转发到 HarborBeacon Admin API：

```text
http://127.0.0.1:4174/api/**
```

当前开发 proxy 等价规则：

```text
/api/harbordesk/state -> http://127.0.0.1:4174/api/state
```

ISO / production 侧可以由 HarborNAS nginx、middleware proxy 或等价 WebUI proxy 实现，但必须保持 `/api/harbordesk/**` 与 HarborOS 原生 `/api/**` 分离，避免把 HarborBeacon admin API 混入 HarborOS middleware API namespace。

## 5. HarborDesk 功能清单

HarborDesk 是 HarborNAS WebUI 内的 operator/admin surface，不是独立服务。

当前需要 HarborBeacon Admin API 支撑的功能：

- Overview 状态：HarborOS principal、writable root、默认 CIDR、默认 camera、HarborBeacon connectivity。
- IM / Gateway 状态：平台状态、连接状态、Feishu / Weixin setup URLs、redacted connector status。
- 通知目标管理：读取、设置默认目标、删除目标。
- 硬件 readiness：CPU、memory、GPU/NPU、推荐模型 profile。
- RAG readiness：知识库能力、索引状态、阻塞项。
- 知识库设置：knowledge root、include/exclude、索引触发、索引 job 状态。
- 文件浏览：为知识库目录选择和校验提供后端支持。
- 模型配置：model endpoints、endpoint 测试、model policies。
- 本地模型：local catalog、download jobs、cancel download。
- HarborOS 状态：HarborOS user、writable root、system capability snapshot。
- IM capability map：WebUI 侧展示 HarborOS 与 IM 能力映射。
- AIoT / camera 管理：discovery scan、manual device add、default camera、device metadata。
- 设备凭据：保存设备凭据，UI 只展示 configured/redacted 状态。
- RTSP / snapshot：RTSP check、snapshot task、camera live/snapshot URLs。
- evidence / validation：device evidence、device validation run。
- share link：camera share-link create/revoke/list。

HarborDesk 典型 API 通过 WebUI proxy 调用：

```text
GET    /api/harbordesk/state
GET    /api/harbordesk/gateway/status
GET    /api/harbordesk/admin/notification-targets
POST   /api/harbordesk/admin/notification-targets/default
DELETE /api/harbordesk/admin/notification-targets/:id
GET    /api/harbordesk/hardware/readiness
GET    /api/harbordesk/rag/readiness
GET    /api/harbordesk/knowledge/settings
PUT    /api/harbordesk/knowledge/settings
POST   /api/harbordesk/knowledge/index/run
GET    /api/harbordesk/knowledge/index/status
GET    /api/harbordesk/files/browse
GET    /api/harbordesk/harboros/status
GET    /api/harbordesk/harboros/im-capability-map
GET    /api/harbordesk/models/endpoints
POST   /api/harbordesk/models/endpoints
PATCH  /api/harbordesk/models/endpoints/:id
POST   /api/harbordesk/models/endpoints/:id/test
GET    /api/harbordesk/models/policies
PUT    /api/harbordesk/models/policies
GET    /api/harbordesk/models/local-catalog
GET    /api/harbordesk/models/local-downloads
POST   /api/harbordesk/models/local-downloads
POST   /api/harbordesk/models/local-downloads/:job_id/cancel
POST   /api/harbordesk/discovery/scan
POST   /api/harbordesk/devices/manual
POST   /api/harbordesk/devices/default-camera
PATCH  /api/harbordesk/devices/:id
POST   /api/harbordesk/defaults
POST   /api/harbordesk/devices/:id/credentials
POST   /api/harbordesk/devices/:id/rtsp-check
GET    /api/harbordesk/devices/:id/evidence
POST   /api/harbordesk/devices/:id/validation/run
POST   /api/harbordesk/cameras/:id/share-link
POST   /api/harbordesk/share-links/:id/revoke
POST   /api/harbordesk/cameras/:id/snapshot
GET    /api/harbordesk/share-links
```

## 6. HarborBot 功能清单

HarborBot 是 HarborNAS WebUI 内的用户侧 multimodal retrieval 页面，不是独立后端服务。

WebUI 路径：

```text
/ui/harborbot
```

后端 API：

```text
POST /api/harbordesk/knowledge/search
GET  /api/harbordesk/knowledge/preview
```

检索能力：

- 支持 documents / images / videos waterfall results。
- 支持 filter：`all`、`images`、`text`、`videos`。
- 请求可控制 `limit`、`include_documents`、`include_images`、`include_videos`。
- 响应包含 `reply_pack`、`supported_modalities`、`pending_modalities`。
- 响应包含 `status`、`degraded`、`blockers`、`warnings`，用于 UI 展示 degraded mode。
- evidence 字段需要保留，用于证明命中来源不是 filename shortcut：
  - `content_source_kinds`
  - `content_indexed`
  - `content_match_used`
  - `filename_match_used`
- preview 通过 HarborBeacon Admin API 受控返回，不能让 WebUI 直接读取任意本地文件路径。

## 7. HarborBeacon 后端能力清单

HarborBeacon 需要作为 ISO 内的业务核心 runtime 发布。

核心 Rust binaries：

```text
agent-hub-admin-api
assistant-task-api
harbor-model-api
validate-contract-schemas
run-e2e-suite
```

建议一并打包的 release / 验证 binaries：

```text
run-drift-matrix
evaluate-release-gate
harborbeacon-agent
benchmark-local-model-backend
```

主要 HTTP surfaces：

| Surface | 地址 | 说明 |
|---|---|---|
| Admin API | `http://127.0.0.1:4174` 或 `0.0.0.0:4174` | HarborDesk / HarborBot / 管理面 API |
| Task API | `http://127.0.0.1:4175` | v2.0 `POST /api/turns` |
| Model API | `http://127.0.0.1:4176/v1` | OpenAI-compatible local model proxy |

Task API active endpoint：

```text
GET  /healthz
POST /api/turns
```

Task API 要求：

- `Authorization: Bearer <shared-token>`
- `X-Contract-Version: 2.0`
- 非 v2.0 contract version 应返回 contract mismatch。
- unknown route 使用 shared non-200 error envelope。

Admin API 支撑：

- HarborDesk state、account management、gateway status。
- release readiness 与 readiness history。
- hardware / RAG readiness。
- knowledge settings、search、preview、index run/status/jobs。
- files browse。
- HarborOS status 和 IM capability map。
- model endpoints、model policies、local catalog/downloads。
- feature availability。
- access members / roles / default delivery surface。
- tasks approvals approve/reject。
- notification targets。
- binding QR / setup mobile。
- camera live MJPEG、snapshot、share links、analyze、validation。
- discovery scan、manual device add、device credentials、RTSP check、device evidence。

## 8. HarborGate IM 能力清单

HarborGate 是 Python package，负责 IM transport 和 delivery。

Live adapters：

```text
feishu
weixin
webhook
```

Placeholder adapters：

```text
telegram
discord
slack
whatsapp
signal
email
wecom
```

核心命令：

```text
harborgate
harborgate-weixin-login
harborgate-weixin-runner
harborgate-weixin-ingress-probe
```

兼容旧命令别名：

```text
im-agent
im-agent-weixin-login
im-agent-weixin-runner
im-agent-weixin-ingress-probe
```

核心 HTTP routes：

```text
GET  /health
POST /messages/<platform>
POST /api/notifications/deliveries
GET  /api/gateway/status
GET  /setup/qr
GET  /setup/qr.svg
GET  /setup
GET  /api/setup/status
POST /api/setup/feishu/configure
```

Notification delivery endpoint：

```text
POST /api/notifications/deliveries
```

要求：

- active v2.0 traffic 使用 `X-Contract-Version: 2.0`。
- 如果设置 `IM_AGENT_SERVICE_TOKEN`，调用方必须使用 `Authorization: Bearer <token>`。
- route resolution 主要通过 `destination.route_key`。
- 没有 `route_key` 时只允许按 contract 定义 fallback 到 destination platform/id/recipient。
- delivery idempotency 使用 `delivery.idempotency_key`。
- gateway status 必须保持 redacted，不泄露平台凭据。

Feishu 能力：

- 默认 websocket / long-connection receive mode。
- 可选 webhook mode。
- 支持 `im.message.receive_v1` direct-message text。
- live send 由 `FEISHU_ENABLE_LIVE_SEND=1` 控制。
- setup portal 支持手机配置 app id / secret / verification token。
- group event gate 保持在 adapter 边界；本轮不承诺 group chat readiness。

Weixin 能力：

- QR login helper。
- iLink relay long-poll runner。
- private DM text inbound normalization。
- text outbound reply with cached `context_token`。
- ingress probe 区分 waiting-for-private-text 和 poll failure。
- 本轮不包含 group chat、image/file/voice send/receive、Weixin webhook mode。

可选 LLM backend：

```text
LLM_BASE_URL
LLM_API_KEY
LLM_MODEL
```

如果 HarborGate 直接启用 HarborBeacon Task API mode，必须确认代码已切到 v2.0 `/api/turns` client；旧的 task client mode 属于 release gate drift item。

## 9. 构建依赖

### HarborBeacon release bundle

推荐在 Linux builder 构建，不在 HarborOS 目标机上编译。

必需工具：

```text
Rust stable
rustup
cargo
python3
tar
sha256sum
find
file
```

推荐目标：

```text
x86_64-unknown-linux-musl
```

musl 静态构建额外需要：

```text
cargo-zigbuild
zig 0.15.1
```

`tools/build_release_bundle.sh` 当前会构建：

```text
harbor-model-api
assistant-task-api
agent-hub-admin-api
validate-contract-schemas
run-e2e-suite
```

### HarborGate Python package

Python 要求：

```text
python >= 3.11
```

runtime Python 依赖：

```text
cryptography >= 44
lark-oapi >= 1.5.3
qrcodegen >= 1.8
```

建议 ISO 构建阶段 vendor wheels 或 site-packages，避免目标机首次启动联网安装。

### HarborNAS WebUI

生产 WebUI 构建：

```bash
yarn install --immutable
yarn build:prod
```

HarborDesk / HarborBot 作为 HarborNAS WebUI dist 的一部分进入 ISO。不要把 `HarborBeacon/frontend/harbordesk` 当成 HarborNAS 最终 UI 构建来源。

## 10. 运行时系统包

建议 ISO 预装：

```text
systemd
python3 >= 3.11
python3-venv
python3-pip
ca-certificates
curl 或 wget
tar
sha256sum
ffmpeg
```

HarborOS control path 需要目标系统具备 HarborOS middleware / CLI 能力。HarborBeacon southbound 优先级保持：

```text
middleware API -> midcli -> browser -> MCP
```

`ffmpeg` 建议作为基础依赖，因为 RTSP snapshot、视频片段、媒体探测、camera evidence 都会用到。

## 11. 可选 AI / 多模态依赖

基础 ISO 不强制内置大模型，但建议预留模型目录和配置。

OpenAI-compatible LLM upstream 可选：

```text
Ollama
vLLM
llama.cpp server
LM Studio
任意 OpenAI-compatible API
```

默认 upstream 示例：

```text
http://127.0.0.1:11434/v1
```

VLM sidecar 可选依赖：

```text
torch
transformers
Pillow
HuggingFaceTB/SmolVLM-256M-Instruct
```

VLM 默认配置：

```text
HARBOR_VLM_SIDECAR_ENABLE=0
HARBOR_VLM_BIND=127.0.0.1:4196
HARBOR_VLM_MODEL_ID=HuggingFaceTB/SmolVLM-256M-Instruct
HARBOR_VLM_MODEL_PATH=<writable-root>/models/huggingfacetb-smolvlm-256m-instruct
HARBOR_VLM_DEVICE=cpu
HARBOR_VLM_LOCAL_FILES_ONLY=1
```

YOLO / vision bridge 可选依赖：

```text
ultralytics
opencv-python-headless
yolov8n.pt
```

如果 ISO 离线交付，建议提前 vendor：

- HarborGate Python wheels / site-packages。
- HarborBeacon Rust release binaries。
- HarborNAS WebUI production dist。
- `yolov8n.pt`，如果启用 YOLO。
- VLM model directory，如果启用 `harbor-vlm-sidecar`。

## 12. 安装目录与状态目录

建议默认目录：

```text
/var/lib/harborbeacon-agent-ci
/var/lib/harborbeacon-agent-ci/current
/var/lib/harborbeacon-agent-ci/runtime
/var/lib/harborbeacon-agent-ci/logs
/var/lib/harborbeacon-agent-ci/captures
/mnt/software/harborbeacon-agent-ci
/mnt/software/harborbeacon-models
```

用途：

| 路径 | 用途 |
|---|---|
| `/var/lib/harborbeacon-agent-ci` | exec-capable install root |
| `/var/lib/harborbeacon-agent-ci/current` | 当前 release symlink |
| `/var/lib/harborbeacon-agent-ci/runtime` | runtime state 根目录 |
| `/var/lib/harborbeacon-agent-ci/logs` | 服务日志辅助目录 |
| `/var/lib/harborbeacon-agent-ci/captures` | snapshot / video / artifact capture |
| `/mnt/software/harborbeacon-agent-ci` | HarborOS writable root |
| `/mnt/software/harborbeacon-models` | 模型缓存目录 |

状态隔离要求：

- HarborBeacon admin state、device registry、task conversations 独立保存。
- HarborGate sessions、platform credential state、Weixin state 独立保存。
- HarborNAS WebUI 不直接读写这些 state 文件，只通过 HTTP API。

## 13. 环境变量与凭据归属

建议环境文件：

```text
/etc/default/harborbeacon-agent-hub
```

核心配置：

```text
WORKSPACE_ROOT=/var/lib/harborbeacon-agent-ci/current
HARBOR_HTTP_BIND=0.0.0.0:4174
HARBOR_PUBLIC_ORIGIN=http://harborbeacon.local:4174
HARBORDESK_DIST=/var/lib/harborbeacon-agent-ci/current/harbordesk/dist/harbordesk
HARBOR_HARBOROS_USER=<service-user>
HARBOR_HARBOROS_WRITABLE_ROOT=/mnt/software/harborbeacon-agent-ci
HARBOR_KNOWLEDGE_INDEX_ROOT=/mnt/software/harborbeacon-agent-ci/knowledge-index

HARBOR_TASK_API_BIND=127.0.0.1:4175
HARBOR_TASK_API_URL=http://127.0.0.1:4175
HARBOR_TASK_API_ADMIN_STATE=/var/lib/harborbeacon-agent-ci/runtime/admin-console.json
HARBOR_TASK_API_DEVICE_REGISTRY=/var/lib/harborbeacon-agent-ci/runtime/device-registry.json
HARBOR_TASK_API_CONVERSATIONS=/var/lib/harborbeacon-agent-ci/runtime/task-api-conversations.json
HARBOR_TASK_API_BEARER_TOKEN=<shared-token>

HARBOR_MODEL_API_BIND=127.0.0.1:4176
HARBOR_MODEL_API_BASE_URL=http://127.0.0.1:4176/v1
HARBOR_MODEL_API_TOKEN=<shared-token>
HARBOR_MODEL_API_BACKEND=openai_proxy
HARBOR_MODEL_API_UPSTREAM_BASE_URL=http://127.0.0.1:11434/v1
HARBOR_MODEL_API_CHAT_MODEL=harbor-local-chat
HARBOR_MODEL_API_EMBEDDING_MODEL=harbor-local-embed

HARBORGATE_BASE_URL=http://127.0.0.1:8787
HARBORGATE_BEARER_TOKEN=<shared-token>
IM_AGENT_SERVICE_TOKEN=<shared-token>
IM_AGENT_CONTRACT_VERSION=2.0
IM_AGENT_HOST=127.0.0.1
IM_AGENT_PORT=8787
IM_AGENT_DATA_DIR=/var/lib/harborbeacon-agent-ci/runtime/harborgate/sessions
IM_AGENT_STATE_DIR=/var/lib/harborbeacon-agent-ci/runtime/harborgate
IM_AGENT_PUBLIC_ORIGIN=http://harborbeacon.local:8787
WEIXIN_STATE_DIR=/var/lib/harborbeacon-agent-ci/runtime/harborgate/weixin

HARBORBEACON_TASK_API_URL=http://127.0.0.1:4175
HARBORBEACON_TASK_API_TOKEN=<shared-token>
HARBORBEACON_ADMIN_API_URL=http://127.0.0.1:4174
HARBORBEACON_ADMIN_API_TOKEN=<shared-token>

HARBOR_RELEASE_INSTALL_ROOT=/var/lib/harborbeacon-agent-ci
HARBOR_LOG_ROOT=/var/lib/harborbeacon-agent-ci/logs
HARBOR_CAPTURE_ROOT=/var/lib/harborbeacon-agent-ci/captures
```

IM 平台凭据只属于 HarborGate。

Feishu 可选配置：

```text
FEISHU_APP_ID=<feishu-app-id>
FEISHU_APP_SECRET=<feishu-app-secret>
FEISHU_CONNECTION_MODE=websocket
FEISHU_ENABLE_LIVE_SEND=1
FEISHU_VERIFICATION_TOKEN=<optional-token>
FEISHU_ENCRYPT_KEY=<optional-key>
```

Weixin 可选配置：

```text
WEIXIN_STATE_DIR=/var/lib/harborbeacon-agent-ci/runtime/harborgate/weixin
WEIXIN_ACCOUNT_ID=<account-id>
WEIXIN_BOT_TOKEN=<bot-token>
WEIXIN_BASE_URL=https://ilinkai.weixin.qq.com
```

首次启用 Weixin 通常需要：

```bash
harborgate-weixin-login
systemctl restart harborgate-weixin-runner.service
```

## 14. Release bundle 建议内容

建议 HarborBeacon / HarborGate release bundle：

```text
bin/
  agent-hub-admin-api
  assistant-task-api
  harbor-model-api
  validate-contract-schemas
  run-e2e-suite

harborgate/
  site-packages/

templates/
  bin/
    harbor-agent-hub-helper
    harborgate
    harborgate-weixin-login
    harborgate-weixin-runner
    harborgate-weixin-ingress-probe
    run-agent-hub-admin-api
    run-assistant-task-api
    run-harbor-model-api
    run-harbor-vlm-sidecar
  systemd/
    agent-hub-admin-api.service.template
    assistant-task-api.service.template
    harbor-model-api.service.template
    harbor-vlm-sidecar.service.template
    harborgate.service.template
    harborgate-weixin-runner.service.template
  harborbeacon-agent-hub.env.template

install/
  install_harboros_release.sh
  rollback_harboros_release.sh

manifest.json
checksums.sha256
```

HarborNAS WebUI dist 建议由 HarborNAS ISO 自身 WebUI 打包流程产出，包含：

```text
/ui/harbordesk
/ui/harborbot
```

如果继续使用 `tools/build_release_bundle.sh`，注意它当前仍支持 `HARBORDESK_DIST_SOURCE` / `frontend/harbordesk` 过渡路径。ISO 正式集成时应优先使用 HarborNAS WebUI production dist，并把旧 `frontend/harbordesk` 标注为 legacy validation shell。

## 15. 安装与启停建议

安装脚本需要完成：

- 校验 `checksums.sha256`。
- 解包到 `/var/lib/harborbeacon-agent-ci/releases/<version>`。
- 更新 `/var/lib/harborbeacon-agent-ci/current` symlink。
- 写入 `/etc/default/harborbeacon-agent-hub`。
- 安装 systemd units。
- `systemctl daemon-reload`。
- enable core services。
- 默认启动 core services。
- `harborgate-weixin-runner` 可安装但不强制启动，直到 Weixin 已登录并配置 account id。
- `harbor-vlm-sidecar` 默认安装但 disabled，直到 `HARBOR_VLM_SIDECAR_ENABLE=1`。

Core services：

```text
harbor-model-api.service
assistant-task-api.service
agent-hub-admin-api.service
harborgate.service
```

Optional services：

```text
harborgate-weixin-runner.service
harbor-vlm-sidecar.service
```

## 16. 安装后验证命令

基础服务状态：

```bash
systemctl status harbor-model-api.service
systemctl status assistant-task-api.service
systemctl status agent-hub-admin-api.service
systemctl status harborgate.service
```

如果安装了 helper：

```bash
/var/lib/harborbeacon-agent-ci/bin/harbor-agent-hub-helper status
/var/lib/harborbeacon-agent-ci/bin/harbor-agent-hub-helper health
/var/lib/harborbeacon-agent-ci/bin/harbor-agent-hub-helper logs gateway --lines 120
```

基础 HTTP health：

```bash
curl http://127.0.0.1:4174/healthz
curl http://127.0.0.1:4175/healthz
curl http://127.0.0.1:4176/healthz
curl http://127.0.0.1:8787/health
```

HarborGate status：

```bash
curl http://127.0.0.1:8787/api/gateway/status \
  -H "X-Contract-Version: 2.0" \
  -H "Authorization: Bearer <shared-token>"
```

Task API v2.0 smoke：

```bash
curl -X POST http://127.0.0.1:4175/api/turns \
  -H "Content-Type: application/json" \
  -H "X-Contract-Version: 2.0" \
  -H "Authorization: Bearer <shared-token>" \
  --data @turn-smoke.json
```

HarborDesk WebUI smoke：

```text
打开 /ui/harbordesk
确认页面请求 /api/harbordesk/state
确认后端实际转发到 http://127.0.0.1:4174/api/state
确认 Gateway、Knowledge、Models、Devices 面板不出现 proxy 404
```

HarborBot WebUI smoke：

```text
打开 /ui/harborbot
确认页面请求 /api/harbordesk/knowledge/search
确认后端实际转发到 http://127.0.0.1:4174/api/knowledge/search
确认返回结果包含 documents / images / videos / reply_pack
确认 degraded / blockers / warnings 能在 UI 中呈现
```

Release gate 工具：

```bash
validate-contract-schemas
run-e2e-suite
run-drift-matrix
evaluate-release-gate
```

## 17. v2.0 Release Gate

ISO 集成验收时必须确认：

- HarborGate active path 不再调用 `/api/tasks`。
- Active service-to-service traffic 使用 `X-Contract-Version: 2.0`。
- Active request builder 不再发出 public `args.resume_token`。
- HarborBeacon 不把 transport `session_id` 当作 business conversation truth。
- HarborGate 不解析 HarborBeacon `active_frame.kind` 来做业务路由。
- HarborBeacon 不保存 IM 原始平台凭据。
- HarborBeacon 不直接投递 IM 消息。
- HarborDesk `/ui/harbordesk` 通过 `/api/harbordesk/**` 访问 HarborBeacon Admin API。
- HarborBot `/ui/harborbot` 通过真实 `/api/harbordesk/knowledge/search` 获取索引结果。
- HarborNAS WebUI `/api/harbordesk/**` proxy 不覆盖 HarborOS 原生 `/api/**`。
- Weixin group chat 不出现在本轮 readiness claim 中。

RC2 live smoke 参考：

- `GET /ui/harbordesk` -> `200`
- `GET /ui/harborbot` -> `200`
- `POST /api/harbordesk/knowledge/search` query `春天的照片` -> one VLM
  content-indexed image, `filename_match_used=false`
- `GET /api/harbordesk/knowledge/preview` for that image -> `image/jpeg`
- protected `POST /api/turns` content retrieval -> `turn.status=completed`
- protected `POST /api/turns` local-first architecture explanation ->
  `turn.status=completed`

## 18. Local-First Promotion Gate

ISO 默认可以保留 `HARBOR_MODEL_API_BACKEND=openai_proxy`，但产品说法必须明确：

- 默认策略是 local-first。
- cloud 只有在 privacy/resource policy 放行时作为受控 fallback。
- SiliconFlow 是当前 `.82` fallback proof，不是默认架构。

只有当 `.82` local model benchmark report 里的 `gate.promotable=true`，才允许规划把默认 backend 切到 `candle` 的单独 cutover rehearsal。否则 ISO 应保留 openai-compatible seam 和 fallback policy，不把 local model 写成已默认启用。

## 19. 已知风险与需要 HarborNAS owner 确认的问题

已知风险：

- HarborGate README 标注：如果启用 `HARBORBEACON_TASK_API_URL`，必须确认代码已从历史 task client 切到 v2.0 `/api/turns` client；否则属于 release gate blocker。
- `tools/build_release_bundle.sh` 仍保留旧 `frontend/harbordesk` 构建路径；正式 ISO 应以 `HarborNAS-webui` 的 `/ui/harbordesk` 和 `/ui/harborbot` 为准。
- VLM / YOLO / 本地 LLM 模型较大，是否内置会显著影响 ISO 体积。
- `ffmpeg` 对 camera / video / snapshot 能力很关键，不建议省略。
- `/mnt/software` 是否稳定可写会影响 writable root 默认值；如果不可写，安装脚本需要 fallback 到 `<install-root>/writable`。

需要 HarborNAS owner 决策：

- ISO 是否接受预编译 Rust 静态二进制 release bundle，而不是目标机编译。
- ISO 是否接受 vendor Python site-packages / wheels。
- production `/api/harbordesk/**` proxy 由 nginx、middleware proxy，还是 HarborNAS WebUI 服务层实现。
- HarborGate Feishu setup portal / QR onboarding 是否直接暴露给 LAN 手机访问。
- Weixin runner 是否默认安装并 disabled，还是仅在用户开启 Weixin 后安装。
- 是否默认内置 YOLO / VLM 模型，还是首启后按需下载。
- HarborNAS WebUI 是否把 HarborDesk / HarborBot 放入默认导航，还是 feature flag 控制。
