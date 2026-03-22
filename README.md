# HarborNAS Local Agent Planning Package

This repository contains the completed planning deliverables for a HarborNAS local-first AI agent project, including architecture, roadmap, quick reference, meeting guide, launch checklist, and document index.

## Documents
- HarborNAS-LocalAgent-Plan.md
- HarborNAS-LocalAgent-Roadmap.md
- HarborNAS-LocalAgent-QuickRef.md
- HarborNAS-LocalAgent-MeetingGuide.md
- HarborNAS-LocalAgent-LaunchChecklist.md
- HarborNAS-LocalAgent-DocumentIndex.md
- HarborNAS-LocalAgent-V2-Assistant-Skills-Roadmap.md
- HarborNAS-Skill-Spec-v1.md
- HarborNAS-Middleware-Endpoint-Contract-v1.md
- HarborNAS-Files-BatchOps-Contract-v1.md
- HarborNAS-Planner-TaskDecompose-Contract-v1.md
- HarborNAS-Contract-E2E-Test-Plan-v1.md
- HarborNAS-CI-Contract-Pipeline-Checklist-v1.md
- HarborNAS-GitHub-Actions-Workflow-Draft-v1.md

## V2 Additions

- `HarborNAS-LocalAgent-V2-Assistant-Skills-Roadmap.md`: assistant + skills integration roadmap with HarborOS control-plane-first policy (`middleware API > midcli > browser > MCP`).
- `HarborNAS-Skill-Spec-v1.md`: standard skill contract (manifest schema, runtime envelope, routing, risk and test requirements), including HarborOS `middleware` API binding and `midcli` fallback rules.
- `HarborNAS-Middleware-Endpoint-Contract-v1.md`: executable endpoint contract for `system.harbor_ops`, including action normalization, API/CLI mapping, error model, and compatibility matrix template.
- `HarborNAS-Files-BatchOps-Contract-v1.md`: executable endpoint contract for `files.batch_ops`, including path policy, route fallback chain, CLI template constraints, and compatibility matrix template.
- `HarborNAS-Planner-TaskDecompose-Contract-v1.md`: executable planner contract for `planner.task_decompose`, including step schema, dependency rules, route-candidate policy, and release gates.
- `HarborNAS-Contract-E2E-Test-Plan-v1.md`: end-to-end validation plan across planner + execution contracts, including environment matrix, fallback checks, drift checks, and release exit criteria.
- `HarborNAS-CI-Contract-Pipeline-Checklist-v1.md`: CI job checklist that maps all contract governance to merge, nightly, and pre-release pipeline stages.
- `HarborNAS-GitHub-Actions-Workflow-Draft-v1.md`: initial GitHub Actions workflow draft mapping contract governance into concrete PR, nightly, and release workflows.

## HarborBeacon — IM 接入与用户交互层

