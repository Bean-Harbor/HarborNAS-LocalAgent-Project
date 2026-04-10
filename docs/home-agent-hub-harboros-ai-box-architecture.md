# Home Agent Hub 架构调整建议

更新时间：2026-04-10

## 1. 目标更新

项目目标从“HarborNAS/HarborOS 上的本地智能代理”升级为“双形态运行的家庭智能中枢”：

1. 作为 HarborOS 内置能力运行，成为 HarborOS 的设备控制与 AI 交互子系统。
2. 作为独立的 ARM / X86 AI BOX 部署，在局域网内承担家庭设备发现、协议适配、事件处理和本地 AI 推理。

这意味着项目不再只是一个“调用 HarborOS 能力的 Agent”，而是要演进为：

- 一个可嵌入 HarborOS 的 Home Hub Runtime
- 一个可独立部署的 Edge AI Runtime
- 一套统一的设备能力模型与 Agent 编排框架

---

## 2. 总体判断

当前仓库的方向是对的，不建议推倒重来。

现有三个基础非常适合继续扩展：

1. `HarborBeacon`
   - 已经承担 IM 接入、会话、审批、附件解析、工具调用入口。
   - 未来可以继续作为“统一人机入口”。

2. `orchestrator`
   - 已经具备路由、审批、审计、执行器抽象。
   - 未来可以从“HarborOS 运维编排”扩展到“家庭设备 + HarborOS 混合编排”。

3. Rust runtime
   - 很适合作为 HarborOS 内置 daemon 或 AI BOX 主进程。
   - 对 ARM / X86 跨平台部署也更友好。

真正要调整的，不是入口层，而是“域模型”和“执行平面”。

---

## 3. 新的产品形态

建议把系统明确拆成两种部署形态，但共用一套核心框架。

### 3.1 HarborOS Embedded Mode

运行位置：
- HarborOS 主机内

角色：
- HarborOS 的 Home Agent 子系统
- 可直接访问本机文件、媒体库、用户账户、HarborOS 服务能力

特点：
- 与 HarborOS WebUI、权限体系、存储体系深度集成
- 适合 NAS + 家庭中枢一体机场景

### 3.2 AI BOX Standalone Mode

运行位置：
- ARM 或 X86 AI BOX

角色：
- 局域网家庭设备控制中心
- 本地 AI 推理节点
- 通过 API / IM / WebUI 对外提供统一入口

特点：
- 可以单独接入摄像头、灯、传感器、门锁等设备
- 可以选择性挂载 HarborOS 作为存储与数据底座

### 3.3 Hybrid Mode

运行位置：
- AI BOX 负责设备实时控制与轻量推理
- HarborOS 负责存储、媒体归档、知识库、重任务处理

这是最推荐的目标形态。

因为它符合家庭场景的真实需求：
- 设备控制要靠近局域网
- 视频与媒体要本地存储
- 编排与 AI 能力要统一

---

## 4. 建议的总架构

建议从当前“三层”升级为“五层”。

```text
[User Entry Layer]
IM / WebUI / Mobile / Voice
        |
[Interaction Gateway]
HarborBeacon
        |
[Agent Orchestration Layer]
Intent / Planner / Policy / Audit / Tool Router
        |
[Home Runtime Layer]
Device Registry / Automation Engine / Event Bus / Media Pipeline
        |
[Execution Layer]
Device Adapters / HarborOS Connectors / Local AI Inference / Cloud Fallback
```

### 4.1 User Entry Layer

保留现在的 IM 优势，但不要只绑定 IM。

建议入口统一支持：
- Feishu / 企业微信 / Telegram 等 IM
- HarborOS WebUI
- AI BOX 本地 Web 控制台
- 后续语音入口

### 4.2 Interaction Gateway

继续由 `HarborBeacon` 承担：
- 消息接入
- 身份映射
- 会话管理
- 审批流
- 富媒体回包

但它的定位应从“IM 机器人”升级成“统一交互网关”。

### 4.3 Agent Orchestration Layer

继续基于当前 `orchestrator`，但要从“任务执行器”升级成“家庭智能编排器”。

需要新增三类能力：
- 设备语义理解：把“看看门口”映射到具体摄像头和动作
- 场景编排：把“有人就开灯通知我”映射成事件规则
- 多节点路由：决定动作落在 HarborOS、AI BOX、本地模型还是云

### 4.4 Home Runtime Layer

这是当前仓库最缺失、但最关键的一层。

