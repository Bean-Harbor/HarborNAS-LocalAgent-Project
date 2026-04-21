#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: run_harboros_vm_smoke.sh --websocket-url URL --username USER --password PASS [options]

Options:
  --env-name NAME                  E2E environment profile (default: env-a)
  --probe-service NAME             Safe service probe target (default: ssh)
  --filesystem-path PATH           Safe listdir target (default: /mnt)
  --allow-mutations                Enable approved write actions
  --mutation-root PATH             Mutation sandbox root
                                  (default: /mnt/software/harborbeacon-agent-ci)
  --approval-token TOKEN           Approval token for HIGH/CRITICAL actions
  --required-approval-token TOKEN  Optional locally enforced expected token
  --approver-id ID                 Approver identity for audit correlation
  --report-dir PATH                Output directory for reports
  --skip-build                     Reuse existing binaries without cargo build
  --run-drift                      Also run run-drift-matrix
  --drift-harbor-ref REF           Drift Harbor ref (default: develop)
  --drift-upstream-ref REF         Drift upstream ref (default: master)
  --harbor-repo-path PATH          Optional Harbor source tree for drift compare
  --upstream-repo-path PATH        Optional upstream source tree for drift compare
  -h, --help                       Show this help
EOF
}

WEBSOCKET_URL=""
USERNAME=""
PASSWORD=""
ENV_NAME="env-a"
PROBE_SERVICE="ssh"
FILESYSTEM_PATH="/mnt"
ALLOW_MUTATIONS=0
MUTATION_ROOT="/mnt/software/harborbeacon-agent-ci"
APPROVAL_TOKEN=""
REQUIRED_APPROVAL_TOKEN=""
APPROVER_ID=""
REPORT_DIR=""
SKIP_BUILD=0
RUN_DRIFT=0
DRIFT_HARBOR_REF="develop"
DRIFT_UPSTREAM_REF="master"
HARBOR_REPO_PATH=""
UPSTREAM_REPO_PATH=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --websocket-url)
      WEBSOCKET_URL="$2"
      shift 2
      ;;
    --username)
      USERNAME="$2"
      shift 2
      ;;
    --password)
      PASSWORD="$2"
      shift 2
      ;;
    --env-name)
      ENV_NAME="$2"
      shift 2
      ;;
    --probe-service)
      PROBE_SERVICE="$2"
      shift 2
      ;;
    --filesystem-path)
      FILESYSTEM_PATH="$2"
      shift 2
      ;;
    --allow-mutations)
      ALLOW_MUTATIONS=1
      shift
      ;;
    --mutation-root)
      MUTATION_ROOT="$2"
      shift 2
      ;;
    --approval-token)
      APPROVAL_TOKEN="$2"
      shift 2
      ;;
    --required-approval-token)
      REQUIRED_APPROVAL_TOKEN="$2"
      shift 2
      ;;
    --approver-id)
      APPROVER_ID="$2"
      shift 2
      ;;
    --report-dir)
      REPORT_DIR="$2"
      shift 2
      ;;
    --skip-build)
      SKIP_BUILD=1
      shift
      ;;
    --run-drift)
      RUN_DRIFT=1
      shift
      ;;
    --drift-harbor-ref)
      DRIFT_HARBOR_REF="$2"
      shift 2
      ;;
    --drift-upstream-ref)
      DRIFT_UPSTREAM_REF="$2"
      shift 2
      ;;
    --harbor-repo-path)
      HARBOR_REPO_PATH="$2"
      shift 2
      ;;
    --upstream-repo-path)
      UPSTREAM_REPO_PATH="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [[ -z "${WEBSOCKET_URL}" || -z "${USERNAME}" || -z "${PASSWORD}" ]]; then
  echo "--websocket-url, --username, and --password are required" >&2
  usage >&2
  exit 2
fi

