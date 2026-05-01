#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
ENV_FILE="${HARBOR_ENV_FILE:-/etc/default/harborbeacon-agent-hub}"

if [[ -f "${ENV_FILE}" ]]; then
  set -a
  # shellcheck disable=SC1090
  . "${ENV_FILE}"
  set +a
fi

WORKSPACE_ROOT="${WORKSPACE_ROOT:-${REPO_ROOT}}"
cd "${WORKSPACE_ROOT}"

if [[ -n "${HARBORBEACON_SERVICE_BIN:-}" ]]; then
  BIN_PATH="${HARBORBEACON_SERVICE_BIN}"
elif [[ -x "${WORKSPACE_ROOT}/target/release/harborbeacon-service" ]]; then
  BIN_PATH="${WORKSPACE_ROOT}/target/release/harborbeacon-service"
else
  BIN_PATH="${WORKSPACE_ROOT}/target/debug/harborbeacon-service"
fi

exec "${BIN_PATH}" \
  --bind "${HARBOR_HTTP_BIND:-0.0.0.0:4174}" \
  --public-origin "${HARBOR_PUBLIC_ORIGIN:-http://harborbeacon.local:4174}" \
  --harbordesk-dist "${HARBORDESK_DIST:-${WORKSPACE_ROOT}/frontend/harbordesk/dist/harbordesk}" \
  --admin-state "${HARBOR_TASK_API_ADMIN_STATE:-.harborbeacon/admin-console.json}" \
  --device-registry "${HARBOR_TASK_API_DEVICE_REGISTRY:-.harborbeacon/device-registry.json}" \
  --conversations "${HARBOR_TASK_API_CONVERSATIONS:-.harborbeacon/task-api-conversations.json}" \
  "$@"
