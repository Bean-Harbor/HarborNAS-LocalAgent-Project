# HarborOS VM 本地验证 Runbook（Windows Host）

更新时间：2026-04-18

## 1. 先给结论

如果你的目标是验证这个仓库当前的 HarborOS 集成链路，最稳的方案是：

1. 在 Windows 宿主机上创建一个 HarborOS 虚拟机。
2. 给虚拟机配置可直连的局域网网络。
3. 让本仓库通过现有的 `middleware -> midcli` 验证链路远程连到这台 HarborOS VM。

如果你的目标是“让 HarborOS 虚拟机直接吃到 NVIDIA GPU 做 CUDA / 推理性能验证”，不要把 Windows 宿主机上的桌面级虚拟化当成主方案。当前更适合把 GPU 验证拆到裸机 HarborOS / Linux AI BOX / Linux 虚拟化宿主机上。

## 2. 为什么这样选

当前官方约束下，Windows 宿主机上的 GPU 虚拟化不适合作为 HarborOS GPU 验证主路径：

- Hyper-V 的 GPU partitioning 官方文档口径在 `Windows Server 2025`，并且只列出少量支持的 NVIDIA 数据中心卡。
- Hyper-V 的 DDA（整卡直通）官方前提也是 `Windows Server` 宿主机，不是普通 Windows 桌面机。
- VMware Workstation Pro 适合跑 Linux/HarborOS 类客体，也支持虚拟机 3D 加速，但它更适合“把系统跑起来并完成联调”，不应被当成当前仓库的 CUDA 直通验证路径。

换句话说：

- 要验证这个仓库：`VM + WebSocket/midcli 联调` 就够了。
- 要验证 GPU：优先 `裸机 HarborOS` 或 `Linux/KVM/Proxmox/vSphere + 受支持 GPU 方案`。

## 3. 本仓库已经支持的验证模式

这个仓库的 live integration 已经预留了 Windows 侧联调入口：

- `tools/cli.cmd`
- `tools/harbor_cli_shim.py`
- `target/release/validate-contract-schemas.exe`
- `target/release/run-e2e-suite.exe`

它的核心思路不是把 HarborOS 工具装到 Windows 上，而是：

```text
Windows Repo
  -> tools/cli.cmd
  -> tools/harbor_cli_shim.py
  -> ws://<harboros-vm>/websocket
  -> HarborOS midcli / middleware surface
```

这正适合“Windows 开发机 + HarborOS VM”的验证方式。

## 4. 推荐部署拓扑

### 4.1 目标拓扑

```text
[Windows 开发机]
  - 本仓库
  - Rust release binaries
  - Python venv
  - PowerShell smoke 脚本
        |
        | WebSocket / WebUI
        v
[HarborOS VM]
  - HarborOS
  - websocket / middleware / midcli
  - 可访问的管理账号
```

### 4.2 网络建议

优先级如下：

1. `Bridge / External Switch`
2. `NAT + 端口转发`

建议优先用桥接或 Hyper-V External Switch，这样：

- Windows 能直接访问 HarborOS WebUI
- `ws://<vm-ip>/websocket` 更简单
- 后面如果你要接局域网里的摄像头/设备，也更接近真实环境

## 5. 虚拟机方案建议

### 5.1 方案 A：先完成仓库验证

适合目标：

- 验证 `service.query`
- 验证 `service.control`
- 验证 `filesystem.listdir/copy/move`
- 验证路由优先级 `middleware API -> midcli`
- 跑本仓库现有 live smoke

推荐做法：

- Hyper-V Gen2 VM 或 VMware Workstation Pro 二选一
- 8 vCPU 起步
- 16 GB RAM 起步，建议 24 GB 或 32 GB
- 120 GB 系统盘起步
- 网络用桥接 / External Switch

说明：

- 这条路径不以 GPU 直通为目标。
- 如果你用 VMware Workstation，可以打开虚拟机 3D 加速，但它只应被视为“提升客体桌面/UI 流畅度”的附加项。

### 5.2 方案 B：真 GPU 验证

适合目标：

- HarborOS 内部模型推理
- CUDA / NVENC / GPU driver 真机能力
- 视频推理负载

推荐做法：

1. HarborOS 裸机安装到单独硬盘或可切换启动盘。
2. 或者把 HarborOS / Linux 放到单独的 AI BOX / 小主机。
3. 或者改用 Linux/KVM/Proxmox/vSphere 这类更适合 GPU 方案的宿主机。

不要默认假设 Windows 宿主机上的桌面虚拟化能无痛完成这类验证。

## 6. HarborOS VM 落地步骤

### 6.1 创建虚拟机

建议：

