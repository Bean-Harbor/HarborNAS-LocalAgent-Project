# HarborNAS Local Agent 文档索引（V2）

## 1. 目标类文档

1. HarborNAS-LocalAgent-Plan.md
: 全局架构与能力边界（个人助手/多模态RAG/智能编排）。
2. HarborNAS-LocalAgent-Roadmap.md
: V2 12 周路线图与任务分配（当前主执行文档）。
3. HarborNAS-LocalAgent-V2-Assistant-Skills-Roadmap.md
: V2 技术路线补充与执行优先级说明。

## 2. 执行与落地类文档

1. HarborNAS-LocalAgent-QuickRef.md
: 框图、路由规则、统一任务对象、每周交付要求。
2. HarborNAS-LocalAgent-LaunchChecklist.md
: 启动阶段分周检查清单（T+0/T+7/T+14/T+30）。
3. HarborNAS-LocalAgent-MeetingGuide.md
: 评审会议流程与讨论问题。

## 3. 契约与治理类文档

1. HarborNAS-Skill-Spec-v1.md
2. HarborNAS-Middleware-Endpoint-Contract-v1.md
3. HarborNAS-Files-BatchOps-Contract-v1.md
4. HarborNAS-Planner-TaskDecompose-Contract-v1.md
5. HarborNAS-Contract-E2E-Test-Plan-v1.md
6. HarborNAS-CI-Contract-Pipeline-Checklist-v1.md
7. HarborNAS-GitHub-Actions-Workflow-Draft-v1.md

## 4. HarborClaw 与 IM 接入层

HarborClaw 是基于 ZeroClaw 二次开发的 AI 助手，预装在 HarborOS 中（与 HarborOS 运行在同一台机器上）。
用户通过 IM（飞书 / 企业微信 / Telegram / Discord / 钉钉 / Slack / MQTT）与 HarborClaw 交互，HarborClaw 通过 CLI、MCP、API 等形式控制 HarborOS 各项功能。

代码包: `harborclaw/`

| 模块 | 职责 |
|---|---|
| `channels.py` | IM 通道注册、消息收发、意图路由 |
| `mcp_adapter.py` | MCP 工具列表 / 调用适配器（ReadOnly 守卫、逐次审批令牌） |
| `autonomy.py` | 自主级别 (ReadOnly / Supervised / Full) 与风险映射 |
| `tool_descriptions.py` | 将 skill manifest 转换为 MCP / TOML 工具描述 |

## 5. 当前版本口径

- 产品北极星: HarborOS 个人助手 + 多模态RAG + 智能编排。
- 用户入口: IM 通道 → HarborClaw → Orchestrator Runtime → HarborOS。
- 路由规则: `middleware API > midcli > browser > MCP`。
- 自主级别: ReadOnly（只读安全）/ Supervised（需审批）/ Full（完全自主）。
- 发布门禁: contract/e2e/drift/release gate 必须可执行。

## 6. 阅读顺序（新成员）

1. HarborNAS-LocalAgent-QuickRef.md
2. HarborNAS-LocalAgent-Roadmap.md
3. HarborNAS-LocalAgent-Plan.md
4. 契约与治理文档组
