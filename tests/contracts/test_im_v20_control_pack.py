from pathlib import Path

from conftest import ROOT, read_doc


GATE_ROOT = ROOT.parent / "HarborGate"


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def test_v20_control_pack_documents_exist_and_are_active() -> None:
    required_beacon = [
        ROOT / "HarborBeacon-HarborGate-v2.0-Upgrade-Runbook.md",
        ROOT / "docs" / "im-v2.0-cutover-rollback-observability-gates.md",
    ]
    missing_beacon = [str(path) for path in required_beacon if not path.exists()]
    assert not missing_beacon

    beacon_docs = "\n".join(_read(path) for path in required_beacon)
    assert "HarborBeacon-HarborGate-Agent-Contract-v2.0.md" in beacon_docs
    assert "POST /api/web/turns" in beacon_docs
    assert "POST /api/turns" in beacon_docs
    assert "deprecated" in beacon_docs.lower()
    assert "conversation.handle" in beacon_docs
    assert "active_frame" in beacon_docs
    assert "continuation" in beacon_docs
    assert "delivery_hints" in beacon_docs

    if not GATE_ROOT.exists():
        return

    required_gate = [
        GATE_ROOT / "HarborBeacon-HarborGate-Agent-Contract-v2.0.md",
        GATE_ROOT / "HarborBeacon-HarborGate-v2.0-Upgrade-Runbook.md",
        GATE_ROOT / "HarborBeacon-HarborGate-v2.0-Cutover-Checklist.md",
    ]
    missing_gate = [str(path) for path in required_gate if not path.exists()]
    assert not missing_gate

    contract = _read(GATE_ROOT / "HarborBeacon-HarborGate-Agent-Contract-v2.0.md")
    assert "POST /api/web/turns" in contract
    assert "`POST /api/turns` remains a deprecated compatibility alias" in contract
    assert "conversation.handle" in contract
    assert "active_frame" in contract
    assert "continuation" in contract
    assert "delivery_hints" in contract
    assert "X-Contract-Version: 2.0" in contract


def test_v15_evidence_is_historical_not_current_gate() -> None:
    evidence = read_doc("HarborBeacon-HarborGate-v1.5-Cutover-Evidence.md")
    old_gate = read_doc("docs/im-v1.5-cutover-rollback-observability-gates.md")

    assert "Historical Status" in evidence
    assert "historical" in evidence.lower()
    assert "HarborBeacon-HarborGate-Agent-Contract-v2.0.md" in evidence
    assert "Historical Status" in old_gate
    assert "im-v2.0-cutover-rollback-observability-gates.md" in old_gate


def test_beacon_entry_docs_point_to_v20_control_pack() -> None:
    agents = read_doc("AGENTS.md")
    readme = read_doc("README.md")
    collaboration = read_doc("HarborBeacon-Harbor-Collaboration-Contract-v2.md")

    for content in (agents, readme, collaboration):
        assert "HarborBeacon-HarborGate-Agent-Contract-v2.0.md" in content
    assert "HarborBeacon-HarborGate-v2.0-Upgrade-Runbook.md" in agents
    assert "POST /api/web/turns" in collaboration
    assert "POST /api/turns" in collaboration


def test_beacon_active_sources_have_no_v15_contract_version() -> None:
    active_sources = [
        ROOT / "src" / "runtime" / "task_api.rs",
        ROOT / "src" / "bin" / "assistant_task_api.rs",
        ROOT / "src" / "bin" / "agent_hub_admin_api.rs",
        ROOT / "src" / "connectors" / "im_gateway.rs",
        ROOT / "src" / "connectors" / "notifications.rs",
        ROOT / "src" / "scripts" / "validate.rs",
        ROOT / "tools" / "install_harboros_release.sh",
        ROOT / "tools" / "release_templates" / "harborbeacon-agent-hub.env.template",
        ROOT / "tools" / "release_templates" / "bin" / "harbor-agent-hub-helper",
    ]
    forbidden = [
        "X-Contract-Version: 1.5",
        '"X-Contract-Version", "1.5"',
        'CONTRACT_VERSION: &str = "1.5"',
        "IM_AGENT_CONTRACT_VERSION=1.5",
        'DEFAULT_CONTRACT_VERSION = "1.5"',
    ]
    offenders = [
        f"{path}:{pattern}"
        for path in active_sources
        for pattern in forbidden
        if path.exists() and pattern in _read(path)
    ]
    assert not offenders


def test_beacon_active_sources_do_not_use_args_resume_token() -> None:
    active_source = ROOT / "src" / "runtime" / "task_api.rs"
    content = _read(active_source)
    forbidden = ['"/resume_token"', '"resume_token":', 'args.resume_token']
    offenders = [pattern for pattern in forbidden if pattern in content]
    assert not offenders


def test_beacon_active_sources_do_not_key_business_truth_by_source_session_id() -> None:
    active_source = ROOT / "src" / "runtime" / "task_api.rs"
    content = _read(active_source)
    forbidden = ["session_id_for_request", "source.session_id"]
    offenders = [pattern for pattern in forbidden if pattern in content]
    assert not offenders
