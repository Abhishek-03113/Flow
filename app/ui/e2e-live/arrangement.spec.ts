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

interface Rect {
  device_id: string;
  x: number;
  y: number;
  width: number;
  height: number;
}

test('8. dragging the PC below the Mac changes the engine layout, and the bottom edge leads to it', async ({
  live,
  browser,
}) => {
  const mac = live.daemon('Mac');
  const pcId = mac.idOf('PC');
  const layout = () => (mac.state.get('screen_layout_changed') as { screens: Rect[] } | undefined)?.screens;
  const pcRect = () => layout()?.find((s) => s.device_id === pcId);
  const macPage = await live.open('Mac');
  const pcPage = await live.open('PC');

  // The engine's default: the PC to the right of the Mac.
  await expect.poll(() => pcRect()).toMatchObject({ x: 1920, y: 0 });
  const before = mac.seen.filter((e) => e.event === 'screen_layout_changed').length;

  const macTile = (await macPage.getByTestId('tile-d1').boundingBox())!;
  const tile = macPage.getByTestId(`tile-${pcId}`);
  const pcTile = (await tile.boundingBox())!;
  await macPage.mouse.move(pcTile.x + pcTile.width / 2, pcTile.y + pcTile.height / 2);
  await macPage.mouse.down();
  await macPage.mouse.move(macTile.x + macTile.width / 2, macTile.y + macTile.height + pcTile.height / 2, {
    steps: 10,
  });
  await macPage.mouse.up();

  // set_screen_layout reached the engine and came back as screen_layout_changed.
  await expect
    .poll(() => mac.seen.filter((e) => e.event === 'screen_layout_changed').length)
    .toBeGreaterThan(before);
  await expect.poll(() => pcRect()?.y).toBe(1080);
  const r = pcRect()!;
  expect(r.x).toBeLessThan(1920);
  expect(r.x + r.width).toBeGreaterThan(0);

  // The right edge leads nowhere now...
  await mac.move(2000, 0);
  await mac.move(40, 0);
  await expect.poll(async () => (await mac.virtualIo()).delivered_locally.length).toBeGreaterThan(1);
  expect(mac.controlState()?.active_screen).toBe('local');

  // ...the bottom edge, above the PC, does.
  const below = Math.round(r.x + r.width / 2); // a column that touches the PC
  await mac.move(below - 1919, 0);
  await mac.move(0, 2000);
  await mac.move(0, 40);
  await expect.poll(() => mac.controlState()?.active_screen).toBe(pcId);
  await expect(macPage.getByTestId('status-title')).toHaveText('Controlling PC');
  await expect(pcPage.getByTestId('status-title')).toHaveText('Mac is controlling this computer');

  // A fresh page shows the arrangement the engine has.
  await macPage.reload();
  const macBox = (await macPage.getByTestId('tile-d1').boundingBox())!;
  const pcBox = (await macPage.getByTestId(`tile-${pcId}`).boundingBox())!;
  expect(pcBox.y).toBeGreaterThanOrEqual(macBox.y + macBox.height - 2);

  // The controlled PC draws the Mac's real arrangement, read-only: the Mac above it.
  const macId = pcPage
    .getByTestId('arrangement')
    .locator('[data-testid^="tile-"]:not([data-testid="tile-d1"])');
  await expect(macId).toHaveCount(1);
  await expect
    .poll(async () => {
      const up = (await macId.boundingBox())!;
      const me = (await pcPage.getByTestId('tile-d1').boundingBox())!;
      return up.y + up.height <= me.y + 2 && up.x < me.x + me.width;
    })
    .toBe(true);

  await composite(browser, '08-arrangement.png', [
    { page: macPage, label: 'Mac: PC dragged below, crossed the bottom edge' },
    { page: pcPage, label: 'PC: being controlled' },
  ]);
});