if [[ "${WEBSOCKET_URL}" != ws://* && "${WEBSOCKET_URL}" != wss://* ]]; then
  echo "WebSocket URL must start with ws:// or wss://" >&2
  exit 2
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
RELEASE_DIR="${REPO_ROOT}/target/release"
VALIDATE_BIN="${RELEASE_DIR}/validate-contract-schemas"
E2E_BIN="${RELEASE_DIR}/run-e2e-suite"
DRIFT_BIN="${RELEASE_DIR}/run-drift-matrix"

if [[ -z "${REPORT_DIR}" ]]; then
  REPORT_DIR="${REPO_ROOT}/.tmp-live/harboros-vm-smoke"
fi
mkdir -p "${REPORT_DIR}"
TIMESTAMP="$(date +%Y%m%d-%H%M%S)"
VALIDATE_REPORT="${REPORT_DIR}/validate-contract-${TIMESTAMP}.json"
E2E_REPORT="${REPORT_DIR}/e2e-${TIMESTAMP}.json"
DRIFT_REPORT="${REPORT_DIR}/drift-${TIMESTAMP}.json"

if [[ "${SKIP_BUILD}" -ne 1 ]]; then
  if ! command -v cargo >/dev/null 2>&1; then
    echo "cargo is required unless --skip-build is used" >&2
    exit 127
  fi

  echo
  echo "==> Building release binaries"
  CARGO_ARGS=(build --release --bin validate-contract-schemas --bin run-e2e-suite)
  if [[ "${RUN_DRIFT}" -eq 1 ]]; then
    CARGO_ARGS+=(--bin run-drift-matrix)
  fi
  (cd "${REPO_ROOT}" && cargo "${CARGO_ARGS[@]}")
fi

if [[ ! -x "${VALIDATE_BIN}" || ! -x "${E2E_BIN}" ]]; then
  echo "required release binaries are missing; rerun without --skip-build or build them first" >&2
  exit 1
fi

if [[ "${RUN_DRIFT}" -eq 1 && ! -x "${DRIFT_BIN}" ]]; then
  echo "run-drift-matrix is missing; rerun without --skip-build or build it first" >&2
  exit 1
fi

MIDDLEWARE_URI="${WEBSOCKET_URL%/websocket}/api/current"
if [[ "${WEBSOCKET_URL}" == */api/current ]]; then
  MIDDLEWARE_URI="${WEBSOCKET_URL}"
fi

resolve_python() {
  if [[ -n "${HARBOR_PYTHON_BIN:-}" ]]; then
    printf '%s\n' "${HARBOR_PYTHON_BIN}"
    return
  fi
  if [[ -x "${REPO_ROOT}/.venv/bin/python" ]]; then
    printf '%s\n' "${REPO_ROOT}/.venv/bin/python"
    return
  fi
  if command -v python3 >/dev/null 2>&1; then
    command -v python3
    return
  fi
  if command -v python >/dev/null 2>&1; then
    command -v python
    return
  fi
  echo "python3 or python is required for Harbor midcli shim" >&2
  exit 127
}

if [[ -z "${HARBOR_MIDCLI_BIN:-}" ]]; then
  PYTHON_BIN="$(resolve_python)"
  CLI_WRAPPER="${REPORT_DIR}/cli-remote.sh"
  cat > "${CLI_WRAPPER}" <<EOF
#!/usr/bin/env bash
set -euo pipefail
exec "${PYTHON_BIN}" "${REPO_ROOT}/tools/harbor_cli_shim.py" "\$@"
EOF
  chmod +x "${CLI_WRAPPER}"
  export HARBOR_MIDCLI_BIN="${CLI_WRAPPER}"
fi
export HARBOR_MIDCLI_URL="${WEBSOCKET_URL}"
export HARBOR_MIDCLI_USER="${USERNAME}"
export HARBOR_MIDCLI_PASSWORD="${PASSWORD}"
export HARBOR_PROBE_SERVICE="${PROBE_SERVICE}"
export HARBOR_FILESYSTEM_PATH="${FILESYSTEM_PATH}"
export HARBOR_MUTATION_ROOT="${MUTATION_ROOT}"
export HARBOR_MIDCLI_TIMEOUT="${HARBOR_MIDCLI_TIMEOUT:-5000}"
export HARBOR_MIDDLEWARE_TIMEOUT="${HARBOR_MIDDLEWARE_TIMEOUT:-5000}"

if [[ "${ALLOW_MUTATIONS}" -eq 1 ]]; then
  export HARBOR_ALLOW_MUTATIONS=1
else
  unset HARBOR_ALLOW_MUTATIONS || true
fi

if [[ -n "${APPROVAL_TOKEN}" ]]; then
  export HARBOR_APPROVAL_TOKEN="${APPROVAL_TOKEN}"
else
  unset HARBOR_APPROVAL_TOKEN || true
fi

if [[ -n "${REQUIRED_APPROVAL_TOKEN}" ]]; then
  export HARBOR_REQUIRED_APPROVAL_TOKEN="${REQUIRED_APPROVAL_TOKEN}"
else
  unset HARBOR_REQUIRED_APPROVAL_TOKEN || true
fi

if [[ -n "${APPROVER_ID}" ]]; then
  export HARBOR_APPROVER_ID="${APPROVER_ID}"
else
  unset HARBOR_APPROVER_ID || true
fi

if [[ -z "${HARBOR_MIDDLEWARE_BIN:-}" ]] && command -v midclt >/dev/null 2>&1; then
  WRAPPER="${REPORT_DIR}/midclt-remote.sh"
  cat > "${WRAPPER}" <<EOF
#!/usr/bin/env bash
set -euo pipefail
exec "$(command -v midclt)" -u "${MIDDLEWARE_URI}" -U "${USERNAME}" -P "${PASSWORD}" "\$@"
EOF
  chmod +x "${WRAPPER}"
  export HARBOR_MIDDLEWARE_BIN="${WRAPPER}"
fi

if [[ -n "${HARBOR_REPO_PATH}" ]]; then
  export HARBOR_SOURCE_REPO_PATH="${HARBOR_REPO_PATH}"
fi

if [[ -n "${UPSTREAM_REPO_PATH}" ]]; then
  export UPSTREAM_SOURCE_REPO_PATH="${UPSTREAM_REPO_PATH}"
fi

echo "Repo root      : ${REPO_ROOT}"
echo "WebSocket URL  : ${WEBSOCKET_URL}"
echo "Probe service  : ${PROBE_SERVICE}"
echo "Filesystem path: ${FILESYSTEM_PATH}"
echo "Mutations      : $([[ "${ALLOW_MUTATIONS}" -eq 1 ]] && echo true || echo false)"
echo "Mutation root  : ${MUTATION_ROOT}"
echo "Report dir     : ${REPORT_DIR}"
echo "Run drift      : $([[ "${RUN_DRIFT}" -eq 1 ]] && echo true || echo false)"

echo
echo "==> Running validate-contract-schemas live probe"
(cd "${REPO_ROOT}" && "${VALIDATE_BIN}" --require-live --report "${VALIDATE_REPORT}")

echo
echo "==> Running run-e2e-suite live probe"
(cd "${REPO_ROOT}" && "${E2E_BIN}" --env "${ENV_NAME}" --require-live --report "${E2E_REPORT}")

if [[ "${RUN_DRIFT}" -eq 1 ]]; then
  echo
  echo "==> Running run-drift-matrix"
  DRIFT_ARGS=(--harbor-ref "${DRIFT_HARBOR_REF}" --upstream-ref "${DRIFT_UPSTREAM_REF}" --report "${DRIFT_REPORT}")
  if [[ -n "${HARBOR_REPO_PATH}" ]]; then
    DRIFT_ARGS+=(--harbor-repo-path "${HARBOR_REPO_PATH}")
  fi
  if [[ -n "${UPSTREAM_REPO_PATH}" ]]; then
    DRIFT_ARGS+=(--upstream-repo-path "${UPSTREAM_REPO_PATH}")
  fi
  (cd "${REPO_ROOT}" && "${DRIFT_BIN}" "${DRIFT_ARGS[@]}")
fi

echo
echo "Smoke run completed."
echo "Validate report: ${VALIDATE_REPORT}"
echo "E2E report     : ${E2E_REPORT}"
if [[ "${RUN_DRIFT}" -eq 1 ]]; then
  echo "Drift report   : ${DRIFT_REPORT}"
fi
