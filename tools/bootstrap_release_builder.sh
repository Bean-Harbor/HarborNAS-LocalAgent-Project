#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: bootstrap_release_builder.sh [options]

Options:
  --rust-target TARGET       Rust target to prepare (default: x86_64-unknown-linux-musl)
  --rustup-toolchain NAME    rustup toolchain to install/use (default: stable)
  --zig-version VERSION      Zig version to install under ~/.local/zig (default: 0.15.1)
  --cargo-zigbuild VERSION   Optional cargo-zigbuild version override
  --verify-only              Check readiness only; do not install anything
  -h, --help                 Show help
EOF
}

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "missing required command: $1" >&2
    exit 127
  fi
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

detect_zig_arch() {
  case "$(uname -m)" in
    x86_64|amd64)
      echo "x86_64"
      ;;
    *)
      echo "unsupported architecture for Zig bootstrap: $(uname -m)" >&2
      exit 2
      ;;
  esac
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

install_rustup_if_missing() {
  append_path_front "${HOME}/.cargo/bin"
  if command -v rustup >/dev/null 2>&1; then
    return 0
  fi
  if [[ "${VERIFY_ONLY}" -eq 1 ]]; then
    echo "rustup missing" >&2
    exit 1
  fi

  require_command curl
  local rustup_init="${TMPDIR:-/tmp}/rustup-init-${USER:-builder}.sh"
  rm -f "${rustup_init}"
  curl -sSf https://sh.rustup.rs -o "${rustup_init}"
  sh "${rustup_init}" -y --profile minimal --default-toolchain "${RUSTUP_TOOLCHAIN}"
  rm -f "${rustup_init}"
  append_path_front "${HOME}/.cargo/bin"
}

configure_rust_target() {
  if rust_target_installed "${RUST_TARGET}"; then
    return 0
  fi
  if [[ "${VERIFY_ONLY}" -eq 1 ]]; then
    echo "rust target not installed: ${RUST_TARGET}" >&2
    exit 1
  fi

  rustup toolchain install "${RUSTUP_TOOLCHAIN}" --profile minimal
  rustup default "${RUSTUP_TOOLCHAIN}"
  rustup target add "${RUST_TARGET}"
}

install_cargo_zigbuild_if_missing() {
  if command -v cargo-zigbuild >/dev/null 2>&1; then
    return 0
  fi
  if [[ "${VERIFY_ONLY}" -eq 1 ]]; then
    echo "cargo-zigbuild missing" >&2
    exit 1
  fi

  if [[ -n "${CARGO_ZIGBUILD_VERSION}" ]]; then
    cargo install cargo-zigbuild --version "${CARGO_ZIGBUILD_VERSION}" --locked
  else
    cargo install cargo-zigbuild --locked
  fi
}

ensure_zig_on_path() {
  local zig_dir="$1"
  append_path_front "${zig_dir}"
  if command -v zig >/dev/null 2>&1; then
    return 0
  fi
  if [[ "${VERIFY_ONLY}" -eq 1 ]]; then
    echo "zig missing" >&2
    exit 1
  fi

  require_command curl
  require_command tar
  local zig_parent
  local zig_tarball
  local zig_arch
  zig_arch="$(detect_zig_arch)"
  zig_parent="$(dirname "${zig_dir}")"
  zig_tarball="${zig_parent}/zig-${zig_arch}-linux-${ZIG_VERSION}.tar.xz"

  mkdir -p "${zig_parent}"
  if [[ ! -f "${zig_tarball}" ]]; then
    curl -L "https://ziglang.org/download/${ZIG_VERSION}/zig-${zig_arch}-linux-${ZIG_VERSION}.tar.xz" -o "${zig_tarball}"
  fi
  rm -rf "${zig_dir}"
  tar -C "${zig_parent}" -xf "${zig_tarball}"
  append_path_front "${zig_dir}"
}

RUST_TARGET="x86_64-unknown-linux-musl"
RUSTUP_TOOLCHAIN="stable"
ZIG_VERSION="0.15.1"
CARGO_ZIGBUILD_VERSION=""
VERIFY_ONLY=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --rust-target)
      RUST_TARGET="$2"
      shift 2
      ;;
    --rustup-toolchain)
      RUSTUP_TOOLCHAIN="$2"
      shift 2
      ;;
    --zig-version)
      ZIG_VERSION="$2"
      shift 2
      ;;
    --cargo-zigbuild)
      CARGO_ZIGBUILD_VERSION="$2"
      shift 2
      ;;
    --verify-only)
      VERIFY_ONLY=1
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

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "bootstrap_release_builder.sh must run on Linux." >&2
  exit 2
fi

append_path_front "${HOME}/.cargo/bin"

install_rustup_if_missing
require_command cargo
require_command rustc

ZIG_DIR="${HOME}/.local/zig/${ZIG_VERSION}/zig-$(detect_zig_arch)-linux-${ZIG_VERSION}"

configure_rust_target
ensure_zig_on_path "${ZIG_DIR}"
install_cargo_zigbuild_if_missing

echo
echo "Release builder ready."
echo "Rust target      : ${RUST_TARGET}"
echo "rustup toolchain : ${RUSTUP_TOOLCHAIN}"
echo "cargo            : $(command -v cargo)"
echo "cargo-zigbuild   : $(command -v cargo-zigbuild)"
echo "zig              : $(command -v zig)"
echo "zig version      : $(zig version)"
echo "Target libdir    : $(rustc --print target-libdir --target "${RUST_TARGET}")"
