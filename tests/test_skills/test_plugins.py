"""Tests for plugin skill manifests and handlers."""
from pathlib import Path

import pytest

from skills.manifest import load_manifest, load_manifests_from_dir
from skills.registry import Registry
from skills.executor import executors_from_manifest, CliExecutor, MiddlewareApiExecutor, MidcliSkillExecutor, BrowserExecutor

BUILTINS_DIR = Path(__file__).resolve().parents[2] / "skills" / "builtins"


# ── Manifest loading ────────────────────────────────────────────────

class TestPluginManifests:
    def test_files_batch_ops_loads(self):
        m = load_manifest(BUILTINS_DIR / "files.batch_ops" / "skill.yaml")
        assert m.id == "files.batch_ops"
        assert set(m.capabilities) == {"files.search", "files.copy", "files.move", "files.archive"}
        assert m.domains == {"files"}
        assert m.harbor_api.enabled is True
        assert m.harbor_cli.enabled is True

    def test_media_video_edit_loads(self):
        m = load_manifest(BUILTINS_DIR / "media.video_edit" / "skill.yaml")
        assert m.id == "media.video_edit"
        assert "video.trim" in m.capabilities
        assert "video.concat" in m.capabilities
        assert m.domains == {"video"}
        assert m.harbor_api.enabled is False
        assert m.harbor_cli.enabled is False

    def test_browser_automation_loads(self):
        m = load_manifest(BUILTINS_DIR / "browser.automation" / "skill.yaml")
        assert m.id == "browser.automation"
        assert "browser.navigate" in m.capabilities
        assert "browser.screenshot" in m.capabilities
        assert m.domains == {"browser"}
        assert m.harbor_api.enabled is False

    def test_all_builtins_load_via_dir(self):
        manifests = load_manifests_from_dir(BUILTINS_DIR)
        assert len(manifests) >= 4  # harbor_ops + 3 plugins
        ids = {m.id for m in manifests}
        assert "system.harbor_ops" in ids
        assert "files.batch_ops" in ids
        assert "media.video_edit" in ids
        assert "browser.automation" in ids


# ── Registry integration ────────────────────────────────────────────

class TestPluginRegistryIntegration:
    def test_load_all_builtins_into_registry(self):
        r = Registry()
        r.load_dir(BUILTINS_DIR)
        assert len(r) >= 4

    def test_find_service_domain(self):
        r = Registry()
        r.load_dir(BUILTINS_DIR)
        svc = r.find_by_domain("service")
        assert any(m.id == "system.harbor_ops" for m in svc)

    def test_find_files_domain(self):
        r = Registry()
        r.load_dir(BUILTINS_DIR)
        files = r.find_by_domain("files")
        assert any(m.id == "files.batch_ops" for m in files)

    def test_find_video_domain(self):
        r = Registry()
        r.load_dir(BUILTINS_DIR)
        vids = r.find_by_domain("video")
        assert any(m.id == "media.video_edit" for m in vids)

    def test_find_browser_domain(self):
        r = Registry()
        r.load_dir(BUILTINS_DIR)
        b = r.find_by_domain("browser")
        assert any(m.id == "browser.automation" for m in b)

    def test_has_file_capabilities(self):
        r = Registry()
        r.load_dir(BUILTINS_DIR)
        assert r.has_capability("files.copy")
        assert r.has_capability("files.archive")

    def test_no_cross_domain_leak(self):
        r = Registry()
        r.load_dir(BUILTINS_DIR)
        svc = r.find_by_domain("service")
        assert not any(m.id == "files.batch_ops" for m in svc)


# ── Executor factory integration ────────────────────────────────────

