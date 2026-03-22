from conftest import ROOT, read_doc


def test_required_contract_documents_exist() -> None:
    required = [
        "HarborNAS-Middleware-Endpoint-Contract-v1.md",
        "HarborNAS-Files-BatchOps-Contract-v1.md",
        "HarborNAS-Planner-TaskDecompose-Contract-v1.md",
        "HarborNAS-Contract-E2E-Test-Plan-v1.md",
    ]
    missing = [name for name in required if not (ROOT / name).exists()]
    assert not missing


def test_v2_roadmap_preserves_executor_order() -> None:
    content = read_doc("HarborNAS-LocalAgent-V2-Assistant-Skills-Roadmap.md")
    expected = [
        "1. Middleware API executor",
        "2. MidCLI executor (CLI via `midcli`)",
        "3. Browser executor",
        "4. MCP executor (fallback only)",
    ]
    positions = [content.index(item) for item in expected]
    assert positions == sorted(positions)


def test_planner_contract_contains_route_priority_schema() -> None:
    content = read_doc("HarborNAS-Planner-TaskDecompose-Contract-v1.md")
    assert '"route_priority": ["middleware_api", "midcli", "browser", "mcp"]' in content


def test_readme_mentions_live_integration_scaffold() -> None:
    content = read_doc("README.md")
    lowered = content.lower()
    assert "middleware" in lowered
    assert "midcli" in lowered