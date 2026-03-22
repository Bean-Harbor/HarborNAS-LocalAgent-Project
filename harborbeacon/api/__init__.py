"""Persistent storage for HarborBeacon settings.

Reads/writes ``/etc/harborbeacon/settings.yaml`` (YAML) on HarborOS.
Falls back to an in-memory dict when the file is absent (dev mode).
"""
from __future__ import annotations

import copy
import os
from pathlib import Path
from typing import Any

import yaml

from harborbeacon.channels import Channel

_DEFAULT_SETTINGS: dict[str, Any] = {
    "channels": [
        {"channel": ch.value, "enabled": False, "extra": {}}
        for ch in Channel
    ],
    "autonomy": {
        "default_level": "Supervised",
        "approval_timeout_seconds": 120,
        "allow_full_for_channels": [],
    },
    "route_priority": ["middleware_api", "midcli", "browser", "mcp"],
}

_SETTINGS_PATH = Path(os.environ.get(
    "HARBORBEACON_SETTINGS_PATH",
    "/etc/harborbeacon/settings.yaml",
))


class SettingsStore:
    """Load / save HarborBeacon settings from YAML or memory."""

    def __init__(self, path: Path | None = None) -> None:
        self._path = path or _SETTINGS_PATH
        self._cache: dict[str, Any] | None = None

    # ---- public API ----

    def load(self) -> dict[str, Any]:
        """Return current settings (from file, or defaults)."""
        if self._cache is not None:
            return copy.deepcopy(self._cache)
        if self._path.exists():
            with open(self._path, "r", encoding="utf-8") as f:
                data = yaml.safe_load(f)
            self._cache = {**copy.deepcopy(_DEFAULT_SETTINGS), **(data or {})}
        else:
            self._cache = copy.deepcopy(_DEFAULT_SETTINGS)
        return copy.deepcopy(self._cache)

    def save(self, settings: dict[str, Any]) -> dict[str, Any]:
        """Persist settings and return the saved copy."""
        self._cache = copy.deepcopy(settings)
        try:
            self._path.parent.mkdir(parents=True, exist_ok=True)
            with open(self._path, "w", encoding="utf-8") as f:
                yaml.safe_dump(settings, f, allow_unicode=True, sort_keys=False)
        except OSError:
            pass  # dev mode — file system may be read-only
        return copy.deepcopy(self._cache)

    def reset(self) -> dict[str, Any]:
        """Restore factory defaults."""
        self._cache = None
        try:
            self._path.unlink(missing_ok=True)
        except OSError:
            pass
        return self.load()

    def reload(self) -> dict[str, Any]:
        """Force re-read from disk."""
        self._cache = None
        return self.load()
