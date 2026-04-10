# Home Agent Hub Phase Backlog

更新时间：2026-04-10

## 当前 MVP

优先完成这条链路：

`局域网自动扫描 -> 识别可用 RTSP 摄像头 -> 抓帧 -> AI 检测 -> 截图推送到 IM`

---

## Phase 1: Auto Discovery Foundation

目标：
- 固化摄像头自动发现所需的最小设备模型
- 建立 Device Registry 和局域网自动发现主链路

任务：
- `src/runtime/registry.rs`：定义 Camera 设备实体、能力字段、在线状态
- `src/runtime/discovery.rs`：定义候选发现、RTSP 验证请求与返回对象
- `src/adapters/onvif.rs`：建立 ONVIF 候选发现契约
- `src/adapters/ssdp.rs`：建立 SSDP 候选发现契约
- `src/adapters/mdns.rs`：建立 mDNS 候选发现契约
- `src/adapters/rtsp.rs`：建立 RTSP Probe 验证契约
- `src/domains/device.rs`：定义 `device.discover`、`device.list`、`device.get`
- 后台设备中心最小需求：能列出候选设备、识别结果和已接入摄像头

验收：
- 能以统一 schema 表示发现到的摄像头
- 至少能自动识别 1 路可用 RTSP 摄像头
- 发现过程无需手工填写摄像头 IP
- WebUI 后台可看到发现结果

## Phase 2: Stream & Snapshot

目标：
- 能从自动识别到的摄像头拿到画面并抓取截图

任务：
- `src/adapters/rtsp.rs`：补 RTSP 连接与流元数据
- `src/runtime/media.rs`：定义截图、媒体路径、抓图结果对象
- `src/domains/device.rs`：补 `device.snapshot`
- `src/connectors/storage.rs`：定义本地媒体存储目标
- 前台摄像头页最小需求：能查看最新截图
- 后台设备中心最小需求：能手动测试抓图

验收：
- 能成功抓取并保存一张截图
- 前台可以展示最新截图
- 后台可以手动触发抓图测试

## Phase 3: AI Detection

目标：
- 能对截图执行一次 AI 检测并返回结果

任务：
- `src/connectors/ai_provider.rs`：定义检测 provider 接口
- `src/control_plane/models.rs`：定义最小模型配置对象
- `src/domains/vision.rs`：定义 `vision.detect_frame`
- `src/runtime/node_runtime.rs`：定义单次检测节点执行结果
- 后台模型中心最小需求：配置一个检测 provider
- 后台运行监控最小需求：看到一次检测执行状态

验收：
- 输入截图可得到检测结果
- 后台可看到调用状态、耗时、结果摘要
- 错误能返回到后台页面

## Phase 4: IM Notification MVP

目标：
- 打通截图 + 检测结果推送到 IM 的闭环

任务：
- `harborbeacon/`：定义截图消息与文本说明回包
- `src/connectors/notifications.rs`：定义 IM 通知对象
- `src/runtime/events.rs`：定义检测命中事件
- `src/control_plane/audit.rs`：记录通知审计
- 后台页面最小需求：配置通知目标
- IM 最小目标：飞书先跑通

验收：
- 从截图检测到飞书收图全链路成功
- 检测结果和截图可一起发送
- 后台可看到通知执行记录

## Phase 5: MVP Governance

目标：
- 让 MVP 具备最小可管理性，而不只是 demo

任务：
- `src/control_plane/users.rs`：补最小用户与角色对象
- `src/control_plane/access.rs`：补摄像头查看和配置权限
- `src/control_plane/approvals.rs`：补高风险查看/导出预留
- 后台总览页：展示设备、检测、通知状态
- 系统设置页：配置 IM、存储、AI provider 基础项

验收：
- 管理员能配置设备、模型、通知三项核心设置
- 普通用户与管理员具备最小角色隔离
- MVP 可稳定演示