class TestPluginExecutorFactory:
    _dummy_api = staticmethod(lambda m, r, a: ({}, 0))
    _dummy_cli = staticmethod(lambda c: ("", 0))

    def test_files_batch_ops_gets_api_and_cli(self):
        m = load_manifest(BUILTINS_DIR / "files.batch_ops" / "skill.yaml")
        execs = executors_from_manifest(m, api_call_fn=self._dummy_api, cli_run_fn=self._dummy_cli)
        routes = {e.route.value for e in execs}
        assert "middleware_api" in routes
        assert "midcli" in routes

    def test_media_video_edit_gets_cli_only(self):
        m = load_manifest(BUILTINS_DIR / "media.video_edit" / "skill.yaml")
        execs = executors_from_manifest(m, cli_run_fn=self._dummy_cli)
        assert len(execs) >= 1
        assert all(isinstance(e, CliExecutor) for e in execs)

    def test_browser_automation_gets_browser_executor(self):
        m = load_manifest(BUILTINS_DIR / "browser.automation" / "skill.yaml")
        execs = executors_from_manifest(m, cli_run_fn=self._dummy_cli)
        types = {type(e) for e in execs}
        assert BrowserExecutor in types or CliExecutor in types

    def test_harbor_ops_gets_api_and_midcli(self):
        m = load_manifest(BUILTINS_DIR / "system.harbor_ops" / "skill.yaml")
        execs = executors_from_manifest(m, api_call_fn=self._dummy_api, cli_run_fn=self._dummy_cli)
        types = {type(e) for e in execs}
        assert MiddlewareApiExecutor in types
        assert MidcliSkillExecutor in types


# ── Handler stubs ────────────────────────────────────────────────────

class TestFilesBatchOpsHandler:
    def test_handler_import(self):
        """Verify handler module can be imported by path."""
        import importlib.util
        handler_path = BUILTINS_DIR / "files.batch_ops" / "handler.py"
        spec = importlib.util.spec_from_file_location("files_batch_ops_handler", handler_path)
        mod = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(mod)
        result = mod.handle("search", source="/pool/data", pattern="*.mp4")
        assert result["status"] == "ok"
        assert result["operation"] == "search"

    def test_handler_copy(self):
        import importlib.util
        handler_path = BUILTINS_DIR / "files.batch_ops" / "handler.py"
        spec = importlib.util.spec_from_file_location("files_batch_ops_handler", handler_path)
        mod = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(mod)
        result = mod.handle("copy", source="/a", destination="/b")
        assert result["status"] == "ok"
        assert result["operation"] == "copy"

    def test_handler_unknown_op(self):
        import importlib.util
        handler_path = BUILTINS_DIR / "files.batch_ops" / "handler.py"
        spec = importlib.util.spec_from_file_location("files_batch_ops_handler", handler_path)
        mod = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(mod)
        result = mod.handle("delete", source="/x")
        assert "error" in result


class TestMediaVideoEditHandler:
    def test_handler_trim(self):
        import importlib.util
        handler_path = BUILTINS_DIR / "media.video_edit" / "handler.py"
        spec = importlib.util.spec_from_file_location("media_video_edit_handler", handler_path)
        mod = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(mod)
        result = mod.handle("trim", input_path="/v/clip.mp4", output_path="/v/out.mp4")
        assert result["status"] == "ok"
        assert result["operation"] == "trim"


class TestBrowserAutomationHandler:
    def test_handler_navigate(self):
        import importlib.util
        handler_path = BUILTINS_DIR / "browser.automation" / "handler.py"
        spec = importlib.util.spec_from_file_location("browser_automation_handler", handler_path)
        mod = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(mod)
        result = mod.handle("navigate", url="https://example.com")
        assert result["status"] == "ok"
        assert result["operation"] == "navigate"

    def test_handler_screenshot(self):
        import importlib.util
        handler_path = BUILTINS_DIR / "browser.automation" / "handler.py"
        spec = importlib.util.spec_from_file_location("browser_automation_handler", handler_path)
        mod = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(mod)
        result = mod.handle("screenshot", url="https://example.com", output_path="/tmp/shot.png")
        assert result["status"] == "ok"
