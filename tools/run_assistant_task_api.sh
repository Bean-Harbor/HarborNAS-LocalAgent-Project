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

if [[ -n "${ASSISTANT_TASK_API_BIN:-}" ]]; then
  BIN_PATH="${ASSISTANT_TASK_API_BIN}"
elif [[ -x "${WORKSPACE_ROOT}/target/release/assistant-task-api" ]]; then
  BIN_PATH="${WORKSPACE_ROOT}/target/release/assistant-task-api"
else
  BIN_PATH="${WORKSPACE_ROOT}/target/debug/assistant-task-api"
fi

exec "${BIN_PATH}" \
  --bind "${HARBOR_TASK_API_BIND:-127.0.0.1:4175}" \
  --admin-state "${HARBOR_TASK_API_ADMIN_STATE:-.harbornas/admin-console.json}" \
  --device-registry "${HARBOR_TASK_API_DEVICE_REGISTRY:-.harbornas/device-registry.json}" \
  --conversations "${HARBOR_TASK_API_CONVERSATIONS:-.harbornas/task-api-conversations.json}" \
  "$@"
