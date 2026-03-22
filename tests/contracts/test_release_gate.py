import json
import sys
from pathlib import Path


SCRIPTS_DIR = Path(__file__).resolve().parents[2] / "scripts"
if str(SCRIPTS_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPTS_DIR))

import evaluate_release_gate  # noqa: E402


def test_release_gate_requires_live_when_requested(tmp_path, monkeypatch) -> None:
    report_path = tmp_path / "drift.json"
    output_path = tmp_path / "summary.json"
    report_path.write_text(json.dumps({"mode": "spec-scaffold", "rows": [], "docs_missing": []}), encoding="utf-8")
    monkeypatch.setattr(sys, "argv", ["evaluate_release_gate.py", str(report_path), "--output", str(output_path), "--require-live"])

    exit_code = evaluate_release_gate.main()
    payload = json.loads(output_path.read_text(encoding="utf-8"))
    assert exit_code == 1
    assert payload["allowed"] is False
    assert "live middleware or midcli probes were not executed" in payload["reasons"]