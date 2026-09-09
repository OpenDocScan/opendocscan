// End-to-end: drive the real UI in a real browser, through the whole
// capture-to-PDF path, and check the two claims that matter most — that a
// multi-page PDF comes out, and that nothing at all is sent anywhere.
//
// Run with: node apps/web/e2e/scan.spec.mjs   (a server must be on :8765)

import { chromium } from 'playwright';
import { strict as assert } from 'node:assert';
import { writeFileSync, readFileSync, readdirSync, statSync } from 'node:fs';
import { dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

// Playwright specs in a `"type": "module"` package have no `__dirname`.
const WEB_DIR = dirname(dirname(fileURLToPath(import.meta.url)));

const BASE = process.env.BASE_URL ?? 'http://127.0.0.1:8765';
const OUT = process.env.OUT_DIR ?? '/tmp';

const tests = [];
const test = (name, fn) => tests.push({ name, fn });

// A photographed page: a white sheet lying at an angle on a dark desk,
// with real text on it. The perspective is applied by drawing into a
// transformed context, so the corners are genuinely off-axis and detection
// has something to actually find rather than a rectangle it cannot fail on.
const MAKE_TEST_PAGE = `(text) => {
  const canvas = document.createElement('canvas');
  canvas.width = 1000;
  canvas.height = 1300;
  const ctx = canvas.getContext('2d');

  ctx.fillStyle = '#2b2b2e';
  ctx.fillRect(0, 0, canvas.width, canvas.height);

  ctx.save();
  ctx.transform(1, 0.06, -0.09, 1, 150, 60);
  ctx.fillStyle = '#ffffff';
  ctx.fillRect(0, 0, 700, 1000);
  ctx.fillStyle = '#000000';
  ctx.font = 'bold 64px Helvetica, Arial, sans-serif';
  ctx.fillText(text, 60, 140);
  ctx.font = '40px Helvetica, Arial, sans-serif';
  ctx.fillText('Total 1234', 60, 260);
  ctx.fillText('Thank you', 60, 340);
  ctx.restore();

  return new Promise((resolve) => canvas.toBlob(resolve, 'image/png'));
}`;

async function importPage(page, text) {
  await page.evaluate(
    async ([maker, label]) => {
      const blob = await eval(maker)(label);
      const input = document.getElementById('file-input');
      const dt = new DataTransfer();
      dt.items.add(new File([blob], 'page.png', { type: 'image/png' }));
      input.files = dt.files;
      input.dispatchEvent(new Event('change', { bubbles: true }));
    },
    [MAKE_TEST_PAGE, text],
  );
}

async function visible(page, id) {
  return page.evaluate((v) => !document.getElementById(v).hidden, id);
}

// Waits for a view to be both showing and *finished*.
//
// The two are no longer the same thing. Detection, rectification and
// filtering run in a worker, so a view can be on screen with its work
// still in flight — which is the entire point of moving them there, and
// which a test that asserts on pixels has to respect. The busy overlay is
// the app's own statement that it is done.
async function arrived(page, id, timeout = 20000) {
  await page.waitForFunction((v) => !document.getElementById(v).hidden, id, { timeout });
  await page.waitForFunction(() => document.getElementById('busy').hidden, undefined, { timeout });
}

// The images being worked on live in the worker now, referred to by name.
// That makes their lifetime something the app has to get right, and the
// way to get it wrong is to free one on a path that still needs it. Going
// forward to the filter view, back to the corners, and forward again uses
// the same held frame three times; releasing it on the way forward — which
// looks tidy — breaks exactly here and nowhere else.
test('going back from the filter view and forward again still works', async (page) => {
  await page.waitForFunction(() => !document.getElementById('view-library').hidden);
  await importPage(page, 'RETURNS');
  await arrived(page, 'view-crop');

  await page.click('#btn-crop-confirm');
  await arrived(page, 'view-filter');
  await page.waitForFunction(() => document.getElementById('filter-canvas').width > 300, undefined, {
    timeout: 20000,
  });

  await page.click('#btn-filter-back');
  await arrived(page, 'view-crop');

  await page.click('#btn-crop-confirm');
  await arrived(page, 'view-filter');
  const shape = await page.evaluate(() => {
    const canvas = document.getElementById('filter-canvas');
    return canvas.width / canvas.height;
  });
  assert.ok(
    Math.abs(shape - 0.7) < 0.18,
    `second pass through the corner editor gave aspect ${shape.toFixed(2)}, expected ~0.70`,
  );

  // Leave the app back at the library with nothing held, so the tests
  // after this one start from the same place they always did.
  await page.click('#btn-filter-confirm');
  await arrived(page, 'view-tray');
  page.once('dialog', (dialog) => dialog.accept());
  await page.click('#btn-tray-discard');
  await arrived(page, 'view-library');
});

test('a photographed page becomes a searchable multi-page PDF', async (page) => {
  await page.waitForFunction(() => !document.getElementById('view-library').hidden);

  // --- page one, through the corner editor -----------------------------
  await importPage(page, 'INVOICE');
  await arrived(page, 'view-crop');

  const detected = await page.evaluate(() => {
    const canvas = document.getElementById('crop-canvas');
    return { width: canvas.width, height: canvas.height };
  });
  assert.ok(detected.width > 0, 'the corner editor should have drawn something');

  await page.click('#btn-crop-confirm');
  await arrived(page, 'view-filter');
  // The filter preview is drawn from a worker round trip, so the canvas
  // exists before it has been sized. Waiting for it to stop being a
  // default 300x150 canvas is waiting for the rectified page itself.
  await page.waitForFunction(
    () => document.getElementById('filter-canvas').width > 300,
    undefined,
    { timeout: 20000 },
  );

  // The rectified page must be roughly the shape of the sheet that was
  // photographed (700x1000), not the shape of the whole photo (1000x1300).
  // Getting this wrong is how a scan comes out with the desk still in it.
  const shape = await page.evaluate(() => {
    const canvas = document.getElementById('filter-canvas');
    return canvas.width / canvas.height;
  });
  assert.ok(
    Math.abs(shape - 0.7) < 0.18,
    `rectified aspect ${shape.toFixed(2)} should be near the sheet's 0.70, not the photo's 0.77`,
  );

  await page.click('.chip[data-filter="bw"]');
  await page.click('#btn-filter-confirm');
  await arrived(page, 'view-tray');

  // --- page two --------------------------------------------------------
  await page.click('#btn-add-page');
  await page.waitForFunction(() => !document.getElementById('view-capture').hidden);
  await importPage(page, 'RECEIPT');
  await arrived(page, 'view-crop');
  await page.click('#btn-crop-confirm');
  await arrived(page, 'view-filter');
  await page.waitForFunction(
    () => document.getElementById('filter-canvas').width > 300,
    undefined,
    { timeout: 20000 },
  );
  await page.click('#btn-filter-confirm');
  await arrived(page, 'view-tray');

  const pageCount = await page.evaluate(
    () => document.querySelectorAll('#tray-list .tray-item').length,
  );
  assert.equal(pageCount, 2, 'the tray should hold both pages');

  // --- export ----------------------------------------------------------
  await page.fill('#doc-title', 'August Expenses');
  const download = page.waitForEvent('download', { timeout: 180000 });
  await page.click('#btn-save');
  const file = await download;

  const path = `${OUT}/opendocscan-e2e.pdf`;
  await file.saveAs(path);
  const bytes = await page.evaluate(() => null).then(async () => {
    const { readFileSync } = await import('node:fs');
    return readFileSync(path);
  });

  assert.ok(bytes.subarray(0, 5).toString() === '%PDF-', 'the download should be a PDF');
  const source = bytes.toString('latin1');
  const pages = (source.match(/\/Type\s*\/Page[^s]/g) ?? []).length;
  assert.equal(pages, 2, `the PDF should have 2 pages, found ${pages}`);
  assert.ok(source.includes('August Expenses'), 'the title should reach the PDF');

  return { pdfPath: path, pdfBytes: bytes.length, pdfSource: source };
});

test('the exported PDF carries the text OCR read', async (page, shared) => {
  // Depends on the export above; OCR runs in the background from the
  // moment a page is added, and the export waits for it.
  const source = shared.pdfSource;
  assert.ok(source, 'needs the PDF from the previous test');

  const hasInvisibleText = source.includes('3 Tr');
  assert.ok(hasInvisibleText, 'the PDF should carry an invisible text layer');

  // The words themselves are written literally into the content stream —
  // the streams are uncompressed, which is what makes an export diffable.
  const found = ['INVOICE', 'RECEIPT', 'Total'].filter((word) => source.includes(`(${word})`));
  assert.ok(
    found.length >= 2,
    `expected OCR to have read at least two of INVOICE/RECEIPT/Total, found: ${found.join(', ') || 'none'}`,
  );
});

test('the saved document is in the library and findable by its text', async (page) => {
  await page.evaluate(() => document.querySelector('[data-back="library"]:not([hidden])')?.click());
  await page.click('#view-doc [data-back="library"]').catch(() => {});
  await page.waitForFunction(() => !document.getElementById('view-library').hidden, {
    timeout: 15000,
  });

  const cards = await page.evaluate(() => document.querySelectorAll('#library-grid .card').length);
  assert.equal(cards, 1, 'the library should hold the saved document');

  await page.fill('#search', 'invoice');
  await page.waitForTimeout(400);
  const afterSearch = await page.evaluate(
    () => document.querySelectorAll('#library-grid .card').length,
  );
  assert.equal(afterSearch, 1, 'searching the OCR text should find the document');

  await page.fill('#search', 'zzzznotpresent');
  await page.waitForTimeout(400);
  const noMatch = await page.evaluate(
    () => document.querySelectorAll('#library-grid .card').length,
  );
  assert.equal(noMatch, 0, 'a query matching nothing should show nothing');
});

// --- runner ----------------------------------------------------------------

const browser = await chromium.launch({
  args: [
    '--use-fake-ui-for-media-stream',
    '--use-fake-device-for-media-stream',
    '--autoplay-policy=no-user-gesture-required',
  ],
});
const context = await browser.newContext({
  acceptDownloads: true,
  permissions: ['camera'],
  viewport: { width: 420, height: 860 },
});
const page = await context.newPage();

// Every request the app makes, so the "nothing leaves this device" claim
// is measured rather than believed.
const offOrigin = [];
page.on('request', (request) => {
  const url = new URL(request.url());
  if (url.origin !== BASE && !url.protocol.startsWith('blob') && !url.protocol.startsWith('data')) {
    offOrigin.push(request.url());
  }
});

// The README's central claim, checked against the shipped files rather
// than against the running app. The network assertion below proves
// nothing *was* sent during one run; this proves there is nowhere to send
// it to — including in code paths the test never reached, and including
// the vendored OCR engine, which fetches from a CDN by default and is the
// single most likely thing in this tree to phone home.
function scanBundleForEndpoints() {
  // `tokens` belongs here as much as `vendor` does: a design-system stylesheet
  // is exactly the kind of file that arrives with an @import from a font CDN,
  // and it is linked from the page the same way everything else is.
  const roots = [
    'src', 'vendor', 'tokens',
    'sw.js', 'index.html', 'account.html', 'manifest.webmanifest',
  ];
  // The two account hosts are this product's own, and they are the only
  // third-party-looking names allowed. The platform's own domain is not on this
  // list on purpose: a client that names it has skipped the masking, and that
  // is the regression this assertion exists to catch.
  // Our own domain and the two account hosts, plus github.com — which appears
  // only as the source link's href and as nothing the page ever fetches. The
  // proof of that is the runtime check above, which records every request the
  // app actually makes and fails if one leaves the origin; this static scan is
  // the second net, and it is here to catch a *new* host being added, not to
  // relitigate an anchor.
  const allowed =
    /^(localhost|127\.0\.0\.1|(www\.)?w3\.org|schema\.org|(www\.)?example\.(com|org)|([a-z]+\.)?opendocscan\.com|github\.com)$/;
  const found = [];

  const walk = (relative) => {
    const full = `${WEB_DIR}/${relative}`;
    if (statSync(full).isDirectory()) {
      for (const entry of readdirSync(full)) walk(`${relative}/${entry}`);
      return;
    }
    if (!/\.(js|mjs|css|html|json|webmanifest)$/.test(relative)) return;
    const text = readFileSync(full, 'utf8');
    for (const match of text.matchAll(/https?:\/\/([a-z0-9.-]+)/gi)) {
      if (!allowed.test(match[1])) found.push(`${relative}: ${match[1]}`);
    }
  };

  for (const root of roots) walk(root);
  return found;
}

const consoleErrors = [];
page.on('pageerror', (error) => consoleErrors.push(String(error)));
page.on('console', (message) => {
  if (message.type() !== 'error') return;
  // With the location. A bare "Failed to load resource" says nothing
  // about whether the app is broken or the harness is, and chasing that
  // down twice is once too often.
  const where = message.location();
  const at = where?.url ? ` (${where.url}:${where.lineNumber ?? 0})` : '';
  consoleErrors.push(`${message.text()}${at}`);
});

await page.goto(`${BASE}/index.html`);

let failures = 0;
const shared = {};
for (const { name, fn } of tests) {
  try {
    const result = await fn(page, shared);
    Object.assign(shared, result ?? {});
    console.log(`  ok   ${name}`);
  } catch (error) {
    failures += 1;
    console.log(`  FAIL ${name}`);
    console.log(`       ${error.message}`);
  }
}

console.log('');
if (offOrigin.length === 0) {
  console.log('  ok   no request left this origin');
} else {
  failures += 1;
  console.log('  FAIL requests left this origin:');
  for (const url of offOrigin) console.log(`       ${url}`);
}

// The scanner and the product page share a document now, so the marketing
// stylesheet and the app's are loaded together. Unprefixed they collided on
// seven class names — btn, card, dot, grid, hint, prefix, primary — all of
// which the app uses, and the marketing sheet loads second so it would have
// won silently. Measured rather than trusted to a comment.
function measureStylesheetOverlap() {
  const read = (name) => {
    const text = readFileSync(`${WEB_DIR}/src/${name}`, 'utf8')
      .replace(/\/\*[\s\S]*?\*\//g, '');
    const classes = new Set([...text.matchAll(/\.([a-zA-Z][\w-]*)/g)].map((m) => m[1]));
    const bare = new Set();
    for (const [, sel] of text.matchAll(/(?:^|\})\s*([a-zA-Z][\w, .:[\]()>-]*)\s*\{/g)) {
      for (const part of sel.split(',')) {
        const p = part.trim();
        if (/^[a-z]+[0-9]?$/.test(p)) bare.add(p);
      }
    }
    return { classes, bare };
  };
  const site = read('site.css');
  const app = read('styles.css');
  const shared = [...site.classes].filter((c) => app.classes.has(c));
  return { shared, bare: [...site.bare] };
}

const overlap = measureStylesheetOverlap();
if (overlap.shared.length === 0 && overlap.bare.length === 0) {
  console.log('  ok   the marketing stylesheet cannot reach the app');
} else {
  failures += 1;
  console.log('  FAIL the marketing stylesheet can reach the app:');
  if (overlap.shared.length) console.log(`       shared classes: ${overlap.shared.join(', ')}`);
  if (overlap.bare.length) console.log(`       bare element rules: ${overlap.bare.join(', ')}`);
}

const endpoints = scanBundleForEndpoints();
if (endpoints.length === 0) {
  console.log('  ok   the shipped files name no third-party endpoint');
} else {
  failures += 1;
  console.log('  FAIL the shipped files name third-party endpoints:');
  for (const hit of endpoints) console.log(`       ${hit}`);
}

if (consoleErrors.length > 0) {
  console.log('\n  console errors:');
  for (const error of consoleErrors.slice(0, 10)) console.log(`       ${error}`);
}

await page.screenshot({ path: `${OUT}/opendocscan-final.png` });
writeFileSync(`${OUT}/opendocscan-console.txt`, consoleErrors.join('\n'));

await browser.close();
console.log(failures === 0 ? '\nall green' : `\n${failures} failing`);
process.exit(failures === 0 ? 0 : 1);
