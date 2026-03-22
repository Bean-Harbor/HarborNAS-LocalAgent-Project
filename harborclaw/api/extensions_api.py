"""REST API endpoints for HarborClaw extension management.

Designed to be registered under ``/api/v2.0/harborclaw/`` inside the
HarborOS middleware.

Endpoint summary
----------------
GET    /extensions            → list all extensions
GET    /extensions/:id        → get one extension detail
POST   /extensions            → import / create extension
PUT    /extensions/:id        → update extension manifest
DELETE /extensions/:id        → remove extension
POST   /extensions/validate   → validate YAML without saving
"""
from __future__ import annotations

import copy
import os
import time
from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import Any

import yaml

from skills.manifest import (
    SkillManifest,
    parse_manifest,
    load_manifest,
    load_manifests_from_dir,
)
from skills.registry import Registry, DuplicateSkillError, SkillNotFoundError


# ---------------------------------------------------------------------------
# Extension types — extensible enum
# ---------------------------------------------------------------------------

VALID_EXTENSION_TYPES = frozenset({"skill", "workflow", "integration", "automation"})

_DEFAULT_TYPE = "skill"


# ---------------------------------------------------------------------------
# DTOs
# ---------------------------------------------------------------------------

@dataclass
class ExtensionSummaryDTO:
    """Lightweight card for list view."""
    id: str
    name: str
    type: str
    version: str
    summary: str
    owner: str
    capabilities: list[str]
    risk_level: str
    enabled: bool


@dataclass
class ValidationResultDTO:
    valid: bool
    extension_id: str | None = None
    extension_name: str | None = None
    errors: list[str] = field(default_factory=list)
    warnings: list[str] = field(default_factory=list)


# ---------------------------------------------------------------------------
# Persistent store
# ---------------------------------------------------------------------------

_EXTENSIONS_DIR = Path(os.environ.get(
    "HARBORCLAW_EXTENSIONS_DIR",
    "/etc/harborclaw/extensions",
))


