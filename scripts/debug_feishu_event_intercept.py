#!/usr/bin/env python3
"""Feishu 事件订阅请求抓包工具

用法:
    python scripts/debug_feishu_event_intercept.py [app_id]

功能:
    1. 打开飞书开放平台，等待扫码登录
    2. 导航到指定 app 的「事件与回调」页面
    3. 拦截所有 developers/v1/event* 相关的 XHR 请求
    4. 自动点击「添加事件」按钮，并捕获控制台真正发出的请求
    5. 把完整 URL + Header + Payload + Response 打印到 stdout

目的: 找出 event/update 的正确 payload 格式
"""
from __future__ import annotations

import json
import sys
import os
import time
import logging

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")
log = logging.getLogger("intercept")

APP_ID = sys.argv[1] if len(sys.argv) > 1 else None
FEISHU_OPEN = "https://open.feishu.cn/app"


def run() -> None:
    try:
        from playwright.sync_api import sync_playwright
    except ImportError:
        print("请先: pip install playwright && playwright install chromium")
        sys.exit(1)

    captured: list[dict] = []

    with sync_playwright() as p:
        browser = p.chromium.launch(headless=False)
        context = browser.new_context()
        page = context.new_page()

        # ── 拦截所有请求 + 响应 ──────────────────────────────────────
        interesting_patterns = [
            "developers/v1/event",
            "developers/v2/event",
            "event/update",
            "event/switch",
            "event/search",
            "event/list",
        ]

        def on_request(req):
            url = req.url
            if any(pat in url for pat in interesting_patterns):
                entry = {
                    "direction": "REQUEST",
                    "method": req.method,
                    "url": url,
                    "headers": dict(req.headers),
                    "post_data": req.post_data,
                }
                captured.append(entry)
                log.info("→ %s %s", req.method, url)
                if req.post_data:
                    log.info("  payload: %s", req.post_data[:500])

        def on_response(resp):
            url = resp.url
            if any(pat in url for pat in interesting_patterns):
                try:
                    body = resp.json()
                except Exception:
                    try:
                        body = resp.text()[:500]
                    except Exception:
                        body = "(unreadable)"
                entry = {
                    "direction": "RESPONSE",
                    "status": resp.status,
                    "url": url,
                    "body": body,
                }
                captured.append(entry)
                log.info("← %s %s  →  %s", resp.status, url, str(body)[:200])

        page.on("request", on_request)
        page.on("response", on_response)

        # ── Step 1: 登录 ──────────────────────────────────────────────
        log.info("打开飞书开放平台，请扫码登录...")
        page.goto(FEISHU_OPEN)
        log.info("等待登录 (最长 180s)...")

        try:
            page.wait_for_url(lambda u: "open.feishu.cn/app" in u and "accounts.feishu" not in u,
                              timeout=180_000)
        except Exception:
            log.warning("URL 未跳转，尝试继续")

        # 等待直到页面不再是登录页
        deadline = time.time() + 180
        while time.time() < deadline:
            if "accounts.feishu" not in page.url and "open.feishu.cn" in page.url:
                break
            time.sleep(1)

        log.info("登录检测完成，当前页面: %s", page.url)
        page.wait_for_timeout(2000)

        # ── Step 2: 导航到 app 事件页 ──────────────────────────────────
        app_id = APP_ID
        if not app_id:
            # 自动从当前 URL 获取
            import re
            m = re.search(r"/app/(cli_[a-zA-Z0-9]+)", page.url)
            if m:
                app_id = m.group(1)
            else:
                # 获取 app 列表
                log.info("未提供 app_id, 尝试获取 app 列表...")
                list_result = page.evaluate("""async () => {
                    try {
                        const r = await fetch('https://open.feishu.cn/developers/v1/app/list', {
                            method: 'POST', credentials: 'include',
                            headers: {'content-type': 'application/json'},
                            body: JSON.stringify({})
                        });
                        return await r.json();
                    } catch(e) { return {error: e.message}; }
                }""")
                log.info("app list: %s", json.dumps(list_result, ensure_ascii=False)[:500])
                apps = []
                if isinstance(list_result, dict):
                    d = list_result.get("data") or {}
                    if isinstance(d, dict):
                        d = d.get("data") or d
                    apps = d.get("apps") or d.get("appList") or []
                if apps:
                    app_id = apps[0].get("appId") or apps[0].get("app_id") or apps[0].get("cli_code")
                    log.info("使用 app_id: %s", app_id)

        if not app_id:
            log.error("无法确定 app_id，请以参数传入: python debug_feishu_event_intercept.py cli_xxxxx")
            browser.close()
            return

        log.info("使用 app_id: %s", app_id)

        # ── Step 3: 先调用我们的内部 API，记录原始响应 ────────────────
        log.info("=== 调用 developers/v1/event/{app_id} 查看当前事件 ===")
        event_info = page.evaluate("""async ([app_id]) => {
            try {
                const r = await fetch(`https://open.feishu.cn/developers/v1/event/${app_id}`, {
                    method: 'POST', credentials: 'include',
                    headers: {'content-type': 'application/json'},
                    body: JSON.stringify({})
                });
                return {status: r.status, ok: r.ok, data: await r.json()};
            } catch(e) { return {error: e.message}; }
        }""", [app_id])
        log.info("当前事件信息: %s", json.dumps(event_info, ensure_ascii=False, indent=2))

        # ── Step 4: 导航到事件与回调页面 ─────────────────────────────
        event_page_url = f"https://open.feishu.cn/app/{app_id}/event-subscribe"
        log.info("导航到事件订阅页面: %s", event_page_url)
        page.goto(event_page_url)
        page.wait_for_timeout(3000)

        # 检查 URL 是否有效
        log.info("事件页面 URL: %s", page.url)
        page.screenshot(path="C:/tmp/feishu_event_page.png")
        log.info("截图: C:/tmp/feishu_event_page.png")

        # ── Step 5: 抓取页面加载时发出的所有 API 调用 ───────────────
        page.wait_for_timeout(2000)

        # dump 到目前为止 captured
        log.info("=== 页面加载期间捕获的事件相关请求 (%d 条) ===", len(captured))
        for c in captured:
            print(json.dumps(c, ensure_ascii=False, indent=2))

        # ── Step 6: 尝试点击「添加事件」并捕获 XHR ────────────────────
        captured_before = len(captured)
        log.info("寻找「添加事件」按钮...")

        add_event_selectors = [
            "text=添加事件",
            "text=Add Event",
            "[data-id='add-event']",
            "button:has-text('添加')",
        ]
        clicked = False
        for sel in add_event_selectors:
            try:
                btn = page.locator(sel).first
                if btn.is_visible(timeout=2000):
                    btn.click()
                    clicked = True
                    log.info("点击了按钮: %s", sel)
                    break
            except Exception:
                continue

        if not clicked:
            log.warning("未找到「添加事件」按钮，尝试 JS 点击...")
            page.evaluate("""() => {
                const btns = Array.from(document.querySelectorAll('button, [role="button"]'));
                const btn = btns.find(b => b.textContent && (b.textContent.includes('添加事件') || b.textContent.includes('Add Event')));
                if (btn) btn.click();
            }""")

        page.wait_for_timeout(2000)
        page.screenshot(path="C:/tmp/feishu_add_event_dialog.png")
        log.info("点击添加事件后截图: C:/tmp/feishu_add_event_dialog.png")

        # ── Step 7: 搜索并选择事件 ────────────────────────────────────
        log.info("查找搜索框，输入 im.message...")
        search_selectors = [
            "input[placeholder*='搜索']",
            "input[placeholder*='Search']",
            "input[placeholder*='search']",
            ".event-search input",
        ]
        for sel in search_selectors:
            try:
                inp = page.locator(sel).first
                if inp.is_visible(timeout=2000):
                    inp.fill("im.message")
                    log.info("在搜索框输入: im.message (selector=%s)", sel)
                    page.wait_for_timeout(1000)
                    break
            except Exception:
                continue

        page.screenshot(path="C:/tmp/feishu_event_search.png")
        log.info("搜索后截图: C:/tmp/feishu_event_search.png")

        # ── Step 8: 打印页面上可见的按钮和文字 ──────────────────────
        page_info = page.evaluate("""() => {
            const btns = Array.from(document.querySelectorAll('button, [role="button"], [role="option"]'))
                .filter(e => e.offsetParent !== null)
                .map(e => e.innerText?.trim().slice(0, 80));
            const items = Array.from(document.querySelectorAll('[class*="event"], [class*="item"], li'))
                .filter(e => e.offsetParent !== null && e.textContent?.includes('im.'))
                .map(e => ({text: e.innerText?.slice(0, 100), cls: e.className?.slice(0, 60)}));
            return {buttons: btns.filter(Boolean).slice(0, 30), eventItems: items.slice(0, 10)};
        }""")
        log.info("页面按钮: %s", json.dumps(page_info.get("buttons"), ensure_ascii=False))
        log.info("事件列表项: %s", json.dumps(page_info.get("eventItems"), ensure_ascii=False))

        # ── Step 9: 手动等待用户点击，看控制台发什么请求 ──────────────
        log.info("")
        log.info("=" * 60)
        log.info("现在请在浏览器中手动点击「im.message.receive_v1」事件，")
        log.info("然后点击「添加」或「确定」按钮。")
        log.info("工具将自动捕获请求。等待 30 秒...")
        log.info("=" * 60)
        page.wait_for_timeout(30_000)

        # ── Step 10: 打印新增的捕获 ──────────────────────────────────
        new_captured = captured[captured_before:]
        log.info("=== 点击操作后新增捕获 (%d 条) ===", len(new_captured))
        for c in new_captured:
            print(json.dumps(c, ensure_ascii=False, indent=2))

        # 保存完整抓包到文件
        out_path = "C:/tmp/feishu_event_intercept.json"
        with open(out_path, "w", encoding="utf-8") as f:
            json.dump(captured, f, ensure_ascii=False, indent=2)
        log.info("完整抓包保存到: %s", out_path)

        browser.close()

    log.info("完成。")


if __name__ == "__main__":
    run()
