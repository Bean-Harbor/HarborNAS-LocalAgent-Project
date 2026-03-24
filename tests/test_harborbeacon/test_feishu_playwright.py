"""Tests for Playwright-based Feishu browser adapter.

Uses mocking to avoid launching a real browser while verifying the
driver's method call patterns and the flow's Playwright integration.
"""
from __future__ import annotations

from unittest.mock import MagicMock, patch, PropertyMock
import pytest

from harborbeacon.api.feishu_browser_setup import (
    FeishuBrowserSetupFlow,
    FeishuBrowserSetupSession,
    SetupStepStatus,
    _sessions,
)
from harborbeacon.api.feishu_playwright import PlaywrightFeishuDriver, HAS_PLAYWRIGHT


# ---------------------------------------------------------------------------
# PlaywrightFeishuDriver unit tests (mocked Playwright)
# ---------------------------------------------------------------------------

class TestPlaywrightDriverImport:
    def test_has_playwright_flag(self) -> None:
        assert HAS_PLAYWRIGHT is True  # playwright is installed


class TestPlaywrightDriverMocked:
    """Test the driver with a fully mocked Playwright backend."""

    @pytest.fixture()
    def mock_pw(self):
        """Patch sync_playwright to return mocks."""
        with patch("harborbeacon.api.feishu_playwright.sync_playwright") as mock_sp:
            mock_context = MagicMock()
            mock_page = MagicMock()
            mock_page.url = "https://open.feishu.cn/app/cli_test123/settings"
            mock_page.query_selector.return_value = True  # login detected

            mock_browser = MagicMock()
            mock_browser.new_context.return_value = mock_context
            mock_context.new_page.return_value = mock_page

            mock_pw_instance = MagicMock()
            mock_pw_instance.chromium.launch.return_value = mock_browser
            mock_sp.return_value.start.return_value = mock_pw_instance

            yield {
                "sync_playwright": mock_sp,
                "pw": mock_pw_instance,
                "browser": mock_browser,
                "context": mock_context,
                "page": mock_page,
            }

    def test_launch_and_close(self, mock_pw: dict) -> None:
        driver = PlaywrightFeishuDriver(headless=True)
        driver.launch()
        assert driver._page is not None
        driver.close()
        assert driver._page is None

    def test_open_login(self, mock_pw: dict) -> None:
        driver = PlaywrightFeishuDriver(headless=True)
        driver.launch()
        url = driver.open_login()
        assert url == "https://open.feishu.cn/app"
        mock_pw["page"].goto.assert_called_once()

    def test_wait_for_login_success(self, mock_pw: dict) -> None:
        driver = PlaywrightFeishuDriver(headless=True)
        driver.launch()
        result = driver.wait_for_login(timeout_sec=5)
        assert result is True

    def test_wait_for_login_via_url(self, mock_pw: dict) -> None:
        mock_pw["page"].query_selector.return_value = None
        mock_pw["page"].url = "https://open.feishu.cn/app/cli_xxx/overview"
        driver = PlaywrightFeishuDriver(headless=True)
        driver.launch()
        result = driver.wait_for_login(timeout_sec=5)
        assert result is True

    def test_create_app(self, mock_pw: dict) -> None:
        page = mock_pw["page"]
        # _js_page_info, _js_fetch (API attempt), _js_click, _js_fill, etc.
        # all go through page.evaluate(). Mock evaluate to return the right
        # values depending on the call.
        call_count = {"n": 0}
        def evaluate_side_effect(*args, **kwargs):
            call_count["n"] += 1
            # _js_page_info returns page info dict
            if call_count["n"] == 1:
                return {"url": page.url, "title": "", "buttons": ["创建自建应用"], "inputs": []}
            # _js_fetch for API discovery → fail so we fall through to DOM
            if isinstance(args[0], str) and 'fetch' in args[0]:
                return {"status": 404, "ok": False, "data": ""}
            # _js_click returns found=True
            if isinstance(args[0], str) and 'click' in args[0]:
                return {"found": True, "text": "创建自建应用", "tag": "BUTTON"}
            # _js_fill returns found=True
            if isinstance(args[0], str) and 'input' in args[0].lower():
                return {"found": True, "placeholder": "应用名称", "name": ""}
            return {"found": True}
        page.evaluate.side_effect = evaluate_side_effect

        driver = PlaywrightFeishuDriver(headless=True)
        driver.launch()
        app_id = driver.create_app(name="TestBot")
        assert app_id == "cli_test123"  # from page.url

    def test_enable_bot(self, mock_pw: dict) -> None:
        page = mock_pw["page"]
        # All interactions go through page.evaluate()
        page.evaluate.return_value = {"found": True, "text": "机器人", "tag": "BUTTON"}
        driver = PlaywrightFeishuDriver(headless=True)
        driver.launch()
        driver.enable_bot()  # should not raise
        page.evaluate.assert_called()

    def test_extract_credentials(self, mock_pw: dict) -> None:
        page = mock_pw["page"]
        # page.content() returns HTML with credentials
        page.content.return_value = (
            '<div>App ID: cli_test1234567890ab</div>'
            '<div>App Secret app_secret: sec_abc12345678901234567890</div>'
        )
        page.url = "https://open.feishu.cn/app/cli_test1234567890ab/settings"
        # _js_click (for "凭证" nav and "显示" button) goes through evaluate
        page.evaluate.return_value = {"found": True, "text": "凭证", "tag": "A"}

        driver = PlaywrightFeishuDriver(headless=True)
        driver.launch()
        creds = driver.extract_credentials()
        assert creds["app_id"] == "cli_test1234567890ab"

    def test_screenshot(self, mock_pw: dict) -> None:
        driver = PlaywrightFeishuDriver(headless=True)
        driver.launch()
        path = driver.screenshot("/tmp/test_shot.png")
        assert path == "/tmp/test_shot.png"
        mock_pw["page"].screenshot.assert_called_once()


