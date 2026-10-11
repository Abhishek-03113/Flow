// Records the product clips used by the landing page as retina frame sequences (Chrome screencast)
// plus an ffmpeg concat list per clip, with a drawn pointer and click ripple. See ../README.md.
import path from 'node:path';
const { chromium } = await import(new URL('../../app/ui/node_modules/playwright/index.mjs', import.meta.url).href);
const BASE = process.env.FLOW_UI_URL ?? 'http://localhost:4173';
const CHROMIUM = process.env.CHROMIUM_PATH;
import fs from 'node:fs';
const OUT = process.argv[2]; const only = process.argv[3];
const B = `${BASE}/?`;
const browser = await chromium.launch(CHROMIUM ? { executablePath: CHROMIUM } : {});
const VP = { width: 1100, height: 720 };
const CURSOR = `
addEventListener('DOMContentLoaded', () => {
  const c = document.createElement('div');
  c.innerHTML = '<svg width="22" height="28" viewBox="0 0 22 28"><path d="M2 2 L2 22 L7.5 17 L11 25.5 L14.5 24 L11 16 L18.5 16 Z" fill="#111" stroke="#fff" stroke-width="1.6" stroke-linejoin="round"/></svg>';
  Object.assign(c.style, {position:'fixed',left:'0',top:'0',zIndex:2147483647,pointerEvents:'none',transform:'translate(-100px,-100px)',filter:'drop-shadow(0 2px 3px rgba(0,0,0,.3))'});
  document.body.appendChild(c);
  addEventListener('mousemove', e => { c.style.transform = 'translate(' + (e.clientX-2) + 'px,' + (e.clientY-2) + 'px)'; }, true);
  addEventListener('mousedown', e => {
    const r = document.createElement('div');
    Object.assign(r.style, {position:'fixed',left:(e.clientX-18)+'px',top:(e.clientY-18)+'px',width:'36px',height:'36px',borderRadius:'50%',background:'rgba(0,122,255,.28)',zIndex:2147483646,pointerEvents:'none',transition:'transform .45s ease, opacity .45s ease',transform:'scale(.3)'});
    document.body.appendChild(r); requestAnimationFrame(() => { r.style.transform='scale(1.4)'; r.style.opacity='0'; }); setTimeout(() => r.remove(), 600);
  }, true);
});`;
let pos = { x: 550, y: 600 };
async function moveTo(page, loc, opts = {}) {
  const b = await loc.boundingBox();
  const x = b.x + b.width * (opts.fx ?? 0.5), y = b.y + b.height * (opts.fy ?? 0.5);
  await glide(page, x, y, opts.ms ?? 700);
}
async function glide(page, x, y, ms = 700) {
  const steps = Math.max(12, Math.round(ms / 16)); const s = { ...pos };
  for (let i = 1; i <= steps; i++) { const t = i / steps; const e = t < .5 ? 4*t*t*t : 1 - Math.pow(-2*t+2, 3)/2;
    await page.mouse.move(s.x + (x - s.x) * e, s.y + (y - s.y) * e); await page.waitForTimeout(16); }
  pos = { x, y };
}
async function click(page, loc, opts) { await moveTo(page, loc, opts); await page.waitForTimeout(150); await page.mouse.down(); await page.waitForTimeout(90); await page.mouse.up(); }
async function record(name, theme, query, hash, script) {
  if (only && only !== name) return;
  const dir = `${path.resolve(OUT)}/tmp-${name}-${theme}`; fs.rmSync(dir, { recursive: true, force: true });
  const ctx = await browser.newContext({ viewport: VP, deviceScaleFactor: 2, colorScheme: theme });
  await ctx.addInitScript(CURSOR);
  const page = await ctx.newPage(); pos = { x: 760, y: 640 };
  fs.mkdirSync(dir, { recursive: true });
  await page.goto(B + query + hash); await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(500);
  await page.mouse.move(pos.x, pos.y);
  const cdp = await ctx.newCDPSession(page); const frames = []; let n = 0;
  cdp.on('Page.screencastFrame', async (f) => {
    const file = `${dir}/${String(n++).padStart(5, '0')}.jpg`;
    fs.writeFileSync(file, Buffer.from(f.data, 'base64')); frames.push({ file, t: f.metadata.timestamp });
    cdp.send('Page.screencastFrameAck', { sessionId: f.sessionId }).catch(() => {});
  });
  await cdp.send('Page.startScreencast', { format: 'jpeg', quality: 92, maxWidth: 2200, maxHeight: 1440, everyNthFrame: 1 });
  await page.waitForTimeout(300);
  await script(page);
  await page.waitForTimeout(700);
  await cdp.send('Page.stopScreencast');
  frames.push({ file: frames.at(-1).file, t: frames.at(-1).t + 0.5 });
  let list = '';
  for (let i = 0; i < frames.length - 1; i++) list += `file '${frames[i].file}'\nduration ${Math.max(0.001, frames[i + 1].t - frames[i].t).toFixed(4)}\n`;
  list += `file '${frames.at(-1).file}'\n`;
  fs.writeFileSync(`${OUT}/${name}-${theme}.txt`, list);
  await ctx.close();
}
for (const t of ['light', 'dark']) {
  await record('switch', t, 'scenario=three-devices', '#/home', async (p) => {
    await p.waitForTimeout(500);
    await click(p, p.getByRole('button', { name: 'Switch to Office PC' }));
    await p.waitForTimeout(1700);
    await click(p, p.getByRole('button', { name: 'Switch to Studio Mac mini' }));
    await p.waitForTimeout(1700);
    await click(p, p.getByRole('button', { name: 'Return to this computer' }));
    await p.waitForTimeout(1600);
  });
  await record('arrange', t, 'scenario=three-devices', '#/home', async (p) => {
    await p.waitForTimeout(400);
    const tile = p.getByTestId('tile-d3');
    const area = await p.getByTestId('arrangement').boundingBox();
    await moveTo(p, tile); await p.waitForTimeout(200); await p.mouse.down();
    await glide(p, area.x + area.width * 0.36, area.y + area.height - 40, 1100);
    await p.waitForTimeout(250); await p.mouse.up(); await p.waitForTimeout(900);
    await p.waitForTimeout(800);
    await click(p, p.getByRole('button', { name: 'Switch to Studio Mac mini' }));
    await p.waitForTimeout(1600);
  });
  await record('pair', t, 'scenario=no-devices', '#/devices', async (p) => {
    await p.waitForTimeout(500);
    await click(p, p.getByRole('button', { name: 'Add Computer…' }).first());
    await p.getByTestId('pairing-found').waitFor(); await p.waitForTimeout(900);
    await click(p, p.getByRole('button', { name: 'Connect to Living Room PC' }));
    await p.getByTestId('pairing-paired').waitFor({ timeout: 20000 }); await p.waitForTimeout(1200);
    await click(p, p.getByRole('button', { name: 'Done' }));
    await p.waitForTimeout(1300);
  });
  await record('onboard', t, 'scenario=fresh-install', '', async (p) => {
    await p.waitForTimeout(900);
    await click(p, p.getByRole('button', { name: 'Get Started' }));
    await p.waitForTimeout(900);
    await click(p, p.getByRole('radio', { name: /Share this computer|Use this computer's keyboard|keyboard and mouse/ }).first());
    await p.waitForTimeout(500);
    await click(p, p.getByRole('button', { name: 'Continue' }));
    await p.waitForTimeout(1100);
    await click(p, p.getByRole('button', { name: 'Skip for Now' }));
    await p.getByTestId('pairing-found').waitFor(); await p.waitForTimeout(700);
    await click(p, p.getByRole('button', { name: 'Connect to Studio Mac mini' }));
    await p.getByTestId('pairing-paired').waitFor({ timeout: 20000 }); await p.waitForTimeout(800);
    await click(p, p.getByRole('button', { name: 'Continue' }));
    await p.waitForTimeout(1500);
  });
}
await browser.close();
