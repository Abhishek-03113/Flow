import { expect, test, type Page } from '@playwright/test';
import { MAIN, snap, url, type Theme } from './helpers';

/**
 * Pairing on networks real computers meet (`docs/redesign/flows-v3.md` E3–E5):
 * the list never dead-ends, a same-role computer is explained before any
 * request, and with nothing discoverable the person gets a checklist and
 * Connect by Address with this computer's own address shown.
 */

const blockBroadcast = (page: Page) =>
  page.evaluate(() =>
    (window as never as { __flowMock: { blockBroadcast(): void } }).__flowMock.blockBroadcast(),
  );

for (const theme of ['light', 'dark'] as Theme[]) {
  test.describe(`pairing on real networks (${theme})`, () => {
    test.use({ colorScheme: theme });
    test.beforeEach(async ({ page }) => page.setViewportSize(MAIN));

    test('a same-role computer is listed but explained, not offered', async ({ page }) => {
      await page.goto(url('scenario=no-devices', '#/devices'));
      await page.getByRole('button', { name: 'Add Computer…' }).first().click();
      const found = page.getByTestId('pairing-found');
      await expect(found).toContainText('Office iMac');
      await expect(found).toContainText('Same role');
      await expect(found.getByRole('button', { name: 'Connect to Office iMac' })).toHaveCount(0);
      await expect(found.getByRole('button', { name: 'Connect to Studio Mac mini' })).toBeVisible();
      await snap(page, '20-pairing-same-role', theme);
    });

    test('nothing found for 10 s: checklist and Connect by Address, then paired', async ({ page }) => {
      await page.clock.install();
      await page.goto(url('scenario=no-devices', '#/devices'));
      await blockBroadcast(page);
      await page.getByRole('button', { name: 'Add Computer…' }).first().click();
      await expect(page.getByTestId('pairing-searching')).toBeVisible();
      await expect(page.getByTestId('pairing-troubleshoot')).toHaveCount(0);

      await page.clock.fastForward(10_500);
      await expect(page.getByTestId('pairing-troubleshoot')).toContainText('Not showing up?');
      const form = page.getByTestId('pair-by-address');
      await expect(form.getByTestId('local-addresses')).toContainText('192.168.1.20');
      await snap(page, '21-pairing-not-showing-up', theme);

      await form.getByRole('textbox').fill('192.168.1.31');
      await form.getByRole('button', { name: 'Connect' }).click();
      await page.clock.fastForward(5_000);
      await expect(page.getByTestId('pairing-paired')).toContainText('Connected to Study PC');
    });
  });
}
