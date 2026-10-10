import { composite, expect, test } from './fixtures';
import { keyLog } from './harness';

test.use({
  world: {
    paired: true,
    devices: [
      { name: 'Mac', os: 'mac', role: 'controller' },
      { name: 'PC', os: 'win', role: 'controlled' },
    ],
  },
});

test('2. switching from the UI: controller drives the PC, keys land only there, then back', async ({
  live,
  browser,
}) => {
  const mac = live.daemon('Mac');
  const pc = live.daemon('PC');
  const macPage = await live.open('Mac');
  const pcPage = await live.open('PC');

  await expect(macPage.getByTestId('status-title')).toHaveText('Your keyboard and mouse are on Mac');
  await expect(pcPage.getByTestId('status-title')).toHaveText('This computer is ready');

  await macPage.getByRole('button', { name: 'Switch to PC' }).click();
  await expect(macPage.getByTestId('status-title')).toHaveText('Controlling PC');
  await expect(pcPage.getByTestId('status-title')).toHaveText('Mac is controlling this computer');
  await expect.poll(() => mac.controlState()?.active_screen).toBe(mac.idOf('PC'));

  await mac.clearIo();
  await pc.clearIo();
  await mac.tap('A');
  await mac.tap('B');
  await expect
    .poll(async () => keyLog((await pc.virtualIo()).injected))
    .toEqual(['down A', 'up A', 'down B', 'up B']);
  const macIo = await mac.virtualIo();
  expect(macIo.suppressed).toBe(true);
  expect(macIo.delivered_locally).toEqual([]);

  // The controlled computer's tips are about being controlled, not about shortcuts it doesn't have.
  await expect(pcPage.getByText('Take your computer back')).toBeVisible();
  await expect(pcPage.getByText('Switch computers', { exact: true })).toHaveCount(0);
  await expect(macPage.getByText('Switch computers', { exact: true })).toBeVisible();

  // On the blue live card the prominent button stays white, also under the pointer.
  const back = macPage.getByRole('button', { name: 'Return to this computer' });
  await back.hover();
  const bg = await back.evaluate((b) => getComputedStyle(b).backgroundColor);
  expect(bg, 'hovered button on the live card').toMatch(/^rgb\((2[2-5]\d), (2[2-5]\d), (2[2-5]\d)\)$/);

  await composite(browser, '02-controlling.png', [
    { page: macPage, label: 'Mac (controller): controlling PC' },
    { page: pcPage, label: 'PC (controlled): being controlled' },
  ]);

  await macPage.getByRole('button', { name: 'Return to this computer' }).click();
  await expect(macPage.getByTestId('status-title')).toHaveText('Your keyboard and mouse are on Mac');
  await expect(pcPage.getByTestId('status-title')).toHaveText('This computer is ready');

  await mac.clearIo();
  await pc.clearIo();
  await mac.tap('C');
  await expect
    .poll(async () => keyLog((await mac.virtualIo()).delivered_locally))
    .toEqual(['down C', 'up C']);
  expect((await mac.virtualIo()).suppressed).toBe(false);
  expect(keyLog((await pc.virtualIo()).injected)).toEqual([]);
});