- CPU: `8` 核以上
- 内存: `16-32 GB`
- 磁盘: `120 GB+`
- 固件: 以 HarborOS 镜像要求为准；如果没有专门模板，优先选接近 Debian / 其他现代 Linux 的模板
- 网络: 桥接 / External Switch

### 6.2 安装 HarborOS

在 VM 内完成：

- HarborOS 基础安装
- WebUI 可访问
- WebSocket / middleware / midcli 所需服务可访问
- 创建一个能访问这些接口的管理员账号

### 6.3 先做最小连通性确认

你至少需要拿到这三个信息：

- HarborOS VM IP，例如 `192.168.50.20`
- WebSocket 地址，例如 `ws://192.168.50.20/websocket`
- HarborOS 用户名 / 密码

## 7. 在本仓库里跑 live smoke

仓库根目录执行：

```powershell
.\tools\run_harboros_vm_smoke.ps1 `
  -WebSocketUrl ws://192.168.50.20/websocket `
  -Username root `
  -Password 'your-password'
```

脚本会做这些事情：

1. 复用 `tools/cli.cmd` 作为 Windows 侧 midcli shim。
2. 设置当前 PowerShell 进程里的 HarborOS 环境变量。
3. 如果缺少 release binary，只定向构建 smoke 需要的二进制：
   - `validate-contract-schemas`
   - `run-e2e-suite`
   - `run-drift-matrix`（仅在 `-RunDrift` 时）
4. 运行：
   - `validate-contract-schemas.exe --require-live`
   - `run-e2e-suite.exe --env env-a --require-live`
5. 把报告写到 `.tmp-live/harboros-vm-smoke/`

如果你还要顺手跑 drift：

```powershell
.\tools\run_harboros_vm_smoke.ps1 `
  -WebSocketUrl ws://192.168.50.20/websocket `
  -Username root `
  -Password 'your-password' `
  -RunDrift `
  -DriftHarborRef develop `
  -DriftUpstreamRef master
```

## 8. 常用环境变量

这个仓库当前 live integration 主要会读这些变量：

- `HARBOR_MIDCLI_BIN`
- `HARBOR_MIDCLI_URL`
- `HARBOR_MIDCLI_USER`
- `HARBOR_MIDCLI_PASSWORD`
- `HARBOR_PROBE_SERVICE`
- `HARBOR_FILESYSTEM_PATH`

安全 probe 默认更偏向：

- `service.query`
- `filesystem.listdir`

也就是先验证“能连通、能走通、路由正确”，而不是一上来就做高风险写操作。

## 9. 推荐验证顺序

建议按下面顺序推进：

1. HarborOS VM 能从浏览器打开 WebUI。
2. Windows 能访问 `ws://<vm-ip>/websocket`。
3. 跑 `validate-contract-schemas --require-live`。
4. 跑 `run-e2e-suite --require-live`。
5. 再做受控 mutation。
6. 最后才决定要不要进入 GPU 验证路径。

## 10. 什么时候该切到“真 GPU 路线”

出现下面任一情况，就不要继续在 Windows-hosted VM 上纠结显卡：

- 你要验证 CUDA 是否可用
- 你要验证 HarborOS 内部模型吞吐
- 你要验证视频推理 / 编码链路
- 你要验证 NVIDIA 驱动和 HarborOS 的真实兼容性

这时应该切到：

- HarborOS 裸机
- Linux AI BOX
- Linux/KVM/Proxmox/vSphere 宿主机

## 11. 与当前仓库边界的关系

这份 runbook 只服务于 HarborOS System Domain 验证：

- `Middleware API -> MidCLI -> Browser/MCP fallback`

它不改变：

- IM ingress
- `route_key`
- notification delivery
- device-native adapter ownership

也就是说，这条路径是在现有仓库边界内把 HarborOS 验证环境落起来，不会把 Home Device Domain 混进来。

## 12. 官方参考

- [Microsoft Learn: GPU partitioning for Hyper-V](https://learn.microsoft.com/en-us/windows-server/virtualization/hyper-v/gpu-partitioning)
- [Microsoft Learn: Deploy graphics devices by using DDA](https://learn.microsoft.com/en-us/windows-server/virtualization/hyper-v/deploy/deploying-graphics-devices-using-dda)
- [Microsoft Learn: Supported Linux and FreeBSD virtual machines for Hyper-V](https://learn.microsoft.com/en-us/windows-server/virtualization/hyper-v/supported-linux-and-freebsd-virtual-machines-for-hyper-v-on-windows)
- [VMware Workstation 17 Pro Release Notes](https://docs.vmware.com/en/VMware-Workstation-Pro/17.0/rn/vmware-workstation-170-pro-release-notes.pdf)
- [Broadcom KB: Enable 3D acceleration in VMware Workstation](https://knowledge.broadcom.com/external/article/329348/the-display-option-in-the-virtual-machin.html)
