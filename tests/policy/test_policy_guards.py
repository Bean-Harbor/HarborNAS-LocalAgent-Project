from conftest import read_doc


def test_files_contract_declares_denied_roots() -> None:
    content = read_doc("HarborNAS-Files-BatchOps-Contract-v1.md")
    assert "Denied roots" in content
    assert "/etc/**" in content


def test_files_contract_declares_allowlist_and_shell_safety() -> None:
    content = read_doc("HarborNAS-Files-BatchOps-Contract-v1.md")
    assert "command template allowlist" in content
    assert "No shell metacharacters in arguments" in content


def test_planner_contract_requires_confirmation_for_high_risk() -> None:
    content = read_doc("HarborNAS-Planner-TaskDecompose-Contract-v1.md")
    assert '"require_confirmation_levels": ["HIGH", "CRITICAL"]' in content
    assert "HIGH/CRITICAL must set `requires_confirmation=true`." in content