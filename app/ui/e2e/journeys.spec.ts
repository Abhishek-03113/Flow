import { expect, test } from '@playwright/test';
import { MAIN, url } from './helpers';

test.beforeEach(async ({ page }) => page.setViewportSize(MAIN));

test('dragging a screen tile snaps it to an edge and persists the layout', async ({ page }) => {
  await page.goto(url('scenario=three-devices', '#/home'));
  const tile = page.getByTestId('tile-d3');
  await expect(tile).toBeVisible();
  const before = await tile.boundingBox();
  const area = (await page.getByTestId('arrangement').boundingBox())!;
  await page.mouse.move(before!.x + before!.width / 2, before!.y + before!.height / 2);
  await page.mouse.down();
  await page.mouse.move(area.x + area.width * 0.35, area.y + area.height - 10, { steps: 8 });
  await expect(page.locator('.fl-ghost')).toBeVisible();
  await page.mouse.up();
  await expect(async () => {
    const after = await tile.boundingBox();
    expect(Math.abs(after!.y - before!.y)).toBeGreaterThan(20);
  }).toPass();
  // Reload keeps nothing (mock), but the tile must not overlap another.
  await page.waitForTimeout(400); // let the snap animation finish
  type Box = { x: number; y: number; width: number; height: number };
  const box = async (id: string) => (await page.getByTestId(`tile-${id}`).boundingBox()) as Box;
  const [a, b, c] = [await box('d1'), await box('d2'), await box('d3')] as [Box, Box, Box];
  const overlap = (p: Box, q: Box) =>
    p.x < q.x + q.width - 1 &&
    q.x < p.x + p.width - 1 &&
    p.y < q.y + q.height - 1 &&
    q.y < p.y + p.height - 1;
  expect({ a, b, c, o: overlap(a, c) || overlap(b, c) }).toMatchObject({ o: false });
});

