"""Skill manifest model and YAML loader.

A manifest describes a skill's identity, capabilities, executor config,
permissions, risk profile, and I/O schemas.  Follows HarborBeacon-Skill-Spec-v1.
"""
from __future__ import annotations

import yaml
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any


@dataclass
class ExecutorConfig:
    """Per-route executor settings from skill.yaml."""
    enabled: bool = False
    command: str | None = None


@dataclass
class HarborApiConfig:
    """harbor_api block — middleware binding."""
    enabled: bool = False
    provider: str = "middleware"
    endpoint_group: str = ""
    allowed_methods: list[str] = field(default_factory=list)
    min_version: str = "v1"


@dataclass
class HarborCliConfig:
    """harbor_cli block — midcli binding."""
    enabled: bool = False
    tool: str = "midcli"
    command_group: str = ""
    allowed_subcommands: list[str] = field(default_factory=list)
    require_structured_output: bool = True


@dataclass
class RiskConfig:
    default_level: str = "LOW"
    requires_confirmation: list[str] = field(default_factory=lambda: ["HIGH", "CRITICAL"])


@dataclass
class SkillManifest:
    """Parsed skill.yaml."""
    id: str
    name: str = ""
    version: str = "0.0.0"
    summary: str = ""
    owner: str = ""
    capabilities: list[str] = field(default_factory=list)
    executors: dict[str, ExecutorConfig] = field(default_factory=dict)
    harbor_api: HarborApiConfig = field(default_factory=HarborApiConfig)
    harbor_cli: HarborCliConfig = field(default_factory=HarborCliConfig)
    permissions: dict[str, Any] = field(default_factory=dict)
    risk: RiskConfig = field(default_factory=RiskConfig)
    input_schema: dict[str, Any] = field(default_factory=dict)
    output_schema: dict[str, Any] = field(default_factory=dict)
    timeouts: dict[str, int] = field(default_factory=dict)
    retries: dict[str, int] = field(default_factory=dict)
    source_path: str | None = None

    @property
    def domains(self) -> set[str]:
        """Extract unique domain prefixes from capabilities (e.g. 'video' from 'video.trim')."""
        return {c.split(".")[0] for c in self.capabilities if "." in c}


def _parse_executor_config(data: dict[str, Any] | None) -> dict[str, ExecutorConfig]:
    if not data:
        return {}
    result = {}
    for key, cfg in data.items():
        if isinstance(cfg, dict):
            result[key] = ExecutorConfig(
                enabled=cfg.get("enabled", False),
                command=cfg.get("command"),
            )
        elif isinstance(cfg, bool):
            result[key] = ExecutorConfig(enabled=cfg)
    return result


def _parse_harbor_api(data: dict[str, Any] | None) -> HarborApiConfig:
    if not data:
        return HarborApiConfig()
    return HarborApiConfig(
        enabled=data.get("enabled", False),
        provider=data.get("provider", "middleware"),
        endpoint_group=data.get("endpoint_group", ""),
        allowed_methods=data.get("allowed_methods", []),
        min_version=data.get("min_version", "v1"),
    )


def _parse_harbor_cli(data: dict[str, Any] | None) -> HarborCliConfig:
    if not data:
        return HarborCliConfig()
    return HarborCliConfig(
        enabled=data.get("enabled", False),
        tool=data.get("tool", "midcli"),
        command_group=data.get("command_group", ""),
        allowed_subcommands=data.get("allowed_subcommands", []),
        require_structured_output=data.get("require_structured_output", True),
    )


def _parse_risk(data: dict[str, Any] | None) -> RiskConfig:
    if not data:
        return RiskConfig()
    return RiskConfig(
        default_level=data.get("default_level", "LOW"),
        requires_confirmation=data.get("requires_confirmation", ["HIGH", "CRITICAL"]),
    )


def load_manifest(path: Path) -> SkillManifest:
    """Load a skill.yaml file and return a SkillManifest."""
    text = path.read_text(encoding="utf-8")
    data = yaml.safe_load(text)
    if not isinstance(data, dict):
        raise ValueError(f"skill.yaml must be a YAML mapping, got {type(data).__name__}")
    return parse_manifest(data, source_path=str(path))


def parse_manifest(data: dict[str, Any], *, source_path: str | None = None) -> SkillManifest:
    """Parse a dict (from YAML or programmatic construction) into a SkillManifest."""
    skill_id = data.get("id")
    if not skill_id:
        raise ValueError("skill manifest must have an 'id' field")

    return SkillManifest(
        id=skill_id,
        name=data.get("name", ""),
        version=data.get("version", "0.0.0"),
        summary=data.get("summary", ""),
        owner=data.get("owner", ""),
        capabilities=data.get("capabilities", []),
        executors=_parse_executor_config(data.get("executors")),
        harbor_api=_parse_harbor_api(data.get("harbor_api")),
        harbor_cli=_parse_harbor_cli(data.get("harbor_cli")),
        permissions=data.get("permissions", {}),
        risk=_parse_risk(data.get("risk")),
        input_schema=data.get("input_schema", {}),
        output_schema=data.get("output_schema", {}),
        timeouts=data.get("timeouts", {}),
        retries=data.get("retries", {}),
        source_path=source_path,
    )


def load_manifests_from_dir(skills_dir: Path) -> list[SkillManifest]:
    """Scan a directory tree for skill.yaml files and load them all."""
    manifests = []
    if not skills_dir.is_dir():
        return manifests
    for yaml_path in sorted(skills_dir.rglob("skill.yaml")):
        manifests.append(load_manifest(yaml_path))
    return manifests
