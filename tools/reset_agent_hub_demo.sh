#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
STATE_DIR="${ROOT_DIR}/.harborbeacon"
ADMIN_STATE="${STATE_DIR}/admin-console.json"
DEVICE_REGISTRY="${STATE_DIR}/device-registry.json"

mkdir -p "${STATE_DIR}"

cat > "${ADMIN_STATE}" <<'JSON'
{
  "binding": {
    "status": "等待扫码",
    "metric": "等待绑定",
    "bound_user": null,
    "channel": "飞书 HarborBeacon Bot",
    "session_code": "",
    "qr_token": ""
  },
  "defaults": {
    "cidr": "auto",
    "discovery": "RTSP Probe",
    "recording": "按事件录制",
    "capture": "图片 + 摘要",
    "ai": "人体检测 + 中文摘要",
    "feishu_group": "客厅安全群",
    "rtsp_username": "admin",
    "rtsp_password": "",
    "rtsp_port": 554,
    "rtsp_paths": [
      "/ch1/main",
      "/h264/ch1/main/av_stream",
      "/Streaming/Channels/101"
    ]
  },
  "feishu_users": []
}
JSON

printf '[]\n' > "${DEVICE_REGISTRY}"

echo "Reset complete."
echo "Admin state: ${ADMIN_STATE}"
echo "Device registry: ${DEVICE_REGISTRY}"
