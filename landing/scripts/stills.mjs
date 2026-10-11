// Captures the product stills used by the landing page from the UI running on mock data.
// Usage: see ../README.md. Writes PNGs to the directory given as the first argument.
const { chromium } = await import(new URL('../../app/ui/node_modules/playwright/index.mjs', import.meta.url).href);
const BASE = process.env.FLOW_UI_URL ?? 'http://localhost:4173';
const CHROMIUM = process.env.CHROMIUM_PATH;
const OUT = process.argv[2];
const B = `${BASE}/?fast=1&`;
const browser = await chromium.launch(CHROMIUM ? { executablePath: CHROMIUM } : {});
async function shot(name, theme, query, hash, size, act, el) {
  const ctx = await browser.newContext({ viewport: size, deviceScaleFactor: 2, colorScheme: theme });
  const page = await ctx.newPage();
  await page.goto(B + query + hash);
  await page.waitForTimeout(500);
  if (act) await act(page);
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(500);
  if (el) await el(page).screenshot({ path: `${OUT}/${name}-${theme}.png` });
  else await page.screenshot({ path: `${OUT}/${name}-${theme}.png` });
  await ctx.close();
}
const MAIN = { width: 1100, height: 720 };
for (const t of ['light', 'dark']) {
  await shot('home', t, 'scenario=three-devices', '#/home', MAIN);
  await shot('diagnostics', t, 'scenario=controlling-remote', '#/diagnostics', MAIN);
  await shot('tray', t, 'scenario=controlling-remote', '#/tray', { width: 360, height: 300 });
}
await browser.close();
