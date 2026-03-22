"""Tests for harborbeacon.api.extensions_api (ExtensionStore + HarborBeaconExtensionsAPI)."""
from __future__ import annotations

import copy
from pathlib import Path
from typing import Any

import pytest
import yaml

from harborbeacon.api.extensions_api import (
    ExtensionStore,
    HarborBeaconExtensionsAPI,
    ValidationResultDTO,
    VALID_EXTENSION_TYPES,
)
from skills.registry import SkillNotFoundError, DuplicateSkillError


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------

@pytest.fixture()
def ext_dir(tmp_path: Path) -> Path:
    return tmp_path / "extensions"


@pytest.fixture()
def store(ext_dir: Path) -> ExtensionStore:
    return ExtensionStore(base_dir=ext_dir)


@pytest.fixture()
def api(store: ExtensionStore) -> HarborBeaconExtensionsAPI:
    return HarborBeaconExtensionsAPI(store=store)


def _sample_manifest(ext_id: str = "test.ext", **overrides: Any) -> dict[str, Any]:
    data: dict[str, Any] = {
        "id": ext_id,
        "name": "Test Extension",
        "version": "1.0.0",
        "summary": "A test extension",
        "owner": "test-team",
        "capabilities": ["test.action"],
        "type": "skill",
    }
    data.update(overrides)
    return data


# ===================================================================
# ExtensionStore
# ===================================================================

class TestExtensionStore:
    """Unit tests for ExtensionStore."""

    def test_list_empty(self, store: ExtensionStore) -> None:
        assert store.list_all() == []

    def test_create_and_list(self, store: ExtensionStore) -> None:
        store.create(_sample_manifest())
        items = store.list_all()
        assert len(items) == 1
        assert items[0].id == "test.ext"
        assert items[0].type == "skill"
        assert items[0].enabled is True

    def test_create_returns_detail_dict(self, store: ExtensionStore) -> None:
        result = store.create(_sample_manifest())
        assert result["id"] == "test.ext"
        assert result["name"] == "Test Extension"
        assert "executors" in result
        assert "harbor_api" in result
        assert "risk" in result

    def test_create_persists_to_disk(self, ext_dir: Path, store: ExtensionStore) -> None:
        store.create(_sample_manifest())
        yaml_path = ext_dir / "test.ext" / "skill.yaml"
        assert yaml_path.exists()
        data = yaml.safe_load(yaml_path.read_text())
        assert data["id"] == "test.ext"
        assert data["type"] == "skill"

    def test_create_duplicate_raises(self, store: ExtensionStore) -> None:
        store.create(_sample_manifest())
        with pytest.raises(DuplicateSkillError):
            store.create(_sample_manifest())

    def test_create_invalid_type_raises(self, store: ExtensionStore) -> None:
        with pytest.raises(ValueError, match="Invalid extension type"):
            store.create(_sample_manifest(type="bogus"))

    def test_get_returns_detail(self, store: ExtensionStore) -> None:
        store.create(_sample_manifest())
        detail = store.get("test.ext")
        assert detail["id"] == "test.ext"
        assert detail["capabilities"] == ["test.action"]

    def test_get_missing_raises(self, store: ExtensionStore) -> None:
        with pytest.raises(SkillNotFoundError):
            store.get("no.such.ext")

    def test_update_changes_fields(self, store: ExtensionStore) -> None:
        store.create(_sample_manifest())
        updated = store.update("test.ext", {
            "id": "test.ext",
            "name": "Updated Name",
            "version": "2.0.0",
            "capabilities": ["test.action", "test.new"],
        })
        assert updated["name"] == "Updated Name"
        assert updated["version"] == "2.0.0"
        assert "test.new" in updated["capabilities"]

    def test_update_cannot_change_id(self, store: ExtensionStore) -> None:
        store.create(_sample_manifest())
        with pytest.raises(ValueError, match="Cannot change extension id"):
            store.update("test.ext", {"id": "other.id"})

    def test_update_missing_raises(self, store: ExtensionStore) -> None:
        with pytest.raises(SkillNotFoundError):
            store.update("no.such", {"id": "no.such"})

    def test_delete_removes_from_registry(self, store: ExtensionStore) -> None:
        store.create(_sample_manifest())
        store.delete("test.ext")
        assert len(store.list_all()) == 0

    def test_delete_removes_from_disk(self, ext_dir: Path, store: ExtensionStore) -> None:
        store.create(_sample_manifest())
        store.delete("test.ext")
        assert not (ext_dir / "test.ext").exists()

    def test_delete_missing_raises(self, store: ExtensionStore) -> None:
        with pytest.raises(SkillNotFoundError):
            store.delete("no.such")

    def test_set_enabled(self, store: ExtensionStore) -> None:
        store.create(_sample_manifest())
        store.set_enabled("test.ext", False)
        items = store.list_all()
        assert items[0].enabled is False

    def test_multiple_extensions(self, store: ExtensionStore) -> None:
        store.create(_sample_manifest("ext.a", name="A"))
        store.create(_sample_manifest("ext.b", name="B"))
        store.create(_sample_manifest("ext.c", name="C", type="workflow"))
        items = store.list_all()
        assert len(items) == 3
        types = {i.type for i in items}
        assert "workflow" in types


