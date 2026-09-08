// One module, one mark.
//
// The app's icons and the website's icons are generated here, from the same
// two SVG sources, so the browser tab and the home-screen icon cannot become
// two different pictures of the same product. The link-preview card is
// generated here too, because it is the product's face in every chat app and a
// second palette there drifts from the site it advertises.
//
//   node scripts/make-assets.mjs
//
// Writes into  apps/web/icons/  and  ../../../opendocscan-website-deploy/ .
//
// The SVGs hardcode their colours. That is not a lapse: an <img>-loaded SVG can
// read neither a webfont nor a custom property, so an export context has to
// carry literals. They are the literals the tokens resolve to — brass is accent
// slot 20 lifted 32% toward white, which is the same value site.css uses for
// the wordmark's dot, so the mark and the wordmark agree.

import { chromium } from 'playwright';
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const WEB_DIR = dirname(dirname(fileURLToPath(import.meta.url)));
const APP_ICONS = join(WEB_DIR, 'icons');
const SITE = join(WEB_DIR, '..', '..', '..', 'opendocscan-website-deploy');

const GROUND = '#111111'; // the family tile ground, as on every sibling mark
const PAPER = '#f8f8f7'; // --gray-050
const RULE = '#989898'; // --gray-400
const BRASS = '#9a976d'; // --brass lifted 32% to white: 6.3:1 on the ground

// A page seen at the angle a camera sees it, inside the brackets that find it.
// Two ideas, because a third dissolves at 16px.
const glyph = `
  <path d="M150 128 L360 158 L340 386 L138 356 Z" fill="${PAPER}"/>
  <g stroke="${RULE}" stroke-width="14" stroke-linecap="round">
    <path d="M180 196 L322 216"/>
    <path d="M176 244 L318 264"/>
    <path d="M172 292 L268 306"/>
  </g>
  <g stroke="${BRASS}" stroke-width="18" stroke-linecap="round" fill="none">
    <path d="M104 156 L104 108 L152 108"/>
    <path d="M360 108 L408 108 L408 156"/>
    <path d="M408 356 L408 404 L360 404"/>
    <path d="M152 404 L104 404 L104 356"/>
  </g>`;

const iconSvg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512" role="img" aria-label="OpenDocScan">
  <rect width="512" height="512" rx="112" fill="${GROUND}"/>
${glyph}
</svg>
`;

// Launchers crop a maskable icon to any shape, so everything that must survive
// sits inside the middle 80% and the ground bleeds to the edge.
const maskableSvg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512" role="img" aria-label="OpenDocScan">
  <rect width="512" height="512" fill="${GROUND}"/>
  <g transform="translate(256 256) scale(0.68) translate(-256 -256)">
${glyph}
  </g>
</svg>
`;

// ---------------------------------------------------------------- rendering

async function png(page, svg, size) {
  await page.setViewportSize({ width: size, height: size });
  await page.setContent(
    `<style>html,body{margin:0;padding:0;background:transparent}svg{display:block;width:${size}px;height:${size}px}</style>${svg}`,
  );
  return page.screenshot({ omitBackground: true });
}

// An ICO is a header, one directory entry per image, then the images. Modern
// decoders accept PNG payloads verbatim, so nothing has to be re-encoded. The
// one subtlety in the whole format: a 256px entry is written as 0.
function ico(images) {
  const header = Buffer.alloc(6);
  header.writeUInt16LE(0, 0);
  header.writeUInt16LE(1, 2); // 1 = icon
  header.writeUInt16LE(images.length, 4);

  const entries = [];
  let offset = 6 + images.length * 16;
  for (const { size, data } of images) {
    const e = Buffer.alloc(16);
    e.writeUInt8(size >= 256 ? 0 : size, 0);
    e.writeUInt8(size >= 256 ? 0 : size, 1);
    e.writeUInt8(0, 2); // palette
    e.writeUInt8(0, 3); // reserved
    e.writeUInt16LE(1, 4); // colour planes
    e.writeUInt16LE(32, 6); // bits per pixel
    e.writeUInt32LE(data.length, 8);
    e.writeUInt32LE(offset, 12);
    entries.push(e);
    offset += data.length;
  }
  return Buffer.concat([header, ...entries, ...images.map((i) => i.data)]);
}

