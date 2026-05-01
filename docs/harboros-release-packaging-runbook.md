# HarborOS Release Packaging / Install Runbook

更新时间：2026-04-30

## 1. 目的

这份 runbook 只回答一件事：

- 怎样把当前已经接近可用的 HarborBeacon / HarborDesk / HarborGate，
  收成 **可重复安装、可升级、可回滚** 的 HarborOS release bundle

它不是业务功能设计文档，也不是 HarborOS live smoke 的替代品。

## 2. 当前发布形态

release-v1 的默认形态固定为：

- Linux builder 负责预构建 HarborBeacon Rust 二进制，包括统一入口 `harborbeacon-service`
- Linux builder 负责预构建 HarborBeacon 单端口推理 facade 所需的内部模型 backend
- Linux builder 负责构建 HarborDesk Angular `dist`
- Linux builder 负责组装 HarborGate Python 运行包
- HarborOS 目标机只负责部署与运行，不在机上执行 `cargo`、`rustc`、`node`、`npm` 或 `pip`
- HarborBeacon Rust Linux 默认目标为 `x86_64-unknown-linux-musl`
- 当目标为 musl 时，builder 使用 `cargo zigbuild --release --target <target>`，并要求 builder 上已有 `cargo-zigbuild` 与 `zig`
- HarborBeacon 单端口封装本地 OpenAI-compatible 模型服务；`harbor-model-api`/Candle/VLM 只作为内部或过渡 backend，不再成为对外 systemd/API 契约

当前默认 builder：

- HarborBeacon build host `192.168.1.197`
- non-root builder bootstrap 入口：`tools/bootstrap_release_builder.sh`

当前默认 HarborOS 目标机：

- `192.168.3.82`

历史参考：

- `192.168.3.223` 曾作为 Debian verifier / builder baseline。
- `192.168.3.182` 曾作为 HarborOS live target；当前 `.82` 替代它作为
  RC/GA target。

当前默认 install root：

- `/var/lib/harborbeacon-agent-ci`

当前 verified writable root：

- `/var/lib/harborbeacon-agent-ci/writable` on the current `.82` RC2 install.
  `/mnt/software/harborbeacon-agent-ci` remains a supported writable-root
  target when present.

当前 RC baseline：

- HarborBeacon: `a5f6da0`
- HarborGate: `57ff759`
- RC2 bundle:
  `harbor-release-20260430-rc2-beacona5f6da0-gate57ff759.tar.gz`
- SHA256:
  `7119842506d38aac82c7e236b7f96a054244bb50be07c5e6b001ac7b0683484c`

## 3. 发布物结构

builder 产出一个单一版本化 bundle，结构固定为：

```text
harbor-release-<version>/
  bin/
    harborbeacon-service
    validate-contract-schemas
    run-e2e-suite
  harbordesk/dist/harbordesk/
  harborgate/bin/harborgate
  harborgate/site-packages/
  install/
    install_harboros_release.sh
    rollback_harboros_release.sh
  templates/
    bin/
      harbor-agent-hub-helper
    systemd/
    harborbeacon-agent-hub.env.template
  manifest.json
  checksums.sha256
```

对应目录布局拆成两部分：

```text
/var/lib/harborbeacon-agent-ci/
  releases/<version>/
  current -> releases/<version>
  runtime/
  captures/
  logs/
```

```text
/mnt/software/harborbeacon-agent-ci/
  ... HarborOS writable / mutation root ...
```

说明：

- install root 必须是可执行 release/runtime 根
- writable root 继续承载 HarborOS mutation proof 与 smoke tooling
- 如果 `/mnt/software/harborbeacon-agent-ci` 不可用，installer 才回退到 `<install-root>/writable`

## 4. Builder 侧命令

在 Linux builder 上执行：

```bash
export HARBORGATE_REPO=/path/to/HarborGate
export RUST_TARGET=x86_64-unknown-linux-musl
export BOOTSTRAP_BUILDER_IF_NEEDED=1
bash ./tools/build_release_bundle.sh
```

