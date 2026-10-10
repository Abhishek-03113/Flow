import { expect, test } from '@playwright/test';
import { MAIN, snap, TRAY, url, type Theme } from './helpers';

for (const theme of ['light', 'dark'] as Theme[]) {
  test.describe(`design gallery (${theme})`, () => {
    test.use({ colorScheme: theme });

    test('main and compact sizes', async ({ page }) => {
      await page.setViewportSize(MAIN);
      await page.goto(url('', '#/design'));
      await expect(page.getByTestId('gallery')).toBeVisible();
      await snap(page, '03-design-system-main', theme);
      await page.setViewportSize({ width: 1100, height: 1500 });
      await snap(page, '03-design-system-full', theme);
      await page.setViewportSize({ width: 1100, height: 1000 });
      await snap(page, '03b-design-system-package', theme);
      await page.setViewportSize({ width: 1100, height: 1500 });
      await page.setViewportSize(TRAY);
      await snap(page, '03-design-system-tray', theme);
    });

    test('alert is keyboard operable', async ({ page }) => {
      await page.setViewportSize(MAIN);
      await page.goto(url('', '#/design'));
      await page.getByRole('button', { name: 'Show alert' }).click();
      await expect(page.getByRole('alertdialog')).toBeVisible();
      await expect(page.getByRole('alertdialog').getByRole('button', { name: 'Cancel' })).toBeFocused();
      await snap(page, '03-design-system-alert', theme);
      await page.keyboard.press('Escape');
      await expect(page.getByRole('alertdialog')).toHaveCount(0);
    });
  });
}
