import json
import sys
from pathlib import Path


SCRIPTS_DIR = Path(__file__).resolve().parents[2] / "scripts"
if str(SCRIPTS_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPTS_DIR))

import run_drift_matrix  # noqa: E402
import run_e2e_suite  # noqa: E402
from harbor_integration import IntegrationConfig  # noqa: E402


def test_e2e_dry_run_does_not_create_mutation_directories(tmp_path, monkeypatch) -> None:
    report_path = tmp_path / "e2e-report.json"
    monkeypatch.setattr(sys, "argv", ["run_e2e_suite.py", "--env", "env-a", "--report", str(report_path)])

    config = IntegrationConfig(
        allow_mutations=False,
        mutation_root="/mnt/software/harborbeacon-agent-ci",
    )
    monkeypatch.setattr(run_e2e_suite.IntegrationConfig, "from_env", classmethod(lambda cls: config))

    monkeypatch.setattr(run_e2e_suite.MiddlewareClient, "is_available", lambda self: False)
    monkeypatch.setattr(run_e2e_suite.MidcliClient, "is_available", lambda self: False)

    def fail_if_called(path: str) -> str:
        raise AssertionError(f"ensure_directory should not be called in dry-run mode: {path}")

    monkeypatch.setattr(run_e2e_suite, "ensure_directory", fail_if_called)

    monkeypatch.setattr(
        run_e2e_suite,
        "execute_service_action",
        lambda **kwargs: {"executor": "middleware_api", "duration_ms": 0},
    )
    monkeypatch.setattr(
        run_e2e_suite,
        "execute_file_action",
        lambda **kwargs: {"executor": "middleware_api", "duration_ms": 0},
    )

    exit_code = run_e2e_suite.main()
    payload = json.loads(report_path.read_text(encoding="utf-8"))
    audit_payload = json.loads(report_path.with_name("audit-coverage-summary.json").read_text(encoding="utf-8"))
    scenarios = {row["name"]: row for row in payload["scenarios"]}
    proof_pack_summary = payload["proof_pack_summary"]

    assert exit_code == 0
    assert payload["ok"] is True
    assert proof_pack_summary["live_status_summary"] == [
        "HarborDesk live status stays separate from proof summary.",
        "route_order=Middleware API -> MidCLI -> Browser/MCP fallback",
        "writable_root=/mnt/software/harborbeacon-agent-ci",
    ]
    assert proof_pack_summary["route_order"] == ["Middleware API", "MidCLI", "Browser/MCP fallback"]
    assert proof_pack_summary["writable_root"] == "/mnt/software/harborbeacon-agent-ci"
    assert proof_pack_summary["verifier_line_labels"]["middleware_first"] == "Windows verifier line"
    assert "Pause if executor_used becomes browser or mcp for service/files actions." in proof_pack_summary["pause_conditions"]
    assert proof_pack_summary["proof_summary"] == [
        "Proof summary covers service.query, files.list, service.restart, files.copy, and files.move.",
        "verifier_line_labels=middleware_first:Windows verifier line · midcli_fallback:Debian shim line",
        "pause_conditions=browser/MCP drift, midcli_fallback spikes, executor loss, or writable-root escape",
    ]
    assert scenarios["planner-to-harbor-ops"]["action_summary"] == "HarborOS service query proof"
    assert scenarios["planner-to-harbor-ops"]["status"] == "skipped"
    assert scenarios["planner-to-harbor-ops"]["proof_label"] == "service.query"
    assert scenarios["planner-to-harbor-ops"]["verifier_line_label"] == "not applicable"
    assert scenarios["planner-to-files-batch-ops"]["action_summary"] == "HarborOS files list proof"
    assert scenarios["planner-to-files-batch-ops"]["status"] == "skipped"
    assert scenarios["planner-to-files-batch-ops"]["proof_label"] == "files.list"
    assert scenarios["planner-to-files-batch-ops"]["verifier_line_label"] == "not applicable"
    assert scenarios["guarded-service-restart"]["action_summary"] == "Approved HarborOS service restart"
    assert scenarios["guarded-service-restart"]["proof_label"] == "service.restart"
    assert scenarios["guarded-service-restart"]["verifier_line_label"] == "Windows verifier line"
    assert scenarios["guarded-files-copy"]["action_summary"] == "Approved HarborOS file copy"
    assert scenarios["guarded-files-copy"]["proof_label"] == "files.copy"
    assert scenarios["guarded-files-copy"]["verifier_line_label"] == "Windows verifier line"
    assert scenarios["guarded-files-move"]["action_summary"] == "Approved HarborOS file move"
    assert scenarios["guarded-files-move"]["proof_label"] == "files.move"
    assert scenarios["guarded-files-move"]["verifier_line_label"] == "Windows verifier line"
    assert scenarios["guarded-service-restart"]["route_mode"] == "middleware_first"
    assert scenarios["guarded-files-copy"]["route_mode"] == "middleware_first"
    assert scenarios["guarded-files-move"]["route_mode"] == "middleware_first"
    assert "route_mode" in audit_payload["required_fields"]
    assert "action_summary" in audit_payload["required_fields"]
    assert "verifier_line_label" in audit_payload["required_fields"]


