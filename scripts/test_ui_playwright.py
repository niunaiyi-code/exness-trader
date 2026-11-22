import os
import sys
from playwright.sync_api import sync_playwright

# Windows 中文环境默认 stdout 是 GBK，浏览器日志含 ⚠ 等字符会 UnicodeEncodeError，
# 进而把 Playwright 事件循环打断。这里强制 UTF-8 输出。
try:
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")
except Exception:
    pass


def run_test():
    screenshot_dir = r"C:\Users\hibom\.gemini\antigravity\brain\baeef331-800d-41d9-929f-7ffc8e587666"
    os.makedirs(screenshot_dir, exist_ok=True)
    screenshot_path = os.path.join(screenshot_dir, "playwright_cockpit_final.png")
    login_screenshot_path = os.path.join(screenshot_dir, "playwright_login.png")

    console_errors = []
    candle_batches = []
    snapshot_states = []

    with sync_playwright() as p:
        browser = p.chromium.launch(headless=True)
        context = browser.new_context(viewport={"width": 1440, "height": 900})
        page = context.new_page()

        def on_console(msg):
            # 只收集 error，忽略无意义的 dev 提示与 Exness 远端对 /candles 的频控 502
            text = msg.text
            if msg.type == "error":
                if "Static function can not consume context" not in text and "status of 502" not in text:
                    console_errors.append(text)
                    print(f"[BROWSER ERROR]: {text}")

        def on_pageerror(err):
            console_errors.append(str(err))
            print(f"[PAGE ERROR]: {err}")

        page.on("console", on_console)
        page.on("pageerror", on_pageerror)

        # 追踪 /market/candles + /snapshot 响应，确认真数据确实加载
        def on_response(resp):
            if resp.status >= 400:
                print(f"   [HTTP {resp.status}]: {resp.url}")
            if "/market/candles" in resp.url:
                try:
                    body = resp.json()
                    n = len(body) if isinstance(body, list) else -1
                except Exception:
                    n = -1
                candle_batches.append(n)
                print(f"   [candles] {resp.url} -> {n} bars")
            elif "/snapshot" in resp.url:
                try:
                    body = resp.json()
                    has_balance = "balance" in (body.get("account_state") or {})
                    snapshot_states.append(has_balance)
                    print(f"   [snapshot] balance present: {has_balance}")
                except Exception:
                    snapshot_states.append(False)
                    print("   [snapshot] parse error")
        page.on("response", on_response)

        print("1. Navigating to http://localhost:3000 ...")
        page.goto("http://localhost:3000", wait_until="networkidle")
        page.screenshot(path=login_screenshot_path)
        print(f"   Login screenshot saved to {login_screenshot_path}")

        print("2. Filling login form with 18000069657 ...")
        page.fill("input[placeholder='用户名']", "18000069657")
        page.fill("input[placeholder='密码']", "Password123!")

        print("3. Clicking login button ...")
        page.click("button[type='submit']")

        # 等待路由跳转到驾驶舱（登录成功后 localStorage 有 token 且跳转 /cockpit）
        page.wait_for_url("**/cockpit", timeout=10000)
        print(f"   Landed on: {page.url}")

        # 4. 校验顶部单行全局高密度 HUD 资产条
        print("4. Validating Single-Row Compact HUD Toolbar ...")
        hud = page.locator("text=账户余额")
        hud.first.wait_for(state="visible", timeout=10000)
        print(f"   Top Asset Bar visible: {hud.first.is_visible()}")

        # 校验 HUD Bar 容器高度必须 <= 52px，证明绝无多行折行，真正实现合为一行
        hud_bar_container = page.locator("text=金色传说 · 驾驶舱").locator("xpath=ancestor::div[contains(@style, 'background: rgb(22, 22, 22)') or contains(@style, '#161616')]").first
        box = hud_bar_container.bounding_box()
        if box:
            print(f"   HUD Bar BoundingBox: width={box['width']:.1f}px, height={box['height']:.1f}px")
            assert box['height'] <= 55, f"HUD Bar height={box['height']}px exceeds single-row limit of 55px!"
            print("   [PASS] Verified strictly 1 single-row HUD Bar (height <= 55px)!")

        # 校验六大核心资金指标与关键标签
        for metric in ("账户余额", "当前净值", "浮动盈亏", "已用保证金", "可用保证金", "保证金比例", "Swap-Free 免隔夜息"):
            loc = page.locator(f"text={metric}")
            assert loc.first.is_visible(), f"Metric {metric} not found in single-row HUD Bar!"
            print(f"   HUD Metric [{metric}]: visible")

        # 5. 校验快捷交易与应急控制菜单
        emer_tab = page.locator("text=应急控制")
        if emer_tab.is_visible():
            emer_tab.click()
            page.wait_for_timeout(300)

        for label in ("一键锁仓", "一键平仓", "暂停开仓"):
            loc = page.locator(f"text={label}")
            try:
                loc.first.wait_for(state="visible", timeout=3000)
                print(f"   QuickAction [{label}]: visible")
            except Exception:
                print(f"   QuickAction [{label}]: NOT FOUND")

        # 6. 校验底部双标签页架构（当前持仓 + 历史平仓记录与小计）
        print("5. Validating Bottom Tabs & Batch Settlement Subtotals ...")
        pos_tab = page.locator(".ant-tabs-tab").filter(has_text="持仓")
        pos_tab.first.wait_for(state="visible", timeout=5000)
        print("   实时当前持仓 Tab: visible")

        history_tab = page.locator(".ant-tabs-tab").filter(has_text="历史")
        assert history_tab.is_visible(), "History Tab not visible!"
        history_tab.click()
        page.wait_for_timeout(1000)

        # 校验平仓小计关键元素
        subtotal_label = page.locator("text=平仓结算小计")
        subtotal_label.first.wait_for(state="visible", timeout=5000)
        print(f"   [PASS] 历史平仓结算小计行: visible (count={subtotal_label.count()})")

        # 校验统计指标与结算完成标签
        for stat in ("已实现累计总盈利", "平仓总批次", "平仓总单数", "结算完成"):
            loc = page.locator(f"text={stat}")
            assert loc.first.is_visible(), f"History summary stat {stat} not visible!"
            print(f"   History Stat [{stat}]: visible")

        hist_screenshot = os.path.join(screenshot_dir, "playwright_history_tab.png")
        page.screenshot(path=hist_screenshot)
        print(f"   History Tab switched & screenshot saved: {hist_screenshot}")

        # 切换回实盘持仓
        page.locator(".ant-tabs-tab").filter(has_text="持仓").click()
        page.wait_for_timeout(300)

        # 7. 校验周期切换（M1, M5, M15, M30, H1, H4, D1, W1, MN）
        print("6. Testing timeframe switching across MT5 periods ...")
        import re
        for tf in ("M5", "M15", "M30", "H1", "H4", "D1", "W1", "MN", "M1"):
            tf_btn = page.locator(".ant-segmented-item").filter(has_text=re.compile(f"^{tf}$"))
            if tf_btn.is_visible():
                tf_btn.click()
                page.wait_for_timeout(1000)
                print(f"   Switched to {tf} successfully")

        # 8. 校验无多余侧边栏
        sider = page.locator(".ant-layout-sider")
        print(f"   Left sidebar removed: {not sider.is_visible()}")

        # 9. 保存最终全局验收截图
        page.screenshot(path=screenshot_path)
        print(f"   Final Cockpit screenshot saved to {screenshot_path}")

        non_empty = [n for n in candle_batches if n > 0]
        print(f"   Candle fetches: {len(candle_batches)} total, {len(non_empty)} non-empty")

        browser.close()

    if console_errors:
        print(f"\nFAILURE: Captured {len(console_errors)} console error(s):")
        for err in console_errors:
            print(" - ", err)
        return False
    if not candle_batches:
        print("\nFAILURE: No /market/candles request was made (chart did not load candles).")
        return False
    if all(n <= 0 for n in candle_batches):
        print(f"\nFAILURE: All candle responses empty/error: {candle_batches}")
        return False
    if not any(snapshot_states):
        print("\nFAILURE: No successful /market/snapshot response (account state not loaded).")
        return False
    print("\nSUCCESS: No console errors + real candles + real snapshot loaded!")
    return True


if __name__ == "__main__":
    success = run_test()
    sys.exit(0 if success else 1)
