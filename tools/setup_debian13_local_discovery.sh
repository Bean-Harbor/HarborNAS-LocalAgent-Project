#!/usr/bin/env bash
set -euo pipefail

HOSTNAME_VALUE="${1:-harborbeacon}"
HTTP_PORT="${HARBOR_HTTP_PORT:-4174}"
SERVICE_DIR="/etc/avahi/services"
SERVICE_FILE="${SERVICE_DIR}/harborbeacon-http.service"

if [[ "${EUID}" -ne 0 ]]; then
  echo "Please run as root: sudo $0 [hostname]"
  exit 1
fi

apt-get update
apt-get install -y avahi-daemon avahi-utils

hostnamectl set-hostname "${HOSTNAME_VALUE}"

mkdir -p "${SERVICE_DIR}"
cat > "${SERVICE_FILE}" <<EOF
<?xml version="1.0" standalone='no'?>
<!DOCTYPE service-group SYSTEM "avahi-service.dtd">
<service-group>
  <name replace-wildcards="yes">%h</name>
  <service>
    <type>_http._tcp</type>
    <port>${HTTP_PORT}</port>
    <txt-record>path=/setup/mobile</txt-record>
    <txt-record>product=HarborBeacon</txt-record>
  </service>
</service-group>
EOF

systemctl enable --now avahi-daemon
systemctl restart avahi-daemon

echo
echo "Local discovery configured."
echo "Static onboarding URL:"
echo "  http://${HOSTNAME_VALUE}.local:${HTTP_PORT}/setup/mobile"
echo
echo "Static QR SVG (after admin API starts):"
echo "  http://${HOSTNAME_VALUE}.local:${HTTP_PORT}/api/binding/static-qr.svg"
echo
echo "Then install long-running services with:"
echo "  sudo ./tools/install_debian13_services.sh"
echo
echo "To skip YOLO model download during install:"
echo "  sudo INSTALL_YOLO_MODEL=0 ./tools/install_debian13_services.sh"
echo
echo "Use this URL to generate a static QR sticker for the device."
