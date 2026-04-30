# HarborDesk Information Architecture

更新时间：2026-04-19

## 1. 设计原则

- HarborDesk 是 HarborBeacon 的统一管理壳，不拆成单独前端仓库或框架
- 一套 WebUI 贯穿 HarborGate、HarborBeacon、HarborOS 和 AIoT
- 第一屏优先展示真实状态、审批、账户、设备和系统边界
- 如果后端能力还没落位，UI 保持 wiring-ready，并明确标注 blocker

## 2. 第一层导航

- `Overview`
- `IM Gateway`
- `Account Management`
- `Tasks & Approvals`
- `Devices & AIoT`
- `HarborOS`
- `Models & Policies`
- `System Settings`

## 3. 页面职责

- `Overview`：产品总览、状态摘要、事件流和操作模型，明确区分 `interactive reply = source-bound` 与 `proactive delivery = member default`
- `IM Gateway`：绑定二维码、桥接 provider、Feishu baseline、Weixin parity track、source-bound queue/failure signals、IM 接入状态
- `Account Management`：成员、角色、ownership、访问控制，并展示 per-member proactive delivery default、binding availability、recent interactive surface
- `Tasks & Approvals`：高风险动作审批、任务结果、审计引用，清楚区分 interaction-linked replies 与 proactive delivery failures
- `Devices & AIoT`：发现、手动录入、预览、分享链路、AIoT 设备治理
- `HarborOS`：系统域控制边界、宿主机控制面、live/proof 区分明确的 summary block
- `Models & Policies`：模型端点状态、endpoint kind/provider/test result、route policy、fallback order、以及 VLM first 的可运营操作位
- `System Settings`：真实 routing/gateway status 和显式 blocker，不展示推测性的部署元数据

## 4. 后端依赖约定

- 优先复用现有 `/api` 能力
- 同源 admin-plane endpoint 优先直接接入现有壳层；拿不到真数据时必须显式显示 blocker
- 账户目录、系统设置、模型中心等如果尚未提供真实接口，页面只能展示明确 blocker
- 前端只渲染后端已经返回的 routing / gateway / delivery 状态，不在 UI 内合成 shell metadata 或伪造渠道状态
- 页面统一采用 `loading / empty / blocker / success` 四态，并在页面顶部明示当前页状态

## 5. 前端约束

- `System Settings` 只显示真实 routing/gateway status 与 explicit blockers，不展示推测性的部署元数据
- `IM Gateway` 的 Feishu / Weixin / queue / failure 信号必须来自后端投影；没有数据就显示空态或 blocker
- `Account Management` 里的 proactive delivery default 以成员粒度展示，binding availability 只能反映后端提供的可绑定状态
- `Tasks & Approvals` 里的回复与投递失败是不同信号源，前端不能把它们合并成单一“通知结果”
- `Models & Policies` 必须把 endpoint status/test result/kind/provider/route policy/fallback order 放在同一页的清晰操作位里，而不是散落成静态说明

## 6. 共享能力

- 统一登录和会话
- 统一通知中心
- 统一资源选择器：Workspace / Room / Device / Task
- 统一审计流：绑定、成员变更、审批、设备动作、策略保存

## 7. 交付与构建

- HarborDesk Angular workspace 位于 `frontend/harbordesk`
- 开发态通过 `proxy.conf.json` 将 `/api` 代理到本机同源 admin API
- 生产构建输出目录为 `frontend/harbordesk/dist/harbordesk`
