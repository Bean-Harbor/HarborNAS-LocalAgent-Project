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

    config = IntegrationConfig(allow_mutations=False, mutation_root="/mnt/agent-ci")
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

    assert exit_code == 0
    assert payload["ok"] is True


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
