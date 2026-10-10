import fs from 'node:fs';
import { composite, expect, test } from './fixtures';

test.use({
  world: {
    paired: true,
    devices: [
      { name: 'Mac', os: 'mac', role: 'controller' },
      { name: 'PC', os: 'win', role: 'controlled' },
    ],
  },
});

interface Diag {
  switch_latency_ms_p50: number | null;
  switch_latency_ms_p95: number | null;
  events_forwarded: number;
  events_dropped: number;
  uptime_s: number;
}

test('11. diagnostics show the engine’s real numbers after a switch; export writes a bundle', async ({
  live,
  browser,
}) => {
  const mac = live.daemon('Mac');
  const diag = () => mac.state.get('diagnostics_changed') as Diag | undefined;
  const page = await live.open('Mac', { hash: '#/diagnostics' });
  const pcPage = await live.open('PC');
  const stat = (label: string) => page.getByTestId(`stat-${label}`).locator('.fl-stat__value');

  // Nothing measured yet.
  await expect(stat('Switch time (typical)')).toHaveText('—');
  await expect(stat('Inputs sent')).toHaveText('0');

  // Switch (hotkey) and type: the engine measures and counts, the page follows its ~2 s ticks.
  await mac.chord(['F13']);
  await expect(pcPage.getByTestId('status-title')).toHaveText('Mac is controlling this computer');
  for (const k of ['H', 'E', 'L', 'L', 'O']) await mac.tap(k);
  await expect.poll(() => diag()?.events_forwarded ?? 0, { timeout: 15_000 }).toBeGreaterThanOrEqual(10);
  await expect.poll(() => diag()?.switch_latency_ms_p50 ?? null).not.toBeNull();

  const d = diag()!;
  await expect(stat('Inputs sent')).toHaveText(d.events_forwarded.toLocaleString('en-US'));
  await expect(stat('Inputs dropped')).toHaveText(String(d.events_dropped));
  await expect(stat('Switch time (typical)')).not.toHaveText('—');
  await expect(stat('Switch time (typical)')).toHaveText(/^(<1|\d+(\.\d)?) ms$/);
  await expect(stat('Switch time (slowest)')).not.toHaveText('—');
  await expect(stat('Running for')).toHaveText(/\d+ (s|min)/);

  // The controlled computer does not switch; its Home names who uses it instead of a switch time.
  await expect(pcPage.getByTestId('home')).toContainText('Used from');
  await expect(pcPage.getByTestId('home')).not.toContainText('Switch time');

  await composite(browser, '11-diagnostics.png', [
    { page, label: 'Mac: Diagnostics after a switch and typing' },
    { page: pcPage, label: 'PC: received the typing' },
  ]);

  // Export Diagnostics: the engine writes a JSON bundle and answers its path.
  await page.getByRole('button', { name: 'Export…' }).click();
  const toast = page.locator('.fl-toast', { hasText: 'Saved to ' });
  await expect(toast).toBeVisible();
  const path = (await toast.textContent())!.replace(/^.*Saved to /, '').trim();
  expect(fs.existsSync(path), path).toBe(true);
  const bundle = JSON.parse(fs.readFileSync(path, 'utf8')) as Record<string, unknown>;
  expect(Object.keys(bundle)).toEqual(expect.arrayContaining(['settings', 'devices', 'diagnostics']));
});
