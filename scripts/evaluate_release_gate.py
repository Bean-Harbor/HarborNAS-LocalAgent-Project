from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("report_path")
    parser.add_argument("--output", default="release-gate-summary.json")
    parser.add_argument("--require-live", action="store_true")
    args = parser.parse_args()

    report = json.loads(Path(args.report_path).read_text(encoding="utf-8"))
    blocking_rows = [row for row in report.get("rows", []) if row.get("blocking")]
    reasons = []

    if report.get("docs_missing"):
        reasons.append("required contract documents are missing")
    if blocking_rows:
        reasons.append("drift matrix contains blocking rows")
    if args.require_live and report.get("mode") != "live-integration":
        reasons.append("live middleware or midcli probes were not executed")

    payload = {
        "mode": report.get("mode", "spec-scaffold"),
        "allowed": not reasons,
        "reasons": reasons,
        "evaluated_rows": len(report.get("rows", [])),
    }

    Path(args.output).write_text(json.dumps(payload, indent=2), encoding="utf-8")
    return 0 if payload["allowed"] else 1


if __name__ == "__main__":
    sys.exit(main())