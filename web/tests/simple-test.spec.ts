import { test, expect } from '@playwright/test';

test('历史记录表格滚动测试', async ({ page }) => {
  // 访问页面
  await page.goto('http://localhost:3002/');
  
  // 登录
  await page.fill('input[placeholder*="用户名"]', '18000069657');
  await page.fill('input[type="password"]', 'Password123!');
  await page.click('button:has-text("登录")');
  
  // 等待页面加载
  await page.waitForTimeout(5000);
  
  // 截图看看页面
  await page.screenshot({ path: 'after-login.png', fullPage: true });
  
  // 找表格并滚动
  const table = page.locator('.ant-table-body').first();
  
  if (await table.count() > 0) {
    const before = await table.evaluate(el => el.scrollTop);
    await table.evaluate(el => el.scrollTop = 300);
    await page.waitForTimeout(500);
    const after = await table.evaluate(el => el.scrollTop);
    
    console.log(`滚动前: ${before}, 滚动后: ${after}`);
    
    await page.screenshot({ path: 'after-scroll.png', fullPage: true });
    
    expect(after).toBeGreaterThan(before);
  } else {
    console.log('没找到表格');
  }
});
