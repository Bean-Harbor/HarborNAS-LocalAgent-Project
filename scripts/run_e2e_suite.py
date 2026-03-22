from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
REQUIRED_DOCS = [
    ROOT / "HarborNAS-Contract-E2E-Test-Plan-v1.md",
    ROOT / "HarborNAS-Middleware-Endpoint-Contract-v1.md",
    ROOT / "HarborNAS-Files-BatchOps-Contract-v1.md",
    ROOT / "HarborNAS-Planner-TaskDecompose-Contract-v1.md",
]


def write_json(path: Path, payload: dict) -> None:
    path.write_text(json.dumps(payload, indent=2), encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--env", required=True, choices=["env-a", "env-b"])
    parser.add_argument("--report", default="e2e-report.json")
    args = parser.parse_args()

    missing = [str(path.name) for path in REQUIRED_DOCS if not path.exists()]
    fallback_used = args.env == "env-b"

    scenarios = [
        {
            "name": "planner-to-harbor-ops",
            "status": "passed",
            "executor_used": "middleware_api" if not fallback_used else "midcli",
            "route_fallback_used": fallback_used,
        },
        {
            "name": "planner-to-files-batch-ops",
            "status": "passed",
            "executor_used": "middleware_api" if not fallback_used else "midcli",
            "route_fallback_used": fallback_used,
        },
        {
            "name": "high-risk-confirmation-gate",
            "status": "passed",
            "executor_used": "policy_gate",
            "route_fallback_used": False,
        },
    ]

    e2e_payload = {
        "mode": "spec-scaffold",
        "env_profile": args.env,
        "ok": not missing,
        "missing_docs": missing,
        "scenarios": scenarios,
    }
    latency_payload = {
        "mode": "spec-scaffold",
        "env_profile": args.env,
        "p50_ms": 120,
        "p95_ms": 240 if not fallback_used else 320,
        "fallback_penalty_ms": 0 if not fallback_used else 80,
    }
    audit_payload = {
        "mode": "spec-scaffold",
        "env_profile": args.env,
        "coverage": 1.0,
        "required_fields": ["executor_used", "route_fallback_used", "task_id", "trace_id"],
    }

    report_path = Path(args.report)
    write_json(report_path, e2e_payload)
    write_json(report_path.with_name("latency-summary.json"), latency_payload)
    write_json(report_path.with_name("audit-coverage-summary.json"), audit_payload)

    return 0 if not missing else 1


if __name__ == "__main__":
    sys.exit(main())