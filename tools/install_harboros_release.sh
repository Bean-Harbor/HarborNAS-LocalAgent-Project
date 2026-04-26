#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: install_harboros_release.sh --bundle PATH [options]

Options:
  --bundle PATH          Path to harbor-release-<version>.tar.gz or extracted bundle dir
  --install-root PATH    Exec-capable install root (default: /var/lib/harborbeacon-agent-ci)
  --writable-root PATH   HarborOS writable root (default: /mnt/software/harborbeacon-agent-ci when available; otherwise <install-root>/writable)
  --service-user USER    systemd service user (default: sudo caller)
  --hostname NAME        Public hostname hint for HarborDesk (default: harborbeacon)
  --env-file PATH        Environment file path (default: /etc/default/harborbeacon-agent-hub)
  --service-token TOKEN  Shared bearer token for HarborBeacon <-> HarborGate traffic
  --public-origin URL    HarborDesk public origin override
  --gateway-public-origin URL  HarborGate public origin override
  --skip-start           Install/update units but do not restart services
  -h, --help             Show help
EOF
}

require_root() {
  if [[ "${EUID}" -ne 0 ]]; then
    echo "Please run as root: sudo $0 ..." >&2
    exit 1
  fi
}

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "missing required command: $1" >&2
    exit 127
  fi
}

random_token() {
  python3 - <<'PY'
import secrets
print(secrets.token_urlsafe(32))
PY
}

default_writable_root() {
  if [[ -d "/mnt/software" || ( -d "/mnt" && -w "/mnt" ) ]]; then
    echo "/mnt/software/harborbeacon-agent-ci"
  else
    echo "${INSTALL_ROOT}/writable"
  fi
}

resolve_ffmpeg_bin() {
  local candidate
  for candidate in "$@"; do
    if [[ -n "${candidate}" && -f "${candidate}" ]] && "${candidate}" -version >/dev/null 2>&1; then
      echo "${candidate}"
      return 0
    fi
  done
  if command -v ffmpeg >/dev/null 2>&1; then
    candidate="$(command -v ffmpeg)"
    if "${candidate}" -version >/dev/null 2>&1; then
      echo "${candidate}"
      return 0
    fi
  fi
  return 1
}

DEFAULT_INSTALL_ROOT="/var/lib/harborbeacon-agent-ci"
INSTALL_ROOT="${DEFAULT_INSTALL_ROOT}"
WRITABLE_ROOT=""
SERVICE_USER="${SUDO_USER:-$(id -un)}"
HOSTNAME_VALUE="harborbeacon"
ENV_FILE="/etc/default/harborbeacon-agent-hub"
SERVICE_TOKEN=""
PUBLIC_ORIGIN=""
GATEWAY_PUBLIC_ORIGIN=""
SKIP_START=0
BUNDLE_PATH=""
INSTALL_ROOT_SET=0
WRITABLE_ROOT_SET=0

CORE_SERVICES=(
  harbor-model-api.service
  assistant-task-api.service
  agent-hub-admin-api.service
  harborgate.service
)
OPTIONAL_WEIXIN_SERVICE="harborgate-weixin-runner.service"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --bundle)
      BUNDLE_PATH="$2"
      shift 2
      ;;
    --install-root)
      INSTALL_ROOT="$2"
      INSTALL_ROOT_SET=1
      shift 2
      ;;
    --writable-root)
      WRITABLE_ROOT="$2"
      WRITABLE_ROOT_SET=1
      shift 2
      ;;
    --service-user)
      SERVICE_USER="$2"
      shift 2
      ;;
    --hostname)
      HOSTNAME_VALUE="$2"
      shift 2
      ;;
    --env-file)
      ENV_FILE="$2"
      shift 2
      ;;
    --service-token)
      SERVICE_TOKEN="$2"
      shift 2
      ;;
    --public-origin)
      PUBLIC_ORIGIN="$2"
      shift 2
      ;;
    --gateway-public-origin)
      GATEWAY_PUBLIC_ORIGIN="$2"
      shift 2
      ;;
    --skip-start)
      SKIP_START=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

require_root
require_command python3
require_command systemctl
require_command tar
require_command sha256sum

if [[ -z "${BUNDLE_PATH}" ]]; then
  echo "--bundle is required" >&2
  usage >&2
  exit 2
fi

