// Install Playwright normally, or set NODE_PATH to a directory containing it.
const { chromium } = require('playwright');

(async () => {
  const [url, output] = process.argv.slice(2);
  if (!url || !output) throw new Error('usage: capture-devnet-dashboard.js URL OUTPUT');
  const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH || '/usr/bin/chromium', headless: true });
  const page = await browser.newPage({ viewport: { width: 1600, height: 2200 }, deviceScaleFactor: 1 });
  await page.goto(url, { waitUntil: 'networkidle', timeout: 60000 });
  await page.waitForTimeout(8000);
  const panels = page.locator('[data-testid^="data-testid Panel header"]');
  if (await panels.count() < 9) throw new Error(`only ${await panels.count()} dashboard panels rendered`);
  const body = await page.locator('body').innerText();
  const failures = body.split('\n').filter(line => /Panel plugin not found|Query error|bad_data|No data/.test(line));
  if (failures.length) throw new Error(`dashboard contains an error or empty panel: ${failures.join(' | ')}`);
  await page.screenshot({ path: output, fullPage: true });
  await browser.close();
})().catch(error => { console.error(error); process.exit(1); });
