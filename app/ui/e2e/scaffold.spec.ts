import { expect, test } from '@playwright/test';
import { MAIN, snap, TRAY, url, type Theme } from './helpers';

for (const theme of ['light', 'dark'] as Theme[]) {
  test.describe(`engine inspector (${theme})`, () => {
    test.use({ colorScheme: theme });

    test('mock transport delivers the full snapshot', async ({ page }) => {
      await page.setViewportSize(MAIN);
      await page.goto(url('scenario=two-devices', '#/debug'));
      await expect(page.getByText('loaded:')).toBeVisible();
      await expect(page.locator('b', { hasText: 'true' }).first()).toBeVisible();
      await expect(page.locator('pre')).toContainText('"activeScreen": "local"');
      await snap(page, '02-scaffold-main', theme);
    });

    test('compact size', async ({ page }) => {
      await page.setViewportSize(TRAY);
      await page.goto(url('scenario=controlling-remote', '#/debug'));
      await expect(page.locator('pre')).toContainText('"activeScreen": "d2"');
      await snap(page, '02-scaffold-tray', theme);
    });
  });
}
