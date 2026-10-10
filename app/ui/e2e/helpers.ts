import type { Page } from '@playwright/test';
import path from 'node:path';

export const SHOT_DIR = path.resolve(import.meta.dirname, '../../../docs/progress/track-c');
export const SHOT_DIR_E = path.resolve(import.meta.dirname, '../../../docs/progress/track-e');
export type Theme = 'light' | 'dark';
export const MAIN = { width: 1100, height: 720 };
export const TRAY = { width: 360, height: 480 };

/** Waits for fonts/animations to settle, then saves `docs/progress/track-c/<name>-<theme>.png`. */
export async function snap(page: Page, name: string, theme: Theme, dir: string = SHOT_DIR): Promise<void> {
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(250);
  await page.screenshot({ path: path.join(dir, `${name}-${theme}.png`) });
}

/** Fast mock: `?fast=1` shrinks simulated delays so journeys finish quickly. */
export const url = (query: string, hash = ''): string => `/?fast=1&${query}${hash}`;