if ! id "${SERVICE_USER}" >/dev/null 2>&1; then
  echo "service user not found: ${SERVICE_USER}" >&2
  exit 2
fi

TEMP_DIR="$(mktemp -d)"
cleanup() {
  rm -rf "${TEMP_DIR}"
}
trap cleanup EXIT

if [[ -d "${BUNDLE_PATH}" ]]; then
  cp -R "${BUNDLE_PATH}" "${TEMP_DIR}/bundle"
  BUNDLE_DIR="${TEMP_DIR}/bundle"
elif [[ -f "${BUNDLE_PATH}" ]]; then
  tar -C "${TEMP_DIR}" -xzf "${BUNDLE_PATH}"
  BUNDLE_DIR="$(find "${TEMP_DIR}" -mindepth 1 -maxdepth 1 -type d | head -n 1)"
else
  echo "bundle path not found: ${BUNDLE_PATH}" >&2
  exit 2
fi

if [[ -z "${BUNDLE_DIR:-}" || ! -f "${BUNDLE_DIR}/manifest.json" ]]; then
  echo "bundle manifest not found under ${BUNDLE_PATH}" >&2
  exit 1
fi

if [[ -f "${BUNDLE_DIR}/checksums.sha256" ]]; then
  (
    cd "${BUNDLE_DIR}"
    sha256sum -c checksums.sha256
  )
fi

VERSION="$(python3 - "${BUNDLE_DIR}/manifest.json" <<'PY'
import json
import pathlib
import sys
print(json.loads(pathlib.Path(sys.argv[1]).read_text(encoding="utf-8")).get("version", "").strip())
PY
)"

if [[ -z "${VERSION}" ]]; then
  echo "failed to resolve release version from bundle manifest" >&2
  exit 1
fi

if [[ -f "${ENV_FILE}" ]]; then
  set -a
  # shellcheck disable=SC1090
  . "${ENV_FILE}"
  set +a
fi

EXISTING_WEIXIN_ACCOUNT_ID="${WEIXIN_ACCOUNT_ID:-}"
EXISTING_WEIXIN_BOT_TOKEN="${WEIXIN_BOT_TOKEN:-}"
EXISTING_WEIXIN_BASE_URL="${WEIXIN_BASE_URL:-}"
EXISTING_WEIXIN_USER_ID="${WEIXIN_USER_ID:-}"
EXISTING_HARBOROS_USER="${HARBOR_HARBOROS_USER:-}"
EXISTING_RELEASE_INSTALL_ROOT="${HARBOR_RELEASE_INSTALL_ROOT:-}"
EXISTING_WRITABLE_ROOT="${HARBOR_HARBOROS_WRITABLE_ROOT:-}"
EXISTING_KNOWLEDGE_INDEX_ROOT="${HARBOR_KNOWLEDGE_INDEX_ROOT:-}"
EXISTING_FFMPEG_BIN="${HARBOR_FFMPEG_BIN:-}"
EXISTING_MODEL_API_BACKEND="${HARBOR_MODEL_API_BACKEND:-}"
EXISTING_MODEL_API_UPSTREAM_BASE_URL="${HARBOR_MODEL_API_UPSTREAM_BASE_URL:-}"
EXISTING_MODEL_API_CHAT_MODEL="${HARBOR_MODEL_API_CHAT_MODEL:-}"
EXISTING_MODEL_API_EMBEDDING_MODEL="${HARBOR_MODEL_API_EMBEDDING_MODEL:-}"
EXISTING_MODEL_API_REQUEST_TIMEOUT_MS="${HARBOR_MODEL_API_REQUEST_TIMEOUT_MS:-}"
EXISTING_MODEL_API_CANDLE_CHAT_MODEL_ID="${HARBOR_MODEL_API_CANDLE_CHAT_MODEL_ID:-${HARBOR_MODEL_API_CANDLE_MODEL_ID:-}}"
EXISTING_MODEL_API_CANDLE_EMBEDDING_MODEL_ID="${HARBOR_MODEL_API_CANDLE_EMBEDDING_MODEL_ID:-}"
EXISTING_MODEL_API_CANDLE_CACHE_DIR="${HARBOR_MODEL_API_CANDLE_CACHE_DIR:-}"

