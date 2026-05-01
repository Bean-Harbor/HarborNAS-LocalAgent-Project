#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
HARBORGATE_REPO="${HARBORGATE_REPO:-$(cd "${REPO_ROOT}/../HarborGate" && pwd)}"
HARBORDESK_DIST_SOURCE="${HARBORDESK_DIST_SOURCE:-}"
HARBORGATE_RUST_BINARY="${HARBORGATE_RUST_BINARY:-}"
OUT_DIR="${OUT_DIR:-${REPO_ROOT}/dist/release-bundles}"
RUST_TARGET="${RUST_TARGET:-x86_64-unknown-linux-musl}"
RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-stable}"
ZIG_VERSION="${ZIG_VERSION:-0.15.1}"
BOOTSTRAP_BUILDER_IF_NEEDED="${BOOTSTRAP_BUILDER_IF_NEEDED:-0}"
INSTALL_ROOT_DEFAULT="${INSTALL_ROOT_DEFAULT:-/var/lib/harborbeacon-agent-ci}"
WRITABLE_ROOT_DEFAULT="${WRITABLE_ROOT_DEFAULT:-/mnt/software/harborbeacon-agent-ci}"

git_ref_or_snapshot() {
  local repo_path="$1"
  if command -v git >/dev/null 2>&1 && git -C "${repo_path}" rev-parse HEAD >/dev/null 2>&1; then
    git -C "${repo_path}" rev-parse HEAD
  else
    echo "snapshot"
  fi
}

git_short_ref_or_snapshot() {
  local repo_path="$1"
  if command -v git >/dev/null 2>&1 && git -C "${repo_path}" rev-parse --short HEAD >/dev/null 2>&1; then
    git -C "${repo_path}" rev-parse --short HEAD
  else
    echo "snapshot"
  fi
}

default_linkage_for_target() {
  local target="$1"
  if [[ "${target}" == *-musl ]]; then
    echo "static"
  else
    echo "dynamic"
  fi
}

default_portability_expectation() {
  local target="$1"
  if [[ "${target}" == *-musl ]]; then
    echo "portable-linux"
  else
    echo "builder-libc-matched"
  fi
}

RUST_LINKAGE="${RUST_LINKAGE:-$(default_linkage_for_target "${RUST_TARGET}")}"
LINUX_PORTABILITY_EXPECTATION="${LINUX_PORTABILITY_EXPECTATION:-$(default_portability_expectation "${RUST_TARGET}")}"

VERSION="${RELEASE_VERSION:-$(date -u +%Y%m%d-%H%M%S)-$(git_short_ref_or_snapshot "${REPO_ROOT}")}"
BUNDLE_NAME="harbor-release-${VERSION}"
BUNDLE_ROOT="${OUT_DIR}/${BUNDLE_NAME}"
PYBUILD_VENV="${OUT_DIR}/.pybuild-${VERSION}"

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "missing required command: $1" >&2
    exit 127
  fi
}

require_directory() {
  if [[ ! -d "$1" ]]; then
    echo "required directory not found: $1" >&2
    exit 1
  fi
}

resolve_harborgate_rust_binary() {
  local candidate
  if [[ -n "${HARBORGATE_RUST_BINARY}" && -f "${HARBORGATE_RUST_BINARY}" ]]; then
    echo "${HARBORGATE_RUST_BINARY}"
    return 0
  fi
  for candidate in \
    "${HARBORGATE_REPO}/target/${RUST_TARGET}/release/harborgate" \
    "${HARBORGATE_REPO}/target/release/harborgate"; do
    if [[ -f "${candidate}" ]]; then
      echo "${candidate}"
      return 0
    fi
  done
  return 1
}

append_path_front() {
  local entry="$1"
  if [[ -n "${entry}" && -d "${entry}" ]]; then
    case ":${PATH}:" in
      *":${entry}:"*) ;;
      *)
        export PATH="${entry}:${PATH}"
        ;;
    esac
  fi
}

builder_zig_dir() {
  echo "${HOME}/.local/zig/${ZIG_VERSION}/zig-x86_64-linux-${ZIG_VERSION}"
}

