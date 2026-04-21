#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: rollback_harboros_release.sh [options]

Options:
  --install-root PATH  Install root (default: /var/lib/harborbeacon-agent-ci)
  --env-file PATH      Environment file to keep release metadata aligned (default: /etc/default/harborbeacon-agent-hub)
  --version VERSION    Explicit release version to reactivate
  --skip-start         Update current symlink only; do not restart services
  -h, --help           Show help
EOF
}

INSTALL_ROOT="/var/lib/harborbeacon-agent-ci"
ENV_FILE="/etc/default/harborbeacon-agent-hub"
TARGET_VERSION=""
SKIP_START=0
CORE_SERVICES=(
  assistant-task-api.service
  agent-hub-admin-api.service
  harborgate.service
)
OPTIONAL_WEIXIN_SERVICE="harborgate-weixin-runner.service"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --install-root)
      INSTALL_ROOT="$2"
      shift 2
      ;;
    --env-file)
      ENV_FILE="$2"
      shift 2
      ;;
    --version)
      TARGET_VERSION="$2"
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

if [[ "${EUID}" -ne 0 ]]; then
  echo "Please run as root: sudo $0 ..." >&2
  exit 1
fi

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "missing required command: $1" >&2
    exit 127
  fi
}

RELEASES_DIR="${INSTALL_ROOT}/releases"
CURRENT_LINK="${INSTALL_ROOT}/current"

if [[ ! -d "${RELEASES_DIR}" ]]; then
  echo "release directory not found: ${RELEASES_DIR}" >&2
  exit 1
fi

if [[ -z "${TARGET_VERSION}" ]]; then
  CURRENT_TARGET="$(readlink -f "${CURRENT_LINK}" || true)"
  mapfile -t RELEASE_NAMES < <(find "${RELEASES_DIR}" -mindepth 1 -maxdepth 1 -type d -printf "%f\n" | sort)
  if [[ "${#RELEASE_NAMES[@]}" -lt 2 ]]; then
    echo "at least two releases are required to roll back automatically" >&2
    exit 1
  fi
  for (( idx=${#RELEASE_NAMES[@]}-1; idx>=0; idx-- )); do
    CANDIDATE="${RELEASES_DIR}/${RELEASE_NAMES[$idx]}"
    if [[ "$(readlink -f "${CANDIDATE}")" != "${CURRENT_TARGET}" ]]; then
      TARGET_VERSION="${RELEASE_NAMES[$idx]}"
      break
    fi
  done
fi

TARGET_DIR="${RELEASES_DIR}/${TARGET_VERSION}"
if [[ ! -d "${TARGET_DIR}" ]]; then
  echo "target release not found: ${TARGET_DIR}" >&2
  exit 1
fi

rm -f "${CURRENT_LINK}"
ln -sfn "${TARGET_DIR}" "${CURRENT_LINK}"

if [[ -f "${ENV_FILE}" ]]; then
  require_command python3
  python3 - "${ENV_FILE}" "${TARGET_VERSION}" "${INSTALL_ROOT}" <<'PY'
from pathlib import Path
import sys

env_path = Path(sys.argv[1])
release_version = sys.argv[2]
install_root = sys.argv[3]
lines = env_path.read_text(encoding="utf-8").splitlines()
updates = {
    "HARBOR_RELEASE_VERSION": release_version,
    "HARBOR_RELEASE_INSTALL_ROOT": install_root,
}
seen = set()
rewritten = []
for line in lines:
    if not line or line.startswith("#") or "=" not in line:
        rewritten.append(line)
        continue
    key, _value = line.split("=", 1)
    if key in updates:
        rewritten.append(f"{key}={updates[key]}")
        seen.add(key)
    else:
        rewritten.append(line)
for key, value in updates.items():
    if key not in seen:
        rewritten.append(f"{key}={value}")
env_path.write_text("\n".join(rewritten) + "\n", encoding="utf-8")
PY
fi

if [[ "${SKIP_START}" -ne 1 ]]; then
  systemctl daemon-reload
  systemctl restart "${CORE_SERVICES[@]}"
  if systemctl is-enabled "${OPTIONAL_WEIXIN_SERVICE}" >/dev/null 2>&1; then
    systemctl restart "${OPTIONAL_WEIXIN_SERVICE}"
  fi
fi

echo
echo "Rollback complete."
echo "Current link : ${CURRENT_LINK}"
echo "Env file     : ${ENV_FILE}"
echo "Version      : ${TARGET_VERSION}"
