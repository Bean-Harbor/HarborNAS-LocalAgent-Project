# HarborNAS Local Agent V2 启动清单

## 1. 启动原则

1. 先交付可运行链路，再扩功能。
2. 先保证治理与审计，再放开自动化。
3. 路由优先级固定：API > MidCLI > Browser > MCP。
4. 用户入口：IM 通道 → HarborClaw → Assistant Runtime → HarborOS。

---

## 2. T+0（今天必须完成）

- [ ] 冻结 V2 范围（个人助手、多模态RAG、智能编排）
- [ ] 确认 P0 技能清单（system/file/rag）
- [ ] 确认 HarborClaw IM 通道优先级（飞书/企微/Telegram 作为 P0）
- [ ] 确认角色分工与 owner
- [ ] 确认 Beta 时间窗口（Week 12）

---

## 3. T+7（第一周完成）

- [ ] Assistant 统一入口 API 可用
- [ ] HarborClaw 至少一个 IM 通道跟通（飞书或 Telegram）
- [ ] 任务状态机与审计字段落库
- [ ] 第一条 `IM → HarborClaw → Planner → MiddlewareExecutor` 跑通
- [ ] CI 可自动产出 contract/e2e/drift/release gate 报告

验收:

- [ ] 有可演示 endpoint
- [ ] 有可重放 trace
- [ ] 有失败原因分类

---

## 4. T+14（第二周完成）

- [ ] Skill registry + manifest 校验上线
- [ ] MidCLI fallback 可用且可审计
- [ ] 高风险审批、dry-run、路径策略可验证

验收:

- [ ] midcli-only 场景可降级不中断
- [ ] 高风险确认覆盖率 100%

---

## 5. T+30（一个月完成）

- [ ] 多模态 RAG（text+image）可用
- [ ] 回答可附引用来源
- [ ] Planner 支持 DAG 与重试

验收:

- [ ] 真实数据集上可复现结果
- [ ] 检索质量有基准报告

---

## 6. 风险清单（启动期）

1. 目标漂移（只做文档不做产品）
: 每周演示必须是端到端运行结果。
2. 回退滥用（Browser/MCP 过度）
: route ratio 周报 + 告警阈值。
3. 高风险动作误执行
: 强制审批 + dry-run 预演。
4. 多模态质量不稳
: 建评测集，按周回归。

---

## 7. 关键KPI（启动后就开始跟踪）

- API route ratio >= 70%
- Task success rate >= 95%
- Orchestration start P95 <= 2s
- High-risk confirmation coverage = 100%
- Regression pass rate >= 98%
