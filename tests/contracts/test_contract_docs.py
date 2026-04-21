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
        "must not reintroduce legacy recipient fallback",
        "Rollback must preserve the frozen boundary",
        "external IM repo",
    ]
    assert all(phrase in content for phrase in required_phrases)


def test_im_cutover_rollback_doc_keeps_legacy_fallback_removed() -> None:
    content = read_doc("docs/im-v1.5-cutover-rollback-observability-gates.md")
    required_phrases = [
        "legacy recipient fallback remains removed during rollback",
        "rollback notes must say that legacy recipient fallback stayed disabled",
    ]
    assert all(phrase in content for phrase in required_phrases)


def test_harboros_webui_summary_separates_live_status_from_proof_summary() -> None:
    index_content = read_doc("docs/webui/index.html")
    app_content = read_doc("docs/webui/app.js")
    runbook_content = read_doc("docs/harboros-vm-validation-runbook.md")
    smoke_content = read_doc("docs/hos-system-domain-cutover-smoke.md")
    preflight_content = read_doc("docs/harboros-192.168.3.165-preflight.md")

    assert "<h4>HarborOS live status</h4>" in index_content
    assert "<h4>HarborOS proof summary</h4>" in index_content
    assert "HarborOS live status and proof summary are rendered separately." in index_content
    assert "HarborDesk renders HarborOS live status and proof summary separately." in app_content
    assert 'const HARBOROS_ROUTE_ORDER = ["Middleware API", "MidCLI", "Browser/MCP fallback"];' in app_content
    assert 'HARBOROS_ROUTE_ORDER.join(" -> ")' in app_content
    assert "writable_root=/mnt/software/harborbeacon-agent-ci" in app_content
    assert "verifier_line_labels=" in app_content
    assert 'middleware_first: "Windows verifier line"' in app_content
    assert 'midcli_fallback: "Debian shim line"' in app_content
    assert "pause_conditions=browser/MCP drift, midcli_fallback spikes, executor loss, or writable-root escape" in app_content
    assert "IM 双通道 readiness 和 proactive delivery 归 IM lane；HarborOS" in runbook_content
    assert "IM dual-channel readiness" in smoke_content
    assert "HarborOS blockers" in smoke_content
    assert "Feishu/Weixin delivery routing issues belong to the IM lane" in preflight_content


def test_current_harboros_docs_promote_169_as_the_active_target() -> None:
    readme_content = read_doc("README.md")
    packaging_content = read_doc("docs/harboros-release-packaging-runbook.md")
    runbook_content = read_doc("docs/harboros-vm-validation-runbook.md")
    cutover_content = read_doc("HarborBeacon-HarborGate-v1.5-Cutover-Evidence.md")

    assert "192.168.3.169" in readme_content
    assert "当前默认 HarborOS 目标机：" in packaging_content
    assert "192.168.3.169" in packaging_content
    assert "192.168.3.223 -> 192.168.3.169" in runbook_content
    assert "HarborOS remains an accepted southbound on `192.168.3.169`" in cutover_content
