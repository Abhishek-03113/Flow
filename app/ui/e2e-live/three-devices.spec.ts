import { expect, test } from './fixtures';
import { keyLog } from './harness';

test.use({
  world: {
    paired: true,
    devices: [
      { name: 'Mac', os: 'mac', role: 'controller' },
      { name: 'PC', os: 'win', role: 'controlled' },
      { name: 'Linux', os: 'lin', role: 'controlled' },
    ],
  },
});

test('10. three computers: cycle from the UI and with the hotkey; each gets only its own keys', async ({
  live,
}) => {
  const mac = live.daemon('Mac');
  const pc = live.daemon('PC');
  const lin = live.daemon('Linux');
  const macPage = await live.open('Mac');
  const pcPage = await live.open('PC');
  const linPage = await live.open('Linux');
  const title = (p: typeof macPage) => p.getByTestId('status-title');
  const clearAll = () => Promise.all([mac.clearIo(), pc.clearIo(), lin.clearIo()]);
  const typed = async () => ({
    mac: keyLog((await mac.virtualIo()).delivered_locally),
    pc: keyLog((await pc.virtualIo()).injected),
    lin: keyLog((await lin.virtualIo()).injected),
  });

  await expect(macPage.getByRole('button', { name: 'Switch to PC' })).toBeEnabled();
  await expect(macPage.getByRole('button', { name: 'Switch to Linux' })).toBeEnabled();

  // From the UI: Mac -> PC -> Linux -> home.
  await macPage.getByRole('button', { name: 'Switch to PC' }).click();
  await expect(title(macPage)).toHaveText('Controlling PC');
  await expect(title(pcPage)).toHaveText('Mac is controlling this computer');
  await expect(title(linPage)).toHaveText('This computer is ready');
  await clearAll();
  await mac.tap('P');
  await expect.poll(typed).toEqual({ mac: [], pc: ['down P', 'up P'], lin: [] });

  await macPage.getByRole('button', { name: 'Switch to Linux' }).click();
  await expect(title(macPage)).toHaveText('Controlling Linux');
  await expect(title(linPage)).toHaveText('Mac is controlling this computer');
  await expect(title(pcPage)).toHaveText('This computer is ready');
  await clearAll();
  await mac.tap('L');
  await expect.poll(typed).toEqual({ mac: [], pc: [], lin: ['down L', 'up L'] });

  await macPage.getByRole('button', { name: 'Return to this computer' }).click();
  await expect(title(macPage)).toHaveText('Your keyboard and mouse are on Mac');
  await expect(title(linPage)).toHaveText('This computer is ready');

  // With the hotkey: local -> one -> the other -> local, every page following live.
  const seen: string[] = [];
  for (let i = 0; i < 3; i++) {
    await mac.chord(['F13']);
    await expect.poll(() => mac.controlState()?.active_screen).not.toBe(i === 0 ? 'local' : seen[i - 1]);
    const active = mac.controlState()!.active_screen;
    seen.push(active);
    const name = active === 'local' ? null : active === mac.idOf('PC') ? 'PC' : 'Linux';
    if (name) {
      await expect(title(macPage)).toHaveText(`Controlling ${name}`);
      await expect(title(name === 'PC' ? pcPage : linPage)).toHaveText('Mac is controlling this computer');
      await expect(title(name === 'PC' ? linPage : pcPage)).toHaveText('This computer is ready');
    } else {
      await expect(title(macPage)).toHaveText('Your keyboard and mouse are on Mac');
    }
  }
  expect(new Set(seen.slice(0, 2))).toEqual(new Set([mac.idOf('PC'), mac.idOf('Linux')]));
  expect(seen[2]).toBe('local');
});
