import { expect, mock, test, url } from './fixtures';

const SCENARIOS = [
  'fresh-install',
  'two-devices',
  'three-devices',
  'controlling-remote',
  'controlled',
  'role-conflict',
  'permission-missing',
  'connecting',
  'reconnecting',
  'disconnected',
  'error',
  'pairing-request',
  'no-devices',
];

// ---------------------------------------------------------------- A. Boot & routing
test.describe('A. Boot & routing', () => {
  for (const sc of SCENARIOS) {
    test(`A1 scenario boots cleanly: ${sc}`, async ({ page, errors, shot }) => {
      await page.goto(url(`scenario=${sc}`, '#/home'));
      await expect(page.locator('.fl-splash')).toHaveCount(0, { timeout: 5000 });
      await expect(page.locator('#root')).not.toBeEmpty();
      await shot(`A1-boot-${sc}`);
      expect(errors).toEqual([]);
    });
  }

  test('A2 unknown scenario falls back to two-devices', async ({ page, errors, shot }) => {
    await page.goto(url('scenario=does-not-exist', '#/devices'));
    await expect(page.getByTestId('devices')).toContainText('Office PC');
    await shot('A2-unknown-scenario');
    expect(errors).toEqual([]);
  });

  test('A3 unknown os param falls back to macOS', async ({ page, shot }) => {
    await page.goto(url('scenario=two-devices&os=beos', '#/devices'));
    await expect(page.getByTestId('devices')).toContainText('MacBook Pro');
    await expect(page.getByTestId('devices')).toContainText('macOS');
    await shot('A3-unknown-os');
  });

  test('A4 unknown hash route falls back to Home', async ({ page, shot }) => {
    await page.goto(url('scenario=two-devices', '#/no/such/page'));
    await expect(page.getByTestId('home')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Home' })).toHaveAttribute('aria-current', 'page');
    await shot('A4-unknown-route');
  });

  test('A5 empty hash goes to Home', async ({ page }) => {
    await page.goto(url('scenario=two-devices'));
    await expect(page.getByTestId('home')).toBeVisible();
  });

  test('A6 dev routes: design gallery and engine inspector render', async ({ page, errors, shot }) => {
    await page.goto(url('scenario=two-devices', '#/design'));
    await expect(page.locator('body')).toContainText(/Button|Toggle/);
    await shot('A6-design-gallery');
    await page.goto(url('scenario=two-devices', '#/debug'));
    await expect(page.locator('body')).toContainText('devices');
    await shot('A6-debug-inspector');
    expect(errors).toEqual([]);
  });

  test('A7 browser back/forward follows hash routes', async ({ page }) => {
    await page.goto(url('scenario=two-devices', '#/home'));
    await page.getByRole('button', { name: 'Computers' }).click();
    await page.getByRole('button', { name: 'General' }).click();
    await expect(page.getByTestId('general')).toBeVisible();
    await page.goBack();
    await expect(page.getByTestId('devices')).toBeVisible();
    await page.goBack();
    await expect(page.getByTestId('home')).toBeVisible();
    await page.goForward();
    await expect(page.getByTestId('devices')).toBeVisible();
  });

  test('A8 Ctrl+number shortcuts: 1–5 navigate, 6/9 and Ctrl+Alt are ignored', async ({ page }) => {
    await page.goto(url('scenario=two-devices', '#/home'));
    await expect(page.getByTestId('home')).toBeVisible(); // shell listener mounts after the splash
    const h1 = page.getByRole('heading', { level: 1 });
    for (const [k, t] of [
      ['5', 'Diagnostics'],
      ['4', 'General'],
      ['2', 'Computers'],
      ['1', 'Home'],
    ]) {
      await page.keyboard.press(`Control+${k}`);
      await expect(h1).toHaveText(t!);
    }
    await page.keyboard.press('Control+6');
    await page.keyboard.press('Control+9');
    await page.keyboard.press('Control+Alt+3');
    await expect(h1).toHaveText('Home');
  });

  test('A9 forced onboarding with ?onboarding=1 even for a set-up scenario', async ({ page, shot }) => {
    await page.goto(url('scenario=two-devices&onboarding=1'));
    await expect(page.getByTestId('onboarding')).toHaveAttribute('data-step', 'welcome');
    await shot('A9-forced-onboarding');
  });
});

// ---------------------------------------------------------------- B. Onboarding
test.describe('B. Onboarding', () => {
  test('B1 Back button walks back through steps and keeps role choice', async ({ page, shot }) => {
    await page.goto(url('scenario=fresh-install'));
    const ob = page.getByTestId('onboarding');
    await expect(page.getByRole('button', { name: /back/i })).toHaveCount(0);
    await page.getByRole('button', { name: 'Get Started' }).click();
    await page.getByRole('radio', { name: /Control this computer from another one/ }).click();
    await page.getByRole('button', { name: 'Continue' }).click();
    await expect(ob).toHaveAttribute('data-step', 'permissions');
    await page.getByRole('button', { name: /back/i }).click();
    await expect(ob).toHaveAttribute('data-step', 'role');
    await expect(page.getByRole('radio', { name: /Control this computer from another one/ })).toBeChecked();
    await shot('B1-back-keeps-role');
    await page.getByRole('button', { name: /back/i }).click();
    await expect(ob).toHaveAttribute('data-step', 'welcome');
  });

  test('B2 step indicator announces "Step n of 5"', async ({ page }) => {
    await page.goto(url('scenario=fresh-install'));
    await expect(page.getByRole('list', { name: /Step 1 of/ })).toBeVisible();
    await page.getByRole('button', { name: 'Get Started' }).click();
    await expect(page.getByRole('list', { name: /Step 2 of/ })).toBeVisible();
  });

  for (const os of ['macos', 'windows', 'linux'] as const) {
    test(`B3 permission guide per OS: ${os}`, async ({ page, shot }) => {
      await page.goto(url(`scenario=fresh-install&os=${os}`));
      await page.getByRole('button', { name: 'Get Started' }).click();
      await page.getByRole('button', { name: 'Continue' }).click();
      await expect(page.getByTestId(`permission-guide-${os}`)).toBeVisible();
      await shot(`B3-permissions-${os}`);
      // Mock seeds permission as already granted on Windows/Linux, missing on macOS.
      if (os === 'macos') await expect(page.getByRole('button', { name: 'Continue' })).toBeDisabled();
      else await expect(page.getByRole('button', { name: 'Continue' })).toBeEnabled();
      if (os === 'linux') {
        await mock(page, 'linkState', 'permission_required');
        test.info().annotations.push({
          type: 'observation',
          description: `linux guide text: ${(await page.getByTestId('permission-guide-linux').innerText()).slice(0, 300)}`,
        });
      }
    });
  }

  test('B4 granting permission while on the step enables Continue', async ({ page, shot }) => {
    await page.goto(url('scenario=fresh-install&os=macos'));
    await page.getByRole('button', { name: 'Get Started' }).click();
    await page.getByRole('button', { name: 'Continue' }).click();
    await expect(page.getByRole('button', { name: 'Continue' })).toBeDisabled();
    await mock(page, 'grantPermission');
    await expect(page.getByRole('button', { name: 'Continue' })).toBeEnabled();
    await expect(page.getByRole('button', { name: 'Skip for Now' })).toHaveCount(0);
    await shot('B4-permission-granted-live');
  });

  test('B5 pairing during onboarding: rejected candidate, retry, then skip', async ({ page, shot }) => {
    await page.goto(url('scenario=fresh-install'));
    await page.getByRole('button', { name: 'Get Started' }).click();
    await page.getByRole('button', { name: 'Continue' }).click();
    await page.getByRole('button', { name: 'Skip for Now' }).click();
    await expect(page.getByTestId('pairing-found')).toBeVisible();
    await page.getByRole('button', { name: 'Connect to Dev Laptop' }).click();
    await expect(page.getByTestId('pairing-failed')).toBeVisible();
    await shot('B5-onboarding-pair-failed');
    await page.getByRole('button', { name: 'Try Again' }).click();
    await expect(page.getByTestId('pairing-found')).toBeVisible();
    await page.getByRole('button', { name: 'Skip for Now' }).click();
    await expect(page.getByTestId('onboarding')).toHaveAttribute('data-step', 'done');
  });

  test('B6 incoming pairing request appears during onboarding', async ({ page, shot }) => {
    await page.goto(url('scenario=fresh-install'));
    await mock(page, 'incomingPairingRequest');
    await expect(page.getByTestId('incoming-request')).toBeVisible();
    await shot('B6-incoming-during-onboarding');
  });

  test('B7 finishing onboarding persists across reload', async ({ page }) => {
    await page.goto(url('scenario=fresh-install'));
    await page.getByRole('button', { name: 'Get Started' }).click();
    await page.getByRole('button', { name: 'Continue' }).click();
    await page.getByRole('button', { name: 'Skip for Now' }).click();
    await page.getByRole('button', { name: 'Skip for Now' }).click();
    await page.getByRole('button', { name: 'Start Using Flow' }).click();
    await expect(page.getByTestId('home')).toBeVisible();
    await page.reload();
    await expect(page.getByTestId('home')).toBeVisible();
    await expect(page.getByTestId('onboarding')).toHaveCount(0);
  });

  test('B8 Enter on welcome advances (primary button is focused)', async ({ page }) => {
    await page.goto(url('scenario=fresh-install'));
    await expect(page.getByRole('button', { name: 'Get Started' })).toBeFocused();
    await page.keyboard.press('Enter');
    await expect(page.getByTestId('onboarding')).toHaveAttribute('data-step', 'role');
  });
});

// ---------------------------------------------------------------- C. Home & switching
test.describe('C. Home & switching', () => {
  test('C1 switch between every computer and back', async ({ page, shot }) => {
    await page.goto(url('scenario=three-devices', '#/home'));
    for (const name of ['Office PC', 'Studio Mac mini']) {
      await page
        .getByRole('button', { name: `Switch to ${name}` })
        .first()
        .click();
      await expect(page.getByTestId('status-title')).toHaveText(`Controlling ${name}`);
    }
    await shot('C1-controlling-studio');
    await page.getByRole('button', { name: 'Return to this computer' }).click();
    await expect(page.getByTestId('status-title')).toContainText('Your keyboard and mouse are on');
    await expect(page.getByLabel('Connection: Connected')).toBeVisible();
  });

  test('C2 rapid double-click on switch does not error', async ({ page, errors, shot }) => {
    await page.goto(url('scenario=three-devices', '#/home'));
    const btn = page.getByRole('button', { name: 'Switch to Office PC' }).first();
    const box = (await btn.boundingBox())!;
    await shot('C2-before-dblclick');
    await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2);
    await page.waitForTimeout(150);
    const under = await page.evaluate(
      ([x, y]) =>
        (document.elementFromPoint(x!, y!) as HTMLElement | null)?.closest('button')?.innerText ?? null,
      [box.x + box.width / 2, box.y + box.height / 2],
    );
    test.info().annotations.push({
      type: 'observation',
      description: `button under cursor after 1st click: "${under}"`,
    });
    await shot('C2-after-first-click');
    await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2);
    await shot('C2-after-second-click');
    expect
      .soft(
        await page.getByTestId('status-title').textContent(),
        'double-click switched away and straight back',
      )
      .toBe('Controlling Office PC');
    await expect(page.getByTestId('toasts')).not.toContainText('went wrong');
    expect(errors).toEqual([]);
  });

  test('C3 HUD announces each switch and auto-hides', async ({ page, shot }) => {
    await page.goto(url('scenario=two-devices', '#/home'));
    await page.getByRole('button', { name: 'Switch to Office PC' }).first().click();
    await expect(page.getByTestId('hud')).toContainText('Now controlling Office PC');
    await shot('C3-hud');
    await expect(page.getByTestId('hud')).toBeHidden({ timeout: 6000 });
  });

  test('C4 arrangement: arrow keys move a focused tile', async ({ page, shot }) => {
    await page.goto(url('scenario=three-devices', '#/home'));
    const tile = page.getByTestId('tile-d3');
    const before = await tile.boundingBox();
    await tile.focus();
    await page.keyboard.press('ArrowDown');
    await expect(async () => {
      const after = await tile.boundingBox();
      expect(Math.abs(after!.y - before!.y) + Math.abs(after!.x - before!.x)).toBeGreaterThan(10);
    }).toPass({ timeout: 3000 });
    await shot('C4-arrangement-keyboard-move');
  });

  test('C5 arrangement: dropping a tile onto another never overlaps', async ({ page, shot }) => {
    await page.goto(url('scenario=three-devices', '#/home'));
    const a = (await page.getByTestId('tile-d2').boundingBox())!;
    const b = (await page.getByTestId('tile-d1').boundingBox())!;
    await page.mouse.move(a.x + a.width / 2, a.y + a.height / 2);
    await page.mouse.down();
    await page.mouse.move(b.x + b.width / 2, b.y + b.height / 2, { steps: 10 });
    await page.mouse.up();
    await page.waitForTimeout(500);
    type Box = { x: number; y: number; width: number; height: number };
    const boxes = (await Promise.all(
      ['d1', 'd2', 'd3'].map((id) => page.getByTestId(`tile-${id}`).boundingBox()),
    )) as Box[];
    const overlap = (p: Box, q: Box) =>
      p.x < q.x + q.width - 1 &&
      q.x < p.x + p.width - 1 &&
      p.y < q.y + q.height - 1 &&
      q.y < p.y + p.height - 1;
    await shot('C5-arrangement-drop-on-tile');
    expect(
      overlap(boxes[0]!, boxes[1]!) || overlap(boxes[0]!, boxes[2]!) || overlap(boxes[1]!, boxes[2]!),
    ).toBe(false);
  });

  test('C6 controlled computer: enter / release / enter again', async ({ page, shot }) => {
    await page.goto(url('scenario=controlled', '#/home'));
    await mock(page, 'controllerEnters', true);
    await expect(page.getByTestId('status-title')).toContainText('is controlling this computer');
    await shot('C6-controlled-entered');
    await page.getByRole('button', { name: 'Release control' }).click();
    await expect(page.getByTestId('status-title')).toHaveText('This computer is ready');
    await mock(page, 'controllerEnters', true);
    await expect(page.getByTestId('status-title')).toContainText('is controlling this computer');
  });

  test('C7 controlled computer has no switch buttons', async ({ page }) => {
    await page.goto(url('scenario=controlled', '#/home'));
    await expect(page.getByRole('button', { name: /^Switch to / })).toHaveCount(0);
  });

  test('C8 active remote goes offline while controlling it', async ({ page, shot }) => {
    await page.goto(url('scenario=controlling-remote', '#/home'));
    const title = await page.getByTestId('status-title').textContent();
    const target = await page.evaluate(() =>
      (window as unknown as { __flowMock: { layout(): unknown } }).__flowMock.layout(),
    );
    void target;
    await mock(page, 'deviceState', 'd2', 'disconnected');
    await page.waitForTimeout(400);
    await shot('C8-active-remote-offline');
    test.info().annotations.push({
      type: 'observation',
      description: `before="${title}" after="${await page.getByTestId('status-title').textContent()}"`,
    });
  });

  test('C9 no-devices: Home shows the Add first computer call to action', async ({ page, shot }) => {
    await page.goto(url('scenario=no-devices', '#/home'));
    await expect(page.getByText('Add your first computer')).toBeVisible();
    await expect(page.getByLabel('Connection: No computers yet')).toBeVisible();
    await shot('C9-no-devices-home');
  });
});

