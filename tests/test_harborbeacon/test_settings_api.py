"""Tests for harborbeacon.api (SettingsStore + HarborBeaconSettingsAPI)."""
from __future__ import annotations

import copy
import json
import os
import tempfile
from pathlib import Path

import pytest
import yaml

from harborbeacon.api import SettingsStore
from harborbeacon.api.settings_api import HarborBeaconSettingsAPI, ConnectivityResultDTO
from harborbeacon.channels import Channel
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
def api(store: SettingsStore) -> HarborBeaconSettingsAPI:
    return HarborBeaconSettingsAPI(store=store)


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
# HarborBeaconSettingsAPI
# ===================================================================

class TestSettingsAPI:
    """Unit tests for HarborBeaconSettingsAPI handlers."""

    def test_get_settings_returns_defaults(self, api: HarborBeaconSettingsAPI) -> None:
        result = api.get_settings()
        assert result["autonomy"]["default_level"] == "Supervised"
        assert len(result["channels"]) == len(Channel)

    def test_put_settings_round_trip(self, api: HarborBeaconSettingsAPI) -> None:
        payload = api.get_settings()
        payload["autonomy"]["default_level"] = "ReadOnly"
        saved = api.put_settings(payload)
        assert saved["autonomy"]["default_level"] == "ReadOnly"
        assert api.get_settings()["autonomy"]["default_level"] == "ReadOnly"

    def test_test_channel_missing_creds(self, api: HarborBeaconSettingsAPI) -> None:
        result = api.test_channel({
            "channel": "telegram",
            "config": {"enabled": True},
        })
        assert result["reachable"] is False
        assert "credentials" in result["error"].lower()

    def test_test_channel_with_creds(self, api: HarborBeaconSettingsAPI) -> None:
        result = api.test_channel({
            "channel": "telegram",
            "config": {"enabled": True, "bot_token": "123:ABC"},
        })
        assert result["reachable"] is True
        assert result["latency_ms"] is not None

    def test_test_channel_feishu(self, api: HarborBeaconSettingsAPI) -> None:
        result = api.test_channel({
            "channel": "feishu",
            "config": {
                "enabled": True,
                "app_id": "cli_xxx",
                "app_secret": "secret",
            },
        })
        assert result["reachable"] is True

    def test_test_channel_mqtt(self, api: HarborBeaconSettingsAPI) -> None:
        result = api.test_channel({
            "channel": "mqtt",
            "config": {
                "enabled": True,
                "extra": {"broker": "localhost"},
            },
        })
        assert result["reachable"] is True

    def test_test_channels_only_enabled(self, api: HarborBeaconSettingsAPI) -> None:
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

    def test_configure_feishu_missing_credentials(
        self, api: HarborBeaconSettingsAPI,
    ) -> None:
        result = api.configure_feishu({"app_id": ""})
        assert result["success"] is False
        assert "required" in result["message"].lower()

    def test_configure_feishu_success(
        self, api: HarborBeaconSettingsAPI, monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        from harborbeacon.api import settings_api as module

        monkeypatch.setattr(
            module,
            "_fetch_feishu_tenant_access_token",
            lambda app_id, app_secret: "tenant_token_x",
        )
        monkeypatch.setattr(
            module,
            "_fetch_feishu_bot_info",
            lambda token: {
                "app_name": "HarborBeaconBot",
                "tenant_key": "tenant_x",
                "open_id": "ou_x",
            },
        )

        result = api.configure_feishu({
            "app_id": "cli_xxx",
            "app_secret": "sec_xxx",
            "webhook_url": "https://example.com/feishu",
        })

        assert result["success"] is True
        assert result["settings_updated"] is True
        assert result["connectivity"]["reachable"] is True

        settings = api.get_settings()
        feishu = next(ch for ch in settings["channels"] if ch["channel"] == "feishu")
        assert feishu["enabled"] is True
        assert feishu["app_id"] == "cli_xxx"
        assert feishu["webhook_url"] == "https://example.com/feishu"
        assert feishu["extra"]["validated"] is True

    def test_configure_feishu_validation_failed(
        self, api: HarborBeaconSettingsAPI, monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        from harborbeacon.api import settings_api as module

        def _raise(*_args: object, **_kwargs: object) -> str:
            raise RuntimeError("bad credentials")

        monkeypatch.setattr(module, "_fetch_feishu_tenant_access_token", _raise)

        result = api.configure_feishu({
            "app_id": "cli_bad",
            "app_secret": "sec_bad",
        })

        assert result["success"] is False
        assert "validation failed" in result["message"].lower()
        assert result["settings_updated"] is False

    def test_get_route_status_defaults(self, api: HarborBeaconSettingsAPI) -> None:
        statuses = api.get_route_status()
        assert len(statuses) == 4
        assert statuses[0]["route"] == "middleware_api"
        assert statuses[0]["priority"] == 1
        assert statuses[0]["available"] is True

    def test_get_route_status_with_checker(self, store: SettingsStore) -> None:
        def fake_checker(route: Route) -> bool:
            return route != Route.BROWSER

        api = HarborBeaconSettingsAPI(store=store, route_checker=fake_checker)
        statuses = api.get_route_status()

        browser = next(s for s in statuses if s["route"] == "browser")
        assert browser["available"] is False

        mw = next(s for s in statuses if s["route"] == "middleware_api")
        assert mw["available"] is True

    def test_put_settings_preserves_custom_route_order(
        self, api: HarborBeaconSettingsAPI,
    ) -> None:
        payload = api.get_settings()
        payload["route_priority"] = ["mcp", "browser", "midcli", "middleware_api"]
        saved = api.put_settings(payload)
        assert saved["route_priority"] == ["mcp", "browser", "midcli", "middleware_api"]

