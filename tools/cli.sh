#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if [[ -n "${HARBOR_PYTHON_BIN:-}" ]]; then
  PYTHON_BIN="${HARBOR_PYTHON_BIN}"
elif [[ -x "${SCRIPT_DIR}/../.venv/bin/python" ]]; then
  PYTHON_BIN="${SCRIPT_DIR}/../.venv/bin/python"
elif command -v python3 >/dev/null 2>&1; then
  PYTHON_BIN="$(command -v python3)"
elif command -v python >/dev/null 2>&1; then
  PYTHON_BIN="$(command -v python)"
else
  echo "python3 or python is required for tools/harbor_cli_shim.py" >&2
  exit 127
fi

exec "${PYTHON_BIN}" "${SCRIPT_DIR}/harbor_cli_shim.py" "$@"