# ===================================================================
# Validation
# ===================================================================

class TestValidation:
    """Tests for ExtensionStore.validate()."""

    def test_valid_manifest(self, store: ExtensionStore) -> None:
        result = store.validate(_sample_manifest())
        assert result.valid is True
        assert result.extension_id == "test.ext"
        assert result.errors == []

    def test_missing_id_is_error(self, store: ExtensionStore) -> None:
        data = _sample_manifest()
        del data["id"]
        result = store.validate(data)
        assert result.valid is False
        assert any("id" in e.lower() for e in result.errors)

    def test_invalid_type_is_error(self, store: ExtensionStore) -> None:
        result = store.validate(_sample_manifest(type="bogus"))
        assert result.valid is False
        assert any("type" in e.lower() for e in result.errors)

    def test_missing_name_is_warning(self, store: ExtensionStore) -> None:
        data = _sample_manifest()
        del data["name"]
        result = store.validate(data)
        assert result.valid is True
        assert any("name" in w.lower() for w in result.warnings)

    def test_high_risk_warning(self, store: ExtensionStore) -> None:
        data = _sample_manifest(risk={"default_level": "CRITICAL"})
        result = store.validate(data)
        assert result.valid is True
        assert any("risk" in w.lower() for w in result.warnings)

    def test_duplicate_warning(self, store: ExtensionStore) -> None:
        store.create(_sample_manifest())
        result = store.validate(_sample_manifest())
        assert result.valid is True
        assert any("already exists" in w for w in result.warnings)

    def test_non_dict_input(self, store: ExtensionStore) -> None:
        result = store.validate("not a dict")  # type: ignore[arg-type]
        assert result.valid is False


# ===================================================================
# HarborBeaconExtensionsAPI
# ===================================================================

class TestExtensionsAPI:
    """Tests for the API handler class."""

    def test_list_empty(self, api: HarborBeaconExtensionsAPI) -> None:
        result = api.list_extensions()
        assert result == []

    def test_create_and_list(self, api: HarborBeaconExtensionsAPI) -> None:
        api.create_extension(_sample_manifest())
        items = api.list_extensions()
        assert len(items) == 1
        assert items[0]["id"] == "test.ext"

    def test_get_extension(self, api: HarborBeaconExtensionsAPI) -> None:
        api.create_extension(_sample_manifest())
        detail = api.get_extension("test.ext")
        assert detail["name"] == "Test Extension"

    def test_update_extension(self, api: HarborBeaconExtensionsAPI) -> None:
        api.create_extension(_sample_manifest())
        updated = api.update_extension("test.ext", {
            "id": "test.ext", "name": "New Name", "version": "3.0.0",
        })
        assert updated["name"] == "New Name"

    def test_delete_extension(self, api: HarborBeaconExtensionsAPI) -> None:
        api.create_extension(_sample_manifest())
        api.delete_extension("test.ext")
        assert api.list_extensions() == []

    def test_validate_extension(self, api: HarborBeaconExtensionsAPI) -> None:
        result = api.validate_extension(_sample_manifest())
        assert result["valid"] is True

    def test_validate_bad_extension(self, api: HarborBeaconExtensionsAPI) -> None:
        result = api.validate_extension({"name": "no id"})
        assert result["valid"] is False

    def test_load_from_preexisting_dir(self, ext_dir: Path) -> None:
        """ExtensionStore picks up YAML files already on disk."""
        skill_dir = ext_dir / "preloaded.skill"
        skill_dir.mkdir(parents=True)
        (skill_dir / "skill.yaml").write_text(yaml.safe_dump({
            "id": "preloaded.skill",
            "name": "Preloaded",
            "version": "1.0.0",
            "capabilities": ["preload.test"],
        }))
        store = ExtensionStore(base_dir=ext_dir)
        items = store.list_all()
        assert len(items) == 1
        assert items[0].id == "preloaded.skill"