prepare_builder_tool_path() {
  append_path_front "${HOME}/.cargo/bin"
  append_path_front "$(builder_zig_dir)"
}

rust_release_dir() {
  echo "${REPO_ROOT}/target/${RUST_TARGET}/release"
}

rust_target_installed() {
  local target="$1"
  local target_libdir
  if ! target_libdir="$(rustc --print target-libdir --target "${target}" 2>/dev/null)"; then
    return 1
  fi
  [[ -d "${target_libdir}" ]] || return 1
  find "${target_libdir}" -maxdepth 1 -type f -name 'libcore-*' | grep -q .
}

bootstrap_builder_if_needed() {
  if [[ "${BOOTSTRAP_BUILDER_IF_NEEDED}" != "1" || "${RUST_TARGET}" != *-musl ]]; then
    return 0
  fi
  "${REPO_ROOT}/tools/bootstrap_release_builder.sh" \
    --rust-target "${RUST_TARGET}" \
    --rustup-toolchain "${RUSTUP_TOOLCHAIN}" \
    --zig-version "${ZIG_VERSION}"
  prepare_builder_tool_path
}

build_rust_binaries() {
  local cargo_args=(
    --release
    --target "${RUST_TARGET}"
    --bin harborbeacon-service
    --bin harbor-model-api
    --bin assistant-task-api
    --bin agent-hub-admin-api
    --bin validate-contract-schemas
    --bin run-e2e-suite
  )
  if [[ "${RUST_TARGET}" == *-musl ]]; then
    cargo zigbuild "${cargo_args[@]}"
  else
    cargo build "${cargo_args[@]}"
  fi
}

assert_binary_linkage() {
  local binary_path="$1"
  local file_output
  file_output="$(file "${binary_path}")"
  case "${RUST_LINKAGE}" in
    static)
      if [[ "${file_output}" != *"statically linked"* && "${file_output}" != *"static-pie linked"* ]]; then
        echo "expected static linkage for ${binary_path}, got: ${file_output}" >&2
        exit 1
      fi
      ;;
    dynamic)
      if [[ "${file_output}" == *"statically linked"* || "${file_output}" == *"static-pie linked"* ]]; then
        echo "expected dynamic linkage for ${binary_path}, got: ${file_output}" >&2
        exit 1
      fi
      ;;
    *)
      echo "unsupported RUST_LINKAGE: ${RUST_LINKAGE}" >&2
      exit 2
      ;;
  esac
}

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "build_release_bundle.sh must run on Linux. Build on the Debian builder, not on HarborOS." >&2
  exit 2
fi

require_command cargo
require_command python3
require_command tar
require_command sha256sum
require_command find
require_command file

prepare_builder_tool_path
bootstrap_builder_if_needed

if [[ "${RUST_TARGET}" == *-musl ]]; then
  require_command cargo-zigbuild
  require_command zig
  if ! rust_target_installed "${RUST_TARGET}"; then
    echo "Rust target ${RUST_TARGET} is not installed. Run ./tools/bootstrap_release_builder.sh or set BOOTSTRAP_BUILDER_IF_NEEDED=1." >&2
    exit 1
  fi
fi

require_directory "${HARBORGATE_REPO}"
require_directory "${REPO_ROOT}/tools/release_templates"

if [[ -n "${HARBORDESK_DIST_SOURCE}" ]]; then
  require_directory "${HARBORDESK_DIST_SOURCE}"
else
  require_command node
  require_command npm
  require_directory "${REPO_ROOT}/frontend/harbordesk"
fi

mkdir -p "${OUT_DIR}"
rm -rf "${BUNDLE_ROOT}" "${PYBUILD_VENV}"
mkdir -p \
  "${BUNDLE_ROOT}/bin" \
  "${BUNDLE_ROOT}/harbordesk/dist" \
  "${BUNDLE_ROOT}/harborgate/bin" \
  "${BUNDLE_ROOT}/harborgate/site-packages" \
  "${BUNDLE_ROOT}/install" \
  "${BUNDLE_ROOT}/templates"

echo
echo "==> Building HarborBeacon release binaries (${RUST_TARGET}, ${RUST_LINKAGE})"
(
  cd "${REPO_ROOT}"
  prepare_builder_tool_path
  build_rust_binaries
)