HarborBeacon 是基于 [ZeroClaw](https://github.com/punkpeye/zeroclaw) 二次开发的 AI 助手，**预装在 HarborOS 中**（与 HarborOS 运行在同一台机器上）。用户通过即时通讯工具与 HarborBeacon 交互，HarborBeacon 通过 CLI、MCP、API 控制 HarborOS 各项功能。

### 架构总览

```text
[IM Channels]          [HarborBeacon]                [Orchestrator]            [HarborOS]
  飞书 / 企微            channels.py                 router / planner         middleware API
  Telegram / Discord  →  mcp_adapter.py           →  policy / audit        →  midcli
  钉钉 / Slack / MQTT    autonomy.py                 skills / executors       system services
                         tool_descriptions.py
```

### 支持的 IM 通道

| 通道 | 枚举值 | 说明 |
|---|---|---|
| 飞书 | `FEISHU` | Lark/飞书机器人 webhook |
| 企业微信 | `WECOM` | 企业微信应用消息 |
| Telegram | `TELEGRAM` | Telegram Bot API |
| Discord | `DISCORD` | Discord Bot gateway |
| 钉钉 | `DINGTALK` | 钉钉机器人 |
| Slack | `SLACK` | Slack App / Bot |
| MQTT | `MQTT` | 轻量级 IoT 消息协议 |

### 自主级别

| 级别 | 说明 | 对应风险 |
|---|---|---|
| `ReadOnly` | 只读操作，无需审批 | LOW |
| `Supervised` | 需逐次审批令牌 | MEDIUM / HIGH |
| `Full` | 完全自主执行 | 仅限管理员配置 |

### 代码包: `harborbeacon/`

- `channels.py`: IM 通道注册、消息路由、意图解析
- `mcp_adapter.py`: MCP 工具适配（ReadOnly 守卫、审批令牌）
- `autonomy.py`: 自主级别与风险等级映射
- `tool_descriptions.py`: skill manifest → MCP/TOML 工具描述转换

## Executable CI Scaffold

- `.github/workflows/contract-pr-check.yml`: PR and branch validation for contract schema checks plus contract, fallback, and policy test suites.
- `.github/workflows/contract-nightly-e2e.yml`: nightly/manual E2E matrix scaffold for `env-a` and `env-b`.
- `.github/workflows/contract-release-drift.yml`: release-branch drift matrix and release gate workflow.
- `scripts/validate_contract_schemas.py`: validates that required contract documents and route-priority rules stay aligned.
- `scripts/run_e2e_suite.py`: emits scaffolded E2E, latency, and audit reports for workflow wiring.
- `scripts/run_drift_matrix.py`: emits the initial drift-matrix artifact for release gating.
- `scripts/evaluate_release_gate.py`: converts drift output into a blocking/non-blocking release decision.
- `tests/contracts`, `tests/fallback`, `tests/policy`: minimal pytest suites that keep the documented routing, fallback, and governance rules from regressing.

Current scope note: the default CI path still works in documentation-only mode, but the same scripts can now switch into live HarborNAS integration mode when `midclt` and/or `cli` are available.

## Live Integration Mode

The four scripts under `scripts/` now support live HarborNAS probing through `middleware` and `midcli`.

- Middleware transport: local `midclt call ...`
- MidCLI transport: non-interactive `cli -m csv -c ...`
- Safe default probes: `service.query` for the configured probe service and `filesystem.listdir` for a configured path

Key environment variables:

- `HARBOR_MIDDLEWARE_BIN`: middleware CLI binary, default `midclt`
- `HARBOR_MIDCLI_BIN`: midcli binary, default `cli`
- `HARBOR_MIDCLI_URL`, `HARBOR_MIDCLI_USER`, `HARBOR_MIDCLI_PASSWORD`: remote midcli connection parameters when probing over websocket
- `HARBOR_PROBE_SERVICE`: safe service probe target, default `ssh`
- `HARBOR_FILESYSTEM_PATH`: safe filesystem probe path, default `/mnt`
- `HARBOR_SOURCE_REPO_PATH`, `UPSTREAM_SOURCE_REPO_PATH`: optional source trees used by `run_drift_matrix.py` for source-level capability comparison
- `HARBOR_ALLOW_MUTATIONS`: set to `1` to let E2E execute approved write operations instead of preview-only
- `HARBOR_APPROVAL_TOKEN`: approval token passed into HIGH-risk operations such as service restart and file move
- `HARBOR_REQUIRED_APPROVAL_TOKEN`: optional expected token value for the local script gate
- `HARBOR_APPROVER_ID`: approver identity written into mutation results for audit correlation
- `HARBOR_MUTATION_ROOT`: sandbox root for mutation fixtures, default `/mnt/agent-ci`

Typical usage:

- `python scripts/validate_contract_schemas.py --require-live`
- `python scripts/run_e2e_suite.py --env env-a --require-live`
- `python scripts/run_drift_matrix.py --harbor-ref develop --upstream-ref master`
- `python scripts/evaluate_release_gate.py drift-matrix-report.json --require-live`

Controlled mutation example:

- `HARBOR_ALLOW_MUTATIONS=1 HARBOR_APPROVAL_TOKEN=approved HARBOR_REQUIRED_APPROVAL_TOKEN=approved HARBOR_MUTATION_ROOT=/mnt/tank/agent-ci python scripts/run_e2e_suite.py --env env-a --require-live`
