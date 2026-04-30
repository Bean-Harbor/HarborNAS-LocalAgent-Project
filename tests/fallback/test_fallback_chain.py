from conftest import read_doc


def test_files_contract_defines_three_stage_fallback_chain() -> None:
    content = read_doc("HarborBeacon-Files-BatchOps-Contract-v1.md")
    assert "1. middleware API" in content
    assert "2. midcli" in content
    assert "3. constrained local CLI templates" in content


def test_v2_roadmap_limits_browser_and_mcp_to_fallback_roles() -> None:
    content = read_doc("HarborBeacon-LocalAgent-V2-Assistant-Skills-Roadmap.md")
    assert "Browser and MCP are used only when API and CLI are unavailable." in content


def test_files_contract_describes_api_to_midcli_fallback() -> None:
    content = read_doc("HarborBeacon-Files-BatchOps-Contract-v1.md")
    assert "fallback to `midcli`" in content