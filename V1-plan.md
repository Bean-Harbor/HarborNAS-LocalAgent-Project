# V1-plan：Release-v1 真人测试闭环（唯一真源 + 执行清单）

## Summary

- 这份 `V1-plan` 是 release-v1 的唯一真源。后续执行必须以它为准，不允许各 lane 自行扩 scope、改边界或偷换目标。
- 本轮目标是交付一个可给真人测试的闭环，用来验证整个项目是否已经真正可用：
  - HarborOS 同机安装 HarborBeacon / HarborDesk / HarborGate
  - HarborDesk 作为唯一配置入口
  - HarborDesk 账号复用 HarborOS 当前登录账号
  - 用户从微信用真实自然语义触发：
    - 抓拍
    - 录一段短视频
    - 查询指定目录里符合特征的照片或视频
  - 媒体文件落到 HarborOS 当前 writable root 下的配置子目录
  - 图片和视频关键帧在落盘时立即进入 OCR/VLM 理解与知识索引
  - 结果和后续对话继续 source-bound 回微信
- 本轮严格约束：
  - 不为了这个 use case 新增新的框架对象
  - 不新增新的 cross-repo 接口
  - 不新增新的 use-case 专用 admin API
  - use case 只能建立在现有框架对象的编排上：
    - `Account Management`
    - `Device Registry / CameraProfile`
    - `RecordingPolicy`
    - `AdminDefaults`
    - `Model Center route policies`
    - `HarborOS writable root / knowledge roots`
- 参考形态锁定为：
  - `主参考 = OpenClaw`
  - `次参考 = Hermes Agent`
  - 落地方式 = OpenClaw 风格的 `agentic interpreter/planner`，但它只能调用 Harbor 现有框架里的既有业务能力

## Frozen Boundaries

- HarborGate -> HarborBeacon 只走 `POST /api/tasks`
- HarborBeacon -> HarborGate 只走 `POST /api/notifications/deliveries`
- 不改 `TaskRequest / TaskResponse`、`message`、`source.route_key`、`args.resume_token`、`X-Contract-Version: 1.5`
- `interactive reply surface = source-bound`
- `proactive delivery surface = user-default-configured`
- HarborBeacon 不恢复 direct platform send，不接管 IM 原始凭据
- HarborOS 继续 `Middleware API -> MidCLI -> Browser/MCP fallback`
- AIoT ownership 不并入 HarborOS
- HarborDesk 不新造独立后台账号体系；release-v1 采用：
  - `HarborOS 当前登录账号 = HarborDesk/admin principal`
- release-v1 不做：
  - HarborOS 完整多用户映射
  - audio / full video understanding
  - 成本治理 / 大规模评测平台
  - 新的 use-case profile / scene object / workflow object

## Execution Checklist

### A. `harbor-framework`

- 把 `general.message` 接入 OpenClaw 风格的 agentic interpreter：
  - 输入是微信自然语义
  - 输出是受约束的内部执行计划
  - 只允许落到既有能力族：
    - camera snapshot
    - recording/media 短视频链
    - knowledge search
- 不新增 `MediaCaptureProfile` 或等价对象
- release-v1 所需的附加参数只允许落在现有对象已有承载位：
  - `RecordingPolicy.clip_length_seconds`
  - `RecordingPolicy.metadata`
  - 现有 model policy metadata
  - 现有 defaults/state 投影
- 补齐短视频主链：
  - 录制结果持久化为既有 media/asset 语义
  - 视频关键帧进入 OCR/VLM ingest
  - 视频检索通过关键帧 sidecar 进入既有 knowledge 结果
- 落实 HarborOS 账号即后台账号：
  - 使用现有 `AuthSource::HarborOs`
  - 让 HarborDesk/admin principal 可复用 HarborOS 当前用户
  - 继续进入既有 `authorize_access(...)` 与 role 判断

### B. `harbor-admin-webui`

- HarborDesk 继续作为唯一配置入口，不新增 use-case 后端对象
- 用现有页面和现有读写面组合出 release-v1 setup flow：
  - `IM Gateway`
  - `Account Management`
  - `Devices & AIoT`
  - `Models & Policies`
  - `System Settings`
- 必须支持配置：
  - acceptance camera
  - recording policy 的 clip length
  - writable root 下的 capture 子目录
  - OCR / VLM / reply policy
  - 默认主动投递面
- 页面必须显式显示：
  - 当前 HarborOS 登录账号
  - 当前选中的摄像头
  - 当前存储目录
  - 当前模型策略
  - 当前微信状态