# ---------------------------------------------------------------------------
# FeishuBrowserSetupFlow with use_playwright=True (mocked driver)
# ---------------------------------------------------------------------------

class TestFlowWithPlaywright:
    """Test FeishuBrowserSetupFlow in playwright mode with mocked driver."""

    @pytest.fixture(autouse=True)
    def _clear(self) -> None:
        _sessions.clear()

    @pytest.fixture()
    def mock_driver(self):
        """Return a mock PlaywrightFeishuDriver."""
        driver = MagicMock(spec=PlaywrightFeishuDriver)
        driver.open_login.return_value = "https://open.feishu.cn/app"
        driver.wait_for_login.return_value = True
        driver.create_app.return_value = "cli_pw_123"
        driver.extract_credentials.return_value = {
            "app_id": "cli_pw_123",
            "app_secret": "sec_pw_456",
        }
        driver.grant_permissions.return_value = 3
        return driver

    def test_playwright_flow_start(self, mock_driver: MagicMock) -> None:
        with patch(
            "harborbeacon.api.feishu_browser_setup.FeishuBrowserSetupFlow._playwright_open_login",
        ) as mock_open:
            mock_open.return_value = "已打开 https://open.feishu.cn/app"
            flow = FeishuBrowserSetupFlow(use_playwright=True)
            session = flow.start()

        # Playwright mode: auto-detect login → status is 'running', not 'wait_user'
        assert session.status == "running"
        assert session.steps[0].status == SetupStepStatus.SUCCESS
        assert session.steps[1].status == SetupStepStatus.RUNNING  # wait_qr_scan is running

    def test_playwright_flow_resume(self, mock_driver: MagicMock) -> None:
        # Start in stub mode first (to create session)
        flow_stub = FeishuBrowserSetupFlow(use_playwright=False)
        session = flow_stub.start()
        sid = session.session_id

        # Now resume with a mocked playwright driver
        flow = FeishuBrowserSetupFlow(use_playwright=True)
        flow._driver = mock_driver

        resumed = flow.resume_after_scan(sid)

        assert resumed.status == "done"
        assert resumed.app_id == "cli_pw_123"
        assert resumed.app_secret == "sec_pw_456"
        mock_driver.create_app.assert_called_once()
        mock_driver.enable_bot.assert_called_once()
        mock_driver.set_callback_url.assert_called_once()
        mock_driver.grant_permissions.assert_called_once()
        mock_driver.extract_credentials.assert_called_once()
        mock_driver.close.assert_called_once()

    def test_playwright_flow_resume_driver_error(self, mock_driver: MagicMock) -> None:
        flow_stub = FeishuBrowserSetupFlow()
        session = flow_stub.start()
        sid = session.session_id

        mock_driver.create_app.side_effect = RuntimeError("页面加载超时")

        flow = FeishuBrowserSetupFlow(use_playwright=True)
        flow._driver = mock_driver

        resumed = flow.resume_after_scan(sid)

        assert resumed.status == "error"
        assert "页面加载超时" in resumed.error
        # Steps after create_app should be skipped
        step_map = {s.key: s.status for s in resumed.steps}
        assert step_map["create_app"] == SetupStepStatus.FAILED
        assert step_map["enable_bot"] == SetupStepStatus.SKIPPED
