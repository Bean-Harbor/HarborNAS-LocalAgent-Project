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

if [[ -n "${FEISHU_HARBOR_BOT_BIN:-}" ]]; then
  BIN_PATH="${FEISHU_HARBOR_BOT_BIN}"
elif [[ -x "${WORKSPACE_ROOT}/target/release/feishu-harbor-bot" ]]; then
  BIN_PATH="${WORKSPACE_ROOT}/target/release/feishu-harbor-bot"
else
  BIN_PATH="${WORKSPACE_ROOT}/target/debug/feishu-harbor-bot"
fi

ARGS=(
  --domain "${FEISHU_DOMAIN:-https://open.feishu.cn}"
  --harbor-host "${HARBOR_HOST:-192.168.3.172}"
  --harbor-user "${HARBOR_USER:-harboros_admin}"
  --harbor-password "${HARBOR_PASSWORD:-123456}"
)

if [[ -n "${FEISHU_APP_ID:-}" ]]; then
  ARGS+=(--app-id "${FEISHU_APP_ID}")
fi
if [[ -n "${FEISHU_APP_SECRET:-}" ]]; then
  ARGS+=(--app-secret "${FEISHU_APP_SECRET}")
fi

exec "${BIN_PATH}" "${ARGS[@]}" "$@"
