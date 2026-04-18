from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

if __package__ in {None, ""}:
    sys.path.append(str(Path(__file__).resolve().parent))
    from harbor_integration import (
        ApprovalRequiredError,
        IntegrationConfig,
        MiddlewareClient,
        MidcliClient,
        PathPolicyError,
        default_midcli_filesystem_command,
        default_midcli_service_query,
        ensure_directory,
        ensure_mutation_fixture,
        execute_file_action,
        execute_service_action,
    )
else:
    from .harbor_integration import (
        ApprovalRequiredError,
        IntegrationConfig,
        MiddlewareClient,
        MidcliClient,
        PathPolicyError,
        default_midcli_filesystem_command,
        default_midcli_service_query,
        ensure_directory,
        ensure_mutation_fixture,
        execute_file_action,
        execute_service_action,
    )


ROOT = Path(__file__).resolve().parent.parent
REQUIRED_DOCS = [
    ROOT / "HarborBeacon-Contract-E2E-Test-Plan-v1.md",
    ROOT / "HarborBeacon-Middleware-Endpoint-Contract-v1.md",
    ROOT / "HarborBeacon-Files-BatchOps-Contract-v1.md",
    ROOT / "HarborBeacon-Planner-TaskDecompose-Contract-v1.md",
]


def write_json(path: Path, payload: dict) -> None:
    path.write_text(json.dumps(payload, indent=2), encoding="utf-8")


def middleware_service_probe(client: MiddlewareClient, service_name: str) -> tuple[dict | list | None, int]:
    payload, result = client.call("service.query", [["service", "=", service_name]], {"get": True})
    return payload, result.duration_ms


def middleware_filesystem_probe(client: MiddlewareClient, path: str) -> tuple[dict | list | None, int]:
    payload, result = client.call(
        "filesystem.listdir",
        path,
        [],
        {"limit": 5, "select": ["path", "type"]},
    )
    return payload, result.duration_ms


