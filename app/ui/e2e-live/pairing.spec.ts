import { composite, expect, test } from './fixtures';
import { keyLog } from './harness';

// Two fresh computers that have never met, Noise + consent like a real install.
// Both start as controllers (the engine default); the PC's user picks its role in onboarding.
test.use({
  world: {
    paired: false,
    security: 'secure',
    devices: [
      { name: 'Mac', os: 'mac', role: 'controller' },
      { name: 'PC', os: 'win', role: 'controller' },
    ],
  },
});

test('1. first run: onboarding, role, pairing with matching codes, Home shows the PC', async ({
  live,
  browser,
}) => {
  const mac = live.daemon('Mac');
  const pc = live.daemon('PC');
  const macPage = await live.open('Mac', { onboarding: true });
  const pcPage = await live.open('PC', { onboarding: true });

  for (const page of [macPage, pcPage]) {
    await expect(page.getByTestId('onboarding')).toHaveAttribute('data-step', 'role');
  }
  // Mac keeps the default; the PC becomes the controlled one.
  await expect(macPage.getByRole('radio', { name: /Share this computer/ })).toHaveAttribute(
    'aria-checked',
    'true',
  );
  await pcPage.getByRole('radio', { name: /Control this computer from another one/ }).click();
  // A virtual OS needs no permission, so setup is skipped: straight to pairing.
  for (const page of [macPage, pcPage]) {
    await page.getByRole('button', { name: 'Get Started' }).click();
    await expect(page.getByTestId('onboarding')).toHaveAttribute('data-step', 'pair');
  }
  await expect.poll(() => pc.settings()?.role).toBe('controlled');
  await expect.poll(() => pc.controlState()?.role).toBe('controlled');

  // The search finds the other computer.
  await expect(macPage.getByRole('button', { name: 'Connect to PC' })).toBeVisible({ timeout: 20_000 });
  await macPage.getByRole('button', { name: 'Connect to PC' }).click();

  // Secure pairing: the same code on both computers; the PC's user accepts.
  const macCode = macPage.getByTestId('pairing-requesting').locator('.fl-code');
  await expect(macCode).toHaveText(/\S/);
  const sheet = pcPage.getByTestId('incoming-request');
  await expect(sheet).toBeVisible();
  const code = (await macCode.textContent())!.trim();
  expect(code).not.toBe('···');
  await expect(sheet.locator('.fl-code')).toHaveText(code);
  await composite(browser, '01-pairing-codes.png', [
    { page: macPage, label: 'Mac: Connect to PC, code shown' },
    { page: pcPage, label: 'PC: incoming request, same code' },
  ]);
  await sheet.getByRole('button', { name: 'Accept' }).click();

  await expect(macPage.getByTestId('pairing-paired')).toContainText('Connected to PC');
  await expect(pcPage.getByTestId('pairing-paired')).toContainText('Connected to Mac');

  for (const page of [macPage, pcPage]) {
    await expect(page.getByTestId('onboarding-done')).toBeVisible();
    await page.getByRole('button', { name: 'Start Using Flow' }).click();
    await expect(page.getByTestId('home')).toBeVisible();
  }
  await expect(macPage.getByTestId('status-title')).toHaveText('Your keyboard and mouse are on Mac');
  await expect(macPage.getByRole('button', { name: 'Switch to PC' })).toBeEnabled();
  await expect(pcPage.getByTestId('status-title')).toHaveText('This computer is ready');

  await macPage.getByRole('button', { name: 'Computers' }).click();
  const row = macPage.getByTestId(`device-${mac.idOf('PC')}`);
  await expect(row).toContainText('PC');
  await expect(row).toContainText('Connected');

  // And the pairing is real: switching forwards input over the new link.
  await pc.clearIo();
  await row.getByRole('button', { name: 'Switch' }).click();
  await expect.poll(() => mac.controlState()?.active_screen).toBe(mac.idOf('PC'));
  await mac.tap('Q');
  await expect.poll(async () => keyLog((await pc.virtualIo()).injected)).toEqual(['down Q', 'up Q']);
});

test('1b. cancelling while the other computer decides withdraws the request there too', async ({ live }) => {
  const mac = live.daemon('Mac');
  const macPage = await live.open('Mac', { hash: '#/devices' });
  const pcPage = await live.open('PC');

  await macPage.getByRole('button', { name: 'Add Computer…' }).first().click();
  await macPage.getByRole('button', { name: 'Connect to PC' }).click({ timeout: 20_000 });
  const requesting = macPage.getByTestId('pairing-requesting');
  await expect(requesting).toBeVisible();
  await expect(pcPage.getByTestId('incoming-request')).toBeVisible();

  await requesting.getByRole('button', { name: 'Cancel' }).click();
  await expect(pcPage.getByTestId('incoming-request')).toBeHidden();
  // Back to the list of nearby computers, nothing paired.
  await expect(macPage.getByRole('button', { name: 'Connect to PC' })).toBeVisible({ timeout: 20_000 });
  expect(mac.devices().filter((d) => !d.is_local)).toEqual([]);
});
