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

test('5. losing the PC: Reconnecting, Stop Trying, Not connected, Try Again, back', async ({
  live,
  browser,
}) => {
  const mac = live.daemon('Mac');
  const macPage = await live.open('Mac');
  const pcPage = await live.open('PC');
  const banner = macPage.getByTestId('banner-region');

  // Drop the link while the Mac is driving the PC.
  await macPage.getByRole('button', { name: 'Switch to PC' }).click();
  await expect(macPage.getByTestId('status-title')).toHaveText('Controlling PC');
  await live.disconnect('PC');

  await expect(banner).toContainText('Reconnecting…');
  await expect(banner).toContainText('Lost the connection to PC');
  await expect(macPage.getByRole('status', { name: /Connection: Reconnecting/ })).toBeVisible();
  // Link loss sends the controller home; its keyboard works locally again.
  await expect(macPage.getByTestId('status-title')).toHaveText('Your keyboard and mouse are on Mac');
  expect(mac.linkState()).toBe('reconnecting');
  await mac.clearIo();
  await mac.tap('L');
  await expect
    .poll(async () => keyLog((await mac.virtualIo()).delivered_locally))
    .toEqual(['down L', 'up L']);
  expect((await mac.virtualIo()).suppressed).toBe(false);
  // The PC's own page lost its engine.
  await expect(pcPage.getByTestId('banner-region')).toContainText('Flow isn’t responding');
  // ...and does not keep showing its last, stale state as if it were current.
  await expect(pcPage.getByTestId('status-title')).toHaveText('Can’t tell who’s in control right now');
  await expect(pcPage.getByTestId('home')).toContainText('Not responding');
  await expect(pcPage.getByRole('button', { name: 'Release control' })).toHaveCount(0);
  await composite(browser, '05-reconnecting.png', [
    { page: macPage, label: 'Mac: PC lost, reconnecting' },
    { page: pcPage, label: 'PC: its engine is gone' },
  ]);

  await banner.getByRole('button', { name: 'Stop Trying' }).click();
  await expect(banner).toContainText('Not connected');
  await expect.poll(() => mac.linkState()).toBe('disconnected');

  // Try Again while the PC is still away: the Mac is looking for it again.
  await banner.getByRole('button', { name: 'Try Again' }).click();
  await expect.poll(() => mac.linkState()).toBe('connecting');
  await expect(macPage.getByRole('status', { name: /Connection: Connecting/ })).toBeVisible();

  await live.reconnect('PC');
  await expect(banner).toBeEmpty({ timeout: 20_000 });
  await expect.poll(() => mac.linkState()).toBe('connected');
  await expect(macPage.getByRole('status', { name: 'Connection: Connected' })).toBeVisible();
  await expect(pcPage.getByTestId('status-title')).toHaveText('This computer is ready');

  // And it works again.
  const pc = live.daemon('PC');
  await macPage.getByRole('button', { name: 'Switch to PC' }).click();
  await expect(pcPage.getByTestId('status-title')).toHaveText('Mac is controlling this computer');
  await pc.clearIo();
  await mac.tap('M');
  await expect.poll(async () => keyLog((await pc.virtualIo()).injected)).toEqual(['down M', 'up M']);
});

test('5b. with nobody stopping it, the controller reconnects on its own', async ({ live }) => {
  const mac = live.daemon('Mac');
  const macPage = await live.open('Mac');
  const banner = macPage.getByTestId('banner-region');
  await live.disconnect('PC');
  await expect(banner).toContainText('Reconnecting…');
  await live.reconnect('PC');
  await expect(banner).toBeEmpty({ timeout: 20_000 });
  await expect.poll(() => mac.linkState()).toBe('connected');
  await expect(macPage.getByRole('button', { name: 'Switch to PC' })).toBeEnabled();
});
