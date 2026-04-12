#!/bin/sh
set -eu

ROOT_DIR="$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)"
DEFAULT_VENV_PY="$ROOT_DIR/.harbornas/.venv-vision/bin/python"
DETECTOR="$ROOT_DIR/tools/detect_person_yolo.py"

if [ -n "${HARBOR_VISION_PYTHON:-}" ]; then
  PYTHON_BIN="$HARBOR_VISION_PYTHON"
elif [ -x "$DEFAULT_VENV_PY" ]; then
  PYTHON_BIN="$DEFAULT_VENV_PY"
elif command -v /usr/local/bin/python3 >/dev/null 2>&1; then
  PYTHON_BIN="/usr/local/bin/python3"
else
  PYTHON_BIN="python3"
fi

if [ "$(uname -s)" = "Darwin" ] && [ "$(sysctl -in hw.optional.arm64 2>/dev/null || echo 0)" = "1" ]; then
  exec env -i \
    HOME="${HOME:-}" \
    PATH="${PATH:-/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin}" \
    LANG="${LANG:-en_US.UTF-8}" \
    /usr/bin/arch -arm64 "$PYTHON_BIN" "$DETECTOR" "$@"
fi

exec env -i \
  HOME="${HOME:-}" \
  PATH="${PATH:-/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin}" \
  LANG="${LANG:-en_US.UTF-8}" \
  "$PYTHON_BIN" "$DETECTOR" "$@"
