import importlib.util
from pathlib import Path


def _load_shim_module():
    shim_path = Path(__file__).resolve().parents[2] / "tools" / "harbor_cli_shim.py"
    spec = importlib.util.spec_from_file_location("harbor_cli_shim", shim_path)
    module = importlib.util.module_from_spec(spec)
    assert spec is not None and spec.loader is not None
    spec.loader.exec_module(module)
    return module


def test_parse_service_query():
    shim = _load_shim_module()
    fields, service = shim.parse_service_query("service query service,state,enable WHERE service == 'ssh'")
    assert fields == ["service", "state", "enable"]
    assert service == "ssh"


def test_parse_service_action():
    shim = _load_shim_module()
    operation, service = shim.parse_service_action("service restart service=ftp")
    assert operation == "restart"
    assert service == "ftp"


def test_parse_filesystem_mutation_copy():
    shim = _load_shim_module()
    op, src, dst, recursive = shim.parse_filesystem_mutation(
        'filesystem copy src="/mnt/agent-ci/a.txt" dst="/mnt/agent-ci/b.txt" recursive=true'
    )
    assert op == "copy"
    assert src == "/mnt/agent-ci/a.txt"
    assert dst == "/mnt/agent-ci/b.txt"
    assert recursive is True


def test_parse_filesystem_mutation_move():
    shim = _load_shim_module()
    op, src, dst, recursive = shim.parse_filesystem_mutation(
        'filesystem move src="/mnt/agent-ci/a.txt" dst="/mnt/agent-ci/dst"'
    )
    assert op == "move"
    assert src == "/mnt/agent-ci/a.txt"
    assert dst == "/mnt/agent-ci/dst"
    assert recursive is False


def test_rows_to_csv():
    shim = _load_shim_module()
    output = shim.rows_to_csv(
        [{"service": "ssh", "state": "STOPPED", "enable": "False"}],
        ["service", "state", "enable"],
    )
    assert output == "service,state,enable\nssh,STOPPED,False\n"
