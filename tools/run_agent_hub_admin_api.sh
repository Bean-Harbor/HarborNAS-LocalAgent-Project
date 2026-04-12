#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
ENV_FILE="${HARBOR_ENV_FILE:-/etc/default/harbornas-agent-hub}"

if [[ -f "${ENV_FILE}" ]]; then
  set -a
  # shellcheck disable=SC1090
  . "${ENV_FILE}"
  set +a
fi

WORKSPACE_ROOT="${WORKSPACE_ROOT:-${REPO_ROOT}}"
cd "${WORKSPACE_ROOT}"

if [[ -n "${AGENT_HUB_ADMIN_API_BIN:-}" ]]; then
  BIN_PATH="${AGENT_HUB_ADMIN_API_BIN}"
elif [[ -x "${WORKSPACE_ROOT}/target/release/agent-hub-admin-api" ]]; then
  BIN_PATH="${WORKSPACE_ROOT}/target/release/agent-hub-admin-api"
else
  BIN_PATH="${WORKSPACE_ROOT}/target/debug/agent-hub-admin-api"
fi

exec "${BIN_PATH}" \
  --bind "${HARBOR_HTTP_BIND:-0.0.0.0:4174}" \
  --public-origin "${HARBOR_PUBLIC_ORIGIN:-http://harbornas.local:4174}" \
  "$@"