如果要显式指定版本或输出目录：

```bash
export RELEASE_VERSION=release-v1-20260419
export OUT_DIR=/tmp/harbor-release-bundles
export HARBORGATE_REPO=/path/to/HarborGate
export RUST_TARGET=x86_64-unknown-linux-musl
export BOOTSTRAP_BUILDER_IF_NEEDED=1
bash ./tools/build_release_bundle.sh
```

如果 builder 还没准备好 musl toolchain，也可以先显式执行：

```bash
bash ./tools/bootstrap_release_builder.sh \
  --rust-target x86_64-unknown-linux-musl \
  --rustup-toolchain stable \
  --zig-version 0.15.1
```

builder 预期：

- `cargo-zigbuild` 与 `zig` 已安装在 Linux builder 上
- musl target 在当前用户态 Rust toolchain 中已安装，不要求 root 或 apt 层面的 system-wide 配置
- musl 目标产物必须是 static linkage
- `manifest.json` 必须记录 `rust_target`、`linkage` 和 Linux portability expectation

builder 结果至少应包含：

- `harborbeacon-service` Linux release binary
- HarborDesk Angular dist
- HarborGate Rust binary slot and Python fallback runtime
- HarborGate vendored site-packages
- `manifest.json`
- `checksums.sha256`
- `harbor-release-<version>.tar.gz`

旧 `assistant-task-api`、`agent-hub-admin-api`、`harbor-model-api`、VLM sidecar
可以作为过渡二进制或 benchmark 工具存在，但不再是 release bundle 的主
systemd/API contract。

## 4.1 Model Backend Benchmark Gate

在把 HarborOS 的默认 backend 从 `openai_proxy` 切到 `candle` 之前，先跑
repo-local benchmark lane。
这里的 lane 分工固定为：

- `.182` 是权威 Candle runtime gate
- `.223` 只用于 build / prefetch / spawned benchmark 证据
- live path 继续通过 `127.0.0.1:4174/api/inference/v1` 暴露默认
  `openai_proxy`
- `4186` 继续保留给 Candle candidate lane

观察已运行服务：

```bash
cargo run --bin benchmark-local-model-backend -- \
  --base-url http://127.0.0.1:4174/api/inference/v1 \
  --healthz-url http://127.0.0.1:4174/api/inference/healthz \
  --backend openai_proxy \
  --output /tmp/local-model-benchmark-openai-proxy.json
```

正式 promotion gate：

```bash
cargo run --bin benchmark-local-model-backend -- \
  --spawn-binary ./target/x86_64-unknown-linux-musl/release/harbor-model-api \
  --backend candle \
  --bind 127.0.0.1:4186 \
  --candle-chat-model-id Qwen/Qwen3-1.7B \
  --candle-embedding-model-id jinaai/jina-embeddings-v2-base-zh \
  --output /tmp/local-model-benchmark-candle.json
```

只有当报告里的 `gate.promotable=true`，才允许规划把 env 里的
`HARBOR_MODEL_API_BACKEND` 改成 `candle` 的单独 cutover rehearsal。否则保持
`openai_proxy`，HarborBeacon 的 local OpenAI-compatible seam 不变。

补充说明：

- `127.0.0.1:4186` 只保留给 `candle` 旁路实验实例，例如
  `harbor-model-api-candle-exp` 这种 transient unit。
- 当前 Candle 候选默认组合是 `Qwen/Qwen3-1.7B + jinaai/jina-embeddings-v2-base-zh`。
- `Qwen3.5` 已明确延期到后续 loader/backend 轮次，不进入这一轮 gate。
- HarborOS env template 允许保留这 3 个 side-lane 变量：
  - `HARBOR_MODEL_API_CANDLE_CHAT_MODEL_ID`
  - `HARBOR_MODEL_API_CANDLE_EMBEDDING_MODEL_ID`
  - `HARBOR_MODEL_API_CANDLE_CACHE_DIR`
