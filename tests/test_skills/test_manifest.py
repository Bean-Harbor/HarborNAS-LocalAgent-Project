"""Tests for skills.manifest — YAML loading and parsing."""
from pathlib import Path

from skills.manifest import (
    ExecutorConfig,
    HarborApiConfig,
    HarborCliConfig,
    SkillManifest,
    load_manifest,
    load_manifests_from_dir,
    parse_manifest,
)


MINIMAL_DATA = {
    "id": "test.skill",
    "name": "Test Skill",
    "version": "1.0.0",
    "capabilities": ["test.cap1", "test.cap2"],
}


def test_parse_manifest_minimal():
    m = parse_manifest(MINIMAL_DATA)
    assert m.id == "test.skill"
    assert m.name == "Test Skill"
    assert m.version == "1.0.0"
    assert m.capabilities == ["test.cap1", "test.cap2"]
    assert m.domains == {"test"}


def test_parse_manifest_missing_id_raises():
    import pytest
    with pytest.raises(ValueError, match="must have an 'id'"):
        parse_manifest({"name": "no id"})


def test_parse_manifest_full():
    data = {
        "id": "media.video_edit",
        "name": "Video Editing",
        "version": "2.0.0",
        "summary": "Edit videos",
        "owner": "harbor-team",
        "capabilities": ["video.trim", "video.concat"],
        "executors": {
            "cli": {"enabled": True, "command": "python handler.py"},
            "browser": {"enabled": False},
        },
        "harbor_api": {
            "enabled": True,
            "provider": "middleware",
            "endpoint_group": "service",
            "allowed_methods": ["query", "start"],
            "min_version": "v1",
        },
        "harbor_cli": {
            "enabled": True,
            "tool": "midcli",
            "command_group": "service",
            "allowed_subcommands": ["status", "start"],
            "require_structured_output": True,
        },
        "risk": {
            "default_level": "MEDIUM",
            "requires_confirmation": ["HIGH", "CRITICAL"],
        },
        "timeouts": {"plan_ms": 2000, "exec_ms": 120000},
        "retries": {"max_attempts": 2, "backoff_ms": 1000},
    }
    m = parse_manifest(data)
    assert m.id == "media.video_edit"
    assert m.executors["cli"].enabled is True
    assert m.executors["cli"].command == "python handler.py"
    assert m.executors["browser"].enabled is False
    assert m.harbor_api.enabled is True
    assert m.harbor_api.allowed_methods == ["query", "start"]
    assert m.harbor_cli.enabled is True
    assert m.harbor_cli.allowed_subcommands == ["status", "start"]
    assert m.risk.default_level == "MEDIUM"
    assert m.domains == {"video"}


def test_parse_manifest_defaults():
    m = parse_manifest({"id": "bare"})
    assert m.name == ""
    assert m.version == "0.0.0"
    assert m.capabilities == []
    assert m.executors == {}
    assert m.harbor_api.enabled is False
    assert m.harbor_cli.enabled is False
    assert m.risk.default_level == "LOW"


def test_load_manifest_from_file(tmp_path):
    yaml_content = "id: file.test\nname: File Test\nversion: 0.1.0\ncapabilities:\n  - file.read\n"
    p = tmp_path / "skill.yaml"
    p.write_text(yaml_content, encoding="utf-8")
    m = load_manifest(p)
    assert m.id == "file.test"
    assert m.source_path == str(p)


def test_load_manifests_from_dir(tmp_path):
    s1 = tmp_path / "skill_a" / "skill.yaml"
    s1.parent.mkdir()
    s1.write_text("id: skill.a\ncapabilities: [a.one]\n")

    s2 = tmp_path / "skill_b" / "skill.yaml"
    s2.parent.mkdir()
    s2.write_text("id: skill.b\ncapabilities: [b.two]\n")

    manifests = load_manifests_from_dir(tmp_path)
    assert len(manifests) == 2
    ids = {m.id for m in manifests}
    assert ids == {"skill.a", "skill.b"}


def test_load_manifests_empty_dir(tmp_path):
    manifests = load_manifests_from_dir(tmp_path)
    assert manifests == []


def test_load_manifests_nonexistent_dir(tmp_path):
    manifests = load_manifests_from_dir(tmp_path / "nope")
    assert manifests == []


def test_load_builtin_harbor_ops():
    """Verify the real system.harbor_ops/skill.yaml loads correctly."""
    skill_yaml = Path(__file__).resolve().parents[2] / "skills" / "builtins" / "system.harbor_ops" / "skill.yaml"
    if skill_yaml.exists():
        m = load_manifest(skill_yaml)
        assert m.id == "system.harbor_ops"
        assert "service.status" in m.capabilities
        assert m.harbor_api.enabled is True
        assert m.harbor_cli.enabled is True
