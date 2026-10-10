import { expect, test } from './fixtures';
import { keyLog, type DaemonClient } from './harness';

test.use({
  world: {
    paired: true,
    devices: [
      { name: 'Mac', os: 'mac', role: 'controller' },
      { name: 'PC', os: 'win', role: 'controlled' },
    ],
  },
});

/** Ctrl+Shift+Space the way a keyboard reports it: modifiers ride on the later keys. */
async function ctrlShiftSpace(d: DaemonClient): Promise<void> {
  await d.keyDown('CONTROL');
  await d.keyDown('SHIFT', ['Ctrl']);
  await d.keyDown('SPACE', ['Ctrl', 'Shift']);
  await d.keyUp('SPACE', ['Ctrl', 'Shift']);
  await d.keyUp('SHIFT', ['Ctrl']);
  await d.keyUp('CONTROL');
}

test('7. settings round-trip: switch key, sharing and edge switching reach the engine and act', async ({
  live,
}) => {
  const mac = live.daemon('Mac');
  const pc = live.daemon('PC');
  const pcId = mac.idOf('PC');
  let page = await live.open('Mac', { hash: '#/shortcuts' });

  // Switch key: pick a preset in the UI; the engine stores it and announces it.
  await page.getByRole('combobox', { name: 'Shortcut preset' }).selectOption('Ctrl+Shift+Space');
  await expect
    .poll(() => (mac.settings()?.switch_key as { keys: string[] } | undefined)?.keys)
    .toEqual(['Ctrl', 'Shift', 'Space']);

  // Edge switching off in the UI.
  await page.getByRole('switch', { name: 'Switch at screen edges' }).click();
  await expect.poll(() => mac.settings()?.edge_switching).toBe(false);

  // A fresh page load reads both back from the engine.
  await page.close();
  page = await live.open('Mac', { hash: '#/shortcuts' });
  await expect(page.getByRole('combobox', { name: 'Shortcut preset' })).toHaveValue('Ctrl+Shift+Space');
  await expect(page.getByRole('switch', { name: 'Switch at screen edges' })).toHaveAttribute(
    'aria-checked',
    'false',
  );

  // The old key no longer switches; it is an ordinary key now.
  await mac.clearIo();
  await mac.chord(['F13']);
  await expect
    .poll(async () => keyLog((await mac.virtualIo()).delivered_locally))
    .toEqual(['down F13', 'up F13']);
  expect(mac.controlState()?.active_screen).toBe('local');

  // With edges off, pushing the pointer off the right edge (where the PC is) does not switch.
  await mac.move(1500, 0);
  await mac.move(40, 0);
  await expect.poll(async () => (await mac.virtualIo()).delivered_locally.length).toBeGreaterThan(2);
  expect(mac.controlState()?.active_screen).toBe('local');

  // The new key switches, and the chord reaches no OS.
  await mac.clearIo();
  await pc.clearIo();
  await ctrlShiftSpace(mac);
  await page.getByRole('button', { name: 'Home', exact: true }).click();
  await expect(page.getByTestId('status-title')).toHaveText('Controlling PC');
  expect(mac.controlState()?.active_screen).toBe(pcId);
  const seen = [
    ...keyLog((await mac.virtualIo()).delivered_locally),
    ...keyLog((await pc.virtualIo()).injected),
  ];
  expect(seen.filter((k) => k.endsWith(' SPACE'))).toEqual([]);

  // Sharing: keyboard off keeps typing on the Mac even while controlling the PC.
  await page.getByRole('button', { name: 'General' }).click();
  const shareKeyboard = page.getByRole('switch', { name: 'Share keyboard' });
  await shareKeyboard.click();
  await expect.poll(() => mac.settings()?.share_keyboard).toBe(false);
  // The toggle is driven by the engine's settings_changed; wait for the page to
  // show it before the next click, or that click would turn it off again.
  await expect(shareKeyboard).not.toBeChecked();
  await mac.clearIo();
  await pc.clearIo();
  await mac.tap('A');
  await expect
    .poll(async () => keyLog((await mac.virtualIo()).delivered_locally))
    .toEqual(['down A', 'up A']);
  expect(keyLog((await pc.virtualIo()).injected)).toEqual([]);

  // ...and back on, typing goes to the PC again.
  await shareKeyboard.click();
  await expect.poll(() => mac.settings()?.share_keyboard).toBe(true);
  await expect(shareKeyboard).toBeChecked();
  await mac.tap('B');
  await expect.poll(async () => keyLog((await pc.virtualIo()).injected)).toEqual(['down B', 'up B']);

  // Pointer speed: Faster in the UI scales what the PC receives (high = 1.6).
  await page.getByRole('radio', { name: 'Faster' }).click();
  await expect.poll(() => mac.settings()?.pointer_sensitivity).toBe('high');
  await pc.clearIo();
  await mac.move(10, 0);
  await expect
    .poll(async () =>
      (await pc.virtualIo()).injected
        .filter((e) => 'Mouse' in e)
        .map((e) => (e as { Mouse: { Move?: { dx: number } } }).Mouse.Move?.dx),
    )
    .toEqual([16]);

  // Edge switching back on: crossing the left edge of the PC returns home.
  await page.getByRole('button', { name: 'Keyboard & Shortcuts' }).click();
  await page.getByRole('switch', { name: 'Switch at screen edges' }).click();
  await expect.poll(() => mac.settings()?.edge_switching).toBe(true);
  await mac.move(-3000, 0);
  await mac.move(-40, 0);
  await expect.poll(() => mac.controlState()?.active_screen).toBe('local');
});