if [[ "${INSTALL_ROOT_SET}" -ne 1 && -n "${EXISTING_RELEASE_INSTALL_ROOT}" ]]; then
  INSTALL_ROOT="${EXISTING_RELEASE_INSTALL_ROOT}"
fi

if [[ "${WRITABLE_ROOT_SET}" -ne 1 ]]; then
  if [[ -n "${EXISTING_WRITABLE_ROOT}" ]]; then
    WRITABLE_ROOT="${EXISTING_WRITABLE_ROOT}"
  else
    WRITABLE_ROOT="$(default_writable_root)"
  fi
fi

FFMPEG_BIN="$(resolve_ffmpeg_bin \
  "${EXISTING_FFMPEG_BIN}" \
  "${INSTALL_ROOT}/runtime/media-tools/bin/ffmpeg" \
  "${WRITABLE_ROOT}/media-tools/bin/ffmpeg" || true)"

SERVICE_TOKEN="${SERVICE_TOKEN:-${HARBOR_TASK_API_BEARER_TOKEN:-${HARBORGATE_BEARER_TOKEN:-${IM_AGENT_SERVICE_TOKEN:-}}}}"
if [[ -z "${SERVICE_TOKEN}" ]]; then
  SERVICE_TOKEN="$(random_token)"
fi

PUBLIC_ORIGIN="${PUBLIC_ORIGIN:-${HARBOR_PUBLIC_ORIGIN:-http://${HOSTNAME_VALUE}.local:4174}}"
GATEWAY_PUBLIC_ORIGIN="${GATEWAY_PUBLIC_ORIGIN:-${IM_AGENT_PUBLIC_ORIGIN:-http://${HOSTNAME_VALUE}.local:8787}}"
HARBOROS_PRINCIPAL="${EXISTING_HARBOROS_USER:-${SERVICE_USER}}"

RELEASES_DIR="${INSTALL_ROOT}/releases"
RUNTIME_DIR="${INSTALL_ROOT}/runtime"
CAPTURES_DIR="${INSTALL_ROOT}/captures"
LOGS_DIR="${INSTALL_ROOT}/logs"
CURRENT_LINK="${INSTALL_ROOT}/current"
RELEASE_DIR="${RELEASES_DIR}/${VERSION}"
STATUS_HELPER_LINK="${INSTALL_ROOT}/bin/harbor-agent-hub-helper"

mkdir -p \
  "${RELEASES_DIR}" \
  "${RUNTIME_DIR}" \
  "${CAPTURES_DIR}" \
  "${LOGS_DIR}" \
  "${WRITABLE_ROOT}" \
  "$(dirname "${ENV_FILE}")" \
  "$(dirname "${STATUS_HELPER_LINK}")"
rm -rf "${RELEASE_DIR}"
mkdir -p "${RELEASE_DIR}"
cp -R "${BUNDLE_DIR}/." "${RELEASE_DIR}/"

rm -f "${CURRENT_LINK}"
ln -sfn "${RELEASE_DIR}" "${CURRENT_LINK}"
ln -sfn "${CURRENT_LINK}/templates/bin/harbor-agent-hub-helper" "${STATUS_HELPER_LINK}"

mkdir -p "${RUNTIME_DIR}/harborgate" "${RUNTIME_DIR}/models"
chown -R "${SERVICE_USER}:${SERVICE_USER}" "${RUNTIME_DIR}" "${CAPTURES_DIR}" "${LOGS_DIR}" "${WRITABLE_ROOT}"

export TEMPLATE_INSTALL_ROOT="${INSTALL_ROOT}"
export TEMPLATE_WRITABLE_ROOT="${WRITABLE_ROOT}"
export TEMPLATE_ENV_FILE="${ENV_FILE}"
export TEMPLATE_SERVICE_USER="${SERVICE_USER}"

render_template() {
  local template_path="$1"
  local output_path="$2"
  python3 - "$template_path" "$output_path" <<'PY'
import os
import pathlib
import sys

template_path = pathlib.Path(sys.argv[1])
output_path = pathlib.Path(sys.argv[2])
payload = template_path.read_text(encoding="utf-8")
payload = payload.replace("__INSTALL_ROOT__", os.environ["TEMPLATE_INSTALL_ROOT"])
payload = payload.replace("__WRITABLE_ROOT__", os.environ["TEMPLATE_WRITABLE_ROOT"])
payload = payload.replace("__ENV_FILE__", os.environ["TEMPLATE_ENV_FILE"])
payload = payload.replace("__SERVICE_USER__", os.environ["TEMPLATE_SERVICE_USER"])
output_path.write_text(payload, encoding="utf-8")
PY
}

