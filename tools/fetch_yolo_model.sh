#!/usr/bin/env bash
set -euo pipefail

MODEL_DIR="${MODEL_DIR:-/var/lib/harbornas/models}"
MODEL_NAME="${MODEL_NAME:-yolov8n.pt}"
MODEL_URL="${MODEL_URL:-https://github.com/ultralytics/assets/releases/download/v8.3.0/yolov8n.pt}"

# This sha256 matches the repo's current local copy used during development.
MODEL_SHA256="${MODEL_SHA256:-f59b3d833e2ff32e194b5bb8e08d211dc7c5bdf144b90d2c8412c47ccfc83b36}"

DEST_PATH="${MODEL_DIR}/${MODEL_NAME}"
TMP_PATH="${DEST_PATH}.tmp"

if [[ "${EUID}" -ne 0 ]]; then
  echo "Please run as root: sudo $0"
  exit 1
fi

mkdir -p "${MODEL_DIR}"

if [[ -f "${DEST_PATH}" ]]; then
  if command -v sha256sum >/dev/null 2>&1; then
    EXISTING_SHA="$(sha256sum "${DEST_PATH}" | awk '{print $1}')"
  else
    EXISTING_SHA="$(python3 - <<PY\nimport hashlib\np='${DEST_PATH}'\nh=hashlib.sha256(open(p,'rb').read()).hexdigest()\nprint(h)\nPY)"
  fi

  if [[ "${EXISTING_SHA}" == "${MODEL_SHA256}" ]]; then
    echo "YOLO model already present: ${DEST_PATH}"
    exit 0
  fi

  echo "Existing model hash mismatch, re-downloading: ${DEST_PATH}"
fi

echo "Downloading YOLO model..."
echo "  url : ${MODEL_URL}"
echo "  dest: ${DEST_PATH}"

if command -v curl >/dev/null 2>&1; then
  curl -fL --retry 3 --retry-delay 2 -o "${TMP_PATH}" "${MODEL_URL}"
elif command -v wget >/dev/null 2>&1; then
  wget -O "${TMP_PATH}" "${MODEL_URL}"
else
  echo "Missing downloader: install curl or wget"
  exit 2
fi

if command -v sha256sum >/dev/null 2>&1; then
  DOWNLOADED_SHA="$(sha256sum "${TMP_PATH}" | awk '{print $1}')"
else
  DOWNLOADED_SHA="$(python3 - <<PY\nimport hashlib\np='${TMP_PATH}'\nh=hashlib.sha256(open(p,'rb').read()).hexdigest()\nprint(h)\nPY)"
fi

if [[ "${DOWNLOADED_SHA}" != "${MODEL_SHA256}" ]]; then
  rm -f "${TMP_PATH}"
  echo "SHA256 mismatch for downloaded model."
  echo "  expected: ${MODEL_SHA256}"
  echo "  got     : ${DOWNLOADED_SHA}"
  exit 3
fi

mv "${TMP_PATH}" "${DEST_PATH}"
chmod 0644 "${DEST_PATH}"

echo "YOLO model installed: ${DEST_PATH}"
