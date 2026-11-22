import { test, expect } from '@playwright/test';

test.describe('Admin Web Dashboard', () => {
  test.beforeEach(async ({ page }) => {
    // 跳转到管理仪表盘
    await page.goto('http://localhost:3001');
  });

  test('should load the dashboard without errors', async ({ page }) => {
    // 等待页面完全加载
    await page.waitForLoadState('networkidle');

    // 检查是否存在控制台错误
    const errors: string[] = [];
    page.on('console', (msg) => {
      if (msg.type() === 'error') {
        errors.push(msg.text());
      }
    });

    // 稍等片刻以便捕获控制台错误
    await page.waitForTimeout(2000);

    // 截图供检查
    await page.screenshot({ path: 'tests/screenshots/dashboard.png', fullPage: true });

    // 检查标题或主内容
    const title = await page.title();
    console.log('Page title:', title);

    // 检查是否有可见的主内容
    const body = await page.locator('body').textContent();
    console.log('Body has content:', body && body.length > 0);

    // 如有错误则报告
    if (errors.length > 0) {
      console.log('Console errors found:');
      errors.forEach((err, i) => console.log(`  ${i + 1}. ${err}`));
    }

    expect(errors.length).toBe(0);
  });

  test('should display account information', async ({ page }) => {
    await page.waitForLoadState('networkidle');

    // 查找常见的仪表盘元素
    const hasAccountInfo = await page.locator('text=/账户|Account|余额|Balance/i').count() > 0;
    const hasStrategyInfo = await page.locator('text=/策略|Strategy|网格|Grid/i').count() > 0;

    console.log('Has account info:', hasAccountInfo);
    console.log('Has strategy info:', hasStrategyInfo);

    // 截图
    await page.screenshot({ path: 'tests/screenshots/account-info.png', fullPage: true });

    expect(hasAccountInfo || hasStrategyInfo).toBeTruthy();
  });

  test('should check for API connection errors', async ({ page }) => {
    const failedRequests: string[] = [];

    page.on('requestfailed', (request) => {
      failedRequests.push(`${request.method()} ${request.url()} - ${request.failure()?.errorText}`);
    });

    page.on('response', (response) => {
      if (response.status() >= 400) {
        failedRequests.push(`${response.status()} ${response.url()}`);
      }
    });

    await page.waitForLoadState('networkidle');
    await page.waitForTimeout(3000);

    if (failedRequests.length > 0) {
      console.log('Failed requests:');
      failedRequests.forEach((req, i) => console.log(`  ${i + 1}. ${req}`));
    }

    await page.screenshot({ path: 'tests/screenshots/with-network.png', fullPage: true });

    // 仅报告，不让测试失败（API 可能未启动）
    console.log('Total failed requests:', failedRequests.length);
  });

  test('should render React components without crashing', async ({ page }) => {
    // 检查 React 是否已渲染
    const hasReactRoot = await page.locator('#root').count() > 0;
    console.log('Has React root element:', hasReactRoot);

    expect(hasReactRoot).toBeTruthy();

    // 检查根节点是否有子节点
    const rootChildren = await page.locator('#root > *').count();
    console.log('React root children count:', rootChildren);

    expect(rootChildren).toBeGreaterThan(0);

    await page.screenshot({ path: 'tests/screenshots/react-render.png', fullPage: true });
  });
});
