/**
 * Playwright fixtures for the live suite. Each test gets its own
 * `flow-harness up` world (ephemeral ports, its own temp data) described by
 * the `world` option, a `DaemonClient` per device for test hooks and checks,
 * and `open(name)` for the real UI connected to that device's engine.
 */
import { test as base, expect, type Browser, type BrowserContext, type Page } from '@playwright/test';
import path from 'node:path';
import { DaemonClient, Harness, type WorldSpec } from './harness';

export { expect };

export const SHOT_DIR = path.resolve(import.meta.dirname, '../../../docs/progress/track-g');

export interface OpenOptions {
  /** Start on the first-run flow instead of Home (default false). */
  onboarding?: boolean;
  hash?: string;
  theme?: 'light' | 'dark';
}

export interface Live {
  harness: Harness;
  /** A control-API client per device (by name), connected for the whole test. */
  daemon(name: string): DaemonClient;
  /** Opens the real UI connected to that device's engine, in its own browser context. */
  open(name: string, options?: OpenOptions): Promise<Page>;
  /** Kills a device's engine; its page's socket drops. */
  disconnect(name: string): Promise<void>;
  /** Restarts it on the same data and port; the daemon client reconnects. */
  reconnect(name: string): Promise<void>;
}

async function newPage(browser: Browser, theme: 'light' | 'dark', onboarded: boolean) {
  const context = await browser.newContext({
    viewport: { width: 1100, height: 720 },
    colorScheme: theme,
  });
  if (onboarded)
    await context.addInitScript(() => {
      try {
        localStorage.setItem('flow.onboarding_complete', '1');
      } catch {
        /* ignore */
      }
    });
  return { context, page: await context.newPage() };
}

export const test = base.extend<{ world: WorldSpec; live: Live }>({
  world: [{ devices: [], paired: true }, { option: true }],
  live: async ({ world, browser }, provide, testInfo) => {
    const harness = await Harness.start(world);
    const clients = new Map<string, DaemonClient>();
    const contexts: BrowserContext[] = [];
    try {
      for (const entry of harness.manifest) clients.set(entry.name, await DaemonClient.connect(entry));
      const live: Live = {
        harness,
        daemon(name) {
          const c = clients.get(name);
          if (!c) throw new Error(`no daemon ${name}`);
          return c;
        },
        async open(name, options = {}) {
          const entry = harness.entry(name);
          const { context, page } = await newPage(browser, options.theme ?? 'light', !options.onboarding);
          contexts.push(context);
          const q = new URLSearchParams({ daemon: entry.ws_url, token: entry.token });
          await page.goto(`/?${q}${options.hash ?? '#/home'}`);
          return page;
        },
        async disconnect(name) {
          clients.get(name)?.close();
          clients.delete(name);
          await harness.disconnect(name);
        },
        async reconnect(name) {
          await harness.reconnect(name);
          clients.set(name, await DaemonClient.connect(harness.entry(name)));
        },
      };
      await provide(live);
    } finally {
      for (const c of contexts) await c.close().catch(() => undefined);
      clients.forEach((c) => c.close());
      if (testInfo.status !== testInfo.expectedStatus)
        await testInfo.attach('flow-harness.log', { body: harness.log.join(''), contentType: 'text/plain' });
      await harness.stop();
    }
  },
});

/** Two pages side by side in one PNG (≤1400 px wide), for docs/progress/track-g. */
export async function composite(
  browser: Browser,
  file: string,
  panes: { page: Page; label: string }[],
  theme: 'light' | 'dark' = 'light',
): Promise<void> {
  const shots: string[] = [];
  for (const { page } of panes) {
    // The switch HUD is transient; wait it out so the shot shows the page.
    await expect(page.locator('.fl-hud')).toHaveCount(0);
    await page.evaluate(() => document.fonts.ready);
    shots.push((await page.screenshot()).toString('base64'));
  }
  const ctx = await browser.newContext({ viewport: { width: 1400, height: 520 }, colorScheme: theme });
  const p = await ctx.newPage();
  const bg = theme === 'dark' ? '#1c1c1e' : '#e9e9ee';
  const fg = theme === 'dark' ? '#f2f2f7' : '#1c1c1e';
  const cells = panes
    .map(
      ({ label }, i) =>
        `<figure><figcaption>${label}</figcaption><img src="data:image/png;base64,${shots[i]}"></figure>`,
    )
    .join('');
  await p.setContent(
    `<style>body{margin:0;padding:16px;background:${bg};color:${fg};font:600 14px system-ui;display:flex;gap:16px}
     figure{margin:0;flex:1}figcaption{margin:0 0 8px}img{width:100%;border-radius:8px;display:block;box-shadow:0 1px 4px #0003}</style>${cells}`,
  );
  await p
    .locator('img')
    .last()
    .evaluate((img: HTMLImageElement) => img.decode());
  await p.screenshot({ path: path.join(SHOT_DIR, file), fullPage: true });
  await ctx.close();
}
