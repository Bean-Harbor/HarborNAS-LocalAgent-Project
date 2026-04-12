#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

HOSTNAME_VALUE="${HARBOR_HOSTNAME:-harbornas}"
SERVICE_USER="${SERVICE_USER:-${SUDO_USER:-$(id -un)}}"
WORKSPACE_ROOT="${WORKSPACE_ROOT:-${REPO_ROOT}}"
ENV_FILE="${HARBOR_ENV_FILE:-/etc/default/harbornas-agent-hub}"
MODEL_DIR="${MODEL_DIR:-/var/lib/harbornas/models}"

if [[ "${EUID}" -ne 0 ]]; then
  echo "Please run as root: sudo $0"
  exit 1
fi

"${SCRIPT_DIR}/setup_debian13_local_discovery.sh" "${HOSTNAME_VALUE}"
chmod 0755 "${WORKSPACE_ROOT}/tools/fetch_yolo_model.sh"
if [[ "${INSTALL_YOLO_MODEL:-1}" != "0" ]]; then
  "${WORKSPACE_ROOT}/tools/fetch_yolo_model.sh"
else
  echo "Skipping YOLO model download (INSTALL_YOLO_MODEL=0)"
fi

cat > "${ENV_FILE}" <<EOF
# HarborNAS Agent Hub runtime environment
WORKSPACE_ROOT=${WORKSPACE_ROOT}
HARBOR_HTTP_BIND=0.0.0.0:4174
HARBOR_PUBLIC_ORIGIN=http://${HOSTNAME_VALUE}.local:4174
FEISHU_DOMAIN=https://open.feishu.cn
HARBOR_HOST=192.168.3.172
HARBOR_USER=harboros_admin
HARBOR_PASSWORD=123456
HARBOR_YOLO_MODEL=${MODEL_DIR}/yolov8n.pt
# FEISHU_APP_ID=
# FEISHU_APP_SECRET=
EOF

cat > /etc/systemd/system/agent-hub-admin-api.service <<EOF
[Unit]
Description=HarborNAS Agent Hub Admin API
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

cat > /etc/systemd/system/feishu-harbor-bot.service <<EOF
[Unit]
Description=HarborNAS Feishu Harbor Bot
After=network-online.target agent-hub-admin-api.service
Wants=network-online.target agent-hub-admin-api.service

[Service]
Type=simple
User=${SERVICE_USER}
WorkingDirectory=${WORKSPACE_ROOT}
EnvironmentFile=-${ENV_FILE}
ExecStart=${WORKSPACE_ROOT}/tools/run_feishu_harbor_bot.sh
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
EOF

chmod 0644 "${ENV_FILE}" /etc/systemd/system/agent-hub-admin-api.service /etc/systemd/system/feishu-harbor-bot.service
chmod 0755 \
  "${WORKSPACE_ROOT}/tools/run_agent_hub_admin_api.sh" \
  "${WORKSPACE_ROOT}/tools/run_feishu_harbor_bot.sh" \
  "${WORKSPACE_ROOT}/tools/fetch_yolo_model.sh"

systemctl daemon-reload
systemctl enable --now agent-hub-admin-api.service feishu-harbor-bot.service

echo
echo "Debian services installed."
echo "Environment file:"
echo "  ${ENV_FILE}"
echo
echo "Static onboarding URL:"
echo "  http://${HOSTNAME_VALUE}.local:4174/setup/mobile"
echo
echo "Static QR SVG:"
echo "  http://${HOSTNAME_VALUE}.local:4174/api/binding/static-qr.svg"