RUST_RELEASE_DIR="$(rust_release_dir)"
for binary in harborbeacon-service harbor-model-api assistant-task-api agent-hub-admin-api validate-contract-schemas run-e2e-suite; do
  assert_binary_linkage "${RUST_RELEASE_DIR}/${binary}"
done

if [[ -n "${HARBORDESK_DIST_SOURCE}" ]]; then
  echo
  echo "==> Reusing prebuilt HarborDesk Angular dist"
  HARBORDESK_DIST_PATH="${HARBORDESK_DIST_SOURCE}"
else
  echo
  echo "==> Building HarborDesk Angular dist"
  (
    cd "${REPO_ROOT}/frontend/harbordesk"
    npm ci
    npm run build
  )
  HARBORDESK_DIST_PATH="${REPO_ROOT}/frontend/harbordesk/dist/harbordesk"
fi

echo
echo "==> Vendoring HarborGate Python runtime"
python3 -m venv "${PYBUILD_VENV}"
"${PYBUILD_VENV}/bin/python" -m pip install --upgrade pip setuptools wheel
"${PYBUILD_VENV}/bin/python" -m pip install --no-compile --target "${BUNDLE_ROOT}/harborgate/site-packages" "${HARBORGATE_REPO}"
find "${BUNDLE_ROOT}/harborgate/site-packages" -type d -name "__pycache__" -prune -exec rm -rf {} +
HARBORGATE_RUST_BUNDLE_PATH=""
if HARBORGATE_RUST_SOURCE="$(resolve_harborgate_rust_binary)"; then
  cp "${HARBORGATE_RUST_SOURCE}" "${BUNDLE_ROOT}/harborgate/bin/harborgate"
  chmod 0755 "${BUNDLE_ROOT}/harborgate/bin/harborgate"
  HARBORGATE_RUST_BUNDLE_PATH="harborgate/bin/harborgate"
else
  echo "==> HarborGate Rust binary not found; bundle will use Python fallback unless HARBORGATE_RUNTIME=rust is configured"
fi

echo
echo "==> Assembling bundle layout"
cp "${RUST_RELEASE_DIR}/harborbeacon-service" "${BUNDLE_ROOT}/bin/harborbeacon-service"
cp "${RUST_RELEASE_DIR}/assistant-task-api" "${BUNDLE_ROOT}/bin/assistant-task-api"
cp "${RUST_RELEASE_DIR}/agent-hub-admin-api" "${BUNDLE_ROOT}/bin/agent-hub-admin-api"
cp "${RUST_RELEASE_DIR}/harbor-model-api" "${BUNDLE_ROOT}/bin/harbor-model-api"
cp "${RUST_RELEASE_DIR}/validate-contract-schemas" "${BUNDLE_ROOT}/bin/validate-contract-schemas"
cp "${RUST_RELEASE_DIR}/run-e2e-suite" "${BUNDLE_ROOT}/bin/run-e2e-suite"
cp -R "${HARBORDESK_DIST_PATH}" "${BUNDLE_ROOT}/harbordesk/dist/"
cp -R "${REPO_ROOT}/tools/release_templates/." "${BUNDLE_ROOT}/templates/"
cp "${REPO_ROOT}/tools/install_harboros_release.sh" "${BUNDLE_ROOT}/install/install_harboros_release.sh"
cp "${REPO_ROOT}/tools/rollback_harboros_release.sh" "${BUNDLE_ROOT}/install/rollback_harboros_release.sh"

python3 - "${BUNDLE_ROOT}" <<'PY'
import pathlib
import sys

bundle_root = pathlib.Path(sys.argv[1])
for path in [
    bundle_root / "install" / "install_harboros_release.sh",
    bundle_root / "install" / "rollback_harboros_release.sh",
]:
    data = path.read_bytes()
    path.write_bytes(data.replace(b"\r\n", b"\n").replace(b"\r", b"\n"))

for path in (bundle_root / "templates" / "bin").glob("*"):
    if path.is_file():
        data = path.read_bytes()
        path.write_bytes(data.replace(b"\r\n", b"\n").replace(b"\r", b"\n"))