// The card is type, and type from pixel math is a bitmap font that looks like
// 1998. Author it as HTML against the real vendored stylesheets and photograph
// it. Note the goto(): page.setContent renders on about:blank, which cannot
// load a file:// stylesheet, and the card comes out unstyled on white.
const CARD = `<!doctype html>
<html class="oa-dark">
<head>
<meta charset="utf-8" />
<link rel="stylesheet" href="tokens/css/colors.css" />
<link rel="stylesheet" href="tokens/css/typography.css" />
<link rel="stylesheet" href="tokens/css/spacing.css" />
<link rel="stylesheet" href="tokens/css/radius.css" />
<link rel="stylesheet" href="tokens/css/fonts.css" />
<style>
  :root { --accent: var(--app-docscan); --accent-lift: color-mix(in oklab, var(--accent) 68%, var(--white)); }
  * { box-sizing: border-box; }
  html, body { margin: 0; padding: 0; }
  body {
    width: 1200px; height: 630px;
    background: var(--bg-page);
    color: var(--text-body);
    font-family: var(--font-sans);
    display: flex; flex-direction: column; justify-content: space-between;
    padding: 72px 80px;
    -webkit-font-smoothing: antialiased;
  }
  .brand { display: flex; align-items: center; gap: 18px;
    font: var(--weight-medium) 34px/1 var(--font-display);
    letter-spacing: var(--logo-tracking); color: var(--text-strong); }
  .brand img { width: 52px; height: 52px; border-radius: 12px; display: block; }
  .brand .prefix { color: var(--text-muted); }
  .brand .dot { color: var(--accent-lift); }
  h1 { font: var(--weight-medium) 76px/1.04 var(--font-display);
       letter-spacing: var(--tracking-display); color: var(--text-strong);
       margin: 0 0 22px; max-width: 20ch; }
  p { font-size: 27px; line-height: 1.45; color: var(--text-muted); margin: 0; max-width: 44ch; }
  .foot { display: flex; gap: 40px; font-family: var(--font-mono); font-size: 20px;
          letter-spacing: var(--tracking-caps); text-transform: uppercase; color: var(--text-faint); }
  .foot b { color: var(--accent-lift); font-weight: var(--weight-medium); }
</style>
</head>
<body>
  <div class="brand">
    <img src="favicon.svg" alt="" />
    <span><span class="prefix">Open</span>DocScan<span class="dot">.</span></span>
  </div>
  <div>
    <h1>Photograph a document. Get a real scan.</h1>
    <p>Straightened, cleaned and searchable — entirely inside your browser tab.</p>
  </div>
  <div class="foot">
    <span><b>0</b> uploads</span>
    <span><b>0</b> accounts</span>
    <span><b>138 KB</b> core</span>
  </div>
</body>
</html>
`;

// ---------------------------------------------------------------------- main

mkdirSync(APP_ICONS, { recursive: true });
mkdirSync(SITE, { recursive: true });

writeFileSync(join(APP_ICONS, 'icon.svg'), iconSvg);
writeFileSync(join(APP_ICONS, 'icon-maskable.svg'), maskableSvg);
writeFileSync(join(SITE, 'favicon.svg'), iconSvg);

const browser = await chromium.launch();
const page = await browser.newPage({ deviceScaleFactor: 1 });

const written = [];
const w = (dir, name, data) => {
  writeFileSync(join(dir, name), data);
  written.push(`${dir === SITE ? 'site' : 'app '}  ${name.padEnd(20)} ${String(data.length).padStart(7)} bytes`);
};

// The app ships the sizes a manifest and iOS ask for; the site ships those plus
// the small ones a bookmark bar and an older browser reach for.
for (const size of [180, 192, 512]) {
  w(APP_ICONS, `icon-${size}.png`, await png(page, iconSvg, size));
}
w(APP_ICONS, 'icon-maskable.png', await png(page, maskableSvg, 512));

for (const size of [16, 32, 48, 180, 192, 512]) {
  w(SITE, `icon-${size}.png`, await png(page, iconSvg, size));
}

w(
  SITE,
  'favicon.ico',
  ico([
    { size: 16, data: await png(page, iconSvg, 16) },
    { size: 32, data: await png(page, iconSvg, 32) },
    { size: 48, data: await png(page, iconSvg, 48) },
  ]),
);

// Written beside its assets rather than set with setContent, so the vendored
// stylesheets and the favicon actually resolve.
const cardPath = join(SITE, '.og-card.html');
writeFileSync(cardPath, CARD);
await page.setViewportSize({ width: 1200, height: 630 });
await page.goto(`file://${cardPath}`);
await page.evaluate(() => document.fonts.ready);
w(SITE, 'og-image.png', await page.screenshot({ clip: { x: 0, y: 0, width: 1200, height: 630 } }));

await browser.close();
console.log(written.join('\n'));
