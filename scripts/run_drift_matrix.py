from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--harbor-ref", required=True)
    parser.add_argument("--upstream-ref", required=True)
    parser.add_argument("--report", default="drift-matrix-report.json")
    args = parser.parse_args()

    checks = [
        "HarborNAS-Middleware-Endpoint-Contract-v1.md",
        "HarborNAS-Files-BatchOps-Contract-v1.md",
        "HarborNAS-Planner-TaskDecompose-Contract-v1.md",
    ]
    missing = [name for name in checks if not (ROOT / name).exists()]

    rows = [
        {
            "capability": "system.harbor_ops",
            "harbor_ref": args.harbor_ref,
            "upstream_ref": args.upstream_ref,
            "status": "documented",
            "blocking": False,
        },
        {
            "capability": "files.batch_ops",
            "harbor_ref": args.harbor_ref,
            "upstream_ref": args.upstream_ref,
            "status": "documented",
            "blocking": False,
        },
        {
            "capability": "planner.task_decompose",
            "harbor_ref": args.harbor_ref,
            "upstream_ref": args.upstream_ref,
            "status": "documented",
            "blocking": False,
        },
    ]

    payload = {
        "mode": "spec-scaffold",
        "harbor_ref": args.harbor_ref,
        "upstream_ref": args.upstream_ref,
        "docs_missing": missing,
        "rows": rows,
        "blocking": bool(missing),
    }

    Path(args.report).write_text(json.dumps(payload, indent=2), encoding="utf-8")
    return 0 if not missing else 1


if __name__ == "__main__":
    sys.exit(main())