class ExtensionStore:
    """Manages extension manifests on disk + an in-memory Registry cache.

    Each extension is stored as ``<extension_id>/skill.yaml`` inside
    ``_base_dir``.  Falls back to pure in-memory mode when the directory
    is not writable.
    """

    def __init__(self, base_dir: Path | None = None) -> None:
        self._base_dir = base_dir or _EXTENSIONS_DIR
        self._registry = Registry()
        self._types: dict[str, str] = {}       # id → extension type
        self._enabled: dict[str, bool] = {}    # id → enabled flag
        self._loaded = False

    # ---- bootstrap -----

    def _ensure_loaded(self) -> None:
        if self._loaded:
            return
        self._loaded = True
        if not self._base_dir.is_dir():
            return
        manifests = load_manifests_from_dir(self._base_dir)
        for m in manifests:
            if m.id not in self._registry._skills:
                self._registry.register(m)
                self._types[m.id] = _detect_type(m)
                self._enabled[m.id] = True

    # ---- queries ----

    def list_all(self) -> list[ExtensionSummaryDTO]:
        self._ensure_loaded()
        out: list[ExtensionSummaryDTO] = []
        for m in self._registry.skills:
            out.append(_manifest_to_summary(m, self._types.get(m.id, _DEFAULT_TYPE), self._enabled.get(m.id, True)))
        return out

    def get(self, ext_id: str) -> dict[str, Any]:
        self._ensure_loaded()
        m = self._registry.get(ext_id)
        return _manifest_to_detail(m, self._types.get(m.id, _DEFAULT_TYPE), self._enabled.get(m.id, True))

    # ---- mutations ----

    def create(self, data: dict[str, Any]) -> dict[str, Any]:
        """Parse, validate, register, and persist a new extension."""
        self._ensure_loaded()
        ext_type = data.pop("type", _DEFAULT_TYPE)
        if ext_type not in VALID_EXTENSION_TYPES:
            raise ValueError(f"Invalid extension type: {ext_type!r}")

        manifest = parse_manifest(data)

        # Check for duplicates
        self._registry.register(manifest)
        self._types[manifest.id] = ext_type
        self._enabled[manifest.id] = True
        self._persist(manifest, ext_type)
        return _manifest_to_detail(manifest, ext_type, True)

    def update(self, ext_id: str, data: dict[str, Any]) -> dict[str, Any]:
        """Update an existing extension manifest."""
        self._ensure_loaded()
        old = self._registry.get(ext_id)  # raises SkillNotFoundError

        ext_type = data.pop("type", self._types.get(ext_id, _DEFAULT_TYPE))
        if ext_type not in VALID_EXTENSION_TYPES:
            raise ValueError(f"Invalid extension type: {ext_type!r}")

        data.setdefault("id", ext_id)
        if data["id"] != ext_id:
            raise ValueError(f"Cannot change extension id from {ext_id!r} to {data['id']!r}")

        new_manifest = parse_manifest(data)

        # Replace in registry
        self._registry._skills[ext_id] = new_manifest
        # Rebuild capability index
        self._rebuild_capability_index()
        self._types[ext_id] = ext_type
        self._persist(new_manifest, ext_type)
        return _manifest_to_detail(new_manifest, ext_type, self._enabled.get(ext_id, True))

    def delete(self, ext_id: str) -> None:
        """Remove an extension from registry and disk."""
        self._ensure_loaded()
        self._registry.get(ext_id)  # raises SkillNotFoundError

        del self._registry._skills[ext_id]
        self._rebuild_capability_index()
        self._types.pop(ext_id, None)
        self._enabled.pop(ext_id, None)

        ext_dir = self._base_dir / ext_id
        if ext_dir.is_dir():
            for f in ext_dir.iterdir():
                f.unlink(missing_ok=True)
            ext_dir.rmdir()

    def set_enabled(self, ext_id: str, enabled: bool) -> None:
        self._ensure_loaded()
        self._registry.get(ext_id)  # raises SkillNotFoundError
        self._enabled[ext_id] = enabled

    # ---- validation (no side effects) ----

    def validate(self, data: dict[str, Any]) -> ValidationResultDTO:
        """Validate a manifest dict without persisting."""
        errors: list[str] = []
        warnings: list[str] = []
        ext_id: str | None = None
        ext_name: str | None = None

        if not isinstance(data, dict):
            return ValidationResultDTO(valid=False, errors=["Input must be a YAML mapping"])

        ext_id = data.get("id")
        ext_name = data.get("name")
        ext_type = data.get("type", _DEFAULT_TYPE)

        if not ext_id:
            errors.append("Missing required field: id")
        if not ext_name:
            warnings.append("Missing recommended field: name")
        if ext_type not in VALID_EXTENSION_TYPES:
            errors.append(f"Invalid type: {ext_type!r}. Must be one of {sorted(VALID_EXTENSION_TYPES)}")

        # Try parsing manifest
        try:
            clean = {k: v for k, v in data.items() if k != "type"}
            clean.setdefault("id", ext_id or "__placeholder__")
            parse_manifest(clean)
        except Exception as exc:
            errors.append(f"Manifest parse error: {exc}")

        # Check duplicate
        self._ensure_loaded()
        if ext_id and ext_id in self._registry._skills:
            warnings.append(f"Extension '{ext_id}' already exists — import will fail unless you update")

        # Risk warnings
        risk_data = data.get("risk", {})
        if isinstance(risk_data, dict) and risk_data.get("default_level") in ("HIGH", "CRITICAL"):
            warnings.append(f"High risk extension (default_level={risk_data['default_level']})")

        return ValidationResultDTO(
            valid=len(errors) == 0,
            extension_id=ext_id,
            extension_name=ext_name,
            errors=errors,
            warnings=warnings,
        )

    # ---- internal helpers ----

    def _persist(self, manifest: SkillManifest, ext_type: str) -> None:
        """Write manifest YAML to disk (best-effort)."""
        try:
            ext_dir = self._base_dir / manifest.id
            ext_dir.mkdir(parents=True, exist_ok=True)
            payload = _manifest_to_yaml_dict(manifest)
            payload["type"] = ext_type
            with open(ext_dir / "skill.yaml", "w", encoding="utf-8") as f:
                yaml.safe_dump(payload, f, allow_unicode=True, sort_keys=False)
        except OSError:
            pass  # dev mode — directory may not be writable

    def _rebuild_capability_index(self) -> None:
        idx: dict[str, list[str]] = {}
        for sid, m in self._registry._skills.items():
            for cap in m.capabilities:
                idx.setdefault(cap, []).append(sid)
        self._registry._capability_index = idx


# ---------------------------------------------------------------------------
# Conversion helpers
# ---------------------------------------------------------------------------

def _detect_type(manifest: SkillManifest) -> str:
    """Heuristic: read 'type' from source_path YAML or default 'skill'."""
    if manifest.source_path:
        try:
            data = yaml.safe_load(Path(manifest.source_path).read_text(encoding="utf-8"))
            if isinstance(data, dict):
                return data.get("type", _DEFAULT_TYPE)
        except Exception:
            pass
    return _DEFAULT_TYPE


