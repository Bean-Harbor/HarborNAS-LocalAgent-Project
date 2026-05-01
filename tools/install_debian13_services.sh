#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

HOSTNAME_VALUE="${HARBOR_HOSTNAME:-harborbeacon}"
SERVICE_USER="${SERVICE_USER:-${SUDO_USER:-$(id -un)}}"
WORKSPACE_ROOT="${WORKSPACE_ROOT:-${REPO_ROOT}}"
ENV_FILE="${HARBOR_ENV_FILE:-/etc/default/harborbeacon-agent-hub}"
MODEL_DIR="${MODEL_DIR:-/var/lib/harborbeacon/models}"
SERVICE_TOKEN="${HARBOR_TASK_API_BEARER_TOKEN:-${SERVICE_TOKEN:-dev-local-harborbeacon-token}}"

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
HARBOR_TASK_API_URL=http://127.0.0.1:4174
HARBOR_TASK_API_ADMIN_STATE=.harborbeacon/admin-console.json
HARBOR_TASK_API_DEVICE_REGISTRY=.harborbeacon/device-registry.json
HARBOR_TASK_API_CONVERSATIONS=.harborbeacon/task-api-conversations.json
HARBOR_TASK_API_BEARER_TOKEN=${SERVICE_TOKEN}
HARBORBEACON_WEB_API_URL=http://127.0.0.1:4174
HARBORBEACON_WEB_API_TOKEN=${SERVICE_TOKEN}
HARBORBEACON_TASK_API_URL=http://127.0.0.1:4174
HARBORBEACON_TASK_API_TOKEN=${SERVICE_TOKEN}
HARBORBEACON_ADMIN_API_URL=http://127.0.0.1:4174
HARBORBEACON_ADMIN_API_TOKEN=${SERVICE_TOKEN}
HARBOR_MODEL_API_BASE_URL=http://127.0.0.1:4174/api/inference/v1
HARBOR_MODEL_API_TOKEN=${SERVICE_TOKEN}
HARBORGATE_RUNTIME=python
HARBOR_YOLO_MODEL=${MODEL_DIR}/yolov8n.pt
EOF

cat > /etc/systemd/system/harborbeacon.service <<EOF
[Unit]
Description=HarborBeacon unified API
After=network-online.target avahi-daemon.service
Wants=network-online.target

[Service]
Type=simple
User=${SERVICE_USER}
WorkingDirectory=${WORKSPACE_ROOT}
EnvironmentFile=-${ENV_FILE}
ExecStart=${WORKSPACE_ROOT}/tools/run_harborbeacon_service.sh
Restart=always
RestartSec=3

[Install]
WantedBy=multi-user.target
EOF

chmod 0644 \
  "${ENV_FILE}" \
  /etc/systemd/system/harborbeacon.service
chmod 0755 \
  "${WORKSPACE_ROOT}/tools/run_harborbeacon_service.sh" \
  "${WORKSPACE_ROOT}/tools/fetch_yolo_model.sh"

for legacy_service in \
  feishu-harbor-bot.service \
  assistant-task-api.service \
  agent-hub-admin-api.service \
  harbor-model-api.service \
  harbor-vlm-sidecar.service; do
  systemctl disable --now "${legacy_service}" >/dev/null 2>&1 || true
  rm -f "/etc/systemd/system/${legacy_service}"
done

systemctl daemon-reload
systemctl enable --now harborbeacon.service

echo
echo "Debian services installed."
echo "Environment file:"
echo "  ${ENV_FILE}"
echo
echo "HarborBeacon API:"
echo "  ${HARBOR_TASK_API_URL:-http://127.0.0.1:4174}"
echo
echo "External HarborBeacon / IM bridge:"
echo "  deploy HarborGate separately and point it at ${HARBOR_TASK_API_URL:-http://127.0.0.1:4174}"
echo
echo "Static onboarding URL:"
echo "  http://${HOSTNAME_VALUE}.local:4174/setup/mobile"
echo
echo "Static QR SVG:"
echo "  http://${HOSTNAME_VALUE}.local:4174/api/binding/static-qr.svg"
