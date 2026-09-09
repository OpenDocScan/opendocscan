import { chromium } from 'playwright';
const b = await chromium.launch();
const ctx = await b.newContext();
const p = await ctx.newPage();

// First visit: registers the worker, but it is not controlling this page yet.
await p.goto('https://opendocscan.com/account', { waitUntil: 'networkidle' });
await p.evaluate(() => navigator.serviceWorker.ready);
const firstControlled = await p.evaluate(() => !!navigator.serviceWorker.controller);

// Second visit: now it is.
await p.reload({ waitUntil: 'networkidle' });
await p.waitForTimeout(1500);
const secondControlled = await p.evaluate(() => !!navigator.serviceWorker.controller);

const text = await p.evaluate(() => {
  const roots = [...document.querySelectorAll('*')].filter(e => e.shadowRoot).map(e => e.shadowRoot.textContent);
  return [document.body.innerText, ...roots].join(' ');
});
const broken = /could not reach the server|check your connection/i.test(text);

// And ask the worker directly what it does with an auth request from here.
const status = await p.evaluate(async () => {
  try { const r = await fetch('https://auth.opendocscan.com/healthz'); return r.status; }
  catch (e) { return 'threw: ' + e.message; }
});

console.log(`  controlled on first load: ${firstControlled}`);
console.log(`  controlled after reload : ${secondControlled}`);
console.log(`  auth fetch from /account: ${status}`);
console.log(`  page shows the error    : ${broken}`);
await b.close();
