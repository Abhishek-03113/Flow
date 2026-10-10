import { expect, test } from './fixtures';
import { keyLog, type InputEvent } from './harness';

// A Windows keyboard driving a Mac: the classic case for remapping.
test.use({
  world: {
    paired: true,
    devices: [
      { name: 'PC', os: 'win', role: 'controller' },
      { name: 'Mac', os: 'mac', role: 'controlled' },
    ],
  },
});

/** `down KEY [mods]` for each key event, to see modifiers too. */
const keysWithMods = (events: InputEvent[]) =>
  events.flatMap((e) => {
    if (!('Keyboard' in e)) return [];
    const k = e.Keyboard as Record<string, { key: string; modifiers: string[] }>;
    const [kind, f] = Object.entries(k)[0]!;
    return [`${kind === 'KeyDown' ? 'down' : 'up'} ${f.key}${f.modifiers.length ? ` [${f.modifiers}]` : ''}`];
  });

test('9. keymap from the engine, remap rules and a recorded shortcut act on a Mac target', async ({
  live,
}) => {
  const pc = live.daemon('PC');
  const mac = live.daemon('Mac');
  const page = await live.open('PC', { hash: '#/shortcuts' });

  // The key map is the engine's (get_keymap), per OS.
  await page.getByRole('combobox', { name: 'Show keys for' }).selectOption('macos');
  const engineMac = await pc.call<{ os: string; keys: { name: string; code: number }[] }>('get_keymap', {
    os: 'macos',
  });
  await expect(page.getByText(`${engineMac.keys.length} keys`)).toBeVisible();
  const keymap = page.getByTestId('keymap');
  await expect(keymap.locator('.fl-keymap__key', { hasText: /^COMMAND0x/ })).toContainText('0x37');
  await expect(keymap.locator('.fl-keymap__key', { hasText: /^A0x00$/ })).toHaveCount(1);

  // A shortcut recorded in the UI with a named key (Ctrl + Up) works on the engine.
  await page.getByRole('button', { name: 'Switch shortcut' }).click();
  await page.keyboard.press('Control+ArrowUp');
  await expect
    .poll(() => (pc.settings()?.switch_key as { keys: string[] } | undefined)?.keys)
    .toEqual(['Ctrl', 'Up']);
  await pc.keyDown('CONTROL');
  await pc.keyDown('UP_ARROW', ['Ctrl']);
  await pc.keyUp('UP_ARROW', ['Ctrl']);
  await pc.keyUp('CONTROL');
  await expect.poll(() => pc.controlState()?.active_screen).toBe(pc.idOf('Mac'));

  // Rule 1: Caps Lock becomes End, everywhere.
  await page.getByRole('button', { name: 'Add Rule' }).click();
  let rule = page.getByTestId('remap-rule').last();
  await rule.getByRole('button', { name: 'Key to remap' }).click();
  await page.keyboard.press('CapsLock');
  await rule.getByRole('button', { name: 'Becomes' }).click();
  await page.keyboard.press('End');
  // Rule 2: Ctrl becomes Command, only when controlling a Mac.
  await page.getByRole('button', { name: 'Add Rule' }).click();
  rule = page.getByTestId('remap-rule').last();
  await rule.getByRole('button', { name: 'Key to remap' }).click();
  await page.keyboard.press('Control');
  await rule.getByRole('button', { name: 'Becomes' }).click();
  await page.keyboard.press('Meta');
  await rule.getByRole('combobox', { name: 'When controlling' }).selectOption('macos');

  await expect
    .poll(() => pc.state.get('remap_rules_changed'))
    .toEqual([
      { from: 'CapsLock', to: 'End', when_target_os: null },
      { from: 'Ctrl', to: 'Meta', when_target_os: 'macos' },
    ]);
  await expect(page.locator('.fl-toast')).toHaveCount(0);

  // What the Mac receives reflects both rules.
  await mac.clearIo();
  await pc.tap('CAPS_LOCK');
  await pc.keyDown('CONTROL');
  await pc.keyDown('C', ['Ctrl']);
  await pc.keyUp('C', ['Ctrl']);
  await pc.keyUp('CONTROL');
  await expect
    .poll(async () => keysWithMods((await mac.virtualIo()).injected))
    .toEqual(['down END', 'up END', 'down COMMAND', 'down C [Meta]', 'up C [Meta]', 'up COMMAND']);

  // A fresh page shows the rules the engine has.
  await page.reload();
  await expect(page.getByTestId('remap-rule')).toHaveCount(2);
  expect(keyLog((await pc.virtualIo()).delivered_locally)).not.toContain('down C');
});
