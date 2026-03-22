"""Tests for skills.registry — Registry class and capability indexing."""
import pytest
from pathlib import Path

from skills.manifest import parse_manifest
from skills.registry import DuplicateSkillError, Registry, SkillNotFoundError  # noqa: F401


def _make_manifest(id_, caps=None, **kw):
    return parse_manifest({"id": id_, "capabilities": caps or [], **kw})


class TestRegistryBasics:
    def test_register_and_get(self):
        r = Registry()
        m = _make_manifest("a.b", ["a.cap1"])
        r.register(m)
        assert r.get("a.b") is m

    def test_get_nonexistent_raises(self):
        r = Registry()
        with pytest.raises(SkillNotFoundError):
            r.get("nope")

    def test_duplicate_raises(self):
        r = Registry()
        m = _make_manifest("dup")
        r.register(m)
        with pytest.raises(DuplicateSkillError):
            r.register(m)

    def test_unregister(self):
        r = Registry()
        m = _make_manifest("x")
        r.register(m)
        r.unregister("x")
        with pytest.raises(SkillNotFoundError):
            r.get("x")

    def test_unregister_nonexistent_silent(self):
        r = Registry()
        r.unregister("nope")  # should not raise

    def test_len(self):
        r = Registry()
        assert len(r) == 0
        r.register(_make_manifest("a"))
        r.register(_make_manifest("b"))
        assert len(r) == 2

    def test_skill_ids(self):
        r = Registry()
        r.register(_make_manifest("x"))
        r.register(_make_manifest("y"))
        assert set(r.skill_ids) == {"x", "y"}


class TestCapabilityIndex:
    def test_find_by_capability(self):
        r = Registry()
        r.register(_make_manifest("s1", ["cap.a", "cap.b"]))
        r.register(_make_manifest("s2", ["cap.b", "cap.c"]))
        assert [m.id for m in r.find_by_capability("cap.a")] == ["s1"]
        assert {m.id for m in r.find_by_capability("cap.b")} == {"s1", "s2"}

    def test_find_by_capability_empty(self):
        r = Registry()
        assert r.find_by_capability("cap.x") == []

    def test_has_capability(self):
        r = Registry()
        r.register(_make_manifest("s1", ["cap.a"]))
        assert r.has_capability("cap.a") is True
        assert r.has_capability("cap.z") is False

    def test_capability_index_updates_on_unregister(self):
        r = Registry()
        r.register(_make_manifest("s1", ["cap.a"]))
        r.unregister("s1")
        assert r.find_by_capability("cap.a") == []
        assert r.has_capability("cap.a") is False


class TestDomainLookup:
    def test_find_by_domain(self):
        r = Registry()
        r.register(_make_manifest("s1", ["service.start", "service.stop"]))
        r.register(_make_manifest("s2", ["files.copy"]))
        svc = r.find_by_domain("service")
        assert len(svc) == 1 and svc[0].id == "s1"
        assert r.find_by_domain("files")[0].id == "s2"

    def test_find_by_domain_empty(self):
        r = Registry()
        assert r.find_by_domain("nope") == []


class TestLoadDir:
    def test_load_dir(self, tmp_path):
        d = tmp_path / "s1"
        d.mkdir()
        (d / "skill.yaml").write_text("id: loaded.s1\ncapabilities: [l.a]\n")
        r = Registry()
        r.load_dir(tmp_path)
        assert r.get("loaded.s1").id == "loaded.s1"

    def test_load_dir_empty(self, tmp_path):
        r = Registry()
        r.load_dir(tmp_path)
        assert len(r) == 0


class TestSummary:
    def test_summary_contains_ids(self):
        r = Registry()
        r.register(_make_manifest("a.b", ["a.cap1"]))
        s = r.summary()
        assert s["total_skills"] == 1
        assert s["total_capabilities"] == 1
        assert s["skills"][0]["id"] == "a.b"
        assert "a.cap1" in s["skills"][0]["capabilities"]


class TestRegisterDict:
    def test_register_dict(self):
        r = Registry()
        r.register_dict({"id": "d1", "capabilities": ["d.one"]})
        m = r.get("d1")
        assert "d.one" in m.capabilities