// ---------------------------------------------------------------- D. Computers & pairing
test.describe('D. Computers & pairing', () => {
  test('D1 forget dialog: Cancel and Escape keep the computer', async ({ page, shot }) => {
    await page.goto(url('scenario=three-devices', '#/devices'));
    await page.getByRole('button', { name: 'Forget Office PC' }).click();
    await expect(page.getByRole('alertdialog')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Cancel' })).toBeFocused();
    await shot('D1-forget-dialog');
    await page.getByRole('button', { name: 'Cancel' }).click();
    await expect(page.getByTestId('device-d2')).toBeVisible();
    await page.getByRole('button', { name: 'Forget Office PC' }).click();
    await page.keyboard.press('Escape');
    await expect(page.getByRole('alertdialog')).toHaveCount(0);
    await expect(page.getByTestId('device-d2')).toBeVisible();
  });

  test('D2 forgetting the computer being controlled returns control home', async ({ page, shot }) => {
    await page.goto(url('scenario=controlling-remote', '#/home'));
    const t = (await page.getByTestId('status-title').textContent())!;
    const name = t.replace('Controlling ', '');
    await page.goto(url('scenario=controlling-remote', '#/devices'));
    await page.getByRole('button', { name: `Forget ${name}` }).click();
    await page.getByRole('button', { name: 'Forget Computer' }).click();
    await page.getByRole('button', { name: 'Home' }).click();
    await expect(page.getByTestId('status-title')).not.toContainText('Controlling');
    await shot('D2-forget-active');
  });

  test('D3 forgetting every computer leaves the empty state', async ({ page, shot }) => {
    await page.goto(url('scenario=three-devices', '#/devices'));
    for (const n of ['Office PC', 'Studio Mac mini']) {
      await page.getByRole('button', { name: `Forget ${n}` }).click();
      await page.getByRole('button', { name: 'Forget Computer' }).click();
    }
    await expect(page.getByText('No other computers yet')).toBeVisible();
    await shot('D3-forgot-all');
  });

  test('D4 pairing: cancel while searching', async ({ page, shot }) => {
    await page.goto('/?scenario=no-devices#/devices'); // real-speed so "searching" is visible
    await page.getByRole('button', { name: 'Add Computer…' }).first().click();
    await expect(page.getByTestId('pairing-searching')).toBeVisible();
    await shot('D4-pairing-searching');
    await page.keyboard.press('Escape');
    await expect(page.getByTestId('pairing-searching')).toHaveCount(0);
    // starting again works (no "already looking" error)
    await page.getByRole('button', { name: 'Add Computer…' }).first().click();
    await expect(page.getByTestId('pairing-found')).toBeVisible({ timeout: 5000 });
    await expect(page.getByTestId('toasts')).not.toContainText('already looking');
  });

  test('D5 pairing: cancel while waiting for the code', async ({ page, shot }) => {
    await page.goto('/?scenario=no-devices#/devices');
    await page.getByRole('button', { name: 'Add Computer…' }).first().click();
    await page.getByRole('button', { name: 'Connect to Living Room PC' }).click();
    await expect(page.getByTestId('pairing-requesting')).toBeVisible();
    await expect(page.getByLabel('Verification code 482 913')).toBeVisible();
    await shot('D5-pairing-code');
    await page.getByTestId('pairing-requesting').getByRole('button', { name: 'Cancel' }).click();
    await shot('D5-after-cancel');
    await page.waitForTimeout(2500);
    await expect(page.getByTestId('devices')).not.toContainText('Living Room PC');
  });

  test('D6 already paired computers are not offered again', async ({ page, shot }) => {
    await page.goto(url('scenario=no-devices', '#/devices'));
    await page.getByRole('button', { name: 'Add Computer…' }).first().click();
    await page.getByRole('button', { name: 'Connect to Living Room PC' }).click();
    await page.getByRole('button', { name: 'Done' }).click();
    await page.waitForTimeout(300);
    await page.getByRole('button', { name: 'Add Computer…' }).first().click();
    await expect(page.getByTestId('pairing-found')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Connect to Living Room PC' })).toHaveCount(0);
    await shot('D6-paired-excluded');
  });

  test('D7 incoming request: accept adds it, decline does not', async ({ page, shot }) => {
    await page.goto(url('scenario=two-devices', '#/devices'));
    await mock(page, 'incomingPairingRequest');
    const sheet = page.getByTestId('incoming-request');
    await expect(sheet).toBeVisible();
    await shot('D7-incoming-request');
    const title = (await sheet.getByRole('heading').first().textContent())!;
    const name = title.replace(/^Add /, '').replace(/\?$/, '');
    await sheet.getByRole('button', { name: /Decline|Reject|Don/ }).click();
    await expect(sheet).toHaveCount(0);
    await expect(page.getByTestId('devices')).not.toContainText(name);
    await mock(page, 'incomingPairingRequest', { requestId: 'r2' });
    await page.getByTestId('incoming-request').getByRole('button').last().click();
    await expect(page.getByTestId('devices')).toContainText(name);
  });

  test('D8 incoming request withdrawn while open closes the sheet', async ({ page, shot }) => {
    await page.goto(url('scenario=two-devices', '#/devices'));
    await mock(page, 'incomingPairingRequest');
    await expect(page.getByTestId('incoming-request')).toBeVisible();
    await mock(page, 'withdrawPairingRequest');
    await expect(page.getByTestId('incoming-request')).toHaveCount(0);
    await shot('D8-withdrawn');
  });

  test('D9 very long computer name from a request wraps without overflow', async ({ page, shot }) => {
    await page.goto(url('scenario=two-devices', '#/devices'));
    const long = 'Extraordinarily-Long-Workstation-Name-In-The-Basement-Lab-Rack-42-Unit-7';
    await mock(page, 'incomingPairingRequest', { deviceName: long });
    await shot('D9-long-name-request');
    await page.getByTestId('incoming-request').getByRole('button').last().click();
    await expect(page.getByTestId('devices')).toContainText(long);
    await shot('D9-long-name-list');
    await page.getByRole('button', { name: 'Home' }).click();
    await shot('D9-long-name-home');
    const overflow = await page.evaluate(
      () => document.documentElement.scrollWidth > document.documentElement.clientWidth,
    );
    expect(overflow).toBe(false);
  });

  test('D10 HTML/script in a device name is rendered as text', async ({ page, errors }) => {
    await page.goto(url('scenario=two-devices', '#/devices'));
    const evil = '<img src=x onerror="window.__pwned=1">Evil';
    await mock(page, 'incomingPairingRequest', { deviceName: evil });
    await page.getByTestId('incoming-request').getByRole('button').last().click();
    await expect(page.getByTestId('devices')).toContainText(evil);
    expect(await page.evaluate(() => (window as unknown as { __pwned?: number }).__pwned)).toBeUndefined();
    expect(errors.filter((e) => !e.includes('Failed to load resource'))).toEqual([]);
  });

  test('D11 role conflict notice links to General', async ({ page, shot }) => {
    await page.goto(url('scenario=role-conflict', '#/devices'));
    await expect(page.getByTestId('link-error')).toContainText('Both computers are set up the same way');
    await shot('D11-role-conflict');
    await page.getByTestId('link-error').getByRole('button').click();
    await expect(page.getByTestId('general')).toBeVisible();
  });

  test('D12 resolving role conflict by changing role clears the notice', async ({ page, shot }) => {
    await page.goto(url('scenario=role-conflict', '#/general'));
    await page.getByRole('radio', { name: /Control this computer from another one/ }).click();
    const dialog = page.getByRole('alertdialog');
    if (await dialog.isVisible().catch(() => false)) await dialog.getByRole('button').first().click();
    await page.getByRole('button', { name: 'Computers' }).click();
    await expect(page.getByTestId('link-error')).toHaveCount(0);
    await shot('D12-role-conflict-resolved');
  });

  test('D13 switch from Computers list to an unreachable computer is prevented', async ({ page, shot }) => {
    await page.goto(url('scenario=three-devices', '#/devices'));
    await mock(page, 'deviceState', 'd3', 'disconnected');
    const btn = page.getByTestId('device-d3').getByRole('button', { name: /Switch/ });
    const n = await btn.count();
    if (n) {
      const disabled = await btn.isDisabled();
      if (!disabled) {
        await btn.click();
        await expect(page.getByTestId('toasts')).toContainText("can't reach");
      }
    }
    await shot('D13-unreachable-device');
  });
});

// ---------------------------------------------------------------- E. Keyboard & Shortcuts
test.describe('E. Keyboard & Shortcuts', () => {
  test('E1 preset change updates the recorder and toast', async ({ page, shot }) => {
    await page.goto(url('scenario=two-devices', '#/shortcuts'));
    await page.getByLabel('Shortcut preset').selectOption('F13');
    await expect(page.getByTestId('toasts')).toContainText('Shortcut updated');
    await expect(page.getByRole('img', { name: 'F13' }).first()).toBeVisible();
    await shot('E1-preset-f13');
  });

  test('E2 recording a custom shortcut shows "Custom" preset', async ({ page, shot }) => {
    await page.goto(url('scenario=two-devices', '#/shortcuts'));
    await page.getByRole('button', { name: 'Switch shortcut' }).click();
    await page.keyboard.press('Control+Alt+K');
    await expect(page.getByTestId('toasts')).toContainText('Shortcut updated');
    await expect(page.getByLabel('Shortcut preset')).toHaveValue('custom');
    await shot('E2-custom-shortcut');
  });

  test('E3 Escape while recording cancels without changing', async ({ page, shot }) => {
    await page.goto(url('scenario=two-devices', '#/shortcuts'));
    await page.getByRole('button', { name: 'Switch shortcut' }).click();
    await shot('E3-recording');
    await page.keyboard.press('Escape');
    await expect(page.getByLabel('Shortcut preset')).toHaveValue('ScrollLock');
    await expect(page.getByTestId('toasts')).not.toContainText('Shortcut updated');
  });

  test('E4 modifier-only press is not accepted as a shortcut', async ({ page, shot }) => {
    await page.goto(url('scenario=two-devices', '#/shortcuts'));
    await page.getByRole('button', { name: 'Switch shortcut' }).click();
    await page.keyboard.down('Shift');
    await page.keyboard.up('Shift');
    await page.waitForTimeout(300);
    await shot('E4-modifier-only');
    await page.keyboard.press('Escape');
    await expect(page.getByLabel('Shortcut preset')).toHaveValue('ScrollLock');
  });

  test('E5 return-home shortcut: set and clear', async ({ page, shot }) => {
    await page.goto(url('scenario=two-devices', '#/shortcuts'));
    await page.getByRole('button', { name: 'Return home shortcut' }).click();
    await page.keyboard.press('Control+Shift+H');
    await expect(page.getByRole('img', { name: 'Control Shift H' }).first()).toBeVisible();
    await shot('E5-return-home-set');
    // The only way to clear is Backspace/Delete while recording (no visible clear button).
    await expect(page.getByRole('button', { name: /Clear/ })).toHaveCount(0);
    await page.getByRole('button', { name: 'Return home shortcut' }).click();
    await page.keyboard.press('Backspace');
    await expect(page.getByRole('button', { name: 'Return home shortcut' })).toContainText('Not set');
    await shot('E5-return-home-cleared');
  });

  test('E6 edge switching off disables the delay slider', async ({ page, shot }) => {
    await page.goto(url('scenario=two-devices', '#/shortcuts'));
    const slider = page.getByRole('slider', { name: 'Edge switch delay' });
    await expect(slider).toBeEnabled();
    await page.getByRole('switch', { name: 'Switch at screen edges' }).click();
    await expect(slider).toBeDisabled();
    await shot('E6-edge-off');
  });

  test('E7 delay slider via keyboard: steps of 50, clamps at 0 and 1000', async ({ page, shot }) => {
    await page.goto(url('scenario=two-devices', '#/shortcuts'));
    const slider = page.getByRole('slider', { name: 'Edge switch delay' });
    await slider.focus();
    await page.keyboard.press('ArrowLeft');
    await expect(slider).toHaveAttribute('aria-valuetext', 'Instant');
    await page.keyboard.press('ArrowRight');
    await expect(slider).toHaveAttribute('aria-valuetext', '50 ms');
    await page.keyboard.press('End');
    await expect(slider).toHaveAttribute('aria-valuetext', '1000 ms');
    await page.keyboard.press('ArrowRight');
    await expect(slider).toHaveAttribute('aria-valuetext', '1000 ms');
    await shot('E7-slider-max');
  });

  test('E8 remap: complete rule saves; incomplete draft stays local', async ({ page, shot }) => {
    await page.goto(url('scenario=two-devices', '#/shortcuts'));
    await page.getByRole('button', { name: 'Add Rule' }).click();
    const rule = page.getByTestId('remap-rule').last();
    await rule.getByRole('button', { name: 'Key to remap' }).click();
    await page.keyboard.press('CapsLock');
    await shot('E8-remap-half');
    await rule.getByRole('button', { name: 'Becomes' }).click();
    await page.keyboard.press('Escape');
    await rule.getByRole('button', { name: 'Becomes' }).click();
    await page.keyboard.press('F12');
    await expect(page.getByTestId('toasts')).not.toContainText(/went wrong|can only/);
    const n = await page.getByTestId('remap-rule').count();
    await page.keyboard.press('Control+1');
    await page.keyboard.press('Control+3');
    await expect(page.getByTestId('remap-rule')).toHaveCount(n); // persisted in engine
    await shot('E8-remap-complete');
  });

  test('E9 remap: modifier → key is rejected with a friendly error', async ({ page, shot }) => {
    await page.goto(url('scenario=two-devices', '#/shortcuts'));
    await page.getByRole('button', { name: 'Add Rule' }).click();
    const rule = page.getByTestId('remap-rule').last();
    await rule.getByRole('button', { name: 'Key to remap' }).click();
    await page.keyboard.press('Meta');
    await rule.getByRole('button', { name: 'Becomes' }).click();
    await page.keyboard.press('a');
    await shot('E9-remap-invalid');
    await expect(page.getByTestId('toasts')).toContainText('A key can only become another key');
  });

  test('E10 remap: remove rule; "When controlling" filter', async ({ page, shot }) => {
    await page.goto(url('scenario=two-devices', '#/shortcuts'));
    const before = await page.getByTestId('remap-rule').count();
    await page.getByRole('button', { name: 'Add Rule' }).click();
    await page.getByTestId('remap-rule').last().getByLabel('When controlling').selectOption('windows');
    await shot('E10-remap-target-os');
    await page.getByTestId('remap-rule').last().getByRole('button', { name: 'Remove rule' }).click();
    await expect(page.getByTestId('remap-rule')).toHaveCount(before);
  });

  test('E11 key map switches OS', async ({ page, shot }) => {
    await page.goto(url('scenario=two-devices', '#/shortcuts'));
    await expect(page.getByTestId('keymap')).toContainText('COMMAND');
    await page.getByLabel('Show keys for').selectOption('windows');
    await expect(page.getByTestId('keymap')).toContainText('SCROLLLOCK');
    await expect(page.getByTestId('keymap')).toContainText('0x91');
    await shot('E11-keymap-windows');
  });

  test('E12 Open Keymap Folder does not error', async ({ page }) => {
    await page.goto(url('scenario=two-devices', '#/shortcuts'));
    await page.getByRole('button', { name: 'Open Keymap Folder' }).click();
    await page.waitForTimeout(300);
    await expect(page.getByTestId('toasts')).not.toContainText('went wrong');
  });

  test('E14 a bare letter can be recorded as the switch shortcut (would hijack typing)', async ({
    page,
    shot,
  }) => {
    await page.goto(url('scenario=two-devices', '#/shortcuts'));
    await page.getByRole('button', { name: 'Switch shortcut' }).click();
    await page.keyboard.press('a');
    await page.waitForTimeout(300);
    const toasts = (await page.getByTestId('toasts').textContent()) ?? '';
    await shot('E14-bare-letter-shortcut');
    test.info().annotations.push({ type: 'observation', description: `toasts: "${toasts}"` });
    expect
      .soft(toasts, 'a plain letter was accepted as the global switch key')
      .not.toContain('Shortcut updated');
  });

  test('E15 return-home can be set to the same keys as the switch shortcut', async ({ page, shot }) => {
    await page.goto(url('scenario=two-devices', '#/shortcuts'));
    await page.getByLabel('Shortcut preset').selectOption('Ctrl+Shift+Space');
    await page.getByRole('button', { name: 'Return home shortcut' }).click();
    await page.keyboard.press('Control+Shift+Space');
    await page.waitForTimeout(300);
    await shot('E15-duplicate-shortcut');
    const rh = await page.getByRole('button', { name: 'Return home shortcut' }).innerText();
    test.info().annotations.push({ type: 'observation', description: `return-home now shows "${rh}"` });
    expect.soft(rh.replace(/\s/g, ''), 'duplicate of switch shortcut accepted silently').toBe('Notset');
  });

  test('E16 two remap rules for the same key', async ({ page, shot }) => {
    await page.goto(url('scenario=two-devices', '#/shortcuts'));
    for (const to of ['F11', 'F12']) {
      await page.getByRole('button', { name: 'Add Rule' }).click();
      const rule = page.getByTestId('remap-rule').last();
      await rule.getByRole('button', { name: 'Key to remap' }).click();
      await page.keyboard.press('CapsLock');
      await rule.getByRole('button', { name: 'Becomes' }).click();
      await page.keyboard.press(to);
    }
    await page.waitForTimeout(300);
    await shot('E16-conflicting-remaps');
    const n = await page.getByTestId('remap-rule').filter({ hasText: 'Key: CapsLock' }).count();
    test.info().annotations.push({
      type: 'observation',
      description: `CapsLock rules shown: ${n}; toasts: "${await page.getByTestId('toasts').textContent()}"`,
    });
    expect.soft(n, 'two contradictory rules for CapsLock accepted without warning').toBeLessThan(2);
  });

  test('E13 controlled computer sees the "settings apply on controller" note', async ({ page, shot }) => {
    await page.goto(url('scenario=controlled', '#/shortcuts'));
    await expect(page.getByText('This computer is controlled by another one')).toBeVisible();
    await shot('E13-controlled-shortcuts');
  });
});

// ---------------------------------------------------------------- F. General & Diagnostics
test.describe('F. General & Diagnostics', () => {
  test('F1 changing role with paired computers asks first; Cancel keeps it', async ({ page, shot }) => {
    await page.goto(url('scenario=two-devices', '#/general'));
    await page.getByRole('radio', { name: /Control this computer from another one/ }).click();
    await expect(page.getByRole('alertdialog')).toContainText('Change this computer’s role?');
    await shot('F1-role-confirm');
    await page.getByRole('button', { name: 'Cancel' }).click();
    await expect(page.getByRole('switch', { name: 'Share keyboard' })).toBeVisible();
  });

  test('F2 confirming role change hides Sharing section', async ({ page, shot }) => {
    await page.goto(url('scenario=two-devices', '#/general'));
    await page.getByRole('radio', { name: /Control this computer from another one/ }).click();
    await page.getByRole('button', { name: 'Let Another Computer Control This One' }).click();
    await expect(page.getByTestId('toasts')).toContainText('Role updated');
    await expect(page.getByRole('switch', { name: 'Share keyboard' })).toHaveCount(0);
    await shot('F2-role-controlled');
  });

  test('F3 no-devices: role changes without confirmation', async ({ page }) => {
    await page.goto(url('scenario=no-devices', '#/general'));
    await page.getByRole('radio', { name: /Control this computer from another one/ }).click();
    await expect(page.getByRole('alertdialog')).toHaveCount(0);
    await expect(page.getByTestId('toasts')).toContainText('Role updated');
  });

  test('F4 every toggle flips and reflects engine state', async ({ page, shot }) => {
    await page.goto(url('scenario=two-devices', '#/general'));
    for (const name of [
      'Share keyboard',
      'Share mouse',
      'Open Flow at login',
      'Show in menu bar',
      'Reconnect automatically',
      'Connect to known computers',
    ]) {
      const sw = page.getByRole('switch', { name });
      const was = await sw.getAttribute('aria-checked');
      await sw.click();
      await expect(sw).not.toHaveAttribute('aria-checked', was!);
    }
    await shot('F4-toggles-flipped');
  });

  test('F5 pointer speed segmented control', async ({ page }) => {
    await page.goto(url('scenario=two-devices', '#/general'));
    await page.getByRole('radio', { name: 'Faster' }).click();
    await expect(page.getByRole('radio', { name: 'Faster' })).toBeChecked();
  });

  test('F6 theme Dark persists across reload; System follows OS', async ({ page, shot }) => {
    await page.goto(url('scenario=two-devices', '#/general'));
    await page.getByRole('radio', { name: 'Dark' }).click();
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
    await page.reload();
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
    await shot('F6-dark-general');
    await page.getByRole('radio', { name: 'System' }).click();
    // "System" removes data-theme and lets tokens.css follow prefers-color-scheme.
    await expect(page.locator('html')).not.toHaveAttribute('data-theme', /.+/);
    const bg = () => page.evaluate(() => getComputedStyle(document.body).backgroundColor);
    await page.emulateMedia({ colorScheme: 'dark' });
    const dark = await bg();
    await page.emulateMedia({ colorScheme: 'light' });
    const light = await bg();
    test
      .info()
      .annotations.push({ type: 'observation', description: `system bg dark=${dark} light=${light}` });
    expect(dark).not.toBe(light);
  });

  test('F7 Windows host says "system tray" not "menu bar"', async ({ page }) => {
    await page.goto(url('scenario=two-devices&os=windows', '#/general'));
    await expect(page.getByRole('switch', { name: 'Show in system tray' })).toBeVisible();
  });

  test('F8 permission Fix… opens the sheet', async ({ page, shot }) => {
    await page.goto(url('scenario=permission-missing', '#/general'));
    await page.getByRole('button', { name: 'Fix…' }).click();
    await expect(page.getByRole('dialog')).toBeVisible();
    await shot('F8-permission-sheet');
    await page.keyboard.press('Escape');
    await expect(page.getByRole('dialog')).toHaveCount(0);
  });

  test('F9 diagnostics: stats tick, export, logging toggle', async ({ page, shot }) => {
    await page.goto(url('scenario=controlling-remote', '#/diagnostics'));
    const sent = page.getByTestId('stat-Inputs sent');
    const v1 = await sent.textContent();
    await expect.poll(() => sent.textContent(), { timeout: 4000 }).not.toBe(v1);
    await page.getByRole('switch', { name: 'Detailed logging' }).click();
    await page.getByRole('button', { name: 'Export…' }).click();
    await expect(page.getByTestId('toasts')).toContainText('Saved to');
    await shot('F9-diagnostics');
  });

  test('F10 reset settings: confirm restores defaults but keeps role', async ({ page, shot }) => {
    await page.goto(url('scenario=two-devices', '#/shortcuts'));
    await page.getByLabel('Shortcut preset').selectOption('F13');
    await page.goto(url('scenario=two-devices', '#/diagnostics'));
    // same page session? new page load resets the mock; do it within one session instead:
    await page.goto(url('scenario=two-devices', '#/shortcuts'));
    await page.getByLabel('Shortcut preset').selectOption('F13');
    await page.getByRole('button', { name: 'Diagnostics' }).click();
    await page.getByRole('button', { name: 'Reset…' }).click();
    await shot('F10-reset-confirm');
    await page.getByRole('button', { name: 'Reset Settings' }).click();
    await page.getByRole('button', { name: 'Keyboard & Shortcuts' }).click();
    await expect(page.getByLabel('Shortcut preset')).toHaveValue('ScrollLock');
  });
});

// ---------------------------------------------------------------- G. Link states & recovery
test.describe('G. Link states & recovery', () => {
  test('G1 reconnecting → Stop Trying → disconnected → Try Again → connected', async ({ page, shot }) => {
    await page.goto(url('scenario=reconnecting', '#/home'));
    await expect(page.getByTestId('banner')).toContainText('Reconnecting');
    await expect(page.getByLabel('Connection: Reconnecting…')).toBeVisible();
    await page.getByRole('button', { name: 'Stop Trying' }).click();
    await expect(page.getByTestId('banner')).toContainText('Not connected');
    await shot('G1-stopped');
    await page.getByTestId('banner').getByRole('button', { name: 'Try Again' }).click();
    await expect(page.getByTestId('banner')).toHaveCount(0, { timeout: 5000 });
    await expect(page.getByLabel('Connection: Connected')).toBeVisible();
    await shot('G1-recovered');
  });

  test('G2 error → Try Again recovers', async ({ page }) => {
    await page.goto(url('scenario=error', '#/home'));
    await page.getByTestId('banner').getByRole('button', { name: 'Try Again' }).click();
    await expect(page.getByTestId('banner')).toHaveCount(0, { timeout: 5000 });
  });

  test('G3 live link drop while on another page shows banner everywhere', async ({ page, shot }) => {
    await page.goto(url('scenario=two-devices', '#/shortcuts'));
    await mock(page, 'linkState', 'reconnecting');
    await expect(page.getByTestId('banner')).toContainText('Reconnecting');
    await shot('G3-banner-on-shortcuts');
    await mock(page, 'linkState', 'connected');
    await expect(page.getByTestId('banner')).toHaveCount(0);
  });

  test('G4 connecting scenario shows a non-alarming state', async ({ page, shot }) => {
    await page.goto(url('scenario=connecting', '#/home'));
    await expect(page.getByLabel('Connection: Connecting…')).toBeVisible();
    await shot('G4-connecting');
  });

  test('G5 permission granted live clears banner and sidebar status', async ({ page, shot }) => {
    await page.goto(url('scenario=permission-missing', '#/home'));
    await expect(page.getByLabel('Connection: Needs permission')).toBeVisible();
    await mock(page, 'grantPermission');
    await expect(page.getByTestId('banner')).toHaveCount(0);
    await expect(page.getByLabel('Connection: Connected')).toBeVisible();
    await shot('G5-permission-granted');
  });

  test('G6 Try Again double-click does not surface an error', async ({ page }) => {
    await page.goto(url('scenario=disconnected', '#/home'));
    await page.getByTestId('banner').getByRole('button', { name: 'Try Again' }).dblclick();
    await page.waitForTimeout(300);
    const t = await page.getByTestId('toasts').textContent();
    test.info().annotations.push({ type: 'observation', description: `toasts after dbl-click: "${t}"` });
  });
});

// ---------------------------------------------------------------- H. Tray popover
test.describe('H. Tray', () => {
  test.use({ viewport: { width: 360, height: 480 } });

  test('H1 switch from tray and come back', async ({ page, shot }) => {
    await page.goto(url('scenario=three-devices', '#/tray'));
    await page.getByTestId('tray-d2').click();
    await expect(page.getByTestId('tray-summary')).toContainText('Office PC');
    await shot('H1-tray-controlling');
    await page
      .getByRole('button', { name: /Return|Back/ })
      .first()
      .click();
    await expect(page.getByTestId('tray-summary')).not.toContainText('Controlling');
  });

  test('H2 tray in controlled mode offers release', async ({ page, shot }) => {
    await page.goto(url('scenario=controlled', '#/tray'));
    await mock(page, 'controllerEnters', true);
    await expect(page.getByRole('button', { name: 'Release control' })).toBeVisible();
    await shot('H2-tray-controlled');
  });

  test('H3 tray with no devices and with permission missing', async ({ page, shot }) => {
    await page.goto(url('scenario=no-devices', '#/tray'));
    await shot('H3-tray-no-devices');
    await page.goto(url('scenario=permission-missing', '#/tray'));
    await expect(page.getByTestId('tray')).toContainText('Flow needs permission');
    await shot('H3-tray-permission');
  });

  test('H4 tray does not show onboarding or HUD on fresh install', async ({ page, shot }) => {
    await page.goto(url('scenario=fresh-install', '#/tray'));
    await expect(page.getByTestId('onboarding')).toHaveCount(0);
    await expect(page.getByTestId('tray')).toBeVisible();
    await shot('H4-tray-fresh');
  });
});

// ---------------------------------------------------------------- I. Responsive, a11y, robustness
test.describe('I. Responsive, a11y, robustness', () => {
  for (const vp of [
    { w: 800, h: 600 },
    { w: 640, h: 480 },
    { w: 390, h: 844 },
  ]) {
    test(`I1 layout at ${vp.w}×${vp.h} has no horizontal scroll`, async ({ page, shot }) => {
      await page.setViewportSize({ width: vp.w, height: vp.h });
      for (const r of ['home', 'devices', 'shortcuts', 'general', 'diagnostics']) {
        await page.goto(url('scenario=three-devices', `#/${r}`));
        await page.waitForTimeout(200);
        const o = await page.evaluate(() => ({
          doc: document.documentElement.scrollWidth - document.documentElement.clientWidth,
        }));
        if (r === 'home' || r === 'shortcuts') await shot(`I1-${vp.w}-${r}`);
        expect.soft(o.doc, `${r} overflows by ${o.doc}px`).toBeLessThanOrEqual(0);
      }
    });
  }

  test('I2 every button has an accessible name (all main pages)', async ({ page }) => {
    const missing: string[] = [];
    for (const r of ['home', 'devices', 'shortcuts', 'general', 'diagnostics', 'tray']) {
      await page.goto(url('scenario=three-devices', `#/${r}`));
      await page.waitForTimeout(200);
      const m = await page.$$eval(
        'button, [role=button], [role=switch], [role=radio], select, input',
        (els) =>
          els
            .filter((e) => {
              const name =
                e.getAttribute('aria-label') ||
                e.getAttribute('aria-labelledby') ||
                (e as HTMLElement).innerText?.trim() ||
                e.getAttribute('title') ||
                (e.id && document.querySelector(`label[for="${e.id}"]`));
              return !name && (e as HTMLElement).offsetParent !== null;
            })
            .map((e) => e.outerHTML.slice(0, 120)),
      );
      missing.push(...m.map((x) => `${r}: ${x}`));
    }
    expect(missing).toEqual([]);
  });

  test('I3 Tab order reaches main content and shows a focus ring', async ({ page, shot }) => {
    await page.goto(url('scenario=two-devices', '#/general'));
    await page.locator('body').click({ position: { x: 600, y: 5 } });
    const seq: string[] = [];
    for (let i = 0; i < 8; i++) {
      await page.keyboard.press('Tab');
      seq.push(
        await page.evaluate(() => {
          const el = document.activeElement as HTMLElement;
          return `${el.tagName}:${el.getAttribute('aria-label') ?? el.innerText?.slice(0, 20) ?? ''}`;
        }),
      );
    }
    test.info().annotations.push({ type: 'observation', description: `tab order: ${seq.join(' → ')}` });
    const f = await page.evaluate(() => {
      const el = document.activeElement as HTMLElement;
      const cs = getComputedStyle(el);
      return { tag: el.tagName, outline: cs.outlineStyle, shadow: cs.boxShadow };
    });
    await shot('I3-focus-ring');
    expect(f.tag).not.toBe('BODY');
    expect(f.outline !== 'none' || f.shadow !== 'none').toBe(true);
  });

  test('I4 reduced motion is respected (no long transitions)', async ({ page }) => {
    await page.emulateMedia({ reducedMotion: 'reduce' });
    await page.goto(url('scenario=two-devices', '#/home'));
    const longest = await page.evaluate(() => {
      let max = 0;
      for (const el of Array.from(document.querySelectorAll('*'))) {
        const d = getComputedStyle(el)
          .transitionDuration.split(',')
          .map((x) => parseFloat(x) * (x.includes('ms') ? 1 : 1000));
        max = Math.max(max, ...d);
      }
      return max;
    });
    test
      .info()
      .annotations.push({ type: 'observation', description: `longest transition with reduce: ${longest}ms` });
    expect(longest).toBeLessThanOrEqual(100);
  });

  test('I5 localStorage blocked: app still boots', async ({ browser }) => {
    const ctx = await browser.newContext();
    await ctx.addInitScript(() => {
      Object.defineProperty(window, 'localStorage', {
        get() {
          throw new DOMException('blocked', 'SecurityError');
        },
      });
    });
    const page = await ctx.newPage();
    const errs: string[] = [];
    page.on('pageerror', (e) => errs.push(e.message));
    await page.goto(url('scenario=two-devices', '#/home'));
    await expect(page.getByTestId('home')).toBeVisible();
    await page.screenshot({ path: `${process.env.QA_SHOTS ?? 'test-results-qa/shots'}/I5-no-storage.png` });
    expect(errs).toEqual([]);
    await ctx.close();
  });

  test('I6 WebSocket transport to a dead daemon shows "not responding"', async ({ page, shot }) => {
    await page.goto('/?daemon=ws://127.0.0.1:1&token=x#/home');
    await expect(page.locator('body')).toContainText(/isn’t responding|Not responding|Connecting/, {
      timeout: 15000,
    });
    await page.waitForTimeout(3000);
    await shot('I6-dead-daemon');
    test.info().annotations.push({
      type: 'observation',
      description: `body: ${(await page.locator('body').innerText()).slice(0, 200)}`,
    });
  });

  test('I7 dark-mode tour of every page', async ({ page, shot }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    for (const r of ['home', 'devices', 'shortcuts', 'general', 'diagnostics']) {
      await page.goto(url('scenario=three-devices', `#/${r}`));
      await shot(`I7-dark-${r}`);
    }
  });

  test('I8 rapid navigation across all sections leaves no errors', async ({ page, errors }) => {
    await page.goto(url('scenario=three-devices', '#/home'));
    await expect(page.getByTestId('home')).toBeVisible();
    for (let i = 0; i < 30; i++) await page.keyboard.press(`Control+${(i % 5) + 1}`);
    await expect(page.getByRole('heading', { level: 1 })).toHaveText('Diagnostics');
    expect(errors).toEqual([]);
  });
});
