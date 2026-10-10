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

test('6. two controllers: both rows explain the conflict, nothing is forwarded, fixing the role heals it', async ({
  live,
  browser,
}) => {
  const mac = live.daemon('Mac');
  const pc = live.daemon('PC');
  const macPage = await live.open('Mac', { hash: '#/devices' });
  const pcPage = await live.open('PC', { hash: '#/general' });
  const pcId = mac.idOf('PC');
  await expect(macPage.getByTestId(`device-${pcId}`)).toContainText('Connected');

  // The PC's user makes it a controller too.
  await pcPage.getByRole('radio', { name: /Share this computer/ }).click();
  await pcPage.getByRole('button', { name: 'Share This Keyboard and Mouse' }).click();
  await expect.poll(() => pc.controlState()?.role).toBe('controller');

  const macRow = macPage.getByTestId(`device-${pcId}`);
  await expect(macRow.getByTestId('link-error')).toContainText('Both computers are set up the same way');
  await expect(macRow.getByTestId('link-error')).toContainText('PC also shares its keyboard and mouse');
  await expect(macRow).toContainText('Needs attention');
  // The row shows what the PC is now, not the role it had when last linked.
  await expect(macRow).toContainText('Windows · Shares keyboard & mouse');
  // A conflict is not a lost link: no "Reconnecting…", the row explains it.
  await expect.poll(() => mac.linkState()).toBe('error');
  await expect.poll(() => pc.linkState()).toBe('error');
  for (const page of [macPage, pcPage]) {
    await expect(page.getByTestId('banner-region')).toBeEmpty();
    await expect(page.getByRole('status', { name: 'Connection: Needs attention' })).toBeVisible();
  }
  await expect(macRow.getByRole('button', { name: 'Switch' })).toHaveCount(0);
  await expect.poll(() => mac.devices().find((d) => d.id === pcId)?.link_error).toBe('role_conflict');

  await pcPage.getByRole('button', { name: 'Computers' }).click();
  const pcRow = pcPage.getByTestId(`device-${pc.idOf('Mac')}`);
  await expect(pcRow.getByTestId('link-error')).toContainText('Both computers are set up the same way');
  await composite(browser, '06-role-conflict.png', [
    { page: macPage, label: 'Mac (controller): PC is a controller too' },
    { page: pcPage, label: 'PC (now also a controller)' },
  ]);

  // No forwarding: Home offers no switch, the hotkey and switch_to go nowhere, keys stay local.
  await macPage.getByRole('button', { name: 'Home', exact: true }).click();
  await expect(macPage.getByTestId('status-title')).toHaveText('Your keyboard and mouse are on Mac');
  await expect(macPage.getByRole('button', { name: 'Switch to PC' })).toHaveCount(0);
  await mac.clearIo();
  await pc.clearIo();
  await mac.chord(['F13']);
  await mac.tap('Z');
  await expect
    .poll(async () => keyLog((await mac.virtualIo()).delivered_locally))
    .toEqual(['down Z', 'up Z']);
  expect(mac.controlState()?.active_screen).toBe('local');
  expect((await pc.virtualIo()).injected).toEqual([]);
  await expect(mac.call('switch_to', { target: pcId })).rejects.toMatchObject({
    code: 'device_not_switchable',
  });

  // "Change Role…" leads to General; making the PC controlled again heals the link.
  await pcPage.getByRole('button', { name: 'Computers' }).click();
  await pcRow.getByRole('button', { name: 'Change Role…' }).click();
  await pcPage.getByRole('radio', { name: /Control this computer from another one/ }).click();
  await pcPage.getByRole('button', { name: 'Let Another Computer Control This One' }).click();
  await macPage.getByRole('button', { name: 'Computers' }).click();
  await expect(macRow.getByTestId('link-error')).toHaveCount(0, { timeout: 20_000 });
  await expect(macRow).toContainText('Connected');
  await expect(macRow.getByRole('button', { name: 'Switch' })).toBeVisible();
  // Healed on both sides on its own, without Try Again.
  await expect(macRow).toContainText('Windows · Controlled');
  await expect.poll(() => mac.linkState()).toBe('connected');
  await expect.poll(() => pc.linkState()).toBe('connected');
  await expect(pcPage.getByRole('status', { name: 'Connection: Connected' })).toBeVisible();
});
