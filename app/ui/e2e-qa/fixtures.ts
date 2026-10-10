import { test as base, expect, type Page } from '@playwright/test';
import fs from 'node:fs';
import path from 'node:path';

export const SHOTS = process.env.QA_SHOTS ?? path.resolve(import.meta.dirname, '../test-results-qa/shots');
fs.mkdirSync(SHOTS, { recursive: true });

export const url = (query: string, hash = '') => `/?fast=1&${query}${hash}`;

type Mock = Record<string, (...a: unknown[]) => unknown>;
export const mock = (page: Page, fn: string, ...args: unknown[]) =>
  page.evaluate(
    ([f, a]) => (window as unknown as { __flowMock: Mock }).__flowMock[f as string]!(...(a as unknown[])),
    [fn, args] as const,
  );

/** Each test collects console errors / page errors; tests can assert none occurred. */
export const test = base.extend<{ errors: string[]; shot: (name: string) => Promise<void> }>({
  errors: async ({ page }, provide) => {
    const errors: string[] = [];
    page.on('pageerror', (e) => errors.push(`pageerror: ${e.message}`));
    page.on('console', (m) => m.type() === 'error' && errors.push(`console: ${m.text()}`));
    await provide(errors);
    if (errors.length)
      test.info().annotations.push({ type: 'console-errors', description: errors.join('\n') });
  },
  shot: async ({ page }, provide) => {
    await provide(async (name: string) => {
      await page.evaluate(() => document.fonts.ready);
      await page.waitForTimeout(300);
      const file = path.join(SHOTS, `${name}.png`);
      await page.screenshot({ path: file });
      test.info().annotations.push({ type: 'screenshot', description: `${name}.png` });
    });
  },
});
export { expect };
