# HarborNAS Local Agent V2 路线图与任务分配

## 1. 目标重申（真实北极星）

本项目不是文档工程，而是在 HarborOS 基础上落地:

1. 个人助手（多终端自然语言交互）
2. 多模态 RAG（文本/图像/音频/视频）
3. 智能编排（规划、路由、执行、审计、回滚）

执行优先级强约束:

1. `Middleware API`
2. `MidCLI`
3. `Browser`
4. `MCP`

---

## 2. V2 分阶段路线图（12 周）

### Phase 0（Week 1）目标对齐与治理基线

交付物:

- V2 统一术语与能力边界（orchestrator / skill / executor / audit）
- 路由与风险策略冻结（API > CLI > Browser > MCP）
- 发布门禁基线（contract + e2e + drift + release gate）

关键任务:

- 冻结 V2 能力清单（P0/P1/P2）
- 对齐现有 contract 与真实产品目标
- 建立版本化里程碑追踪（每周评审）

### Phase 1（Week 2-3）Assistant 核心闭环

交付物:

- 多终端统一会话入口（Web/Mobile API + IM 通道）
- HarborBeacon IM 接入：飞书 / 企微 / Telegram / Discord / 钉钉 / Slack / MQTT 一键配置
- 任务状态机：`queued -> planned -> executing -> completed/failed`
- 第一条端到端链路：IM → HarborBeacon → Planner → MiddlewareExecutor → Result

关键任务:

- 构建 session API 与任务持久化
- 集成 HarborBeacon channels.py，打通 IM → 意图解析 → MCP adapter 链路
- 规范统一任务 envelope（task_id/trace_id/executor_used/risk_level）
- 打通 HarborOS 常见系统类动作（query/start/stop/restart）

### Phase 2（Week 4-5）Skills Runtime 与智能编排

交付物:

- Skill registry + manifest loader + schema 校验
- Planner v1（意图拆解、依赖编排、路由候选）
- Policy gate（高风险确认、dry-run、路径策略）

关键任务:

- 定义 skill manifest 与版本策略
- 编排器支持 DAG 与失败重试
- 高风险动作审批流程接入

### Phase 3（Week 6-8）多模态 RAG 产品化

交付物:

- 多模态 ingestion pipeline（text/image/audio/video）
- 统一检索接口（dense+sparse+rerank）
- Assistant 可调用 RAG skill 回答真实用户问题

关键任务:

- 建立多模态 embedding 与索引策略
- 建设 metadata filter 与检索评估集
- 打通回答引用、溯源、审计记录

### Phase 4（Week 9-10）智能回退与可靠性工程

交付物:

- Route fallback engine（API->CLI->Browser->MCP）
- 失败分类、重试策略、熔断策略
- 可观测性面板（route ratio、失败原因、P95）

关键任务:

- route 决策日志与 replay 能力
- drift matrix 升级为周度兼容报告
- 关键链路压测与容量基线

### Phase 5（Week 11-12）Beta 发布与运营闭环

交付物:

- V2 beta 版本（可供真实用户试用）
- 运维手册、应急预案、回滚策略
- 版本验收报告与下一阶段 backlog

关键任务:

- beta 用户灰度与反馈收集
- 缺陷收敛与高优先级修复
- V2 GA 前置清单输出

---

## 3. 任务分配（按工作流，不按文件）

## 3.1 角色定义

- 架构/编排负责人（A）
- 平台/数据负责人（B）
- 可靠性/安全/QA 负责人（C）
- PM（P）

## 3.2 RACI（核心工作包）

| 工作包 | R | A | C | I |
|---|---|---|---|---|
| HarborBeacon IM 通道接入 | B | A | C,P | 全员 |
| Assistant 会话与任务状态机 | B | A | C,P | 全员 |
| Planner 与路由策略 | A | A | B,C | P |
| MiddlewareExecutor | A | A | B,C | P |
| MidCLIExecutor | B | A | C | P |
| Browser/MCP fallback | B | A | C | P |
| Skills registry/runtime | B | A | C | P |
| 多模态 RAG pipeline | B | A | C | P |
| 风险门禁（审批/dry-run/path policy） | C | A | B | P |
| 可观测性与审计 | C | A | B | P |
| CI/CD 门禁与发布 | C | A | B,P | 全员 |

---

## 4. 每周执行节奏（建议）

1. 周一：里程碑对齐 + 风险评估（30 分钟）
2. 周三：技术评审（架构、接口、回归）
3. 周五：可运行演示（必须是端到端，不是 PPT）

每周必须产出:

- 可执行增量（代码 + 测试 + 文档）
- 指标快照（成功率、P95、fallback ratio、失败分类）
- 下周明确阻塞项与负责人

---

## 5. 里程碑验收标准（Definition of Done）

### M1（Week 3）

- 用户能通过 IM 通道（飞书/企微/Telegram 等） → HarborBeacon → 统一入口触发 HarborOS 系统类操作
- 审计字段完整：`task_id`、`trace_id`、`executor_used`
- 核心回归测试通过

### M2（Week 5）

- Skills runtime 可加载/执行/隔离技能
- Planner 可输出结构化计划并执行
- 高风险操作必须确认

### M3（Week 8）

- 多模态 RAG 可在真实文件上稳定工作
- 检索质量达标（有基准集和报告）
- Assistant 回答含来源引用

### M4（Week 10）

- fallback 链路可观测、可复现、可解释
- midcli-only 场景不阻断发布（按降级策略）
- 关键链路 P95 可控

### M5（Week 12）

- beta 版对外可用
- 运行手册与回滚策略完备
- 发布评审通过

---

## 6. V2 KPI（发布门禁）

1. HarborOS 领域任务 API 路由占比 >= 70%
2. fallback 成功率 >= 95%
3. 高风险确认覆盖率 = 100%
4. 任务成功率 >= 95%（排除外部依赖故障）
5. 编排启动 P95 <= 2s
6. 回归通过率 >= 98%

---

## 7. 当前建议优先级（立刻执行）

P0:

1. 完成 HarborBeacon IM 接入 → Assistant 主链路闭环（IM → HarborBeacon → Planner → API executor）
2. 固化 skill manifest 与运行时约束
3. 完成多模态 RAG 最小可用链路（text + image）

P1:

1. 补齐 audio/video ingestion
2. 强化 fallback 可观测与自动化分析
3. 建立 beta 用户反馈闭环

P2:

1. Browser/MCP 场景深度优化
2. 编排策略自适应优化
3. 成本优化与缓存层强化