建议新增一个独立子系统，负责：
- 设备发现
- 设备注册与能力建模
- 状态缓存
- 事件订阅
- 自动化规则执行
- 流媒体处理

这层应该成为项目的新核心。

### 4.5 Execution Layer

底层执行按四类划分：

1. Device Adapters
   - ONVIF
   - RTSP
   - mDNS / SSDP
   - Matter
   - 厂商私有协议

2. HarborOS Connectors
   - Middleware API
   - MidCLI
   - 本地文件系统 / 媒体库接口

3. Local AI Inference
   - VLM / ASR / OCR / Detection
   - 轻量策略模型

4. Cloud Fallback
   - 复杂推理、摘要、跨模态问答

---

## 5. 框架调整重点

## 5.1 先调整“域模型”，不要先堆功能

当前项目的核心域还是：
- `service`
- `files`
- `weather`
- `photo`

这适合 MVP，但不够支撑 Home Agent Hub。

建议扩成以下领域：

### A. `device`

统一设备实体：
- `device.discover`
- `device.list`
- `device.get`
- `device.control`
- `device.snapshot`
- `device.stream.open`
- `device.ptz`

### B. `scene`

面向家庭场景与房间语义：
- `scene.resolve`
- `scene.list`
- `scene.activate`

例如：
- “门口摄像头”
- “客厅灯”
- “老人房”

### C. `automation`

事件驱动自动化：
- `automation.create_rule`
- `automation.enable_rule`
- `automation.disable_rule`
- `automation.test_rule`

### D. `vision`

摄像头 AI 分析能力：
- `vision.detect_person`
- `vision.detect_motion`
- `vision.describe_frame`
- `vision.search_event`

### E. `media`

视频与截图资产：
- `media.snapshot.save`
- `media.clip.export`
- `media.timeline.query`

### F. `system`

保留原有 HarborOS / 主机系统操作：
- `system.service.*`
- `system.files.*`

也就是说，原来的 `service` / `files` 不是废弃，而是下沉成 `system` 域的一部分。

## 5.2 路由策略要从“单主机”变成“多执行面”

当前路由优先级是：

`Middleware API -> MidCLI -> Browser -> MCP`

这个优先级对 HarborOS 域仍然成立，但对 Home Hub 不够。

建议调整成“分域路由”：

### HarborOS System Domain

继续保持：

`Middleware API -> MidCLI -> Browser -> MCP`

### Home Device Domain

建议改为：

`Native Adapter -> LAN Bridge -> HarborOS Connector -> Cloud/MCP`

含义：
- 设备控制优先走本地协议适配器
- 设备不应先绕 HarborOS CLI
- HarborOS 更适合承担存储、权限和管理平台角色

## 5.3 增加 Device Registry

建议新增统一设备注册中心，保存：
- `device_id`
- `kind`
- `vendor`
- `model`
- `protocol`
- `capabilities`
- `location`
- `auth_config_ref`
- `last_seen_at`
- `health_status`

推荐最初用 SQLite：
- 对 ARM / X86 友好
- 足够支撑单家庭场景
- 便于后续嵌入 HarborOS

## 5.4 增加 Event Bus

家庭自动化离不开事件流。

建议新增统一事件模型：
- 设备上线/离线
- 移动检测
- 人形检测
- 门磁触发
- 自动化执行结果
- 用户确认事件

第一阶段不建议上重消息系统。

推荐：
- 进程内事件总线
- SQLite 持久化事件表
- 需要跨进程时再引入 NATS 或 Redis Streams

## 5.5 AI 推理层做成 Sidecar，而不是塞进主流程

如果要兼容 ARM 和 X86，不建议把所有 AI 能力硬编码进主 runtime。

建议分成两层：

1. Core Runtime（Rust）
   - 设备发现
   - 规则引擎
   - 事件分发
   - 编排与审计

2. AI Sidecar（Python / model service）
   - 图像描述
   - 人形检测
   - OCR
   - ASR
   - 可选本地 LLM / VLM

这样做的好处：
- Rust 核心更稳
- 模型依赖隔离
- ARM 和 X86 可以按硬件能力替换 sidecar

---

## 6. 代码结构调整建议

在不推翻现有仓库的前提下，建议逐步演进到下面的结构：

```text
harborbeacon/                # 统一交互网关
src/
  orchestrator/              # 任务编排、策略、审批、审计
  planner/
  skills/
  home_runtime/              # 新增：家庭设备运行时核心
    registry/                # 设备注册中心
    discovery/               # ONVIF / mDNS / SSDP / Matter 发现
    adapters/                # 设备协议适配层
    automation/              # 规则引擎
    events/                  # 事件模型与事件总线
    media/                   # 截图、流、片段处理
    topology/                # 房间、区域、设备关系
  connectors/                # HarborOS / AI sidecar / cloud 接口
  domains/                   # 新的领域动作定义
    device/
    scene/
    automation/
    vision/
    media/
    system/
```