def test_e2e_reports_midcli_fallback_route_mode_when_shim_is_used(tmp_path, monkeypatch) -> None:
    report_path = tmp_path / "e2e-report.json"
    monkeypatch.setattr(sys, "argv", ["run_e2e_suite.py", "--env", "env-b", "--report", str(report_path)])

    config = IntegrationConfig(
        allow_mutations=False,
        mutation_root="/mnt/software/harborbeacon-agent-ci",
    )
    monkeypatch.setattr(run_e2e_suite.IntegrationConfig, "from_env", classmethod(lambda cls: config))

    monkeypatch.setattr(run_e2e_suite.MiddlewareClient, "is_available", lambda self: False)
    monkeypatch.setattr(run_e2e_suite.MidcliClient, "is_available", lambda self: False)
    monkeypatch.setattr(
        run_e2e_suite,
        "execute_service_action",
        lambda **kwargs: {"executor": "midcli", "route_fallback_used": True, "duration_ms": 0},
    )
    monkeypatch.setattr(
        run_e2e_suite,
        "execute_file_action",
        lambda **kwargs: {"executor": "midcli", "route_fallback_used": True, "duration_ms": 0},
    )

    exit_code = run_e2e_suite.main()
    payload = json.loads(report_path.read_text(encoding="utf-8"))
    scenarios = {row["name"]: row for row in payload["scenarios"]}
    proof_pack_summary = payload["proof_pack_summary"]

    assert exit_code == 0
    assert proof_pack_summary["live_status_summary"] == [
        "HarborDesk live status stays separate from proof summary.",
        "route_order=Middleware API -> MidCLI -> Browser/MCP fallback",
        "writable_root=/mnt/software/harborbeacon-agent-ci",
    ]
    assert proof_pack_summary["route_order"] == ["Middleware API", "MidCLI", "Browser/MCP fallback"]
    assert proof_pack_summary["verifier_line_labels"]["midcli_fallback"] == "Debian shim line"
    assert proof_pack_summary["writable_root"] == "/mnt/software/harborbeacon-agent-ci"
    assert proof_pack_summary["proof_summary"] == [
        "Proof summary covers service.query, files.list, service.restart, files.copy, and files.move.",
        "verifier_line_labels=middleware_first:Windows verifier line · midcli_fallback:Debian shim line",
        "pause_conditions=browser/MCP drift, midcli_fallback spikes, executor loss, or writable-root escape",
    ]
    assert scenarios["planner-to-harbor-ops"]["proof_label"] == "service.query"
    assert scenarios["planner-to-harbor-ops"]["action_summary"] == "HarborOS service query proof"
    assert scenarios["planner-to-harbor-ops"]["status"] == "skipped"
    assert scenarios["planner-to-harbor-ops"]["verifier_line_label"] == "not applicable"
    assert scenarios["planner-to-files-batch-ops"]["proof_label"] == "files.list"
    assert scenarios["planner-to-files-batch-ops"]["action_summary"] == "HarborOS files list proof"
    assert scenarios["planner-to-files-batch-ops"]["status"] == "skipped"
    assert scenarios["planner-to-files-batch-ops"]["verifier_line_label"] == "not applicable"
    assert scenarios["guarded-service-restart"]["proof_label"] == "service.restart"
    assert scenarios["guarded-service-restart"]["action_summary"] == "Approved HarborOS service restart"
    assert scenarios["guarded-service-restart"]["verifier_line_label"] == "Debian shim line"
    assert scenarios["guarded-files-copy"]["proof_label"] == "files.copy"
    assert scenarios["guarded-files-copy"]["action_summary"] == "Approved HarborOS file copy"
    assert scenarios["guarded-files-copy"]["verifier_line_label"] == "Debian shim line"
    assert scenarios["guarded-files-move"]["proof_label"] == "files.move"
    assert scenarios["guarded-files-move"]["action_summary"] == "Approved HarborOS file move"
    assert scenarios["guarded-files-move"]["verifier_line_label"] == "Debian shim line"
    assert scenarios["guarded-service-restart"]["route_mode"] == "midcli_fallback"
    assert scenarios["guarded-files-copy"]["route_mode"] == "midcli_fallback"
    assert scenarios["guarded-files-move"]["route_mode"] == "midcli_fallback"


def test_drift_matrix_midcli_only_is_degraded_not_blocking(tmp_path, monkeypatch) -> None:
    report_path = tmp_path / "drift.json"
    monkeypatch.setattr(
        sys,
        "argv",
        [
            "run_drift_matrix.py",
            "--harbor-ref",
            "develop",
            "--upstream-ref",
            "master",
            "--report",
            str(report_path),
        ],
    )

    monkeypatch.setattr(run_drift_matrix, "live_middleware_capabilities", lambda client: {})
    monkeypatch.setattr(
        run_drift_matrix,
        "live_midcli_capabilities",
        lambda client, config: {
            "service.query": True,
            "service.control": True,
            "filesystem.listdir": True,
            "filesystem.copy": True,
            "filesystem.move": True,
        },
    )
    monkeypatch.setattr(run_drift_matrix, "discover_source_capabilities", lambda repo_path: {})

    exit_code = run_drift_matrix.main()
    payload = json.loads(report_path.read_text(encoding="utf-8"))

    assert exit_code == 0
    assert payload["blocking"] is False

    rows = {row["capability"]: row for row in payload["rows"]}
    assert rows["system.harbor_ops"]["status"] == "degraded"
    assert rows["system.harbor_ops"]["blocking"] is False
    assert rows["files.batch_ops"]["status"] == "degraded"
    assert rows["files.batch_ops"]["blocking"] is False
