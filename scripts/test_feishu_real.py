#!/usr/bin/env python3
"""Real Playwright test — opens Feishu Open Platform in a visible browser.

Run:  python3 scripts/test_feishu_real.py

Flow:
  1. Launches Chromium (non-headless) → navigates to Feishu Open Platform
  2. Shows QR code — user scans with Feishu App
  3. **Auto-detects login** (no Enter key or button click required)
  4. Attempts automated: create app → enable bot → set callback → permissions → extract creds

Each step saves a debug screenshot to /tmp/feishu_step_*.png regardless of
success or failure, so you can inspect what the browser saw.
"""
from __future__ import annotations

import sys
import os
import logging

# Ensure project root is on path
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s  %(name)s  %(levelname)s  %(message)s",
)

from harborbeacon.api.feishu_browser_setup import (
    FeishuBrowserSetupFlow,
    _sessions,
    _drivers,
)

def main() -> None:
    _sessions.clear()
    _drivers.clear()

    print("=" * 60)
    print("  飞书扫码配置 — 真实 Playwright 测试")
    print("=" * 60)
    print()
    print("  浏览器即将弹出，请在飞书 App 中扫描二维码。")
    print("  扫码后系统会自动检测登录并继续配置，无需手动确认。")
    print()

    flow = FeishuBrowserSetupFlow(
        use_playwright=True,
        headless=False,
        app_name="HarborBeacon-Bot",
        callback_url="https://192.168.1.100:8443/api/v2.0/harborbeacon/webhook/feishu",
    )

    # run_blocking() does everything: open browser → wait for QR scan → auto-continue
    print("正在启动浏览器…")
    session = flow.run_blocking()

    print()
    print(f"  结果: status={session.status}")
    if session.error:
        print(f"  错误: {session.error}")
    for s in session.steps:
        icon = {"success": "✓", "failed": "✗", "skipped": "→", "running": "…"}.get(s.status.value, " ")
        print(f"    {icon} {s.status.value:10s}  {s.label_zh}  {s.detail}")

    if session.app_id:
        print(f"\n  app_id     = {session.app_id}")
        print(f"  app_secret = {session.app_secret[:8]}…" if session.app_secret else "  app_secret = (empty)")

    print()
    print("截图已保存到 /tmp/feishu_step_*.png，可用于调试。")
    print("测试完成。")


if __name__ == "__main__":
    main()
