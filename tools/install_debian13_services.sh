#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

HOSTNAME_VALUE="${HARBOR_HOSTNAME:-harborbeacon}"
SERVICE_USER="${SERVICE_USER:-${SUDO_USER:-$(id -un)}}"
WORKSPACE_ROOT="${WORKSPACE_ROOT:-${REPO_ROOT}}"
ENV_FILE="${HARBOR_ENV_FILE:-/etc/default/harborbeacon-agent-hub}"
MODEL_DIR="${MODEL_DIR:-/var/lib/harborbeacon/models}"

if [[ "${EUID}" -ne 0 ]]; then
  echo "Please run as root: sudo $0"
  exit 1
fi

"${SCRIPT_DIR}/setup_debian13_local_discovery.sh" "${HOSTNAME_VALUE}"

# Runtime dependencies for RTSP probing/snapshot + YOLO bridge.
apt-get update
apt-get install -y ffmpeg python3 python3-venv python3-pip curl ca-certificates

chmod 0755 "${WORKSPACE_ROOT}/tools/fetch_yolo_model.sh"
if [[ "${INSTALL_YOLO_MODEL:-1}" != "0" ]]; then
  "${WORKSPACE_ROOT}/tools/fetch_yolo_model.sh"
else
  echo "Skipping YOLO model download (INSTALL_YOLO_MODEL=0)"
fi

# Optional: install YOLO python deps into a local venv for stable runtime.
if [[ "${INSTALL_VISION_DEPS:-1}" != "0" ]]; then
  chmod 0755 "${WORKSPACE_ROOT}/tools/setup_vision_venv.sh"
  "${WORKSPACE_ROOT}/tools/setup_vision_venv.sh" "${SERVICE_USER}"
else
  echo "Skipping vision python deps install (INSTALL_VISION_DEPS=0)"
fi

cat > "${ENV_FILE}" <<EOF
# HarborBeacon runtime environment
WORKSPACE_ROOT=${WORKSPACE_ROOT}
HARBOR_HTTP_BIND=0.0.0.0:4174
HARBOR_PUBLIC_ORIGIN=http://${HOSTNAME_VALUE}.local:4174
HARBOR_TASK_API_BIND=127.0.0.1:4175
HARBOR_TASK_API_URL=http://127.0.0.1:4175
HARBOR_TASK_API_ADMIN_STATE=.harborbeacon/admin-console.json
HARBOR_TASK_API_DEVICE_REGISTRY=.harborbeacon/device-registry.json
HARBOR_TASK_API_CONVERSATIONS=.harborbeacon/task-api-conversations.json
HARBOR_YOLO_MODEL=${MODEL_DIR}/yolov8n.pt
EOF

cat > /etc/systemd/system/assistant-task-api.service <<EOF
[Unit]
Description=HarborBeacon Assistant Task API
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=${SERVICE_USER}
WorkingDirectory=${WORKSPACE_ROOT}
EnvironmentFile=-${ENV_FILE}
ExecStart=${WORKSPACE_ROOT}/tools/run_assistant_task_api.sh
Restart=always
RestartSec=3

[Install]
WantedBy=multi-user.target
EOF

cat > /etc/systemd/system/agent-hub-admin-api.service <<EOF
[Unit]
Description=HarborBeacon Admin API
After=network-online.target avahi-daemon.service
Wants=network-online.target

[Service]
Type=simple
User=${SERVICE_USER}
WorkingDirectory=${WORKSPACE_ROOT}
EnvironmentFile=-${ENV_FILE}
ExecStart=${WORKSPACE_ROOT}/tools/run_agent_hub_admin_api.sh
Restart=always
RestartSec=3

[Install]
WantedBy=multi-user.target
EOF

chmod 0644 \
  "${ENV_FILE}" \
  /etc/systemd/system/assistant-task-api.service \
  /etc/systemd/system/agent-hub-admin-api.service
chmod 0755 \
  "${WORKSPACE_ROOT}/tools/run_assistant_task_api.sh" \
  "${WORKSPACE_ROOT}/tools/run_agent_hub_admin_api.sh" \
  "${WORKSPACE_ROOT}/tools/fetch_yolo_model.sh"

if systemctl list-unit-files --full --all 2>/dev/null | grep -Fq "feishu-harbor-bot.service"; then
  systemctl disable --now feishu-harbor-bot.service || true
fi
rm -f /etc/systemd/system/feishu-harbor-bot.service

systemctl daemon-reload
systemctl enable --now assistant-task-api.service agent-hub-admin-api.service

echo
echo "Debian services installed."
echo "Environment file:"
echo "  ${ENV_FILE}"
echo
echo "Assistant Task API:"
echo "  ${HARBOR_TASK_API_URL:-http://127.0.0.1:4175}"
echo
echo "External HarborBeacon / IM bridge:"
echo "  deploy separately and point it at ${HARBOR_TASK_API_URL:-http://127.0.0.1:4175}"
echo
echo "Static onboarding URL:"
echo "  http://${HOSTNAME_VALUE}.local:4174/setup/mobile"
echo
echo "Static QR SVG:"
echo "  http://${HOSTNAME_VALUE}.local:4174/api/binding/static-qr.svg"
