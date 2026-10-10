// Exploratory QA probes against real engines (npm run qa:live). Failing cases are open findings.
import path from 'node:path';
import { expect, test } from '../../e2e-live/fixtures';
import { keyLog } from '../../e2e-live/harness';

const SHOTS = process.env.QA_SHOTS ?? 'test-results-live/qa-shots';
const note = (d: string) => test.info().annotations.push({ type: 'observation', description: d });

test.use({
  world: {
    paired: true,
    devices: [
      { name: 'Mac', os: 'mac', role: 'controller' },
      { name: 'PC', os: 'win', role: 'controlled' },
    ],
  },
});

test('L1 bare letter as switch key: engine accepts it and the letter stops typing', async ({ live }) => {
  const mac = live.daemon('Mac');
  const pc = live.daemon('PC');
  const page = await live.open('Mac', { hash: '#/shortcuts' });
  await page.getByRole('button', { name: 'Switch shortcut' }).click();
  await page.keyboard.press('a');
  await expect.poll(() => (mac.settings()?.switch_key as { keys: string[] }).keys).toEqual(['A']);
  await page.screenshot({ path: path.join(SHOTS, 'L1-bare-letter-switch-key.png') });
  await mac.clearIo();
  await pc.clearIo();
  await mac.tap('A');
  await page.waitForTimeout(500);
  const local = keyLog((await mac.virtualIo()).delivered_locally);
  note(
    `after tapping A: active_screen=${mac.controlState()?.active_screen}, delivered locally=${JSON.stringify(local)}`,
  );
  expect
    .soft(local, 'typing "a" on the controller now switches computers instead of typing')
    .toEqual(['down A', 'up A']);
});

test('L2 return-home key identical to switch key: which wins?', async ({ live }) => {
  const mac = live.daemon('Mac');
  const page = await live.open('Mac', { hash: '#/shortcuts' });
  await page.getByRole('combobox', { name: 'Shortcut preset' }).selectOption('F13');
  await page.getByRole('button', { name: 'Return home shortcut' }).click();
  await page.keyboard.press('Pause');
  await expect
    .poll(() => (mac.settings()?.return_home_key as { keys: string[] } | null)?.keys)
    .toEqual(['Pause']);
  await page.getByRole('combobox', { name: 'Shortcut preset' }).selectOption('Pause');
  await page.waitForTimeout(400);
  const s = mac.settings();
  note(`settings: switch=${JSON.stringify(s?.switch_key)} return_home=${JSON.stringify(s?.return_home_key)}`);
  await page.screenshot({ path: path.join(SHOTS, 'L2-duplicate-shortcuts.png') });
  await mac.chord(['PAUSE']);
  await page.waitForTimeout(400);
  const first = mac.controlState()?.active_screen;
  await mac.chord(['PAUSE']);
  await page.waitForTimeout(400);
  const second = mac.controlState()?.active_screen;
  note(`pressing Pause twice: ${first} → ${second}`);
  expect
    .soft(s?.return_home_key, 'engine stored a return-home key equal to the switch key')
    .not.toEqual(s?.switch_key);
});

test('L3 two contradictory remap rules for one key: what does the target receive?', async ({ live }) => {
  const mac = live.daemon('Mac');
  const pc = live.daemon('PC');
  const page = await live.open('Mac', { hash: '#/shortcuts' });
  for (const to of ['F11', 'F12']) {
    await page.getByRole('button', { name: 'Add Rule' }).click();
    const rule = page.getByTestId('remap-rule').last();
    await rule.getByRole('button', { name: 'Key to remap' }).click();
    await page.keyboard.press('CapsLock');
    await rule.getByRole('button', { name: 'Becomes' }).click();
    await page.keyboard.press(to);
  }
  await page.waitForTimeout(500);
  note(`engine rules: ${JSON.stringify(mac.state.get('remap_rules_changed'))}`);
  await page.screenshot({ path: path.join(SHOTS, 'L3-contradictory-remaps.png') });
  await page.getByRole('button', { name: 'Home', exact: true }).click();
  await page.getByRole('button', { name: 'Switch to PC' }).click();
  await expect(page.getByTestId('status-title')).toHaveText('Controlling PC');
  await pc.clearIo();
  await mac.tap('CAPS_LOCK');
  await page.waitForTimeout(500);
  note(`PC injected: ${JSON.stringify(keyLog((await pc.virtualIo()).injected))}`);
});

test('L4 engine dies under an open page, then comes back', async ({ live }) => {
  const page = await live.open('Mac');
  await expect(page.getByTestId('status-title')).toBeVisible();
  await live.disconnect('Mac');
  await page.waitForTimeout(3000);
  await page.screenshot({ path: path.join(SHOTS, 'L4-engine-dead.png') });
  note(`while dead: ${(await page.locator('body').innerText()).slice(0, 160).replace(/\n/g, ' / ')}`);
  await live.reconnect('Mac');
  await page.waitForTimeout(6000);
  await page.screenshot({ path: path.join(SHOTS, 'L4-engine-back.png') });
  const text = (await page.locator('body').innerText()).slice(0, 160).replace(/\n/g, ' / ');
  note(`after restart (no reload): ${text}`);
  expect
    .soft(text, 'UI does not reconnect on its own after the engine restarts')
    .not.toMatch(/isn’t responding/);
});

test('L5 double-click on "Switch to PC" with real engines', async ({ live }) => {
  const mac = live.daemon('Mac');
  const page = await live.open('Mac');
  await page.getByRole('button', { name: 'Switch to PC' }).dblclick();
  await page.waitForTimeout(800);
  await page.screenshot({ path: path.join(SHOTS, 'L5-live-dblclick.png') });
  note(
    `after dblclick: active_screen=${mac.controlState()?.active_screen}, title=${await page.getByTestId('status-title').textContent()}`,
  );
});

test('L6 engine rejects out-of-range / empty values sent directly', async ({ live }) => {
  const mac = live.daemon('Mac');
  const out: string[] = [];
  for (const [cmd, payload] of [
    ['update_settings', { edge_switch_delay_ms: 99999 }],
    ['update_settings', { edge_switch_delay_ms: -5 }],
    ['set_switch_key', { label: '', keys: [] }],
    ['switch_to', { target: 'no-such-device' }],
    ['set_role', { role: 'admin' }],
    ['set_remap_rules', { rules: [{ from: 'Ctrl', to: 'A', when_target_os: null }] }],
    ['remove_device', { device_id: 'nope' }],
    ['bogus_command', {}],
  ] as const) {
    try {
      await mac.call(cmd, payload as never);
      out.push(`${cmd} ${JSON.stringify(payload)} → ACCEPTED`);
    } catch (e) {
      out.push(`${cmd} ${JSON.stringify(payload)} → ${String((e as Error).message ?? e).slice(0, 90)}`);
    }
  }
  for (const o of out) note(o);
  expect(out.filter((o) => o.endsWith('ACCEPTED'))).toEqual([]);
});
