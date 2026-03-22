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
        discover_source_capabilities,
        default_midcli_filesystem_command,
        default_midcli_service_query,
        file_operation_risk,
        service_operation_risk,
    )
else:
    from .harbor_integration import (
        IntegrationConfig,
        MiddlewareClient,
        MidcliClient,
        discover_source_capabilities,
        default_midcli_filesystem_command,
        default_midcli_service_query,
        file_operation_risk,
        service_operation_risk,
    )


ROOT = Path(__file__).resolve().parent.parent

CAPABILITY_COMMANDS = {
    "service.query": lambda config: default_midcli_service_query(config),
    "service.control": lambda config: f"service start service={config.probe_service}",
    "filesystem.listdir": lambda config: default_midcli_filesystem_command(config),
    "filesystem.copy": lambda config: f"filesystem copy src={config.filesystem_path}/source dst={config.filesystem_path}/target",
    "filesystem.move": lambda config: f"filesystem move src={config.filesystem_path}/source dst={config.filesystem_path}",
}


def live_middleware_capabilities(client: MiddlewareClient) -> dict[str, bool]:
    if not client.is_available():
        return {}

    methods, _ = client.get_methods(target="REST")
    return {
        "service.query": "service.query" in methods,
        "service.control": "service.control" in methods,
        "filesystem.listdir": "filesystem.listdir" in methods,
        "filesystem.copy": "filesystem.copy" in methods,
        "filesystem.move": "filesystem.move" in methods,
    }


def live_midcli_capabilities(client: MidcliClient, config: IntegrationConfig) -> dict[str, bool]:
    if not client.is_available():
        return {}

    capabilities: dict[str, bool] = {}
    for capability, command_factory in CAPABILITY_COMMANDS.items():
        try:
            client.run(command_factory(config), print_template=capability != "service.query")
            capabilities[capability] = True
        except Exception:
            capabilities[capability] = False
    return capabilities


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--harbor-ref", required=True)
    parser.add_argument("--upstream-ref", required=True)
    parser.add_argument("--report", default="drift-matrix-report.json")
    parser.add_argument("--harbor-repo-path")
    parser.add_argument("--upstream-repo-path")
    args = parser.parse_args()

    checks = [
        "HarborNAS-Middleware-Endpoint-Contract-v1.md",
        "HarborNAS-Files-BatchOps-Contract-v1.md",
        "HarborNAS-Planner-TaskDecompose-Contract-v1.md",
    ]
    missing = [name for name in checks if not (ROOT / name).exists()]

    config = IntegrationConfig.from_env()
    harbor_repo_path = args.harbor_repo_path or config.harbor_repo_path
    upstream_repo_path = args.upstream_repo_path or config.upstream_repo_path

    middleware_caps = live_middleware_capabilities(MiddlewareClient(config))
    midcli_caps = live_midcli_capabilities(MidcliClient(config), config)
    harbor_source_caps = discover_source_capabilities(harbor_repo_path)
    upstream_source_caps = discover_source_capabilities(upstream_repo_path)

    rows = [
        {
            "capability": "system.harbor_ops",
            "harbor_ref": args.harbor_ref,
            "upstream_ref": args.upstream_ref,
            "middleware_live": middleware_caps.get("service.query"),
            "midcli_live": midcli_caps.get("service.query"),
            "harbor_source": harbor_source_caps.get("service.query"),
            "upstream_source": upstream_source_caps.get("service.query"),
            "risk_levels": {
                "query": service_operation_risk("status"),
                "control": service_operation_risk("restart"),
            },
            "status": "ok" if middleware_caps.get("service.query") else "missing",
            "blocking": not middleware_caps.get("service.query", False),
        },
        {
            "capability": "files.batch_ops",
            "harbor_ref": args.harbor_ref,
            "upstream_ref": args.upstream_ref,
            "middleware_live": all(middleware_caps.get(name, False) for name in ["filesystem.listdir", "filesystem.copy", "filesystem.move"]),
            "midcli_live": all(midcli_caps.get(name, False) for name in ["filesystem.listdir", "filesystem.copy", "filesystem.move"]),
            "harbor_source": all(harbor_source_caps.get(name, False) for name in ["filesystem.listdir", "filesystem.copy", "filesystem.move"]),
            "upstream_source": all(upstream_source_caps.get(name, False) for name in ["filesystem.listdir", "filesystem.copy", "filesystem.move"]),
            "risk_levels": {
                "copy": file_operation_risk("copy"),
                "move": file_operation_risk("move"),
            },
            "status": "ok" if middleware_caps.get("filesystem.listdir") else "missing",
            "blocking": not middleware_caps.get("filesystem.listdir", False),
        },
        {
            "capability": "planner.task_decompose",
            "harbor_ref": args.harbor_ref,
            "upstream_ref": args.upstream_ref,
            "middleware_live": middleware_caps.get("service.query") and middleware_caps.get("filesystem.listdir"),
            "midcli_live": midcli_caps.get("service.query") and midcli_caps.get("filesystem.listdir"),
            "harbor_source": None,
            "upstream_source": None,
            "status": "derived",
            "blocking": False,
        },
    ]

    blocking_rows = [row for row in rows if row["blocking"]]

    payload = {
        "mode": "live-integration" if middleware_caps or midcli_caps else "spec-scaffold",
        "harbor_ref": args.harbor_ref,
        "upstream_ref": args.upstream_ref,
        "harbor_repo_path": harbor_repo_path,
        "upstream_repo_path": upstream_repo_path,
        "docs_missing": missing,
        "rows": rows,
        "blocking": bool(missing or blocking_rows),
    }

    Path(args.report).write_text(json.dumps(payload, indent=2), encoding="utf-8")
    return 0 if not missing else 1


if __name__ == "__main__":
    sys.exit(main())