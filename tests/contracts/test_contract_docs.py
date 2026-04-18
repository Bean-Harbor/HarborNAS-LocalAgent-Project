from conftest import ROOT, read_doc


def test_required_contract_documents_exist() -> None:
    required = [
        "HarborBeacon-Middleware-Endpoint-Contract-v1.md",
        "HarborBeacon-Files-BatchOps-Contract-v1.md",
        "HarborBeacon-Planner-TaskDecompose-Contract-v1.md",
        "HarborBeacon-Contract-E2E-Test-Plan-v1.md",
        "HarborBeacon-HarborGate-v1.5-Cutover-Evidence.md",
        "docs/harboros-real-integration-parity-note.md",
    ]
    missing = [name for name in required if not (ROOT / name).exists()]
    assert not missing


def test_v2_roadmap_preserves_executor_order() -> None:
    content = read_doc("HarborBeacon-LocalAgent-V2-Assistant-Skills-Roadmap.md")
    expected = [
        "1. Middleware API executor",
        "2. MidCLI executor (CLI via `midcli`)",
        "3. Browser executor",
        "4. MCP executor (fallback only)",
    ]
    positions = [content.index(item) for item in expected]
    assert positions == sorted(positions)


def test_planner_contract_contains_route_priority_schema() -> None:
    content = read_doc("HarborBeacon-Planner-TaskDecompose-Contract-v1.md")
    assert '"route_priority": ["middleware_api", "midcli", "browser", "mcp"]' in content


def test_readme_mentions_live_integration_scaffold() -> None:
    content = read_doc("README.md")
    lowered = content.lower()
    assert "middleware" in lowered
    assert "midcli" in lowered


def test_harborbeacon_harborgate_v15_cutover_evidence_covers_frozen_seam() -> None:
    content = read_doc("HarborBeacon-HarborGate-v1.5-Cutover-Evidence.md")
    required_phrases = [
        "POST /api/tasks",
        "POST /api/notifications/deliveries",
        "GET /api/gateway/status",
        "X-Contract-Version: 1.5",
        "resume_token",
        "route_key",
        "accepted-request delivery failures remain `HTTP 200` with `ok=false`",
        "direct platform delivery count is `0`",
        "Rollback must preserve the frozen boundary",
        "external IM repo",
    ]
    assert all(phrase in content for phrase in required_phrases)


def test_im_cutover_rollback_doc_mentions_legacy_fallback_switch() -> None:
    content = read_doc("docs/im-v1.5-cutover-rollback-observability-gates.md")
    required_phrases = [
        "legacy recipient fallback may only be re-enabled via",
        "HARBORBEACON_ENABLE_LEGACY_IM_RECIPIENT_FALLBACK=1",
        "rollback notes must say whether legacy recipient fallback is disabled or explicitly re-enabled",
    ]
    assert all(phrase in content for phrase in required_phrases)
