from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

if __package__ in {None, ""}:
    sys.path.append(str(Path(__file__).resolve().parent))
    from harbor_integration import (
        IntegrationConfig,
        MiddlewareClient,
        MidcliClient,
        CapabilityUnavailableError,
        default_midcli_service_query,
    )
else:
    from .harbor_integration import (
        IntegrationConfig,
        MiddlewareClient,
        MidcliClient,
        CapabilityUnavailableError,
        default_midcli_service_query,
    )


ROOT = Path(__file__).resolve().parent.parent

REQUIRED_FILES = [
    "HarborNAS-LocalAgent-V2-Assistant-Skills-Roadmap.md",
    "HarborNAS-Middleware-Endpoint-Contract-v1.md",
    "HarborNAS-Files-BatchOps-Contract-v1.md",
    "HarborNAS-Planner-TaskDecompose-Contract-v1.md",
    "HarborNAS-Contract-E2E-Test-Plan-v1.md",
    "HarborNAS-CI-Contract-Pipeline-Checklist-v1.md",
    "HarborNAS-GitHub-Actions-Workflow-Draft-v1.md",
]

REQUIRED_MIDDLEWARE_METHODS = [
    "service.query",
    "service.control",
    "filesystem.listdir",
    "filesystem.copy",
    "filesystem.move",
]


def build_checks() -> list[dict[str, object]]:
    checks: list[dict[str, object]] = []

    for relative_path in REQUIRED_FILES:
        path = ROOT / relative_path
        checks.append(
            {
                "name": f"exists:{relative_path}",
                "passed": path.exists(),
                "details": str(path),
            }
        )

    v2_doc = (ROOT / "HarborNAS-LocalAgent-V2-Assistant-Skills-Roadmap.md").read_text(encoding="utf-8")
    files_doc = (ROOT / "HarborNAS-Files-BatchOps-Contract-v1.md").read_text(encoding="utf-8")
    planner_doc = (ROOT / "HarborNAS-Planner-TaskDecompose-Contract-v1.md").read_text(encoding="utf-8")

    checks.extend(
        [
            {
                "name": "route-priority:control-plane-first",
                "passed": all(
                    item in v2_doc
                    for item in [
                        "1. Middleware API executor",
                        "2. MidCLI executor (CLI via `midcli`)",
                        "3. Browser executor",
                        "4. MCP executor (fallback only)",
                    ]
                ),
                "details": "V2 roadmap must define the strict executor order.",
            },
            {
                "name": "files-contract:path-policy",
                "passed": all(
                    item in files_doc
                    for item in [
                        "Allowed read roots",
                        "Allowed write roots",
                        "Denied roots",
                        "command template allowlist",
                    ]
                ),
                "details": "Files contract must define path policy and allowlist constraints.",
            },
            {
                "name": "planner-contract:route-priority",
                "passed": '"route_priority": ["middleware_api", "midcli", "browser", "mcp"]' in planner_doc,
                "details": "Planner contract must preserve the approved route priority order.",
            },
        ]
    )

    return checks


def build_live_checks(config: IntegrationConfig) -> list[dict[str, object]]:
    checks: list[dict[str, object]] = []

    middleware = MiddlewareClient(config)
    if middleware.is_available():
        try:
            methods, _ = middleware.get_methods(target="REST")
            checks.extend(
                {
                    "name": f"middleware-method:{method_name}",
                    "passed": method_name in methods,
                    "skipped": False,
                    "details": "Checked with core.get_methods target=REST.",
                }
                for method_name in REQUIRED_MIDDLEWARE_METHODS
            )
        except Exception as exc:
            checks.append(
                {
                    "name": "middleware-live-probe",
                    "passed": False,
                    "skipped": False,
                    "details": str(exc),
                }
            )
    else:
        checks.append(
            {
                "name": "middleware-live-probe",
                "passed": False,
                "skipped": True,
                "details": f"middleware binary not found: {config.middleware_bin}",
            }
        )

    midcli = MidcliClient(config)
    if midcli.is_available():
        try:
            rows, result = midcli.run_csv_query(default_midcli_service_query(config))
            checks.append(
                {
                    "name": "midcli-service-query",
                    "passed": bool(rows) or "service" in result.stdout.lower(),
                    "skipped": False,
                    "details": default_midcli_service_query(config),
                }
            )
        except Exception as exc:
            checks.append(
                {
                    "name": "midcli-service-query",
                    "passed": False,
                    "skipped": False,
                    "details": str(exc),
                }
            )
    else:
        checks.append(
            {
                "name": "midcli-service-query",
                "passed": False,
                "skipped": True,
                "details": f"midcli binary not found: {config.midcli_bin}",
            }
        )

    return checks


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--report", default="validate-contract-report.json")
    parser.add_argument("--skip-live", action="store_true")
    parser.add_argument("--require-live", action="store_true")
    args = parser.parse_args()

    checks = build_checks()
    if not args.skip_live:
        checks.extend(build_live_checks(IntegrationConfig.from_env()))

    passed = all(check.get("passed") or check.get("skipped") for check in checks)
    live_executed = any(not check.get("skipped") for check in checks if check["name"].startswith(("middleware-", "midcli-")))
    if args.require_live and not live_executed:
        passed = False
        checks.append(
            {
                "name": "live-probe-required",
                "passed": False,
                "skipped": False,
                "details": "--require-live was set but no live middleware/midcli probe executed.",
            }
        )

    payload = {
        "mode": "live-integration" if live_executed else "spec-scaffold",
        "passed": passed,
        "check_count": len(checks),
        "checks": checks,
    }

    Path(args.report).write_text(json.dumps(payload, indent=2), encoding="utf-8")
    if not passed:
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())