render_template "${RELEASE_DIR}/templates/systemd/assistant-task-api.service.template" "/etc/systemd/system/assistant-task-api.service"
render_template "${RELEASE_DIR}/templates/systemd/agent-hub-admin-api.service.template" "/etc/systemd/system/agent-hub-admin-api.service"
render_template "${RELEASE_DIR}/templates/systemd/harbor-model-api.service.template" "/etc/systemd/system/harbor-model-api.service"
render_template "${RELEASE_DIR}/templates/systemd/harborgate.service.template" "/etc/systemd/system/harborgate.service"
render_template "${RELEASE_DIR}/templates/systemd/harborgate-weixin-runner.service.template" "/etc/systemd/system/harborgate-weixin-runner.service"

append_optional_env() {
  local key="$1"
  local value="${2:-}"
  if [[ -n "${value}" ]]; then
    printf '%s=%s\n' "${key}" "${value}" >> "${ENV_FILE}"
  fi
}

cat > "${ENV_FILE}" <<EOF
# HarborBeacon / HarborDesk / HarborGate release runtime
WORKSPACE_ROOT=${CURRENT_LINK}
HARBOR_HTTP_BIND=0.0.0.0:4174
HARBOR_PUBLIC_ORIGIN=${PUBLIC_ORIGIN}
HARBORDESK_DIST=${CURRENT_LINK}/harbordesk/dist/harbordesk
HARBOR_HARBOROS_USER=${HARBOROS_PRINCIPAL}
HARBOR_HARBOROS_WRITABLE_ROOT=${WRITABLE_ROOT}
HARBOR_KNOWLEDGE_INDEX_ROOT=${EXISTING_KNOWLEDGE_INDEX_ROOT:-${WRITABLE_ROOT}/knowledge-index}

