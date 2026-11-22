import os
import sys
import json
from playwright.sync_api import sync_playwright

try:
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")
except Exception:
    pass

def test_history_deals():
    screenshot_dir = r"C:\Users\hibom\.gemini\antigravity\brain\a9ef0291-ea47-43f3-a6b1-734b8c89a249\scratch"
    os.makedirs(screenshot_dir, exist_ok=True)
    screenshot_path = os.path.join(screenshot_dir, "playwright_history_deals.png")

    console_errors = []
    deals_requests = []
    deals_responses = []

    print("=== 开始 Playwright 历史交易功能自动化测试 ===")

    with sync_playwright() as p:
        browser = p.chromium.launch(headless=True)
        context = browser.new_context(viewport={"width": 1440, "height": 900})
        page = context.new_page()

        def on_console(msg):
            text = msg.text
            # 过滤常见的 React / Antd 开发环境提示
            if msg.type == "error":
                if "Static function can not consume context" not in text and "destroyOnClose" not in text:
                    console_errors.append(text)
                    print(f"[BROWSER ERROR]: {text}")
            elif "[TraderCockpit]" in text:
                print(f"[APP LOG]: {text}")

        def on_pageerror(err):
            console_errors.append(str(err))
            print(f"[PAGE ERROR]: {err}")

        page.on("console", on_console)
        page.on("pageerror", on_pageerror)

        def on_request(req):
            if "history/deals" in req.url:
                deals_requests.append(req.url)
                print(f"[REQUEST] -> {req.url}")

        def on_response(resp):
            if "history/deals" in resp.url:
                status = resp.status
                try:
                    body = resp.json()
                except Exception as e:
                    body = f"Failed to parse json: {e}"
                deals_responses.append({"status": status, "url": resp.url, "body": body})
                print(f"[RESPONSE] <- HTTP {status} {resp.url}")
                if isinstance(body, dict):
                    if "error" in body:
                        print(f"           [ERROR BODY]: {body['error']}")
                    items = body.get("items", [])
                    print(f"           已返回 {len(items)} 条平仓成交记录, has_more={body.get('has_more')}, next_cursor={body.get('next_cursor')}")

        page.on("request", on_request)
        page.on("response", on_response)

        # 1. 访问首页并登录
        print("1. 正在访问前端登录页 http://localhost:3000 ...")
        page.goto("http://localhost:3000", wait_until="networkidle")

        print("2. 自动填写交易员账号 18000069657 并登录 ...")
        page.fill("input[placeholder='用户名']", "18000069657")
        page.fill("input[placeholder='密码']", "Password123!")
        page.click("button[type='submit']")

        # 2. 等待进入驾驶舱
        page.wait_for_url("**/cockpit", timeout=10000)
        print(f"3. 成功登录并跳转至驾驶舱: {page.url}")

        # 3. 等待网络请求稳定
        page.wait_for_timeout(2000)

        # 4. 寻找并切换到底部 "历史" 标签页
        print("4. 查找底部 '历史' 标签页...")
        history_tab = page.locator(".ant-tabs-tab").filter(has_text="历史")
        history_tab.first.wait_for(state="visible", timeout=10000)
        print("   找到 '历史' 标签，正在点击切换...")
        history_tab.first.click()

        page.wait_for_timeout(1500)

        # 5. 校验历史交易面板关键元素渲染（限定在当前激活的 Tab 容器内）
        active_pane = page.locator(".ant-tabs-tabpane-active")
        print("5. 校验历史交易面板关键元素渲染:")
        header_tip = active_pane.locator("text=Exness 真实历史平仓成交")
        assert header_tip.is_visible(), "未找到头部提示信息 'Exness 真实历史平仓成交'"
        print("   [OK] 头部说明文案显示正常")

        order_count_stat = active_pane.locator("text=平仓总单数")
        assert order_count_stat.is_visible(), "未找到 '平仓总单数' 指标"
        print("   [OK] '平仓总单数' 统计区域显示正常")

        profit_stat = active_pane.locator("text=已实现累计总盈利")
        assert profit_stat.is_visible(), "未找到 '已实现累计总盈利' 指标"
        print("   [OK] '已实现累计总盈利' 统计区域显示正常")

        # 校验表格列头
        for col in ["订单号", "品种", "方向", "平仓手数", "平仓价格", "已实现盈亏", "平仓时间"]:
            loc = active_pane.locator(f"th:has-text('{col}'), .ant-table-cell:has-text('{col}')")
            assert loc.first.is_visible(), f"表格列头 '{col}' 未渲染"
            print(f"   [OK] 表格列头 '{col}' 渲染正常")

        # 6. 检查表格数据行
        table_rows = active_pane.locator(".ant-table-tbody tr.ant-table-row")
        row_count = table_rows.count()
        print(f"6. 历史交易表格渲染行数: {row_count}")

        if row_count > 0:
            for idx in range(min(3, row_count)):
                row_text = table_rows.nth(idx).inner_text().replace("\n", " | ")
                print(f"   - 成交记录 [{idx + 1}]: {row_text}")
        else:
            empty_loc = active_pane.locator(".ant-empty, text=暂无数据")
            if empty_loc.is_visible():
                print("   当前账户尚无历史平仓记录（空状态展示正常）")

        # 7. 截图保存验证
        page.screenshot(path=screenshot_path)
        print(f"7. 历史交易功能截图已保存至: {screenshot_path}")

        # 8. 测试“加载更多”交互
        more_btn = active_pane.locator("button:has-text('加载更多历史平仓记录')")
        if more_btn.is_visible():
            print("8. 找到 '加载更多历史平仓记录' 按钮，模拟点击触发分页加载...")
            more_btn.click()
            page.wait_for_timeout(2000)
            new_row_count = table_rows.count()
            print(f"   分页加载完成，行数从 {row_count} 增加至 {new_row_count}")
            assert new_row_count >= row_count, "分页加载后行数未增加"

        browser.close()

    print("\n=== 测试汇总报告 ===")
    print(f"发起的 /history/deals 请求次数: {len(deals_requests)}")
    print(f"收到的 /history/deals 响应次数: {len(deals_responses)}")
    success = True
    if not deals_requests:
        print("[FAIL] 前端未发出 /history/deals 请求！")
        success = False
    elif any(r["status"] != 200 for r in deals_responses):
        print(f"[FAIL] 存在非 200 响应: {[r['status'] for r in deals_responses]}")
        success = False
    else:
        print("[PASS] /history/deals API 请求与响应全部成功 (HTTP 200)！")

    if console_errors:
        print(f"[WARN] 捕获到 {len(console_errors)} 个控制台错误:")
        for err in console_errors:
            print(f"  - {err}")
    else:
        print("[PASS] 前端运行无任何严重 Console Error！")

    return success

if __name__ == "__main__":
    ok = test_history_deals()
    sys.exit(0 if ok else 1)
