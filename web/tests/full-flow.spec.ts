import { test, expect } from '@playwright/test';
import fs from 'fs';

test('Full dashboard flow with login', async ({ page }) => {
  console.log('=== TESTING FULL DASHBOARD FLOW ===\n');

  // 捕获控制台日志与错误
  page.on('console', msg => console.log('BROWSER:', msg.text()));
  page.on('pageerror', err => console.log('PAGE ERROR:', err.message));
  page.on('requestfailed', req => console.log('REQUEST FAILED:', req.url(), req.failure()?.errorText));
  page.on('request', req => {
    if (req.url().includes('/auth/login')) {
      console.log('LOGIN REQUEST:', req.method(), req.url(), req.postData());
    }
  });
  page.on('response', async res => {
    if (res.url().includes('/auth/login')) {
      console.log('LOGIN RESPONSE:', res.status(), await res.text().catch(() => 'body read failed'));
    }
  });

  // 步骤 1：跳转到仪表盘
  console.log('1. Navigating to http://localhost:3001...');
  await page.goto('http://localhost:3001');
  await page.waitForLoadState('networkidle');

  // 步骤 2：检查是否显示登录页
  console.log('2. Checking login page...');
  const loginTitle = await page.locator('text=/登录|Login/i').first().textContent();
  console.log('   Login title:', loginTitle);

  await page.screenshot({ path: 'tests/screenshots/01-login-page.png', fullPage: true });

  // 步骤 3：检查用户提示
  const hasUserHint = await page.locator('text=/18000069657/').count() > 0;
  const hasAdminHint = await page.locator('text=/admin/').count() > 0;
  console.log('   Has user account hint:', hasUserHint);
  console.log('   Has admin account hint:', hasAdminHint);

  // 步骤 4：用用户账户尝试登录
  console.log('\n3. Attempting login with user account (18000069657)...');

  // 查找并填写用户名输入框
  const usernameInput = page.locator('input[type="text"]').first();
  await usernameInput.fill('18000069657');
  console.log('   ✓ Username filled');

  // 查找并填写密码输入框
  const passwordInput = page.locator('input[type="password"]').first();
  await passwordInput.fill('Password123!');
  console.log('   ✓ Password filled');

  await page.screenshot({ path: 'tests/screenshots/02-credentials-filled.png', fullPage: true });

  // 查找并点击提交按钮
  const submitButton = page.locator('button[type="submit"]').first();
  console.log('   ✓ Submit button found');

  await submitButton.click();
  console.log('   ✓ Submit clicked');

  // 步骤 5：等待跳转/响应
  console.log('\n4. Waiting for dashboard to load...');

  // 等待成功提示或仪表盘出现
  try {
    await Promise.race([
      page.waitForSelector('text=/登录成功|Login successful/i', { timeout: 3000 }),
      page.waitForSelector('text=/退出登录|Logout/i', { timeout: 3000 }),
    ]);
    console.log('   ✓ Login success detected');
  } catch (e) {
    console.log('   ⚠️ No success indicator within 3s');
  }

  // 留出 React 重渲染时间
  await page.waitForTimeout(1000);

  await page.screenshot({ path: 'tests/screenshots/03-after-login.png', fullPage: true });

  // 检查 localStorage
  const storageState = await page.evaluate(() => {
    return {
      token: localStorage.getItem('token'),
      user_id: localStorage.getItem('user_id'),
      username: localStorage.getItem('username'),
      role: localStorage.getItem('role'),
    };
  });
  console.log('   LocalStorage:', JSON.stringify(storageState, null, 2));

  // 步骤 6：等待 setIsLoggedIn(true) 后 React 重渲染
  console.log('\n5. Waiting for dashboard UI to appear...');
  try {
    await page.waitForSelector('text=/退出登录|Logout/i', { timeout: 5000 });
    console.log('   ✓ Dashboard UI loaded');
  } catch (e) {
    console.log('   ⚠️ Dashboard UI did not appear - forcing reload');
    await page.reload();
    await page.waitForTimeout(2000);
  }

  // 步骤 7：检查当前在仪表盘还是仍在登录页
  const currentUrl = page.url();
  const bodyText = await page.locator('body').textContent();

  console.log('   Current URL:', currentUrl);
  console.log('   Body text preview:', bodyText?.substring(0, 300));

  // 检查仪表盘元素
  const hasDashboardContent =
    (await page.locator('text=/账户|Account|仓位|Position|策略|Strategy/i').count()) > 0;

  const stillOnLogin =
    (await page.locator('input[placeholder*="账号"], button:has-text("登 录")').count()) > 0;

  console.log('   Has dashboard content:', hasDashboardContent);
  console.log('   Still on login page:', stillOnLogin);

  // 步骤 7：若仍在登录页，检查错误信息
  if (stillOnLogin) {
    console.log('\n⚠️  Still on login page - checking for errors...');

    const errorMessage = await page.locator('.ant-form-item-explain-error, .ant-message-error').textContent().catch(() => null);
    if (errorMessage) {
      console.log('   Error message:', errorMessage);
    }

    // 检查网络面板
    const failedRequests: string[] = [];
    page.on('requestfailed', (request) => {
      failedRequests.push(`${request.method()} ${request.url()}`);
    });

    page.on('response', (response) => {
      if (response.status() >= 400) {
        failedRequests.push(`HTTP ${response.status()}: ${response.url()}`);
      }
    });

    if (failedRequests.length > 0) {
      console.log('   Failed requests:');
      failedRequests.forEach(req => console.log('     -', req));
    }
  }

  // 步骤 8：保存 HTML 供调试
  const html = await page.content();
  fs.writeFileSync('tests/debug-after-login.html', html);
  console.log('\n✓ Full HTML saved to: tests/debug-after-login.html');

  // 步骤 9：若用户登录失败则测试管理员登录
  if (stillOnLogin) {
    console.log('\n5. Trying admin login...');

    await page.locator('input[type="text"]').first().fill('admin');
    await page.locator('input[type="password"]').first().fill('admin123');
    await page.screenshot({ path: 'tests/screenshots/04-admin-credentials.png', fullPage: true });

    await page.locator('button[type="submit"]').first().click();
    await page.waitForTimeout(2000);

    await page.screenshot({ path: 'tests/screenshots/05-admin-after-login.png', fullPage: true });

    const adminDashboard =
      (await page.locator('text=/账户|Account|仓位|Position|策略|Strategy/i').count()) > 0;
    console.log('   Admin dashboard loaded:', adminDashboard);
  }

  console.log('\n=== TEST COMPLETE ===');
});
