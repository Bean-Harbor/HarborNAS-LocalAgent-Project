from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path


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


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--report", default="validate-contract-report.json")
    args = parser.parse_args()

    checks = build_checks()
    passed = all(check["passed"] for check in checks)
    payload = {
        "mode": "spec-scaffold",
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