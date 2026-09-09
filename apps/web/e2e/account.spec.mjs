// The account surface, which fails silently when it fails.
//
// Every check here has shipped broken in this suite at least once, and none of
// them announce themselves: the page renders, the buttons appear, and nobody
// is ever signed in. So they are assertions, not a manual checklist.
//
// Run with: node apps/web/e2e/account.spec.mjs   (a server must be on :8765)
//
// Nothing here drives a real Google or wallet sign-in. The identity provider is
// out of scope; everything either side of it is not.

import { chromium } from 'playwright';
import { readFileSync } from 'node:fs';
import { dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const WEB_DIR = dirname(dirname(fileURLToPath(import.meta.url)));
// This suite tests deployed configuration — CORS, the return_to allow-list, the
// masked hosts — so it runs against the real origin by default. A local server
// cannot answer these: its origin is not in `allowed_origins` and never should
// be, so a local run would fail CORS for a reason that says nothing about the
// product. Override for a staging host.
const BASE = process.env.BASE_URL ?? 'https://app.opendocscan.com';

const tests = [];
const test = (name, fn) => tests.push({ name, fn });

// Everything a visitor can actually read, shadow roots included. The account
// elements render their own headings inside a shadow DOM, so an assertion on
// `body.innerText` alone silently tests nothing about them.
const visibleText = (page) =>
  page.evaluate(() => {
    const roots = [...document.querySelectorAll('*')]
      .filter((el) => el.shadowRoot)
      .map((el) => el.shadowRoot.textContent);
    return [document.body.innerText, ...roots].join(' ');
  });

// The client's own module is the source of truth for both hostnames.
const module = readFileSync(`${WEB_DIR}/src/openapps.js`, 'utf8');
const AUTH = module.match(/OPENAPPS_BASE_URL = '([^']+)'/)[1];
const GATEWAY = module.match(/OPENAPPS_GATEWAY_URL = '([^']+)'/)[1];

// ---------------------------------------------------------------- the masking

test('both hostnames wear this product name, not the platform one', () => {
  for (const [label, url] of [['auth', AUTH], ['gateway', GATEWAY]]) {
    const host = new URL(url).hostname;
    if (!host.endsWith('.opendocscan.com')) {
      throw new Error(`${label} is ${host}, which is not this product's domain`);
    }
  }
});

test('the platform host is named in exactly one module, and not in it either', () => {
  // Masking `auth.` and leaving `gateway.` is the specific way this goes wrong,
  // and it costs one line to check.
  const files = ['src/app.js', 'src/scanner.js', 'src/ocr.js', 'src/store.js',
                 'index.html', 'account.html', 'sw.js'];
  const offenders = files.filter((f) =>
    /openapps\.network/.test(readFileSync(`${WEB_DIR}/${f}`, 'utf8')));
  if (offenders.length) {
    throw new Error(`the platform's domain appears in: ${offenders.join(', ')}`);
  }
});

test('the service worker allows exactly the host the client talks to', () => {
  // A worker cannot import the module, so the two are separate literals and
  // this is what keeps them honest.
  const sw = readFileSync(`${WEB_DIR}/sw.js`, 'utf8');
  const allowed = sw.match(/const ACCOUNT_ORIGIN = '([^']+)'/)[1];
  if (allowed !== AUTH) {
    throw new Error(`sw.js allows ${allowed}, the client uses ${AUTH}`);
  }
});

// ------------------------------------------------------------------ the hosts

test('both hosts answer over TLS and report healthy', async () => {
  for (const url of [AUTH, GATEWAY]) {
    const response = await fetch(`${url}/healthz`);
    const body = await response.json();
    if (body?.ok !== true) throw new Error(`${url}/healthz said ${JSON.stringify(body)}`);
  }
});

test('the auth host accepts its own /signin as a return_to', async () => {
  // The single check that catches a missing `auth.` entry in allowed_origins.
  // It is not a CORS entry — it is the allow-list return_to is validated
  // against — and without it the server rejects its own sign-in page with a
  // 400 that nothing in the product's UI would ever surface.
  const target = encodeURIComponent(`${AUTH}/signin`);
  const response = await fetch(
    `${AUTH}/v1/auth/oidc/google/start?return_to=${target}`,
    { redirect: 'manual' },
  );
  if (response.status !== 307) {
    throw new Error(`expected 307, got ${response.status} — check allowed_origins`);
  }
});

// ------------------------------------------------------------------- the page

test('the account page reaches the server rather than reporting it unreachable',
  async (page) => {
    await page.goto(`${BASE}/account.html`, { waitUntil: 'networkidle' });
    // "Could not reach the server" is what CORS looks like from the outside,
    // and it is the most common failure. Assert the error is absent rather
    // than that a button exists.
    //
    // Through `visibleText`, not `body.innerText`: the element renders that
    // error inside its own shadow root, so the first version of this check read
    // an empty string and passed while the page said it in red. A test that
    // cannot see the failure it is named for is worse than no test.
    const text = await visibleText(page);
    if (/could not reach the server|check your connection/i.test(text)) {
      throw new Error('the page reports the server unreachable — this is CORS');
    }
  });

