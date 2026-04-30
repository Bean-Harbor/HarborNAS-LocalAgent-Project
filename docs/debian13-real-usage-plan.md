# Debian 13 Real Usage Plan

更新时间：2026-04-17

## 目标

- 在 Debian 13 机器上长期运行 HarborBeacon
- 设备通过网线接入局域网
- 使用静态二维码把手机带到本机后台配置页
- 在手机上填写飞书机器人的 `app_id` / `app_secret`
- 配置完成后，通过飞书触发自动扫描摄像头

## 真实使用入口

推荐把设备的固定二维码做成：

- `http://harborbeacon.local:4174/setup/mobile`

这要求设备在局域网内：

- 主机名固定为 `harborbeacon`
- 运行 `avahi-daemon`
- 广播 `_http._tcp` 服务

仓库里已提供部署辅助脚本：

- `tools/setup_debian13_local_discovery.sh`
- `tools/install_debian13_services.sh`

运行方式：

- `sudo ./tools/setup_debian13_local_discovery.sh harborbeacon`
- `sudo ./tools/install_debian13_services.sh`

安装后建议直接把下面这个静态二维码地址做成机身贴纸：

- `http://harborbeacon.local:4174/api/binding/static-qr.svg`

## 模型依赖（YOLO）

仓库不会提交 `yolov8n.pt` 这类权重文件；Debian 安装脚本会默认下载并校验 YOLO 模型到本机：

- 默认路径：`/var/lib/harborbeacon/models/yolov8n.pt`
- 默认环境变量：`HARBOR_YOLO_MODEL=/var/lib/harborbeacon/models/yolov8n.pt`

如需跳过模型下载：

- `sudo INSTALL_YOLO_MODEL=0 ./tools/install_debian13_services.sh`

## 服务建议

建议至少长期运行两个本地服务：

- `assistant-task-api`
- `agent-hub-admin-api`

现在 `assistant-task-api` 会常驻在本机 `127.0.0.1:4175`，作为 HarborBeacon -> Home Agent Hub 的本地任务桥接层。外部 IM / 飞书桥接已迁到其他项目，不再作为这个仓库的常驻服务。

其中：

- `assistant-task-api` 负责 HarborBeacon 的 camera domain task 请求、补参续执行和会话状态持久化
- `agent-hub-admin-api` 负责二维码、手机配置页、默认策略和设备库

外部 HarborBeacon / IM bridge 现在需要单独部署，并把它的 Task API 基地址指向：

- `http://127.0.0.1:4175`

本仓库的 Debian 安装脚本不再额外安装旧 `feishu-harbor-bot` systemd 服务。

当前仓库提供了一个最小 bridge 参考启动器：

- `python3 tools/run_harborbeacon_bridge.py --channels-config /etc/harborbeacon/channels.yaml --task-api-url http://127.0.0.1:4175`

这个 runner 现在已经内置：

- Feishu 真实出站发送
- Telegram 真实出站发送
- 其他平台暂时仍以日志 sender 兜底，适合外部 bridge 项目继续补自己的 sender

配置文件示例可参考：

- `examples/harborbeacon.channels.example.yaml`

## 摄像头发现建议

建议分三层：

1. `ONVIF / WS-Discovery`
2. `SSDP / mDNS`
3. `RTSP 端口探测 + 常见路径探测`

当前仓库里已经有发现框架边界：

- `src/runtime/discovery.rs`
- `src/adapters/onvif.rs`
- `src/adapters/ssdp.rs`
- `src/adapters/mdns.rs`

短期内可先保留 `RTSP Probe` 作为兜底路径，但正式版本应优先补全：

- ONVIF WS-Discovery
- 基于设备唯一 ID 的去重与 IP 更新

## 用户交互建议

正式版本建议把飞书交互收敛为：

1. `扫描摄像头`
2. `发现 3 台候选设备，回复序号接入，或回复“忽略 2”`
3. `接入 1`
4. `这台摄像头需要密码，请回复：密码 xxxxxx`
5. `已接入：客厅摄像头，可直接说：看看客厅摄像头`

当前实现已经支持：

- 扫描后生成待确认候选列表
- 回复 `接入 1`
- 回复 `忽略 2`
- 回复 `密码 xxxxxx`
- 这些会话状态持久化到 `.harborbeacon/task-api-conversations.json`，本机 Task API 重启后不会直接丢失

不建议长期保留偏调试风格的原始 ffmpeg / RTSP 错误全文回显。

## 明天部署优先级

1. Debian 13 上启用固定主机名与 Avahi
2. 用静态二维码访问 `harborbeacon.local`
3. 在手机页填写并保存飞书 Bot 凭证
4. 启动 `assistant-task-api` 与 `agent-hub-admin-api`，确认本机服务正常
5. 部署外部 HarborBeacon / IM bridge，并把它指向 `http://<Debian 机器>:4175`
6. 从 IM 中执行 `扫描摄像头`，再用 `接入 1` / `密码 xxxxxx` 完成首台摄像头接入
