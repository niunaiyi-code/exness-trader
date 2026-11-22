import { test, expect } from '@playwright/test';
import fs from 'fs';

test('Test Strategy Library Access', async ({ page }) => {
  console.log('=== TESTING STRATEGY LIBRARY ===\n');

  // 捕获控制台错误
  const consoleErrors: string[] = [];
  page.on('console', (msg) => {
    if (msg.type() === 'error') {
      consoleErrors.push(msg.text());
    }
  });

  // 捕获网络错误
  const networkErrors: string[] = [];
  page.on('requestfailed', (request) => {
    networkErrors.push(`Failed: ${request.method()} ${request.url()} - ${request.failure()?.errorText}`);
  });

  page.on('response', (response) => {
    if (response.status() >= 400) {
      networkErrors.push(`HTTP ${response.status()}: ${response.url()}`);
    }
  });

  // 步骤 1：登录
  console.log('1. Logging in...');
  await page.goto('http://localhost:3001');
  await page.waitForLoadState('networkidle');

  await page.locator('input[type="text"]').first().fill('admin');
  await page.locator('input[type="password"]').first().fill('admin123');
  await page.locator('button[type="submit"]').first().click();

  await page.waitForTimeout(2000);
  console.log('   ✓ Logged in as admin');

  await page.screenshot({ path: 'tests/screenshots/strategy-01-dashboard.png', fullPage: true });

  // 步骤 2：查找策略库链接/按钮
  console.log('\n2. Looking for strategy library navigation...');

  const bodyText = await page.locator('body').textContent();
  console.log('   Page text preview:', bodyText?.substring(0, 500));

  // 尝试查找导航链接
  const allLinks = await page.locator('a').all();
  console.log(`   Found ${allLinks.length} links on page`);

  const linkTexts: string[] = [];
  for (const link of allLinks) {
    const text = await link.textContent();
    if (text) linkTexts.push(text.trim());
  }
  console.log('   Link texts:', linkTexts);

  // 尝试查找菜单项
  const menuItems = await page.locator('[role="menuitem"], .ant-menu-item, .ant-dropdown-menu-item').all();
  console.log(`   Found ${menuItems.length} menu items`);

  // Check for "策略" or "Strategy" text
  const hasStrategyText = await page.locator('text=/策略|Strategy|战略/i').count();
  console.log('   Has strategy-related text:', hasStrategyText);

  // 步骤 3：尝试点击策略相关元素
  console.log('\n3. Attempting to navigate to strategy library...');

  // 先尝试直接通过 URL 跳转
  console.log('   Trying direct URL: /strategies');
  await page.goto('http://localhost:3001/strategies');
  await page.waitForTimeout(2000);

  await page.screenshot({ path: 'tests/screenshots/strategy-02-direct-url.png', fullPage: true });

  const strategiesUrl = page.url();
  console.log('   Current URL after navigation:', strategiesUrl);

  const strategiesBodyText = await page.locator('body').textContent();
  console.log('   Body text preview:', strategiesBodyText?.substring(0, 500));

  // 检查是否返回 404 或错误页
  const has404 = await page.locator('text=/404|Not Found|找不到/i').count() > 0;
  const hasError = await page.locator('text=/Error|错误/i').count() > 0;

  console.log('   Has 404 page:', has404);
  console.log('   Has error page:', hasError);

  // 步骤 4：检查可用路由
  console.log('\n4. Trying other potential routes...');

  const routesToTry = [
    '/strategy',
    '/admin/strategies',
    '/config/strategies',
    '/settings',
  ];

  for (const route of routesToTry) {
    console.log(`\n   Trying: ${route}`);
    await page.goto(`http://localhost:3001${route}`);
    await page.waitForTimeout(1000);

    const url = page.url();
    const text = await page.locator('body').textContent();
    console.log(`     URL: ${url}`);
    console.log(`     Has content: ${text && text.length > 100}`);

    if (!text?.includes('404') && !text?.includes('Not Found')) {
      await page.screenshot({
        path: `tests/screenshots/strategy-route-${route.replace(/\//g, '-')}.png`,
        fullPage: true
      });
      console.log(`     ✓ Route works! Screenshot saved.`);
    }
  }

  // 步骤 5：报告所有错误
  console.log('\n5. Error summary:');

  if (consoleErrors.length > 0) {
    console.log('\n   Console Errors:');
    consoleErrors.forEach((err, i) => console.log(`     ${i + 1}. ${err}`));
  } else {
    console.log('   ✓ No console errors');
  }

  if (networkErrors.length > 0) {
    console.log('\n   Network Errors:');
    networkErrors.forEach((err, i) => console.log(`     ${i + 1}. ${err}`));
  } else {
    console.log('   ✓ No network errors');
  }

  // 保存最终 HTML
  const html = await page.content();
  fs.writeFileSync('tests/debug-strategy-library.html', html);
  console.log('\n✓ Full HTML saved to: tests/debug-strategy-library.html');

  console.log('\n=== TEST COMPLETE ===');
});
