#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

SERVICE_USER="${1:-}"
VENV_DIR="${VISION_VENV_DIR:-${REPO_ROOT}/.harbornas/.venv-vision}"

if [[ "${EUID}" -ne 0 ]]; then
  echo "Please run as root: sudo $0 [service-user]"
  exit 1
fi

if ! command -v python3 >/dev/null 2>&1; then
  echo "python3 missing; please install python3 first"
  exit 2
fi

mkdir -p "$(dirname "${VENV_DIR}")"
python3 -m venv "${VENV_DIR}"

"${VENV_DIR}/bin/python" -m pip install --upgrade pip setuptools wheel
"${VENV_DIR}/bin/python" -m pip install ultralytics opencv-python-headless

if [[ -n "${SERVICE_USER}" ]]; then
  chown -R "${SERVICE_USER}:${SERVICE_USER}" "$(dirname "${VENV_DIR}")"
fi

echo "Vision venv ready:"
echo "  ${VENV_DIR}"

