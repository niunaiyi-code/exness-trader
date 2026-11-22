import { test, expect } from '@playwright/test';

test.describe('历史记录表格滚动测试', () => {
  test.beforeEach(async ({ page }) => {
    // 访问页面
    await page.goto('http://localhost:3002/');
    
    // 等待页面加载完成
    await page.waitForLoadState('networkidle');
    
    // 检查是否需要登录
    const hasLoginForm = await page.locator('input[type="password"]').count() > 0;
    
    if (hasLoginForm) {
      console.log('检测到登录页面，正在登录...');
      
      // 填写用户名（开发环境默认账号）
      await page.fill('input[placeholder*="用户名"]', '18000069657');
      
      // 填写密码
      await page.fill('input[type="password"]', 'Password123!');
      
      // 点击登录按钮
      await page.click('button:has-text("登录")');
      
      // 等待跳转到主页面
      await page.waitForLoadState('networkidle');
      await page.waitForTimeout(3000);
      
      console.log('登录成功，当前 URL:', page.url());
    }
    
    // 等待页面内容加载
    await page.waitForTimeout(2000);
    
    // 截图看看当前页面状态
    await page.screenshot({ path: 'test-results/page-logged-in.png', fullPage: true });
  });

  test('测试 1: 表格可以正常滚动', async ({ page }) => {
    // 打印页面上所有卡片标题
    const cardTitles = await page.locator('.ant-card-head-title').allTextContents();
    console.log('页面上的卡片标题:', cardTitles);
    
    // 查找历史订单卡片
    const historyCard = page.locator('.ant-card').filter({ hasText: /历史订单|历史记录/ });
    const historyCardCount = await historyCard.count();
    console.log('找到历史订单卡片数量:', historyCardCount);
    
    if (historyCardCount === 0) {
      console.log('页面上没有历史订单卡片，可能数据未加载');
      // 截图
      await page.screenshot({ path: 'test-results/no-history-card.png', fullPage: true });
      test.skip();
      return;
    }
    
    await expect(historyCard.first()).toBeVisible({ timeout: 5000 });
    
    // 查找表格滚动容器
    const scrollContainer = historyCard.first().locator('.ant-table-body');
    const scrollContainerCount = await scrollContainer.count();
    console.log('找到滚动容器数量:', scrollContainerCount);
    
    if (scrollContainerCount === 0) {
      console.log('卡片内没有表格滚动容器');
      await page.screenshot({ path: 'test-results/no-scroll-container.png', fullPage: true });
      test.skip();
      return;
    }
    
    await expect(scrollContainer).toBeVisible({ timeout: 5000 });
    
    // 获取初始滚动位置
    const initialScrollTop = await scrollContainer.evaluate(el => el.scrollTop);
    console.log('初始滚动位置:', initialScrollTop);
    
    // 滚动到中间位置
    await scrollContainer.evaluate(el => {
      el.scrollTop = 200;
    });
    
    // 等待滚动完成
    await page.waitForTimeout(300);
    
    // 验证滚动位置已改变
    const newScrollTop = await scrollContainer.evaluate(el => el.scrollTop);
    console.log('滚动后位置:', newScrollTop);
    
    // 截图
    await page.screenshot({ path: 'test-results/scroll-test.png', fullPage: true });
    
    expect(newScrollTop).toBeGreaterThan(initialScrollTop);
  });

  test('测试 2: 滚动到底部触发加载更多', async ({ page }) => {
    const scrollContainer = page.locator('.ant-card').filter({ hasText: '历史订单' }).locator('.ant-table-body');
    
    // 等待初始数据加载
    await page.waitForTimeout(1000);
    
    // 获取当前行数
    const initialRowCount = await page.locator('.ant-card').filter({ hasText: '历史订单' }).locator('.ant-table-tbody tr').count();
    console.log('初始行数:', initialRowCount);
    
    // 滚动到底部
    await scrollContainer.evaluate(el => {
      el.scrollTop = el.scrollHeight;
    });
    
    // 等待加载
    await page.waitForTimeout(2000);
    
    // 验证触发了加载（可能显示"加载中..."或行数增加）
    const loadingVisible = await page.locator('text=加载中...').isVisible().catch(() => false);
    const newRowCount = await page.locator('.ant-card').filter({ hasText: '历史订单' }).locator('.ant-table-tbody tr').count();
    
    console.log('加载指示器可见:', loadingVisible);
    console.log('新行数:', newRowCount);
    
    // 至少满足一个条件：显示加载中 或 行数增加
    expect(loadingVisible || newRowCount > initialRowCount).toBeTruthy();
  });

  test('测试 3: 多次滚动无抖动', async ({ page }) => {
    const scrollContainer = page.locator('.ant-card').filter({ hasText: '历史订单' }).locator('.ant-table-body');
    
    // 记录滚动过程中的位置
    const scrollPositions: number[] = [];
    
    // 执行多次滚动
    for (let i = 0; i <= 5; i++) {
      const targetScroll = i * 60;
      await scrollContainer.evaluate((el, target) => {
        el.scrollTop = target;
      }, targetScroll);
      
      await page.waitForTimeout(100);
      
      const currentScroll = await scrollContainer.evaluate(el => el.scrollTop);
      scrollPositions.push(currentScroll);
      console.log(`滚动 ${i}: 目标=${targetScroll}, 实际=${currentScroll}`);
    }
    
    // 验证滚动位置递增（允许小误差）
    for (let i = 1; i < scrollPositions.length; i++) {
      expect(scrollPositions[i]).toBeGreaterThanOrEqual(scrollPositions[i - 1] - 5);
    }
    
    // 截图
    await page.screenshot({ path: 'history-scroll-smooth.png', fullPage: false });
  });

  test('测试 4: 虚拟滚动验证 - DOM 节点数量', async ({ page }) => {
    const scrollContainer = page.locator('.ant-card').filter({ hasText: '历史订单' }).locator('.ant-table-body');
    
    // 等待数据加载
    await page.waitForTimeout(1000);
    
    // 获取渲染的行数
    const renderedRows = await page.locator('.ant-card').filter({ hasText: '历史订单' }).locator('.ant-table-tbody tr').count();
    console.log('渲染的 DOM 行数:', renderedRows);
    
    // 滚动到底部
    await scrollContainer.evaluate(el => {
      el.scrollTop = el.scrollHeight;
    });
    
    await page.waitForTimeout(500);
    
    // 再次获取渲染的行数
    const renderedRowsAfterScroll = await page.locator('.ant-card').filter({ hasText: '历史订单' }).locator('.ant-table-tbody tr').count();
    console.log('滚动后渲染的 DOM 行数:', renderedRowsAfterScroll);
    
    // 虚拟滚动应该保持相似的 DOM 节点数量（±20 行以内）
    expect(Math.abs(renderedRowsAfterScroll - renderedRows)).toBeLessThan(20);
  });
});
