"""Tests for harborclaw.api (SettingsStore + HarborClawSettingsAPI)."""
from __future__ import annotations

import copy
import json
import os
import tempfile
from pathlib import Path

import pytest
import yaml

from harborclaw.api import SettingsStore
from harborclaw.api.settings_api import HarborClawSettingsAPI, ConnectivityResultDTO
from harborclaw.channels import Channel
from orchestrator.contracts import Route


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------

@pytest.fixture()
def tmp_settings_path(tmp_path: Path) -> Path:
    return tmp_path / "settings.yaml"


@pytest.fixture()
def store(tmp_settings_path: Path) -> SettingsStore:
    return SettingsStore(path=tmp_settings_path)


@pytest.fixture()
def api(store: SettingsStore) -> HarborClawSettingsAPI:
    return HarborClawSettingsAPI(store=store)


# ===================================================================
# SettingsStore
# ===================================================================

class TestSettingsStore:
    """Unit tests for SettingsStore."""

    def test_load_defaults_when_no_file(self, store: SettingsStore) -> None:
        settings = store.load()
        assert isinstance(settings, dict)
        assert "channels" in settings
        assert "autonomy" in settings
        assert "route_priority" in settings
        assert settings["autonomy"]["default_level"] == "Supervised"

    def test_load_returns_deep_copy(self, store: SettingsStore) -> None:
        s1 = store.load()
        s2 = store.load()
        assert s1 == s2
        s1["autonomy"]["default_level"] = "Full"
        s3 = store.load()
        assert s3["autonomy"]["default_level"] == "Supervised"

    def test_save_and_reload(self, store: SettingsStore, tmp_settings_path: Path) -> None:
        settings = store.load()
        settings["autonomy"]["default_level"] = "Full"
        store.save(settings)

        # File should exist
        assert tmp_settings_path.exists()

        # Re-read from disk
        reloaded = store.reload()
        assert reloaded["autonomy"]["default_level"] == "Full"

    def test_save_persists_channels(self, store: SettingsStore) -> None:
        settings = store.load()
        settings["channels"][0]["enabled"] = True
        settings["channels"][0]["app_id"] = "test-id"
        store.save(settings)

        fresh = store.reload()
        assert fresh["channels"][0]["enabled"] is True
        assert fresh["channels"][0]["app_id"] == "test-id"

    def test_reset_restores_defaults(self, store: SettingsStore) -> None:
        settings = store.load()
        settings["autonomy"]["default_level"] = "Full"
        store.save(settings)

        reset = store.reset()
        assert reset["autonomy"]["default_level"] == "Supervised"

    def test_default_channels_match_enum(self, store: SettingsStore) -> None:
        settings = store.load()
        channel_names = {c["channel"] for c in settings["channels"]}
        enum_names = {ch.value for ch in Channel}
        assert channel_names == enum_names

    def test_default_route_priority(self, store: SettingsStore) -> None:
        settings = store.load()
        assert settings["route_priority"] == [
            "middleware_api", "midcli", "browser", "mcp",
        ]


# ===================================================================
# HarborClawSettingsAPI
# ===================================================================

class TestSettingsAPI:
    """Unit tests for HarborClawSettingsAPI handlers."""

    def test_get_settings_returns_defaults(self, api: HarborClawSettingsAPI) -> None:
        result = api.get_settings()
        assert result["autonomy"]["default_level"] == "Supervised"
        assert len(result["channels"]) == len(Channel)

    def test_put_settings_round_trip(self, api: HarborClawSettingsAPI) -> None:
        payload = api.get_settings()
        payload["autonomy"]["default_level"] = "ReadOnly"
        saved = api.put_settings(payload)
        assert saved["autonomy"]["default_level"] == "ReadOnly"
        assert api.get_settings()["autonomy"]["default_level"] == "ReadOnly"

    def test_test_channel_missing_creds(self, api: HarborClawSettingsAPI) -> None:
        result = api.test_channel({
            "channel": "telegram",
            "config": {"enabled": True},
        })
        assert result["reachable"] is False
        assert "credentials" in result["error"].lower()

    def test_test_channel_with_creds(self, api: HarborClawSettingsAPI) -> None:
        result = api.test_channel({
            "channel": "telegram",
            "config": {"enabled": True, "bot_token": "123:ABC"},
        })
        assert result["reachable"] is True
        assert result["latency_ms"] is not None

    def test_test_channel_feishu(self, api: HarborClawSettingsAPI) -> None:
        result = api.test_channel({
            "channel": "feishu",
            "config": {
                "enabled": True,
                "app_id": "cli_xxx",
                "app_secret": "secret",
            },
        })
        assert result["reachable"] is True

    def test_test_channel_mqtt(self, api: HarborClawSettingsAPI) -> None:
        result = api.test_channel({
            "channel": "mqtt",
            "config": {
                "enabled": True,
                "extra": {"broker": "localhost"},
            },
        })
        assert result["reachable"] is True

    def test_test_channels_only_enabled(self, api: HarborClawSettingsAPI) -> None:
        # All disabled by default
        results = api.test_channels()
        assert results == []

        # Enable one
        settings = api.get_settings()
        for ch in settings["channels"]:
            if ch["channel"] == "slack":
                ch["enabled"] = True
                ch["bot_token"] = "xoxb-test"
        api.put_settings(settings)

        results = api.test_channels()
        assert len(results) == 1
        assert results[0]["channel"] == "slack"
        assert results[0]["reachable"] is True

    def test_get_route_status_defaults(self, api: HarborClawSettingsAPI) -> None:
        statuses = api.get_route_status()
        assert len(statuses) == 4
        assert statuses[0]["route"] == "middleware_api"
        assert statuses[0]["priority"] == 1
        assert statuses[0]["available"] is True

    def test_get_route_status_with_checker(self, store: SettingsStore) -> None:
        def fake_checker(route: Route) -> bool:
            return route != Route.BROWSER

        api = HarborClawSettingsAPI(store=store, route_checker=fake_checker)
        statuses = api.get_route_status()

        browser = next(s for s in statuses if s["route"] == "browser")
        assert browser["available"] is False

        mw = next(s for s in statuses if s["route"] == "middleware_api")
        assert mw["available"] is True

    def test_put_settings_preserves_custom_route_order(
        self, api: HarborClawSettingsAPI,
    ) -> None:
        payload = api.get_settings()
        payload["route_priority"] = ["mcp", "browser", "midcli", "middleware_api"]
        saved = api.put_settings(payload)
        assert saved["route_priority"] == ["mcp", "browser", "midcli", "middleware_api"]

    def test_put_settings_channel_toggle(self, api: HarborClawSettingsAPI) -> None:
        settings = api.get_settings()
        # Enable discord
        for ch in settings["channels"]:
            if ch["channel"] == "discord":
                ch["enabled"] = True
                ch["bot_token"] = "MTIzNDU2.abc"
        api.put_settings(settings)

        reloaded = api.get_settings()
        discord = next(c for c in reloaded["channels"] if c["channel"] == "discord")
        assert discord["enabled"] is True
        assert discord["bot_token"] == "MTIzNDU2.abc"

    def test_autonomy_allow_full_for_channels(self, api: HarborClawSettingsAPI) -> None:
        settings = api.get_settings()
        settings["autonomy"]["allow_full_for_channels"] = ["telegram", "discord"]
        saved = api.put_settings(settings)
        assert saved["autonomy"]["allow_full_for_channels"] == ["telegram", "discord"]
