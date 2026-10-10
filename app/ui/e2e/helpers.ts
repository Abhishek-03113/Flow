import type { Page } from '@playwright/test';
import path from 'node:path';

/**
 * Where milestone screenshots go: `docs/progress/<track>/` only when
 * `FLOW_UPDATE_PROGRESS_SHOTS=1` (to refresh the committed ones on purpose),
 * otherwise `<outputDir>/screenshots/<track>/`, which is git-ignored, so a
 * test run never dirties the tree.
 */
export function shotDir(track: string, outputDir = 'test-results'): string {
  return process.env.FLOW_UPDATE_PROGRESS_SHOTS === '1'
    ? path.resolve(import.meta.dirname, '../../../docs/progress', track)
    : path.resolve(import.meta.dirname, '..', outputDir, 'screenshots', track);
}

export const SHOT_DIR = shotDir('track-c');
export const SHOT_DIR_E = shotDir('track-e');
export type Theme = 'light' | 'dark';
export const MAIN = { width: 1100, height: 720 };
export const TRAY = { width: 360, height: 480 };

/** Waits for fonts/animations to settle, then saves `<SHOT_DIR>/<name>-<theme>.png` (see `shotDir`). */
export async function snap(page: Page, name: string, theme: Theme, dir: string = SHOT_DIR): Promise<void> {
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(250);
  await page.screenshot({ path: path.join(dir, `${name}-${theme}.png`) });
}

/** Fast mock: `?fast=1` shrinks simulated delays so journeys finish quickly. */
export const url = (query: string, hash = ''): string => `/?fast=1&${query}${hash}`;