test('keyboard: sidebar arrow keys and Ctrl+number navigate sections', async ({ page }) => {
  await page.goto(url('scenario=two-devices', '#/home'));
  await page.getByRole('button', { name: 'Home' }).focus();
  await page.keyboard.press('ArrowDown');
  await expect(page.getByRole('button', { name: 'Computers' })).toBeFocused();
  await page.keyboard.press('Enter');
  await expect(page.getByRole('heading', { level: 1, name: 'Computers' })).toBeVisible();
  await page.keyboard.press('Control+3');
  await expect(page.getByRole('heading', { level: 1, name: 'Keyboard & Shortcuts' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Keyboard & Shortcuts' })).toHaveAttribute(
    'aria-current',
    'page',
  );
});

test('adding a computer: search → connect → code → paired → listed', async ({ page }) => {
  await page.goto(url('scenario=no-devices', '#/devices'));
  await expect(page.getByText('No other computers yet')).toBeVisible();
  await page.getByRole('button', { name: 'Add Computer…' }).first().click();
  await expect(page.getByTestId('pairing-found')).toBeVisible();
  await page.getByRole('button', { name: 'Connect to Living Room PC' }).click();
  await expect(page.getByTestId('pairing-requesting')).toContainText('482 913');
  await expect(page.getByTestId('pairing-paired')).toContainText('Connected to Living Room PC');
  await page.getByRole('button', { name: 'Done' }).click();
  await expect(page.getByTestId('devices')).toContainText('Living Room PC');
});

test('pairing failure is held until dismissed and can be retried', async ({ page }) => {
  await page.goto(url('scenario=no-devices', '#/devices'));
  await page.getByRole('button', { name: 'Add Computer…' }).first().click();
  await page.getByRole('button', { name: 'Connect to Dev Laptop' }).click();
  await expect(page.getByTestId('pairing-failed')).toContainText("Dev Laptop didn't accept");
  await page.waitForTimeout(500); // engine has gone idle by now; the outcome is still shown
  await expect(page.getByTestId('pairing-failed')).toBeVisible();
  await page.getByRole('button', { name: 'Try Again' }).click();
  await expect(page.getByTestId('pairing-found')).toBeVisible();
});

test('forget a computer needs confirmation', async ({ page }) => {
  await page.goto(url('scenario=three-devices', '#/devices'));
  await page.getByRole('button', { name: 'Forget Studio Mac mini' }).click();
  await expect(page.getByRole('alertdialog')).toContainText('Forget Studio Mac mini?');
  await page.getByRole('button', { name: 'Forget Computer' }).click();
  await expect(page.getByTestId('device-d3')).toHaveCount(0);
});

test('switching control from the Home card shows the HUD and updates the tray-style summary', async ({
  page,
}) => {
  await page.goto(url('scenario=three-devices', '#/home'));
  await page.getByRole('button', { name: 'Switch to Studio Mac mini' }).click();
  await expect(page.getByTestId('hud')).toContainText('Now controlling Studio Mac mini');
  await expect(page.getByTestId('status-title')).toHaveText('Controlling Studio Mac mini');
  await page.getByRole('button', { name: 'Return to this computer' }).click();
  await expect(page.getByTestId('hud')).toContainText('Back on MacBook Pro');
});

test('controlled computer: remote controller enters, HUD announces, release control', async ({ page }) => {
  await page.goto(url('scenario=controlled', '#/home'));
  await page.evaluate(() =>
    (window as never as { __flowMock: { controllerEnters(b: boolean): void } }).__flowMock.controllerEnters(
      false,
    ),
  );
  await expect(page.getByTestId('status-title')).toHaveText('This computer is ready');
  await page.evaluate(() =>
    (window as never as { __flowMock: { controllerEnters(b: boolean): void } }).__flowMock.controllerEnters(
      true,
    ),
  );
  await expect(page.getByTestId('hud')).toContainText('Desk PC took control');
  await page.getByRole('button', { name: 'Release control' }).click();
  await expect(page.getByTestId('status-title')).toHaveText('This computer is ready');
});

test('settings: record a shortcut, theme override and permission request', async ({ page }) => {
  await page.goto(url('scenario=two-devices', '#/shortcuts'));
  await page.getByRole('button', { name: 'Switch shortcut' }).click();
  await page.keyboard.press('Control+Shift+Space');
  await expect(page.getByTestId('toasts')).toContainText('Shortcut updated');
  await expect(page.getByRole('button', { name: 'Switch shortcut' })).toHaveAccessibleName('Switch shortcut');
  await expect(page.getByRole('img', { name: 'Control Shift Space' }).first()).toBeVisible();

  await page.goto(url('scenario=two-devices', '#/general'));
  await page.getByRole('radio', { name: 'Dark' }).click();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');

  await page.goto(url('scenario=permission-missing', '#/home'));
  await page.getByRole('button', { name: 'Allow…' }).click();
  await page.getByRole('button', { name: 'Allow Access' }).click();
  await expect(page.getByText('Allowed').first()).toBeVisible({ timeout: 5000 });
  // macOS: the grant applies after a restart, offered right there.
  await page.getByTestId('permission-sheet').getByRole('button', { name: 'Restart Flow' }).click();
  await page.getByRole('button', { name: 'Done' }).click();
  await expect(page.getByTestId('banner')).toHaveCount(0);
});

test('every link state shows a banner; healthy shows none', async ({ page }) => {
  for (const [name, expected] of [
    ['reconnecting', 'Reconnecting'],
    ['disconnected', 'Not connected'],
    ['error', 'Sharing is paused'],
    ['permission-missing', 'Flow needs permission'],
  ]) {
    await page.goto(url(`scenario=${name}`, '#/home'));
    await expect(page.getByTestId('banner')).toContainText(expected!);
  }
  await page.goto(url('scenario=two-devices', '#/home'));
  await expect(page.getByTestId('status-title')).toBeVisible();
  await expect(page.getByTestId('banner')).toHaveCount(0);
});

test('a typing key on its own is refused as the switch shortcut', async ({ page }) => {
  await page.goto(url('scenario=two-devices', '#/shortcuts'));
  await page.getByRole('button', { name: 'Switch shortcut' }).click();
  await page.keyboard.press('a');
  await expect(page.getByTestId('toasts')).toContainText('That key is used for typing');
  await expect(page.getByRole('combobox', { name: 'Shortcut preset' })).toHaveValue('ScrollLock');
  // With a modifier it is fine.
  await page.getByRole('button', { name: 'Switch shortcut' }).click();
  await page.keyboard.press('Control+Alt+K');
  await expect(page.getByTestId('toasts')).toContainText('Shortcut updated');
});

test('the return-home shortcut cannot repeat the switch shortcut', async ({ page }) => {
  await page.goto(url('scenario=two-devices', '#/shortcuts'));
  await page.getByRole('combobox', { name: 'Shortcut preset' }).selectOption('Ctrl+Shift+Space');
  await page.getByRole('button', { name: 'Return home shortcut' }).click();
  await page.keyboard.press('Control+Shift+Space');
  await expect(page.getByTestId('toasts')).toContainText('need different keys');
  await expect(page.getByRole('button', { name: 'Return home shortcut' })).toContainText('Not set');
});

test('a second remap rule for the same key is refused and stays an unsaved draft', async ({ page }) => {
  await page.goto(url('scenario=two-devices', '#/shortcuts'));
  const saved = await page.getByTestId('remap-rule').count();
  for (const to of ['F11', 'F12']) {
    await page.getByRole('button', { name: 'Add Rule' }).click();
    const rule = page.getByTestId('remap-rule').last();
    await rule.getByRole('button', { name: 'Key to remap' }).click();
    await page.keyboard.press('CapsLock');
    await rule.getByRole('button', { name: 'Becomes' }).click();
    await page.keyboard.press(to);
  }
  await expect(page.getByTestId('toasts')).toContainText('already a rule for that key');
  await expect(page.getByText('Becomes: F12')).toHaveCount(0);
  // Leaving the page drops the draft; the engine kept only the first rule.
  // Wait for each page to render: the hash changes before React does, and two
  // presses in one frame never leave Shortcuts, so the draft would survive.
  await page.keyboard.press('Control+1');
  await expect(page.getByTestId('home')).toBeVisible();
  await page.keyboard.press('Control+3');
  await expect(page.getByTestId('shortcuts')).toBeVisible();
  await expect(page.getByTestId('remap-rule')).toHaveCount(saved + 1);
});

test('double-clicking Switch to … stays switched', async ({ page }) => {
  await page.goto(url('scenario=three-devices', '#/home'));
  await page.getByRole('button', { name: 'Switch to Office PC' }).dblclick();
  await expect(page.getByTestId('status-title')).toHaveText('Controlling Office PC');
  await page.waitForTimeout(300);
  await expect(page.getByTestId('status-title')).toHaveText('Controlling Office PC');
});

test('screen tiles never overlap, even at the smallest window size', async ({ page }) => {
  type Box = { x: number; y: number; width: number; height: number };
  const overlap = (p: Box, q: Box) =>
    Math.min(p.x + p.width, q.x + q.width) - Math.max(p.x, q.x) > 1 &&
    Math.min(p.y + p.height, q.y + q.height) - Math.max(p.y, q.y) > 1;
  for (const width of [760, 900, 1100]) {
    await page.setViewportSize({ width, height: 600 });
    await page.goto(url('scenario=three-devices', '#/home'));
    await expect(page.getByTestId('tile-d3')).toBeVisible();
    const boxes = (await Promise.all(
      ['d1', 'd2', 'd3'].map((id) => page.getByTestId(`tile-${id}`).boundingBox()),
    )) as Box[];
    const pairs = [
      [0, 1],
      [1, 2],
      [0, 2],
    ] as const;
    for (const [i, j] of pairs)
      expect(overlap(boxes[i]!, boxes[j]!), `${width}px: tiles ${i}/${j}`).toBe(false);
  }
});