- 只要 `/healthz` 仍报告 `degraded` / `ready=false`，或者 chat / embeddings 任一
  gate 没过，就不能把 `4186` 的结果写成 Candle 已可切默认 backend。
- `.223` 的 spawned benchmark 只保留为 build / prefetch 兼容性证据；是否允许
  进入 cutover rehearsal，只看 `.182` 的 target-runtime report。
- 当 `.182` 已经拿到 `gate.promotable=true` 的 Candle report 时，下一步也仍然是
  单独开一轮 cutover rehearsal，而不是直接把 HarborBeacon 单端口推理面从
  `openai_proxy` 改成 `candle`。

## 5. HarborOS 安装

把 tarball 复制到 HarborOS 后，以 root 安装：

```bash
sudo bash ./install_harboros_release.sh \
  --bundle /path/to/harbor-release-<version>.tar.gz \
  --install-root /var/lib/harborbeacon-agent-ci \
  --writable-root /mnt/software/harborbeacon-agent-ci
```

安装脚本负责：

- 创建/校验 install root 下的 `releases/`, `current/`, `runtime/`, `captures/`, `logs/`
- 创建/校验 HarborOS writable root
- 把 bundle 解包到 `releases/<version>/`
- 更新 `current/` 软链接
- 写入单一 env-file
- 显式写入 `HARBORBEACON_WEB_API_URL=http://127.0.0.1:4174`
- 显式写入 `HARBORBEACON_ADMIN_API_URL=http://127.0.0.1:4174`
- 显式写入 `HARBORBEACON_ADMIN_API_TOKEN=<service-token>`
- 显式写入 `HARBOR_MODEL_API_BASE_URL=http://127.0.0.1:4174/api/inference/v1`
- 显式写入 `HARBOR_MODEL_API_TOKEN=<service-token>`
- 写入 `HARBOR_HARBOROS_WRITABLE_ROOT=<writable-root>`
- 安装/更新 2 个 systemd 服务单元
- 旧 unit 被 disable/remove，避免旧多端口拓扑在升级后继续漂移
- 更新 `${install-root}/bin/harbor-agent-hub-helper -> current/templates/bin/harbor-agent-hub-helper`
- `daemon-reload`
- 默认 enable/start 2 个 core services

固定安装的 2 个服务单元：

- `harborbeacon.service`
- `harborgate.service`

clean install 的健康预期：

- 默认活跃服务是 `harborbeacon.service`
- 默认活跃服务是 `harborgate.service`
- Weixin/Feishu runtime 是 `harborgate.service` 内部 task，不新增平台级 systemd 服务

## 5.1 `.182` 常驻测试状态助手

安装完成后，`.182` 上固定通过同一个 helper 看 resident stack：

- `/var/lib/harborbeacon-agent-ci/bin/harbor-agent-hub-helper status`
- `/var/lib/harborbeacon-agent-ci/bin/harbor-agent-hub-helper health`
- `sudo /var/lib/harborbeacon-agent-ci/bin/harbor-agent-hub-helper logs gateway --lines 120`

约定：

- `status` 输出 `harborbeacon.service` 与 `harborgate.service` 的 `is-enabled / is-active / MainPID` 风格摘要 JSON
- `health` 顺序检查 `127.0.0.1:4174`、`127.0.0.1:4174/api/inference/healthz`、`127.0.0.1:8787` 的 loopback health
- `health` 还会带 service auth + `X-Contract-Version: 2.0` 调 `GET /api/gateway/status`
- Weixin 摘要优先读 gateway redacted truth；如果 gateway 暂时不可读，再回退到 `WEIXIN_STATE_DIR/accounts/<account>.runtime.json`
- Weixin 观测至少包含：
  - `last_poll_at`
  - `last_getupdates_error`
  - `last_private_text_message_at`