- 不新增 HarborDesk 登录页；不引入第二套后台身份系统

### C. `harbor-im-gateway`

- 继续把微信作为 release-v1 的主验证面，但不改变双通道同级边界
- 继续保证：
  - 微信私聊 ingress 健康
  - source-bound 回复全部回微信
  - `needs_input -> resume` 回微信
  - 抓拍/录制/检索结果回微信
  - source-bound / proactive observability 分开
- 不扩群聊
- 不改 recipient shape
- 不改 frozen seam

### D. `harbor-aiot`

- 以 TP-Link/Tapo 作为 acceptance camera，但不写死厂商
- 继续坚持 Home Device Domain：
  - snapshot：native snapshot 优先，失败再 ffmpeg fallback
  - clip：基于现有 recording/media 语义补成执行闭环
  - keyframe：进入现有 VLM-first ingest
- `Devices & AIoT` 页必须能看见：
  - selected camera
  - stream/snapshot/clip capability
  - native snapshot availability
  - room / name / vendor / model
  - ownership note

### E. `harbor-hos-control`

- 不新增新的用户可见 HarborOS 文件接口
- 目录创建/校验只作为现有配置保存和执行初始化的内部副作用
- 收口 HarborOS 同机安装形态：
  - HarborBeacon task API
  - HarborBeacon admin API
  - HarborDesk Angular 同源托管
  - HarborGate
  - Weixin runner
- 继续只负责 writable root、目录确保、文件落盘、system-domain route 和 HarborOS 用户上下文集成

### F. `harbor-architect`

- 每轮都只按这份 `V1-plan` 做验收，不接受 lane 自行改题
- 所有 lane 默认后台执行；主线程只负责：
  - 边界治理
  - 热点文件控制
  - checkpoint 验收
  - blocker 升级
  - 最终集成
- 任一 lane 一旦需要：
  - 新框架对象
  - 新 cross-repo 接口
  - 新 use-case 专用 admin API
  - 破坏 frozen seam
  必须立刻升级为 architect blocker

## Acceptance Gates

- 只有同时满足以下条件，release-v1 才算 ready for human testing：
  - HarborDesk 在 HarborOS 同机可打开并可配置
  - HarborOS 当前登录账号可直接成为 HarborDesk/admin principal
  - 微信自然语义抓拍成功，图片落盘并可检索
  - 微信自然语义短视频成功，视频落盘并可通过关键帧语义检索
  - 结果与后续交互始终 source-bound 回微信
  - 所有变量都可在 HarborDesk 中配置，而不是写死在代码里
- 明确 blocker：
  - 只能靠新增新框架对象才能完成
  - 只能靠新增新 cross-repo 接口才能完成
  - 只能靠固定短语匹配而不是真实自然语义才能完成
  - HarborDesk 需要独立账号体系才能完成
  - `camera/device` ownership 漂移到 HarborOS

## Test Plan

- HarborOS 同机安装 smoke
  - HarborBeacon / HarborDesk / HarborGate / Weixin runner 可启动
  - HarborDesk 同源打开
- HarborDesk 配置 smoke
  - acceptance camera 可选
  - capture 子目录可配置
  - clip length 可配置
  - model policies 可配置
  - 默认主动投递面可配置
- 微信自然语义 smoke
  - 多种说法触发抓拍
  - 多种说法触发录短视频
  - 多种说法触发“按特征找照片/视频”
- 媒体与检索 smoke
  - 图片落盘即 OCR/VLM ingest
  - 视频落盘即关键帧 ingest
  - 图片命中返回 citations
  - 视频命中基于关键帧语义代理
- 边界回归
  - HarborBeacon direct platform delivery count = `0`
  - HarborBeacon 不重新拥有 IM 原始凭据
  - HarborOS route 不漂移
  - `camera/device` 不被 HarborOS executors 抢占
  - `audio / full video semantics` 继续明确 pending

## Assumptions

- 已锁定：
  - `OpenClaw = v1 主参考`
  - `Hermes = 后续演进参考`
  - `视频 v1 = 短视频 + 关键帧检索`
  - `部署形态 = HarborBeacon / HarborDesk / HarborGate 全都装在 HarborOS`
  - `账号形态 = HarborOS 当前登录账号即后台账号`
- release-v1 的目录配置是“当前 HarborOS writable root 下的可配置子目录”；当前 verified root 仍在 `/mnt/software/harborbeacon-agent-ci` 之内。
- 如果任何一步只能通过扩框架来完成，这不是“顺手实现”，而是明确 blocker。
