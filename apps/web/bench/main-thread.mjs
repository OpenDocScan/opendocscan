// How long the interface is frozen during one complete scan.
//
// The per-operation numbers in `wasm-bench.mjs` say how fast the core is.
// They do not say whether the app *feels* fast, because that depends on
// something else entirely: whether the work happens on the thread that
// draws. A long task is the browser's own name for a stretch where it
// could not respond to anything — a tap, a scroll, a camera frame — so
// summing them over a real flow is as close as a script gets to measuring
// the thing a person actually notices.
//
// Run with:  node apps/web/bench/main-thread.mjs   (server on :8765)
//   BASE_URL=http://127.0.0.1:8766 node ... to point at another build.

import { chromium } from 'playwright';

const BASE = process.env.BASE_URL ?? 'http://127.0.0.1:8765';

const MAKE_PAGE = `(text) => {
  const canvas = document.createElement('canvas');
  canvas.width = 2400; canvas.height = 3200;
  const ctx = canvas.getContext('2d');
  ctx.fillStyle = '#2b2b2e'; ctx.fillRect(0, 0, canvas.width, canvas.height);
  ctx.save();
  ctx.transform(1, 0.06, -0.09, 1, 360, 150);
  ctx.fillStyle = '#ffffff'; ctx.fillRect(0, 0, 1680, 2400);
  ctx.fillStyle = '#000000';
  ctx.font = 'bold 150px Helvetica, Arial, sans-serif';
  ctx.fillText(text, 140, 340);
  ctx.font = '96px Helvetica, Arial, sans-serif';
  for (let i = 0; i < 14; i += 1) ctx.fillText('Line ' + i + ' of body text', 140, 600 + i * 130);
  ctx.restore();
  return new Promise((resolve) => canvas.toBlob(resolve, 'image/png'));
}`;

const browser = await chromium.launch();
const page = await browser.newPage();
await page.goto(`${BASE}/index.html`);
await page.waitForFunction(() => !document.getElementById('view-library').hidden);
// Let the engine finish loading, so its compile is not charged to the scan.
await page.waitForTimeout(2500);

await page.evaluate(() => {
  window.__long = [];
  new PerformanceObserver((list) => {
    for (const entry of list.getEntries()) window.__long.push(entry.duration);
  }).observe({ entryTypes: ['longtask'] });
});

const started = Date.now();
await page.evaluate(async ([maker]) => {
  const blob = await eval(maker)('INVOICE');
  const input = document.getElementById('file-input');
  const dt = new DataTransfer();
  dt.items.add(new File([blob], 'page.png', { type: 'image/png' }));
  input.files = dt.files;
  input.dispatchEvent(new Event('change', { bubbles: true }));
}, [MAKE_PAGE]);

const settled = async (id) => {
  await page.waitForFunction((v) => !document.getElementById(v).hidden, id, { timeout: 60000 });
  await page.waitForFunction(() => document.getElementById('busy').hidden, undefined, { timeout: 60000 });
};

await settled('view-crop');
await page.click('#btn-crop-confirm');
await settled('view-filter');
await page.waitForFunction(() => document.getElementById('filter-canvas').width > 300, undefined, {
  timeout: 60000,
});

// Six slider nudges: the interaction most likely to feel broken, because
// each one is a full filter pass and the thumb is still moving.
for (const value of [10, 20, 30, 20, 10, 0]) {
  await page.evaluate((v) => {
    const slider = document.getElementById('brightness');
    slider.value = String(v);
    slider.dispatchEvent(new Event('input', { bubbles: true }));
  }, value);
  await page.waitForTimeout(120);
}

await page.click('#btn-filter-confirm');
await settled('view-tray');

const wall = Date.now() - started;
const long = await page.evaluate(() => window.__long);
await browser.close();

const total = long.reduce((sum, ms) => sum + ms, 0);
const worst = long.length ? Math.max(...long) : 0;
console.log(`main-thread bench — ${BASE}`);
console.log(`  wall clock, import to page in tray   ${wall} ms`);
console.log(`  long tasks                           ${long.length}`);
console.log(`  total time the UI could not respond  ${total.toFixed(0)} ms`);
console.log(`  longest single freeze                ${worst.toFixed(0)} ms`);
