// Photographs the real app, driving the real pipeline, on real documents.
//
// Never a mockup. Every screenshot below is the shipped page holding
// output that the shipped core produced from a file on disk, which is the
// only kind of picture worth putting in front of someone deciding whether
// to trust the thing.
//
//   ./scripts/make-fixtures.sh          # renders a publishable document
//   node apps/web/e2e/capture.mjs <output-dir> [fixture-dir]
//
// The fixture directory must carry a PUBLISHABLE marker, and this refuses to
// run without one. That is not ceremony. The most useful documents to test
// this app on are the ones already on the machine, and on this machine those
// are a CV, two invoices, a bank statement, a company deck and a customer's
// furniture drawing with their name and address in the title block. A capture
// run defaulting to that directory put the customer's drawing into a published
// document once already; it was caught by opening the PNG, which is not a
// mechanism. `scripts/make-fixtures.sh` writes the marker, so the guard can
// only be satisfied by rendering a document chosen to be published.

import { chromium } from 'playwright';
import { readdirSync, readFileSync, mkdirSync, existsSync } from 'node:fs';
import { basename, join } from 'node:path';

const BASE = process.env.BASE_URL ?? 'http://127.0.0.1:8765';
const OUT = process.argv[2] ?? 'docs/screenshots';
const FIXTURES = process.argv[3] ?? '/tmp/docscan-public';

if (!existsSync(join(FIXTURES, 'PUBLISHABLE'))) {
  throw new Error(
    `${FIXTURES} has no PUBLISHABLE marker, so these screenshots cannot be published.\n` +
      'Run ./scripts/make-fixtures.sh to render a document that may be, or pass a\n' +
      'fixture directory that carries the marker.',
  );
}

mkdirSync(OUT, { recursive: true });

const files = readdirSync(FIXTURES)
  .filter((name) => /\.(png|jpg|jpeg)$/i.test(name))
  .sort();
if (files.length === 0) throw new Error(`no images in ${FIXTURES}`);

const asDataUri = (path) => `data:image/png;base64,${readFileSync(path).toString('base64')}`;

// Feeds a real file through the real file input, the way the browser
// would hand over one the user picked.
async function importFiles(page, paths) {
  await page.evaluate(
    async ([uris, names]) => {
      const dt = new DataTransfer();
      for (const [i, uri] of uris.entries()) {
        const blob = await (await fetch(uri)).blob();
        dt.items.add(new File([blob], names[i], { type: 'image/png' }));
      }
      const input = document.getElementById('file-input');
      input.files = dt.files;
      input.dispatchEvent(new Event('change', { bubbles: true }));
    },
    [paths.map(asDataUri), paths.map((p) => basename(p))],
  );
}

const arrived = async (page, id) => {
  await page.waitForFunction((v) => !document.getElementById(v).hidden, id, { timeout: 60000 });
  await page.waitForFunction(() => document.getElementById('busy').hidden, undefined, {
    timeout: 60000,
  });
};

const shot = async (page, name) => {
  // A toast covering the controls is the classic ruined screenshot, so
  // wait it out rather than photograph it — except where the toast *is*
  // the subject, which none of these are.
  await page
    .waitForFunction(() => document.getElementById('toast').hidden, undefined, { timeout: 5000 })
    .catch(() => {});
  // Settle, briefly, only after the content is already known to be there —
  // a fixed wait *instead* of a content check is how a capture script
  // ends up photographing a spinner on a slow machine.
  await page.waitForTimeout(180);
  await page.screenshot({ path: join(OUT, name) });
  console.log(`  ${name}`);
};

const browser = await chromium.launch();

for (const scheme of ['dark', 'light']) {
  const suffix = scheme === 'dark' ? '' : '-light';
  const context = await browser.newContext({
    // A phone, because that is what this app is for. The study it answers
    // found the whole category is mobile.
    viewport: { width: 430, height: 932 },
    deviceScaleFactor: 2,
    colorScheme: scheme,
    isMobile: true,
    hasTouch: true,
  });
  const page = await context.newPage();
  await page.goto(`${BASE}/index.html`);
  await page.waitForFunction(() => !document.getElementById('view-library').hidden);
  await page.waitForTimeout(1200);

  // 01 — the empty library: the first thing a new user sees.
  await shot(page, `01-empty${suffix}.png`);

  // 02 — the corner editor, on a real document.
  await importFiles(page, [join(FIXTURES, files[0])]);
  await arrived(page, 'view-crop');
  await shot(page, `02-corners${suffix}.png`);

  // 03 — the filter view, showing the rectified page.
  await page.click('#btn-crop-confirm');
  await arrived(page, 'view-filter');
  await page.waitForFunction(() => document.getElementById('filter-canvas').width > 300, undefined, {
    timeout: 60000,
  });
  await shot(page, `03-filter${suffix}.png`);

  // 04 — black and white, the filter that halves a scan's size.
  await page.click('.chip[data-filter="bw"]');
  await page.waitForTimeout(400);
  await shot(page, `04-bw${suffix}.png`);

  // 05 — the page tray, holding several real documents.
  await page.click('.chip[data-filter="enhance"]');
  await page.waitForTimeout(300);
  await page.click('#btn-filter-confirm');
  await arrived(page, 'view-tray');

  for (const name of files.slice(1, 5)) {
    await page.click('#btn-add-page');
    await page.waitForFunction(() => !document.getElementById('view-capture').hidden);
    await importFiles(page, [join(FIXTURES, name)]);
    await arrived(page, 'view-crop');
    await page.click('#btn-crop-confirm');
    await arrived(page, 'view-filter');
    await page.waitForFunction(
      () => document.getElementById('filter-canvas').width > 300,
      undefined,
      { timeout: 60000 },
    );
    await page.click('#btn-filter-confirm');
    await arrived(page, 'view-tray');
  }
  // Wait for the background OCR to finish on every page, so the tray
  // shows what it read rather than what it is still reading. Matched on
  // the badge's own text, which is what a reader of the screenshot sees.
  await page
    .waitForFunction(
      () =>
        document.querySelectorAll('#tray-list .tray-item').length > 0 &&
        ![...document.querySelectorAll('#tray-list .tray-item')].some((item) =>
          item.textContent.includes('reading'),
        ),
      undefined,
      { timeout: 300000 },
    )
    .catch(() => {});
  await page.waitForTimeout(800);
  await shot(page, `05-tray${suffix}.png`);

  // 06 — the saved document, which is where saving lands you.
  await page.fill('#doc-title', 'Reference Documents');
  const download = page.waitForEvent('download', { timeout: 240000 });
  await page.click('#btn-save');
  await download;
  await arrived(page, 'view-doc');
  await page.waitForTimeout(800);
  await shot(page, `06-document${suffix}.png`);

  // 07 — the library, now holding it.
  await page.click('#view-doc [data-back="library"]');
  await arrived(page, 'view-library');
  await page.waitForTimeout(600);
  await shot(page, `07-library${suffix}.png`);

  // 08 — search, which matches on the text OCR read, not on the title.
  await page.fill('#search', 'the');
  await page.waitForTimeout(800);
  await shot(page, `08-search${suffix}.png`);
  await page.fill('#search', '');
  await page.waitForTimeout(400);

  if (scheme === 'dark') {
    // 09 — the about sheet, which is where the privacy claim is made.
    await page.click('#btn-about');
    await page.waitForTimeout(600);
    await shot(page, '09-about.png');
    await page.keyboard.press('Escape').catch(() => {});
  }

  await context.close();
}

await browser.close();
console.log(`\nwrote screenshots to ${OUT}`);
