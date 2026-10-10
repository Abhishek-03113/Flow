import { expect, test } from '@playwright/test';
import { MAIN, SHOT_DIR_E, snap, url } from './helpers';

// Fresh installs, as the real engine reports them: nothing paired is not an error.
test('no paired computer: neutral status, no error banner, no "null ms"', async ({ page }) => {
  await page.setViewportSize(MAIN);
  await page.goto(url('scenario=no-devices', '#/home'));
  await expect(page.getByText('Add your first computer')).toBeVisible();
  await expect(page.getByTestId('banner')).toHaveCount(0);
  await expect(page.getByText('No computers yet').first()).toBeVisible();
  await expect(page.locator('body')).not.toContainText('null');
  await snap(page, '07-e2-home-no-computers', 'light', SHOT_DIR_E);
});

test('permission guidance lists each missing permission separately (Linux)', async ({ page }) => {
  await page.setViewportSize(MAIN);
  await page.goto(url('scenario=permission-missing&os=linux', '#/home'));
  await page.getByRole('button', { name: 'Allow…' }).first().click();
  const guide = page.getByTestId('permission-guide-linux');
  await expect(guide).toContainText('Read keyboard and mouse');
  await expect(guide).toContainText('Create virtual input device');
  await snap(page, '08-e2-permission-sheet-linux', 'light', SHOT_DIR_E);
});

test('reconnecting: Stop Trying cancels without disabling auto-reconnect', async ({ page }) => {
  await page.setViewportSize(MAIN);
  await page.goto(url('scenario=reconnecting', '#/home'));
  await page.getByRole('button', { name: 'Stop Trying' }).click();
  await expect(page.getByRole('button', { name: 'Try Again' }).first()).toBeVisible();
  await page.goto(url('scenario=reconnecting', '#/general'));
  await expect(page.getByRole('switch', { name: 'Reconnect automatically' })).toBeChecked();
});