PY

chmod 0755 \
  "${BUNDLE_ROOT}/install/install_harboros_release.sh" \
  "${BUNDLE_ROOT}/install/rollback_harboros_release.sh"

find "${BUNDLE_ROOT}/templates/bin" -type f -exec chmod 0755 {} +

HARBORBEACON_GIT_REF="$(git_ref_or_snapshot "${REPO_ROOT}")"
HARBORGATE_GIT_REF="$(git_ref_or_snapshot "${HARBORGATE_REPO}")"
BUILT_AT_UTC="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
python3 \
  - "${BUNDLE_ROOT}/manifest.json" \
  "${VERSION}" \
  "${BUILT_AT_UTC}" \
  "${HARBORBEACON_GIT_REF}" \
  "${HARBORGATE_GIT_REF}" \
  "${RUST_TARGET}" \
  "${RUST_LINKAGE}" \
  "${LINUX_PORTABILITY_EXPECTATION}" \
  "${INSTALL_ROOT_DEFAULT}" \
  "${WRITABLE_ROOT_DEFAULT}" \
  "${HARBORGATE_RUST_BUNDLE_PATH}" <<'PY'
import json
import pathlib
import sys

manifest_path = pathlib.Path(sys.argv[1])
payload = {
    "bundle_name": manifest_path.parent.name,
    "version": sys.argv[2],
    "built_at_utc": sys.argv[3],
    "components": {
        "harborbeacon": {
            "git_ref": sys.argv[4],
            "rust_target": sys.argv[6],
            "linkage": sys.argv[7],
            "linux_portability_expectation": sys.argv[8],
            "binaries": [
                "bin/harborbeacon-service",
                "bin/harbor-model-api",
                "bin/assistant-task-api",
                "bin/agent-hub-admin-api",
                "bin/validate-contract-schemas",
                "bin/run-e2e-suite",
            ],
            "runtime_launchers": [
                "templates/bin/run-harborbeacon-service",
                "templates/bin/run-harbor-vlm-sidecar",
                "templates/bin/harbor-vlm-sidecar",
            ],
        },
        "harbordesk": {
            "dist": "harbordesk/dist/harbordesk",
        },
        "harborgate": {
            "git_ref": sys.argv[5],
            "rust_binary": sys.argv[11],
            "site_packages": "harborgate/site-packages",
            "python_fallback": "harborgate/site-packages",
            "runtime_selector_env": "HARBORGATE_RUNTIME",
            "launchers": [
                "templates/bin/harborgate",
                "templates/bin/harborgate-weixin-runner",
                "templates/bin/harborgate-weixin-login",
                "templates/bin/harborgate-weixin-ingress-probe",
            ],
        },
    },
    "install": {
        "install_script": "install/install_harboros_release.sh",
        "rollback_script": "install/rollback_harboros_release.sh",
        "install_root_default": sys.argv[9],
        "writable_root_default": sys.argv[10],
        "helper_scripts": [
            "templates/bin/harbor-agent-hub-helper",
        ],
        "service_names": [
            "harborbeacon.service",
            "harborgate.service",
        ],
    },
}
manifest_path.write_text(json.dumps(payload, ensure_ascii=False, indent=2), encoding="utf-8")
PY

(
  cd "${BUNDLE_ROOT}"
  find . -type f ! -name "checksums.sha256" -print0 | sort -z | xargs -0 sha256sum > checksums.sha256
)

TARBALL_PATH="${OUT_DIR}/${BUNDLE_NAME}.tar.gz"
rm -f "${TARBALL_PATH}"
tar -C "${OUT_DIR}" -czf "${TARBALL_PATH}" "${BUNDLE_NAME}"
(
  cd "${OUT_DIR}"
  sha256sum "${BUNDLE_NAME}.tar.gz" > "${BUNDLE_NAME}.tar.gz.sha256"
)

rm -rf "${PYBUILD_VENV}"

echo
echo "Release bundle ready:"
echo "  ${BUNDLE_ROOT}"
echo "Tarball:"
echo "  ${TARBALL_PATH}"
