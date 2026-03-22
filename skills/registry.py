"""Skill registry: loads manifests, indexes capabilities, resolves skills.

The registry is the single source of truth for what skills are installed
and what capabilities they provide. It integrates with the Router by
providing executor information from manifests.
"""
from __future__ import annotations

from pathlib import Path
from typing import Any

from .manifest import SkillManifest, load_manifest, load_manifests_from_dir, parse_manifest


class SkillNotFoundError(KeyError):
    pass


class DuplicateSkillError(ValueError):
    pass


class Registry:
    """In-memory skill registry with capability indexing."""

    def __init__(self) -> None:
        self._skills: dict[str, SkillManifest] = {}
        # capability -> skill_id(s) that provide it
        self._capability_index: dict[str, list[str]] = {}

    @property
    def skill_ids(self) -> list[str]:
        return sorted(self._skills.keys())

    @property
    def skills(self) -> list[SkillManifest]:
        return [self._skills[k] for k in self.skill_ids]

    def __len__(self) -> int:
        return len(self._skills)

    # ---- registration ----

    def register(self, manifest: SkillManifest) -> None:
        """Register a skill manifest. Raises DuplicateSkillError on conflict."""
        if manifest.id in self._skills:
            raise DuplicateSkillError(f"Skill already registered: {manifest.id}")
        self._skills[manifest.id] = manifest
        for cap in manifest.capabilities:
            self._capability_index.setdefault(cap, []).append(manifest.id)

    def register_dict(self, data: dict[str, Any]) -> SkillManifest:
        """Parse and register from a raw dict."""
        m = parse_manifest(data)
        self.register(m)
        return m

    def load_dir(self, skills_dir: Path) -> int:
        """Load all skill.yaml files from a directory and register them.
        Returns the number of skills loaded.
        """
        manifests = load_manifests_from_dir(skills_dir)
        for m in manifests:
            if m.id not in self._skills:
                self.register(m)
        return len(manifests)

    # ---- lookup ----

    def get(self, skill_id: str) -> SkillManifest:
        """Get a skill by ID. Raises SkillNotFoundError if missing."""
        try:
            return self._skills[skill_id]
        except KeyError:
            raise SkillNotFoundError(f"Skill not found: {skill_id}")

    def find_by_capability(self, capability: str) -> list[SkillManifest]:
        """Return all skills that provide a given capability."""
        ids = self._capability_index.get(capability, [])
        return [self._skills[sid] for sid in ids]

    def find_by_domain(self, domain: str) -> list[SkillManifest]:
        """Return all skills whose capabilities start with the given domain."""
        results = []
        seen = set()
        for cap, sids in self._capability_index.items():
            if cap.split(".")[0] == domain:
                for sid in sids:
                    if sid not in seen:
                        seen.add(sid)
                        results.append(self._skills[sid])
        return results

    def has_capability(self, capability: str) -> bool:
        return capability in self._capability_index

    # ---- unregister ----

    def unregister(self, skill_id: str) -> None:
        """Remove a skill from the registry."""
        manifest = self._skills.pop(skill_id, None)
        if manifest is None:
            return
        for cap in manifest.capabilities:
            ids = self._capability_index.get(cap, [])
            self._capability_index[cap] = [s for s in ids if s != skill_id]
            if not self._capability_index[cap]:
                del self._capability_index[cap]

    # ---- summary ----

    def summary(self) -> dict[str, Any]:
        return {
            "total_skills": len(self._skills),
            "total_capabilities": sum(len(m.capabilities) for m in self._skills.values()),
            "skills": [
                {"id": m.id, "version": m.version, "capabilities": m.capabilities}
                for m in self.skills
            ],
        }