### 6.1 `harborbeacon/`

保留现有结构，但增加：
- WebUI / 本地配网页能力
- 设备绑定入口
- 二维码绑定流程
- 多 Hub / 多节点目标选择

### 6.2 `src/orchestrator/`

保留现有审批、审计、路由框架，新增：
- 分域路由策略
- 场景解析
- 自动化计划生成
- 多节点执行目标选择

### 6.3 `src/home_runtime/`

这是新主角。

建议优先做下面几个模块：
- `registry`
- `discovery`
- `adapters/onvif`
- `events`
- `automation`

第一阶段先把摄像头链路打通，再扩其他设备。

---

## 7. 部署建议

## 7.1 三种运行配置

建议不要只有一个部署方案，而是定义三个 profile。

### Profile A: `harboros-lite`

适合：
- HarborOS 主机直接内置
- 不跑重模型

组件：
- HarborBeacon
- Orchestrator
- Home Runtime
- HarborOS Connector

### Profile B: `aibox-standard`

适合：
- 独立 ARM / X86 盒子
- 本地轻量视觉能力

组件：
- HarborBeacon
- Orchestrator
- Home Runtime
- AI Sidecar
- 本地 Web Console

### Profile C: `hybrid-cluster`

适合：
- AI BOX + HarborOS 协同

组件分工：
- AI BOX：发现设备、执行实时控制、处理事件
- HarborOS：存储媒体、管理规则、长期索引、重任务 AI

## 7.2 ARM / X86 的设计原则

为了兼容 ARM 与 X86，建议坚持：

1. 主 runtime 尽量 Rust 静态编译
2. 模型能力做成可插拔 sidecar
3. 媒体处理尽量依赖 ffmpeg / gstreamer 这类成熟组件
4. 数据层优先 SQLite，避免一开始引入过重中间件
5. 配置与插件体系统一，避免不同架构维护两套逻辑

---

## 8. MVP 重排建议

当前仓库锚点是：
- 飞书上传照片
- 天气查询

这两个 case 仍然有价值，但不足以验证新的 Home Hub 目标。

建议改成新的三阶段 MVP。

## 阶段 1：Home Hub 基础闭环

目标：
- AI BOX / HarborOS 能自动发现一个摄像头
- 用户通过 IM 绑定 Hub
- 用户说“看看门口”
- 系统返回截图

最小能力：
- ONVIF 发现
- Camera 设备注册
- `device.snapshot`
- HarborBeacon 回图

## 阶段 2：可控摄像头闭环

目标：
- 用户说“往左转”
- 摄像头 PTZ 执行

最小能力：
- `device.ptz`
- 审批策略
- 执行日志

## 阶段 3：自动化闭环

目标：
- 用户说“有人出现就通知我并开灯”

最小能力：
- 事件订阅
- 简单规则引擎
- 灯控制适配器
- IM 通知

原有两个 case 的建议位置：
- `photo.upload_to_nas` 继续保留，作为 HarborOS 媒体归档能力
- `weather.query` 下沉成演示型非核心能力，不再作为主锚点

---

## 9. 实施顺序建议

建议按下面顺序推进，而不是同时做很多协议和模型。

### Step 1

先把领域动作重新命名和分层：
- 引入 `device` / `automation` / `vision` / `media` / `system`

### Step 2

补 `home_runtime` 骨架：
- registry
- discovery
- events
- adapters/onvif

### Step 3

打通“发现摄像头 -> 截图 -> IM 返回”

### Step 4

再补 PTZ 与事件规则

### Step 5

最后再补本地视觉 sidecar

---

## 10. 一句话结论

这个项目应该从“HarborOS 上的本地 Agent”升级为“HarborOS / AI BOX 双形态的家庭设备智能中枢”。

框架上最重要的调整不是换技术栈，而是：

1. 把 `orchestrator` 从系统运维编排扩成家庭场景编排。
2. 新增 `home_runtime` 作为设备发现、注册、事件和自动化核心。
3. 把 HarborOS 连接器和设备连接器分开。
4. 把 AI 能力做成 sidecar，保证 ARM / X86 可落地。
5. 把 MVP 主线改成“摄像头发现与控制”，而不是继续以天气查询为主。