def _manifest_to_summary(
    m: SkillManifest, ext_type: str, enabled: bool,
) -> ExtensionSummaryDTO:
    return ExtensionSummaryDTO(
        id=m.id,
        name=m.name,
        type=ext_type,
        version=m.version,
        summary=m.summary,
        owner=m.owner,
        capabilities=list(m.capabilities),
        risk_level=m.risk.default_level,
        enabled=enabled,
    )


def _manifest_to_detail(m: SkillManifest, ext_type: str, enabled: bool) -> dict[str, Any]:
    return {
        "id": m.id,
        "name": m.name,
        "type": ext_type,
        "version": m.version,
        "summary": m.summary,
        "owner": m.owner,
        "capabilities": list(m.capabilities),
        "executors": {k: {"enabled": v.enabled, "command": v.command} for k, v in m.executors.items()},
        "harbor_api": {
            "enabled": m.harbor_api.enabled,
            "provider": m.harbor_api.provider,
            "endpoint_group": m.harbor_api.endpoint_group,
            "allowed_methods": m.harbor_api.allowed_methods,
            "min_version": m.harbor_api.min_version,
        },
        "harbor_cli": {
            "enabled": m.harbor_cli.enabled,
            "tool": m.harbor_cli.tool,
            "command_group": m.harbor_cli.command_group,
            "allowed_subcommands": m.harbor_cli.allowed_subcommands,
            "require_structured_output": m.harbor_cli.require_structured_output,
        },
        "permissions": dict(m.permissions),
        "risk": {
            "default_level": m.risk.default_level,
            "requires_confirmation": list(m.risk.requires_confirmation),
        },
        "input_schema": dict(m.input_schema),
        "output_schema": dict(m.output_schema),
        "enabled": enabled,
    }


def _manifest_to_yaml_dict(m: SkillManifest) -> dict[str, Any]:
    """Serialise a SkillManifest back to a plain dict suitable for YAML."""
    d: dict[str, Any] = {
        "id": m.id,
        "name": m.name,
        "version": m.version,
        "summary": m.summary,
        "owner": m.owner,
        "capabilities": list(m.capabilities),
    }
    if m.harbor_api.enabled:
        d["harbor_api"] = {
            "enabled": True,
            "provider": m.harbor_api.provider,
            "endpoint_group": m.harbor_api.endpoint_group,
            "allowed_methods": m.harbor_api.allowed_methods,
            "min_version": m.harbor_api.min_version,
        }
    if m.harbor_cli.enabled:
        d["harbor_cli"] = {
            "enabled": True,
            "tool": m.harbor_cli.tool,
            "command_group": m.harbor_cli.command_group,
            "allowed_subcommands": m.harbor_cli.allowed_subcommands,
            "require_structured_output": m.harbor_cli.require_structured_output,
        }
    if m.executors:
        d["executors"] = {k: {"enabled": v.enabled, "command": v.command} for k, v in m.executors.items()}
    if m.permissions:
        d["permissions"] = dict(m.permissions)
    if m.risk.default_level != "LOW" or m.risk.requires_confirmation != ["HIGH", "CRITICAL"]:
        d["risk"] = {
            "default_level": m.risk.default_level,
            "requires_confirmation": list(m.risk.requires_confirmation),
        }
    return d


# ---------------------------------------------------------------------------
# API handler class
# ---------------------------------------------------------------------------

class HarborClawExtensionsAPI:
    """Stateless handler object — one method per endpoint.

    Instantiate with an ``ExtensionStore``.
    """

    def __init__(self, store: ExtensionStore | None = None) -> None:
        self._store = store or ExtensionStore()

    # GET /extensions
    def list_extensions(self) -> list[dict[str, Any]]:
        return [asdict(s) for s in self._store.list_all()]

    # GET /extensions/:id
    def get_extension(self, ext_id: str) -> dict[str, Any]:
        return self._store.get(ext_id)

    # POST /extensions
    def create_extension(self, body: dict[str, Any]) -> dict[str, Any]:
        return self._store.create(body)

    # PUT /extensions/:id
    def update_extension(self, ext_id: str, body: dict[str, Any]) -> dict[str, Any]:
        return self._store.update(ext_id, body)

    # DELETE /extensions/:id
    def delete_extension(self, ext_id: str) -> None:
        self._store.delete(ext_id)

    # POST /extensions/validate
    def validate_extension(self, body: dict[str, Any]) -> dict[str, Any]:
        result = self._store.validate(body)
        return asdict(result)