- `logs` 不起新 daemon，只是展开 `journalctl -u ...`
- `.182` 如果保留默认 journald 权限，`logs` 需要 `sudo` 或 `systemd-journal` 组权限

## 6. 回滚

回滚不是回退数据目录，而是切回上一个版本：

```bash
sudo bash ./rollback_harboros_release.sh \
  --install-root /var/lib/harborbeacon-agent-ci
```

或显式切回某个版本：

```bash
sudo bash ./rollback_harboros_release.sh \
  --install-root /var/lib/harborbeacon-agent-ci \
  --version release-v1-20260419
```

回滚动作固定为：

- 更新 `current/` 指向
- 更新 env-file 中的 `HARBOR_RELEASE_VERSION`，避免回滚后元数据漂移
- 重启 2 个 core systemd 服务
- 回滚前停用旧 unit，保持双服务拓扑

## 7. 安装后验收

安装完成后，继续用现有 HarborOS smoke 做验收，而不是让安装脚本自己冒充 smoke。

这里要保持一个明确口径：

- release install root 可以是 `/var/lib/harborbeacon-agent-ci`
- HarborOS mutation root / writable root 仍然可以是 `/mnt/software/harborbeacon-agent-ci`
- smoke proof 继续引用 writable root，而不是把 install root 当成 mutation proof
- HarborGate admin sync 依赖 `:4174`，不要再让它 fallback 到旧 task API 端口
- 本地 OpenAI-compatible 模型服务固定通过 `:4174/api/inference/v1` 暴露；HarborBeacon 继续拥有检索语义、排序和引用包装
- `HARBOR_KNOWLEDGE_INDEX_ROOT` 必须指向 writable root，例如 `/mnt/software/harborbeacon-agent-ci/knowledge-index`；不要让 retrieval 回落到相对路径 `.harborbeacon/knowledge-index`

Windows host：

```powershell
.\tools\run_harboros_vm_smoke.ps1 `
  -WebSocketUrl ws://192.168.3.182/websocket `
  -Username <harboros-user> `
  -Password '<password>' `
  -AllowMutations `
  -MutationRoot /mnt/software/harborbeacon-agent-ci `
  -ApprovalToken approved `
  -RequiredApprovalToken approved
```

Linux verifier：

```bash
bash ./tools/run_harboros_vm_smoke.sh \
  --websocket-url ws://192.168.3.165/websocket \
  --username <harboros-user> \
  --password '<password>' \
  --allow-mutations \
  --mutation-root /mnt/software/harborbeacon-agent-ci \
  --approval-token approved \
  --required-approval-token approved
```

## 8. 这条 lane 的边界

这条 release packaging / install lane：

- 不新增新的框架对象
- 不新增新的 cross-repo 接口
- 不新增新的 use-case 专用 admin API
- 不新造 HarborDesk 独立账号体系

它只负责把现有 v1 能力收成正式安装形态。

## 9. 当前已知 blocker 口径

如果发布安装失败，优先按下面口径归因：

1. exec-root mismatch
   - install root 落在 `noexec` 或不可执行挂载点
   - operator 把 release/runtime 根误放到 writable root
2. binary portability mismatch
   - Rust Linux target 不是预期的 `x86_64-unknown-linux-musl`
   - builder 没产出 static linkage，导致目标机 libc 不匹配
3. IM runtime configuration absence
   - clean install 没有 Weixin 凭据，因此 `harborgate.service` 内部 Weixin runtime 应被视为 skipped，而不是 bundle 损坏
4. bundle incompleteness
   - 缺 HarborGate Python 运行包
   - 缺 HarborDesk dist
   - 缺 systemd units / env-file / install script
5. builder / host dependency gap
   - builder 缺 `cargo-zigbuild` / `zig`
   - builder 缺 `node/npm`
   - HarborOS 缺 `python3` / `systemd`

如果问题需要靠新增框架对象、改 frozen seam 或加新 admin API 才能解决，
这不是 install lane 内的问题，而是 architect blocker。
