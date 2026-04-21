# HarborOS Release Packaging / Install Runbook

更新时间：2026-04-20

## 1. 目的

这份 runbook 只回答一件事：

- 怎样把当前已经接近可用的 HarborBeacon / HarborDesk / HarborGate，
  收成 **可重复安装、可升级、可回滚** 的 HarborOS release bundle

它不是业务功能设计文档，也不是 HarborOS live smoke 的替代品。

## 2. 当前发布形态

release-v1 的默认形态固定为：

- Linux builder 负责预构建 HarborBeacon Rust 二进制
- Linux builder 负责构建 HarborDesk Angular `dist`
- Linux builder 负责组装 HarborGate Python 运行包
- HarborOS 目标机只负责部署与运行，不在机上执行 `cargo`、`rustc`、`node`、`npm` 或 `pip`
- HarborBeacon Rust Linux 默认目标为 `x86_64-unknown-linux-musl`
- 当目标为 musl 时，builder 使用 `cargo zigbuild --release --target <target>`，并要求 builder 上已有 `cargo-zigbuild` 与 `zig`

当前默认 builder：

- Debian verifier `192.168.3.223`
- non-root builder bootstrap 入口：`tools/bootstrap_release_builder.sh`

当前默认 HarborOS 目标机：

- `192.168.3.169`

当前默认 install root：

- `/var/lib/harborbeacon-agent-ci`

当前 verified writable root：

- `/mnt/software/harborbeacon-agent-ci`

## 3. 发布物结构

builder 产出一个单一版本化 bundle，结构固定为：

```text
harbor-release-<version>/
  bin/
    assistant-task-api
    agent-hub-admin-api
    validate-contract-schemas
    run-e2e-suite
  harbordesk/dist/harbordesk/
  harborgate/site-packages/
  install/
    install_harboros_release.sh
    rollback_harboros_release.sh
  templates/
    bin/
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

- `assistant-task-api` Linux release binary
- `agent-hub-admin-api` Linux release binary
- HarborDesk Angular dist
- HarborGate vendored site-packages
- `manifest.json`
- `checksums.sha256`
- `harbor-release-<version>.tar.gz`

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
- 写入 `HARBOR_HARBOROS_WRITABLE_ROOT=<writable-root>`
- 安装/更新 4 个 systemd 服务单元
- `daemon-reload`
- 默认 enable/start 3 个 core services
- 仅在已有 Weixin account config 时 enable/start `harborgate-weixin-runner`
- 若未配置 Weixin，则明确输出 `not configured, skipped`

固定安装的 4 个服务单元：

- `assistant-task-api.service`
- `agent-hub-admin-api.service`
- `harborgate.service`
- `harborgate-weixin-runner.service`

clean install 的健康预期：

- 默认活跃服务是 `assistant-task-api.service`
- 默认活跃服务是 `agent-hub-admin-api.service`
- 默认活跃服务是 `harborgate.service`
- `harborgate-weixin-runner.service` 在未配置 Weixin 凭据时允许保持 inactive/disabled

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
- 重启 3 个 core systemd 服务
- 仅在 `harborgate-weixin-runner.service` 已启用时重启该可选服务

## 7. 安装后验收

安装完成后，继续用现有 HarborOS smoke 做验收，而不是让安装脚本自己冒充 smoke。

这里要保持一个明确口径：

- release install root 可以是 `/var/lib/harborbeacon-agent-ci`
- HarborOS mutation root / writable root 仍然可以是 `/mnt/software/harborbeacon-agent-ci`
- smoke proof 继续引用 writable root，而不是把 install root 当成 mutation proof

Windows host：

```powershell
.\tools\run_harboros_vm_smoke.ps1 `
  -WebSocketUrl ws://192.168.3.169/websocket `
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
3. optional-service configuration absence
   - clean install 没有 Weixin 凭据，因此 `harborgate-weixin-runner` 应被视为 skipped，而不是 bundle 损坏
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
