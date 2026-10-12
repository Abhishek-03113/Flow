import { expect, test } from '@playwright/test';
import { MAIN, snap, url, type Theme } from './helpers';

for (const theme of ['light', 'dark'] as Theme[]) {
  test.describe(`onboarding (${theme})`, () => {
    test.use({ colorScheme: theme });

    test('first launch on macOS: role → setup (allow, restart) → pair', async ({ page }) => {
      await page.setViewportSize(MAIN);
      await page.goto(url('scenario=fresh-install'));
      const ob = page.getByTestId('onboarding');
      await expect(ob).toHaveAttribute('data-step', 'role');
      await page.getByRole('radio', { name: /Control this computer from another one/ }).click();
      await snap(page, '04-onboarding-1-role', theme);
      await page.getByRole('button', { name: 'Get Started' }).click();

      await expect(ob).toHaveAttribute('data-step', 'setup');
      const guide = page.getByTestId('permission-guide-macos');
      await expect(guide).toBeVisible();
      await snap(page, '04-onboarding-2-setup', theme);
      await guide.getByRole('button', { name: 'Allow Access' }).click();
      // macOS applies the grant after a restart: one click, and onboarding resumes at pairing.
      await expect(guide.getByTestId('permission-restart')).toBeVisible({ timeout: 10_000 });
      await snap(page, '04-onboarding-2b-restart', theme);
      await guide.getByRole('button', { name: 'Restart Flow' }).click();
      await expect(page.getByRole('button', { name: 'Continue' })).toBeEnabled();
      await page.getByRole('button', { name: 'Continue' }).click();

      await expect(ob).toHaveAttribute('data-step', 'pair');
      await expect(page.getByTestId('pairing-found')).toBeVisible();
      await snap(page, '04-onboarding-3-pair', theme);
      await page.getByRole('button', { name: 'Connect to Studio Mac mini' }).click();
      await expect(page.getByTestId('pairing-requesting')).toBeVisible();
      await expect(page.getByTestId('pairing-paired')).toBeVisible();
      await expect(page.getByTestId('onboarding-done')).toBeVisible();
      await snap(page, '04-onboarding-3b-paired', theme);
      await page.getByRole('button', { name: 'Start Using Flow' }).click();
      await expect(page.getByTestId('home')).toBeVisible();
      await expect(page.getByTestId('status-title')).toContainText('Waiting for');
    });

    test('first launch on Linux: one button sets up input access and the firewall', async ({ page }) => {
      await page.setViewportSize(MAIN);
      await page.goto(url('scenario=fresh-install&os=linux'));
      await page.getByRole('button', { name: 'Get Started' }).click();
      const guide = page.getByTestId('permission-guide-linux');
      await expect(guide).toBeVisible();
      await expect(guide).toContainText('Read keyboard and mouse');
      await expect(guide).toContainText('Create virtual input device');
      await expect(guide).toContainText('Incoming connections');
      await expect(page.getByRole('button', { name: 'Continue' })).toHaveCount(0);
      await snap(page, '04-onboarding-2-setup-linux', theme);
      await guide.getByRole('button', { name: 'Set Up Automatically' }).click();
      await expect(guide.locator('[data-testid="permission-item"][data-granted="false"]')).toHaveCount(0, {
        timeout: 10_000,
      });
      await page.getByRole('button', { name: 'Continue' }).click();
      await expect(page.getByTestId('onboarding')).toHaveAttribute('data-step', 'pair');
    });

    test('a cancelled password prompt says so and can be retried', async ({ page }) => {
      await page.setViewportSize(MAIN);
      await page.goto(url('scenario=fresh-install&os=linux'));
      await page.evaluate(() =>
        (
          window as never as { __flowMock: { failNextPermissionSetup(code: string): void } }
        ).__flowMock.failNextPermissionSetup('permission_setup_cancelled'),
      );
      await page.getByRole('button', { name: 'Get Started' }).click();
      const guide = page.getByTestId('permission-guide-linux');
      await guide.getByRole('button', { name: 'Set Up Automatically' }).click();
      await expect(page.getByTestId('toasts')).toContainText('Setup was cancelled');
      await guide.getByRole('button', { name: 'Set Up Automatically' }).click();
      await expect(guide.locator('[data-testid="permission-item"][data-granted="false"]')).toHaveCount(0, {
        timeout: 10_000,
      });
    });

    test('Windows needs no setup: straight from role to pairing', async ({ page }) => {
      await page.setViewportSize(MAIN);
      await page.goto(url('scenario=fresh-install&os=windows'));
      await page.getByRole('button', { name: 'Get Started' }).click();
      await expect(page.getByTestId('onboarding')).toHaveAttribute('data-step', 'pair');
    });
  });
}