HARBOR_MODEL_API_BIND=127.0.0.1:4176
HARBOR_MODEL_API_BASE_URL=http://127.0.0.1:4176/v1
HARBOR_MODEL_API_TOKEN=${SERVICE_TOKEN}
HARBOR_MODEL_API_BACKEND=${EXISTING_MODEL_API_BACKEND:-openai_proxy}
HARBOR_MODEL_API_UPSTREAM_BASE_URL=${EXISTING_MODEL_API_UPSTREAM_BASE_URL:-http://127.0.0.1:11434/v1}
HARBOR_MODEL_API_CHAT_MODEL=${EXISTING_MODEL_API_CHAT_MODEL:-harbor-local-chat}
HARBOR_MODEL_API_EMBEDDING_MODEL=${EXISTING_MODEL_API_EMBEDDING_MODEL:-harbor-local-embed}
HARBOR_MODEL_API_REQUEST_TIMEOUT_MS=${EXISTING_MODEL_API_REQUEST_TIMEOUT_MS:-30000}

HARBOR_TASK_API_BIND=127.0.0.1:4175
HARBOR_TASK_API_URL=http://127.0.0.1:4175
HARBOR_TASK_API_ADMIN_STATE=${RUNTIME_DIR}/admin-console.json
HARBOR_TASK_API_DEVICE_REGISTRY=${RUNTIME_DIR}/device-registry.json
HARBOR_TASK_API_CONVERSATIONS=${RUNTIME_DIR}/task-api-conversations.json
HARBOR_TASK_API_BEARER_TOKEN=${SERVICE_TOKEN}

HARBORGATE_BASE_URL=http://127.0.0.1:8787
HARBORGATE_BEARER_TOKEN=${SERVICE_TOKEN}
IM_AGENT_SERVICE_TOKEN=${SERVICE_TOKEN}
IM_AGENT_CONTRACT_VERSION=2.0
IM_AGENT_HOST=127.0.0.1
IM_AGENT_PORT=8787
IM_AGENT_DATA_DIR=${RUNTIME_DIR}/harborgate/sessions
IM_AGENT_STATE_DIR=${RUNTIME_DIR}/harborgate
IM_AGENT_PUBLIC_ORIGIN=${GATEWAY_PUBLIC_ORIGIN}
WEIXIN_STATE_DIR=${RUNTIME_DIR}/harborgate/weixin

HARBORBEACON_TASK_API_URL=http://127.0.0.1:4175
HARBORBEACON_TASK_API_TOKEN=${SERVICE_TOKEN}
HARBORBEACON_ADMIN_API_URL=http://127.0.0.1:4174
HARBORBEACON_ADMIN_API_TOKEN=${SERVICE_TOKEN}

HARBOR_RELEASE_INSTALL_ROOT=${INSTALL_ROOT}
HARBOR_RELEASE_VERSION=${VERSION}
HARBOR_LOG_ROOT=${LOGS_DIR}
HARBOR_CAPTURE_ROOT=${CAPTURES_DIR}
EOF

append_optional_env "WEIXIN_ACCOUNT_ID" "${EXISTING_WEIXIN_ACCOUNT_ID}"
append_optional_env "WEIXIN_BOT_TOKEN" "${EXISTING_WEIXIN_BOT_TOKEN}"
append_optional_env "WEIXIN_BASE_URL" "${EXISTING_WEIXIN_BASE_URL}"
append_optional_env "WEIXIN_USER_ID" "${EXISTING_WEIXIN_USER_ID}"
append_optional_env "HARBOR_FFMPEG_BIN" "${FFMPEG_BIN}"
append_optional_env "HARBOR_MODEL_API_CANDLE_CHAT_MODEL_ID" "${EXISTING_MODEL_API_CANDLE_CHAT_MODEL_ID}"
append_optional_env "HARBOR_MODEL_API_CANDLE_EMBEDDING_MODEL_ID" "${EXISTING_MODEL_API_CANDLE_EMBEDDING_MODEL_ID}"
append_optional_env "HARBOR_MODEL_API_CANDLE_CACHE_DIR" "${EXISTING_MODEL_API_CANDLE_CACHE_DIR}"

chmod 0644 \
  "${ENV_FILE}" \
  /etc/systemd/system/harbor-model-api.service \
  /etc/systemd/system/assistant-task-api.service \
  /etc/systemd/system/agent-hub-admin-api.service \
  /etc/systemd/system/harborgate.service \
  /etc/systemd/system/harborgate-weixin-runner.service
find "${RELEASE_DIR}/templates/bin" -type f -exec chmod 0755 {} +

systemctl daemon-reload
systemctl enable "${CORE_SERVICES[@]}"

if [[ "${SKIP_START}" -ne 1 ]]; then
  systemctl restart "${CORE_SERVICES[@]}"
  CORE_SERVICE_STATUS="enabled and restarted"
else
  CORE_SERVICE_STATUS="enabled, start skipped"
fi

if [[ -n "${EXISTING_WEIXIN_ACCOUNT_ID}" ]]; then
  systemctl enable "${OPTIONAL_WEIXIN_SERVICE}"
  if [[ "${SKIP_START}" -ne 1 ]]; then
    systemctl restart "${OPTIONAL_WEIXIN_SERVICE}"
    WEIXIN_RUNNER_STATUS="configured, enabled and restarted"
  else
    WEIXIN_RUNNER_STATUS="configured, enabled, start skipped"
  fi
else
  systemctl disable "${OPTIONAL_WEIXIN_SERVICE}" >/dev/null 2>&1 || true
  systemctl stop "${OPTIONAL_WEIXIN_SERVICE}" >/dev/null 2>&1 || true
  WEIXIN_RUNNER_STATUS="not configured, skipped (unit installed, service disabled/stopped)"
fi

echo
echo "HarborOS release installed."
echo "Version      : ${VERSION}"
echo "Install root : ${INSTALL_ROOT}"
echo "Writable root: ${WRITABLE_ROOT}"
echo "Current link : ${CURRENT_LINK}"
echo "Env file     : ${ENV_FILE}"
echo "Service user : ${SERVICE_USER}"
echo "Core services: ${CORE_SERVICE_STATUS}"
echo "Optional     : harborgate-weixin-runner -> ${WEIXIN_RUNNER_STATUS}"
echo "Helper       : ${STATUS_HELPER_LINK}"
echo "Quick checks : ${STATUS_HELPER_LINK} status | ${STATUS_HELPER_LINK} health | ${STATUS_HELPER_LINK} logs gateway"
