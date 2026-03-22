import json
import sys
from pathlib import Path


SCRIPTS_DIR = Path(__file__).resolve().parents[2] / "scripts"
if str(SCRIPTS_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPTS_DIR))

from harbor_integration import IntegrationConfig, MiddlewareClient, MidcliClient, discover_source_capabilities, parse_csv_rows  # noqa: E402


def test_parse_csv_rows_returns_structured_rows() -> None:
    rows = parse_csv_rows("service,state\nssh,RUNNING\n")
    assert rows == [{"service": "ssh", "state": "RUNNING"}]


def test_middleware_client_builds_midclt_call(monkeypatch) -> None:
    captured = {}
    config = IntegrationConfig(middleware_bin="midclt")

    monkeypatch.setattr("harbor_integration.command_exists", lambda name: name == "midclt")

    def fake_run(argv, timeout):
        captured["argv"] = argv
        return type("Result", (), {"argv": argv, "stdout": json.dumps({"service.query": {}}), "stderr": "", "returncode": 0, "duration_ms": 7})()

    monkeypatch.setattr("harbor_integration.run_command", fake_run)

    client = MiddlewareClient(config)
    methods, _ = client.get_methods(target="REST")
    assert methods == {"service.query": {}}
    assert captured["argv"] == ["midclt", "call", "core.get_methods", "null", '"REST"']


def test_midcli_client_builds_noninteractive_command(monkeypatch) -> None:
    captured = {}
    config = IntegrationConfig(midcli_bin="cli", midcli_url="ws://nas/websocket", midcli_user="root", midcli_password="secret")

    monkeypatch.setattr("harbor_integration.command_exists", lambda name: name == "cli")

    def fake_run(argv, timeout):
        captured["argv"] = argv
        return type("Result", (), {"argv": argv, "stdout": "service,state\nssh,RUNNING\n", "stderr": "", "returncode": 0, "duration_ms": 9})()

    monkeypatch.setattr("harbor_integration.run_command", fake_run)

    client = MidcliClient(config)
    rows, _ = client.run_csv_query("service query service,state WHERE service == 'ssh'")
    assert rows == [{"service": "ssh", "state": "RUNNING"}]
    assert captured["argv"] == [
        "cli",
        "--url",
        "ws://nas/websocket",
        "--user",
        "root",
        "--password",
        "secret",
        "-m",
        "csv",
        "-c",
        "service query service,state WHERE service == 'ssh'",
    ]


def test_discover_source_capabilities_reads_repo_files(tmp_path) -> None:
    service_api = tmp_path / "src/middlewared/middlewared/api/v27_0_0/service.py"
    filesystem_api = tmp_path / "src/middlewared/middlewared/api/v27_0_0/filesystem.py"
    service_plugin = tmp_path / "src/middlewared/middlewared/plugins/service.py"
    filesystem_plugin = tmp_path / "src/middlewared/middlewared/plugins/filesystem.py"
    for path in [service_api, filesystem_api, service_plugin, filesystem_plugin]:
        path.parent.mkdir(parents=True, exist_ok=True)

    service_api.write_text("class ServiceControlArgs: pass\n", encoding="utf-8")
    filesystem_api.write_text("FilesystemListdirArgs\nFilesystemCopyArgs\nFilesystemMoveArgs\n", encoding="utf-8")
    service_plugin.write_text("class ServiceService:\n    def control(self):\n        pass\n    def query(self):\n        pass\n", encoding="utf-8")
    filesystem_plugin.write_text("def listdir(self):\n    pass\ndef copy(self):\n    pass\ndef move(self):\n    pass\n", encoding="utf-8")

    caps = discover_source_capabilities(str(tmp_path))
    assert caps["service.query"] is True
    assert caps["service.control"] is True
    assert caps["filesystem.listdir"] is True
    assert caps["filesystem.copy"] is True
    assert caps["filesystem.move"] is True