test('the client actually holds the masked host at runtime', async (page) => {
  // Asserting on the constant would pass even if configure() never ran.
  const base = await page.evaluate(async () => {
    const mod = await import('./vendor/openapps/openapps-ui.js');
    return mod.getClient()?.baseUrl ?? null;
  });
  if (base !== AUTH) throw new Error(`client.baseUrl is ${base}, expected ${AUTH}`);
});

test('nothing a visitor can read names the platform, shadow roots included',
  async (page) => {
    // The string that bites lives inside the login element's own shadow DOM,
    // not in any file this app wrote, so grepping the source would miss it.
    const visible = await visibleText(page);
    if (/openapps/i.test(visible)) {
      throw new Error('the platform name is visible on the account page');
    }
  });

test('the sign-in panel carries the product glyph, not a placeholder letter',
  async (page) => {
    const mark = await page.evaluate(
      () => document.querySelector('openapps-login')?.getAttribute('mark'));
    if (!mark || /^[A-Za-z]$/.test(mark) || mark === 'O') {
      throw new Error(`mark is ${JSON.stringify(mark)} — a letter reads as a placeholder`);
    }
    // A glyph the font cannot paint is a tofu box. Compare against a codepoint
    // guaranteed to be missing.
    const isTofu = await page.evaluate((glyph) => {
      const measure = (text) => {
        const span = document.createElement('span');
        span.style.cssText = 'position:absolute;visibility:hidden;font-size:64px';
        span.textContent = text;
        document.body.append(span);
        const width = span.getBoundingClientRect().width;
        span.remove();
        return width;
      };
      return measure(glyph) === measure('￿');
    }, mark);
    if (isTofu) throw new Error(`the mark ${mark} renders as tofu in this font`);
  });

test('signed out, the page offers sign-in and shows no balance', async (page) => {
  const text = await visibleText(page);
  if (!/sign in to opendocscan/i.test(text)) {
    throw new Error(
      `the sign-in heading is missing; the page reads: ${text.slice(0, 160).replace(/\s+/g, ' ')}`);
  }
  // A balance of 0 rendered signed out reads as a real balance of zero.
  const balanceMounted = await page.evaluate(
    () => !document.getElementById('signed-in')?.hidden);
  if (balanceMounted) throw new Error('the balance is mounted while signed out');
});

test('the page says an account is optional, because it is', async (page) => {
  const text = await visibleText(page);
  if (!/optional/i.test(text) || !/works signed out|nothing here is behind it/i.test(text)) {
    throw new Error('the page does not say scanning works without an account');
  }
});

// ------------------------------------------------------- the entry point

test('the account control is in the header, on the right, and leaves the app',
  async (page) => {
    await page.goto(`${BASE}/`, { waitUntil: 'networkidle' });
    const box = await page.evaluate(() => {
      const el = document.getElementById('btn-account');
      if (!el) return null;
      const r = el.getBoundingClientRect();
      return { x: r.x, width: r.width, top: r.top, href: el.getAttribute('href'),
               label: el.getAttribute('aria-label'), title: el.getAttribute('title') };
    });
    if (!box) throw new Error('no account control in the header');
    if (box.x + box.width / 2 < window_width(page) / 2) {
      throw new Error('the account control is not on the right half');
    }
    if (box.top > 120) throw new Error('the account control is not near the top');
    if (!/account/.test(box.href)) throw new Error(`it points at ${box.href}`);
    if (box.label !== 'Account' || box.title !== 'Account') {
      throw new Error('aria-label and title must both read "Account"');
    }
  });

test('no footer offers the account instead', async (page) => {
  const inFooter = await page.evaluate(() =>
    [...document.querySelectorAll('footer a')].some((a) => /account/i.test(a.href)));
  if (inFooter) throw new Error('the account is linked from a footer');
});

// ------------------------------------------------ the scanner stays sealed

test('the scanner still cannot reach the account host', async (page) => {
  // The exemption is scoped to the account page. If it were origin-wide the
  // scanner could talk to a server, which is the thing the worker exists to
  // prevent.
  await page.goto(`${BASE}/`, { waitUntil: 'networkidle' });
  await page.evaluate(() => navigator.serviceWorker.ready);
  const status = await page.evaluate(async (auth) => {
    try {
      const response = await fetch(`${auth}/healthz`);
      return response.status;
    } catch {
      return 'threw';
    }
  }, AUTH);
  if (status !== 403) {
    throw new Error(`the scanner reached the account host (${status}), expected 403`);
  }
});

// ------------------------------------------------------------------- runner

let width = 1280;
function window_width() { return width; }

const browser = await chromium.launch({ args: ['--no-sandbox'] });
const context = await browser.newContext({ viewport: { width, height: 900 } });
const page = await context.newPage();
await page.goto(`${BASE}/account.html`, { waitUntil: 'networkidle' });

let failures = 0;
for (const { name, fn } of tests) {
  try {
    await fn(page);
    console.log(`  ok   ${name}`);
  } catch (error) {
    failures += 1;
    console.log(`  FAIL ${name}`);
    console.log(`       ${error.message}`);
  }
}

await browser.close();
console.log(failures === 0 ? '\nall green' : `\n${failures} failing`);
process.exit(failures === 0 ? 0 : 1);
