"""Tests for harborbeacon.tool_descriptions — TOML generation."""
import pytest

from skills.manifest import SkillManifest, HarborApiConfig, HarborCliConfig, RiskConfig
from skills.registry import Registry

from harborbeacon.tool_descriptions import (
    manifest_to_tool_descriptions,
    registry_to_toml,
    manifest_to_skill_toml,
)


def _manifest(**overrides) -> SkillManifest:
    defaults = dict(
        id="system.harbor_ops",
        name="HarborOS Service Operations",
        version="1.0.0",
        summary="Manage HarborOS services",
        owner="harbor-team",
        capabilities=["service.status", "service.start"],
        harbor_api=HarborApiConfig(enabled=True, allowed_methods=["query", "start"]),
        harbor_cli=HarborCliConfig(enabled=True, allowed_subcommands=["status", "start"]),
        risk=RiskConfig(default_level="LOW"),
    )
    defaults.update(overrides)
    return SkillManifest(**defaults)


class TestManifestToToolDescriptions:
    def test_returns_one_entry_per_capability(self):
        m = _manifest()
        descs = manifest_to_tool_descriptions(m)
        assert set(descs.keys()) == {"service.status", "service.start"}

    def test_description_contains_operation(self):
        m = _manifest()
        descs = manifest_to_tool_descriptions(m)
        assert "status" in descs["service.status"]

    def test_description_contains_summary(self):
        m = _manifest(summary="My summary")
        descs = manifest_to_tool_descriptions(m)
        assert "My summary" in descs["service.status"]

    def test_empty_summary_uses_name(self):
        m = _manifest(summary="")
        descs = manifest_to_tool_descriptions(m)
        assert "HarborOS Service Operations" in descs["service.status"]


class TestRegistryToToml:
    def test_generates_valid_toml_lines(self):
        reg = Registry()
        reg.register(_manifest())
        toml = registry_to_toml(reg)
        assert '"service.status"' in toml
        assert '"service.start"' in toml

    def test_contains_header_comment(self):
        reg = Registry()
        reg.register(_manifest())
        toml = registry_to_toml(reg)
        assert toml.startswith("# Auto-generated")

    def test_empty_registry(self):
        reg = Registry()
        toml = registry_to_toml(reg)
        assert "Auto-generated" in toml

    def test_multiple_skills(self):
        reg = Registry()
        reg.register(_manifest(id="a", capabilities=["a.op1"]))
        reg.register(_manifest(id="b", capabilities=["b.op2"]))
        toml = registry_to_toml(reg)
        assert '"a.op1"' in toml
        assert '"b.op2"' in toml


class TestManifestToSkillToml:
    def test_contains_name_and_version(self):
        m = _manifest()
        toml = manifest_to_skill_toml(m)
        assert 'name = "HarborOS Service Operations"' in toml
        assert 'version = "1.0.0"' in toml

    def test_contains_description(self):
        m = _manifest()
        toml = manifest_to_skill_toml(m)
        assert 'description = "Manage HarborOS services"' in toml

    def test_contains_capabilities(self):
        m = _manifest()
        toml = manifest_to_skill_toml(m)
        assert '"service.status" = true' in toml
        assert '"service.start" = true' in toml

    def test_harbor_section(self):
        m = _manifest()
        toml = manifest_to_skill_toml(m)
        assert "api_enabled = true" in toml
        assert "cli_enabled = true" in toml

    def test_api_methods_listed(self):
        m = _manifest()
        toml = manifest_to_skill_toml(m)
        assert '"query"' in toml
        assert '"start"' in toml

    def test_min_autonomy_low_risk(self):
        m = _manifest(risk=RiskConfig(default_level="LOW"))
        toml = manifest_to_skill_toml(m)
        assert 'min_autonomy = "Supervised"' in toml

    def test_min_autonomy_high_risk(self):
        m = _manifest(risk=RiskConfig(default_level="HIGH"))
        toml = manifest_to_skill_toml(m)
        assert 'min_autonomy = "Full"' in toml

    def test_owner_field(self):
        m = _manifest(owner="my-team")
        toml = manifest_to_skill_toml(m)
        assert 'owner = "my-team"' in toml
