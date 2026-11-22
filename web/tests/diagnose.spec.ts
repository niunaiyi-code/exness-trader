import { test } from '@playwright/test';

test('Diagnose dashboard content', async ({ page }) => {
  await page.goto('http://localhost:3001');
  await page.waitForLoadState('networkidle');

  // 捕获所有控制台消息
  const logs: string[] = [];
  page.on('console', (msg) => {
    logs.push(`[${msg.type()}] ${msg.text()}`);
  });

  // 捕获网络错误
  const networkErrors: string[] = [];
  page.on('requestfailed', (request) => {
    networkErrors.push(`Failed: ${request.url()} - ${request.failure()?.errorText}`);
  });

  page.on('response', (response) => {
    if (response.status() >= 400) {
      networkErrors.push(`HTTP ${response.status()}: ${response.url()}`);
    }
  });

  // 等待异步内容
  await page.waitForTimeout(3000);

  // 获取整页 HTML
  const html = await page.content();

  // 获取可见文本
  const bodyText = await page.locator('body').textContent();

  // 获取 React 根节点内容
  const rootHTML = await page.locator('#root').innerHTML().catch(() => 'No #root found');

  // 检查指定元素
  const h1Count = await page.locator('h1').count();
  const h2Count = await page.locator('h2').count();
  const divCount = await page.locator('div').count();

  // 获取所有 h1 与 h2 文本
  const headers: string[] = [];
  for (let i = 0; i < h1Count; i++) {
    const text = await page.locator('h1').nth(i).textContent();
    if (text) headers.push(`H1: ${text.trim()}`);
  }
  for (let i = 0; i < h2Count; i++) {
    const text = await page.locator('h2').nth(i).textContent();
    if (text) headers.push(`H2: ${text.trim()}`);
  }

  // 输出诊断信息
  console.log('=== PAGE DIAGNOSTICS ===\n');

  console.log('Page Title:', await page.title());
  console.log('URL:', page.url());
  console.log('\nElement Counts:');
  console.log('  H1:', h1Count);
  console.log('  H2:', h2Count);
  console.log('  DIV:', divCount);

  console.log('\nHeaders Found:');
  headers.forEach(h => console.log('  ', h));

  console.log('\nBody Text Preview (first 500 chars):');
  console.log(bodyText?.substring(0, 500));

  console.log('\n#root innerHTML Preview (first 1000 chars):');
  console.log(rootHTML.substring(0, 1000));

  if (logs.length > 0) {
    console.log('\nConsole Logs:');
    logs.forEach(log => console.log('  ', log));
  }

  if (networkErrors.length > 0) {
    console.log('\nNetwork Errors:');
    networkErrors.forEach(err => console.log('  ', err));
  }

  // 保存完整 HTML 供检查
  const fs = require('fs');
  fs.writeFileSync('tests/debug-page.html', html);
  console.log('\nFull HTML saved to: tests/debug-page.html');

  await page.screenshot({ path: 'tests/screenshots/full-diagnostic.png', fullPage: true });
  console.log('Screenshot saved to: tests/screenshots/full-diagnostic.png');
});
