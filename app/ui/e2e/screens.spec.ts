import { expect, test } from '@playwright/test';
import { MAIN, snap, TRAY, url, type Theme } from './helpers';

const N = '04-screens';

for (const theme of ['light', 'dark'] as Theme[]) {
  test.describe(`screens (${theme})`, () => {
    test.use({ colorScheme: theme });
    test.beforeEach(async ({ page }) => page.setViewportSize(MAIN));

    test('home: controlling nobody, with arrangement', async ({ page }) => {
      await page.goto(url('scenario=three-devices', '#/home'));
      await expect(page.getByTestId('status-title')).toContainText('MacBook Pro');
      await expect(page.getByTestId('arrangement')).toBeVisible();
      await snap(page, `${N}-home`, theme);
    });

    test('home: controlling a remote computer', async ({ page }) => {
      await page.goto(url('scenario=controlling-remote', '#/home'));
      await expect(page.getByTestId('status-title')).toHaveText('Controlling Office PC');
      await snap(page, `${N}-home-remote`, theme);
    });

    test('home: controlled', async ({ page }) => {
      await page.goto(url('scenario=controlled', '#/home'));
      await expect(page.getByTestId('status-title')).toContainText('Desk PC is controlling this computer');
      await snap(page, `${N}-home-controlled`, theme);
    });

    test('devices with a role conflict', async ({ page }) => {
      await page.goto(url('scenario=role-conflict', '#/devices'));
      await expect(page.getByTestId('link-error')).toContainText('Both computers are set up the same way');
      await snap(page, `${N}-devices-conflict`, theme);
    });

    test('devices list', async ({ page }) => {
      await page.goto(url('scenario=three-devices', '#/devices'));
      await expect(page.getByTestId('device-d2')).toBeVisible();
      await snap(page, `${N}-devices`, theme);
    });

    test('shortcuts', async ({ page }) => {
      await page.goto(url('scenario=two-devices', '#/shortcuts'));
      await expect(page.getByTestId('shortcuts')).toBeVisible();
      await snap(page, `${N}-shortcuts`, theme);
    });

    test('general', async ({ page }) => {
      await page.goto(url('scenario=two-devices', '#/general'));
      await expect(page.getByTestId('general')).toBeVisible();
      await snap(page, `${N}-general`, theme);
    });

    test('diagnostics', async ({ page }) => {
      await page.goto(url('scenario=controlling-remote', '#/diagnostics'));
      await expect(page.getByTestId('diagnostics')).toBeVisible();
      await snap(page, `${N}-diagnostics`, theme);
    });

    for (const [name, text] of [
      ['reconnecting', 'Reconnecting…'],
      ['disconnected', 'Not connected'],
      ['error', 'Sharing is paused'],
      ['permission-missing', 'Flow needs permission'],
    ] as const) {
      test(`banner: ${name}`, async ({ page }) => {
        await page.goto(url(`scenario=${name}`, '#/home'));
        await expect(page.getByTestId('banner')).toContainText(text);
        await snap(page, `${N}-banner-${name}`, theme);
      });
    }

    test('pairing sheet and incoming request', async ({ page }) => {
      await page.goto(url('scenario=two-devices', '#/devices'));
      await page.getByRole('button', { name: 'Add Computer…' }).first().click();
      await expect(page.getByTestId('pairing-found')).toBeVisible();
      await snap(page, `${N}-pairing-found`, theme);
      await page.getByRole('button', { name: 'Cancel' }).click();
      await page.evaluate(() =>
        (
          window as never as { __flowMock: { incomingPairingRequest(): void } }
        ).__flowMock.incomingPairingRequest(),
      );
      await expect(page.getByTestId('incoming-request')).toBeVisible();
      await snap(page, `${N}-incoming-request`, theme);
    });

    test('switch HUD', async ({ page }) => {
      await page.goto(url('scenario=three-devices', '#/home'));
      await page.getByRole('button', { name: 'Switch to Office PC' }).click();
      await expect(page.getByTestId('hud')).toContainText('Now controlling Office PC');
      await snap(page, `${N}-hud`, theme);
    });

    test('tray popover', async ({ page }) => {
      await page.setViewportSize(TRAY);
      await page.goto(url('scenario=controlling-remote', '#/tray'));
      await expect(page.getByTestId('tray-summary')).toContainText('Controlling Office PC');
      await snap(page, `${N}-tray`, theme);
    });

    test('tray popover reconnecting', async ({ page }) => {
      await page.setViewportSize(TRAY);
      await page.goto(url('scenario=reconnecting', '#/tray'));
      await expect(page.getByTestId('banner')).toBeVisible();
      await snap(page, `${N}-tray-reconnecting`, theme);
    });
  });
}
