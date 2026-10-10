import { expect, test } from '@playwright/test';
import { MAIN, snap, url, type Theme } from './helpers';

for (const theme of ['light', 'dark'] as Theme[]) {
  test.describe(`onboarding (${theme})`, () => {
    test.use({ colorScheme: theme });

    test('first launch journey: welcome → role → permissions → pair → done', async ({ page }) => {
      await page.setViewportSize(MAIN);
      await page.goto(url('scenario=fresh-install'));
      const ob = page.getByTestId('onboarding');
      await expect(ob).toHaveAttribute('data-step', 'welcome');
      await snap(page, '04-onboarding-1-welcome', theme);

      await page.getByRole('button', { name: 'Get Started' }).click();
      await expect(ob).toHaveAttribute('data-step', 'role');
      await page.getByRole('radio', { name: /Control this computer from another one/ }).click();
      await snap(page, '04-onboarding-2-role', theme);
      await page.getByRole('button', { name: 'Continue' }).click();

      await expect(ob).toHaveAttribute('data-step', 'permissions');
      await expect(page.getByTestId('permission-guide-macos')).toBeVisible();
      await expect(page.getByRole('button', { name: 'Continue' })).toBeDisabled();
      await snap(page, '04-onboarding-3-permissions', theme);
      await page.getByRole('button', { name: 'Skip for Now' }).click();

      await expect(ob).toHaveAttribute('data-step', 'pair');
      await expect(page.getByTestId('pairing-found')).toBeVisible();
      await snap(page, '04-onboarding-4-pair', theme);
      await page.getByRole('button', { name: 'Connect to Studio Mac mini' }).click();
      await expect(page.getByTestId('pairing-requesting')).toBeVisible();
      await snap(page, '04-onboarding-4b-pair-code', theme);
      await expect(page.getByTestId('pairing-paired')).toBeVisible();
      await page.getByRole('button', { name: 'Continue' }).click();

      await expect(ob).toHaveAttribute('data-step', 'done');
      await snap(page, '04-onboarding-5-done', theme);
      await page.getByRole('button', { name: 'Start Using Flow' }).click();
      await expect(page.getByTestId('home')).toBeVisible();
      await expect(page.getByTestId('status-title')).toContainText('Waiting for');
    });

    test('linux permission guidance', async ({ page }) => {
      await page.setViewportSize(MAIN);
      await page.goto(url('scenario=fresh-install&os=windows'));
      await page.getByRole('button', { name: 'Get Started' }).click();
      await page.getByRole('button', { name: 'Continue' }).click();
      await expect(page.getByTestId('permission-guide-windows')).toBeVisible();
      await expect(page.getByRole('button', { name: 'Continue' })).toBeEnabled();
    });
  });
}