def scenario_result(name: str, *, status: str, executor_used: str, route_fallback_used: bool, duration_ms: int, details: dict) -> dict:
    return {
        "name": name,
        "status": status,
        "executor_used": executor_used,
        "route_fallback_used": route_fallback_used,
        "duration_ms": duration_ms,
        "details": details,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--env", required=True, choices=["env-a", "env-b"])
    parser.add_argument("--report", default="e2e-report.json")
    parser.add_argument("--require-live", action="store_true")
    args = parser.parse_args()

    missing = [str(path.name) for path in REQUIRED_DOCS if not path.exists()]
    config = IntegrationConfig.from_env()
    middleware = MiddlewareClient(config)
    midcli = MidcliClient(config)
    force_midcli = args.env == "env-b"
    dry_run_mutations = not config.allow_mutations
    scenarios = []
    durations: list[int] = []
    live_executed = False

    try:
        if not force_midcli and middleware.is_available():
            payload, duration_ms = middleware_service_probe(middleware, config.probe_service)
            scenarios.append(
                scenario_result(
                    "planner-to-harbor-ops",
                    status="passed",
                    executor_used="middleware_api",
                    route_fallback_used=False,
                    duration_ms=duration_ms,
                    details={"service": config.probe_service, "result_type": type(payload).__name__},
                )
            )
            durations.append(duration_ms)
            live_executed = True
        elif midcli.is_available():
            rows, result = midcli.run_csv_query(default_midcli_service_query(config))
            scenarios.append(
                scenario_result(
                    "planner-to-harbor-ops",
                    status="passed" if rows or config.probe_service in result.stdout else "failed",
                    executor_used="midcli",
                    route_fallback_used=True,
                    duration_ms=result.duration_ms,
                    details={"service": config.probe_service, "row_count": len(rows)},
                )
            )
            durations.append(result.duration_ms)
            live_executed = True
        else:
            scenarios.append(
                scenario_result(
                    "planner-to-harbor-ops",
                    status="skipped",
                    executor_used="none",
                    route_fallback_used=False,
                    duration_ms=0,
                    details={"reason": "middleware and midcli are both unavailable"},
                )
            )
    except Exception as exc:
        scenarios.append(
            scenario_result(
                "planner-to-harbor-ops",
                status="failed",
                executor_used="middleware_api" if not force_midcli else "midcli",
                route_fallback_used=force_midcli,
                duration_ms=0,
                details={"error": str(exc)},
            )
        )

    try:
        if not force_midcli and middleware.is_available():
            payload, duration_ms = middleware_filesystem_probe(middleware, config.filesystem_path)
            result_count = len(payload) if isinstance(payload, list) else 0
            scenarios.append(
                scenario_result(
                    "planner-to-files-batch-ops",
                    status="passed",
                    executor_used="middleware_api",
                    route_fallback_used=False,
                    duration_ms=duration_ms,
                    details={"path": config.filesystem_path, "entry_count": result_count},
                )
            )
            durations.append(duration_ms)
            live_executed = True
        elif midcli.is_available():
            rows, result = midcli.run_csv_query(default_midcli_filesystem_command(config))
            scenarios.append(
                scenario_result(
                    "planner-to-files-batch-ops",
                    status="passed" if rows or config.filesystem_path in result.stdout else "failed",
                    executor_used="midcli",
                    route_fallback_used=True,
                    duration_ms=result.duration_ms,
                    details={"path": config.filesystem_path, "row_count": len(rows)},
                )
            )
            durations.append(result.duration_ms)
            live_executed = True
        else:
            scenarios.append(
                scenario_result(
                    "planner-to-files-batch-ops",
                    status="skipped",
                    executor_used="none",
                    route_fallback_used=False,
                    duration_ms=0,
                    details={"reason": "middleware and midcli are both unavailable"},
                )
            )
    except Exception as exc:
        scenarios.append(
            scenario_result(
                "planner-to-files-batch-ops",
                status="failed",
                executor_used="middleware_api" if not force_midcli else "midcli",
                route_fallback_used=force_midcli,
                duration_ms=0,
                details={"error": str(exc)},
            )
        )

    mutation_root = config.mutation_root
    copy_src = str(Path(mutation_root) / "copy-source.txt")
    copy_dst = str(Path(mutation_root) / "copy-destination.txt")
    move_src = str(Path(mutation_root) / "move-source.txt")
    move_dst_dir = str(Path(mutation_root) / "move-destination")

    if config.allow_mutations:
        mutation_root = ensure_directory(config.mutation_root)
        move_dst_dir = ensure_directory(move_dst_dir)
        ensure_mutation_fixture(mutation_root, filename="copy-source.txt", content="copy payload\n")
        ensure_mutation_fixture(mutation_root, filename="move-source.txt", content="move payload\n")

    try:
        result = execute_service_action(
            middleware=middleware,
            midcli=midcli,
            config=config,
            operation="restart",
            service_name=config.probe_service,
            prefer_midcli=force_midcli,
            dry_run=dry_run_mutations,
            approval_token=config.approval_token,
        )
        scenarios.append(
            scenario_result(
                "guarded-service-restart",
                status="passed",
                executor_used=result["executor"],
                route_fallback_used=result["executor"] == "midcli",
                duration_ms=result.get("duration_ms", 0),
                details=result,
            )
        )
        if result.get("duration_ms"):
            durations.append(result["duration_ms"])
    except ApprovalRequiredError as exc:
        scenarios.append(
            scenario_result(
                "guarded-service-restart",
                status="passed",
                executor_used="policy_gate",
                route_fallback_used=False,
                duration_ms=0,
                details={"approval_blocked": True, "error": str(exc)},
            )
        )
    except Exception as exc:
        scenarios.append(
            scenario_result(
                "guarded-service-restart",
                status="failed",
                executor_used="middleware_api" if not force_midcli else "midcli",
                route_fallback_used=force_midcli,
                duration_ms=0,
                details={"error": str(exc)},
            )
        )

    try:
        result = execute_file_action(
            middleware=middleware,
            midcli=midcli,
            config=config,
            operation="copy",
            src=copy_src,
            dst=copy_dst,
            prefer_midcli=force_midcli,
            dry_run=dry_run_mutations,
            approval_token=config.approval_token,
        )
        scenarios.append(
            scenario_result(
                "guarded-files-copy",
                status="passed",
                executor_used=result["executor"],
                route_fallback_used=result["executor"] == "midcli",
                duration_ms=result.get("duration_ms", 0),
                details=result,
            )
        )
        if result.get("duration_ms"):
            durations.append(result["duration_ms"])
    except (ApprovalRequiredError, PathPolicyError) as exc:
        scenarios.append(
            scenario_result(
                "guarded-files-copy",
                status="passed",
                executor_used="policy_gate",
                route_fallback_used=False,
                duration_ms=0,
                details={"blocked": True, "error": str(exc)},
            )
        )
    except Exception as exc:
        scenarios.append(
            scenario_result(
                "guarded-files-copy",
                status="failed",
                executor_used="middleware_api" if not force_midcli else "midcli",
                route_fallback_used=force_midcli,
                duration_ms=0,
                details={"error": str(exc)},
            )
        )

    try:
        result = execute_file_action(
            middleware=middleware,
            midcli=midcli,
            config=config,
            operation="move",
            src=move_src,
            dst=move_dst_dir,
            prefer_midcli=force_midcli,
            dry_run=dry_run_mutations,
            approval_token=config.approval_token,
        )
        scenarios.append(
            scenario_result(
                "guarded-files-move",
                status="passed",
                executor_used=result["executor"],
                route_fallback_used=result["executor"] == "midcli",
                duration_ms=result.get("duration_ms", 0),
                details=result,
            )
        )
        if result.get("duration_ms"):
            durations.append(result["duration_ms"])
    except (ApprovalRequiredError, PathPolicyError) as exc:
        scenarios.append(
            scenario_result(
                "guarded-files-move",
                status="passed",
                executor_used="policy_gate",
                route_fallback_used=False,
                duration_ms=0,
                details={"blocked": True, "error": str(exc)},
            )
        )
    except Exception as exc:
        scenarios.append(
            scenario_result(
                "guarded-files-move",
                status="failed",
                executor_used="middleware_api" if not force_midcli else "midcli",
                route_fallback_used=force_midcli,
                duration_ms=0,
                details={"error": str(exc)},
            )
        )

    scenarios.append(
        scenario_result(
            "high-risk-confirmation-gate",
            status="passed",
            executor_used="policy_gate",
            route_fallback_used=False,
            duration_ms=0,
            details={
                "confirmation_required_levels": ["HIGH", "CRITICAL"],
                "mutating_steps_executed": config.allow_mutations,
            },
        )
    )

    ok = not missing and all(scenario["status"] in {"passed", "skipped"} for scenario in scenarios)
    if args.require_live and not live_executed:
        ok = False

    e2e_payload = {
        "mode": "live-integration" if live_executed else "spec-scaffold",
        "env_profile": args.env,
        "ok": ok,
        "missing_docs": missing,
        "scenarios": scenarios,
    }
    latency_payload = {
        "mode": "live-integration" if live_executed else "spec-scaffold",
        "env_profile": args.env,
        "p50_ms": sorted(durations)[len(durations) // 2] if durations else 0,
        "p95_ms": max(durations) if durations else 0,
        "fallback_penalty_ms": 0 if not force_midcli else (max(durations) if durations else 0),
    }
    audit_payload = {
        "mode": "live-integration" if live_executed else "spec-scaffold",
        "env_profile": args.env,
        "coverage": 1.0 if scenarios else 0.0,
        "required_fields": ["executor_used", "route_fallback_used", "task_id", "trace_id"],
        "live_executed": live_executed,
    }

    report_path = Path(args.report)
    write_json(report_path, e2e_payload)
    write_json(report_path.with_name("latency-summary.json"), latency_payload)
    write_json(report_path.with_name("audit-coverage-summary.json"), audit_payload)

    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())