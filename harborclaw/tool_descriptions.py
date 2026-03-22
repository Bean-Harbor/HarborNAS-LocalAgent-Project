"""Generate HarborClaw TOML tool descriptions from skill manifests.

HarborClaw discovers tools via ``tool_descriptions/<lang>.toml`` files.
This module converts our SkillManifest capabilities into that format,
enabling HarborClaw to display localised tool descriptions to users.
"""
from __future__ import annotations

from typing import Any

from skills.manifest import SkillManifest
from skills.registry import Registry

from .autonomy import Autonomy, risk_to_autonomy


def manifest_to_tool_descriptions(
    manifest: SkillManifest,
    *,
    lang: str = "en",
) -> dict[str, str]:
    """Return a {tool_name: description} mapping for one manifest.

    Each capability becomes one tool entry.
    """
    descriptions: dict[str, str] = {}
    for cap in manifest.capabilities:
        _, operation = cap.split(".", 1) if "." in cap else (cap, cap)
        desc = f"{manifest.summary} — {operation}" if manifest.summary else f"{manifest.name}: {operation}"
        descriptions[cap] = desc
    return descriptions


def registry_to_toml(registry: Registry, *, lang: str = "en") -> str:
    """Generate a TOML string with tool descriptions for all registered skills.

    Compatible with HarborClaw's ``tool_descriptions/<lang>.toml`` format.
    """
    lines = [f"# Auto-generated HarborOS tool descriptions ({lang})", ""]
    for manifest in registry.skills:
        for cap in manifest.capabilities:
            desc = manifest_to_tool_descriptions(manifest).get(cap, cap)
            # TOML key = tool name (dotted keys need quoting)
            lines.append(f'"{cap}" = "{_escape_toml(desc)}"')
        lines.append("")
    return "\n".join(lines)


def manifest_to_skill_toml(manifest: SkillManifest) -> str:
    """Generate a HarborClaw SKILL.toml for a single skill manifest.

    Skills live at ``/etc/harborclaw/skills/<name>/SKILL.toml``.
    """
    min_autonomy = risk_to_autonomy(
        _risk_level_from_str(manifest.risk.default_level)
    )
    lines = [
        f'name = "{_escape_toml(manifest.name)}"',
        f'version = "{manifest.version}"',
        f'description = "{_escape_toml(manifest.summary)}"',
        f'owner = "{_escape_toml(manifest.owner)}"',
        f'min_autonomy = "{min_autonomy.value}"',
        "",
        "[capabilities]",
    ]
    for cap in manifest.capabilities:
        lines.append(f'  "{cap}" = true')

    lines.append("")
    lines.append("[harbor]")
    lines.append(f"  api_enabled = {_toml_bool(manifest.harbor_api.enabled)}")
    lines.append(f"  cli_enabled = {_toml_bool(manifest.harbor_cli.enabled)}")

    if manifest.harbor_api.allowed_methods:
        methods = ", ".join(f'"{m}"' for m in manifest.harbor_api.allowed_methods)
        lines.append(f"  api_methods = [{methods}]")
    if manifest.harbor_cli.allowed_subcommands:
        cmds = ", ".join(f'"{c}"' for c in manifest.harbor_cli.allowed_subcommands)
        lines.append(f"  cli_subcommands = [{cmds}]")

    lines.append("")
    return "\n".join(lines)


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _escape_toml(s: str) -> str:
    return s.replace("\\", "\\\\").replace('"', '\\"')


def _toml_bool(v: bool) -> str:
    return "true" if v else "false"


def _risk_level_from_str(s: str) -> Any:
    from assistant.contracts import RiskLevel
    return RiskLevel(s)
