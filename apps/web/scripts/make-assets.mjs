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
// Writes into  apps/web/icons/ , ../../../opendocscan-website-deploy/ , and the
// Flutter app's Android mipmaps and iOS AppIcon set. Four surfaces, one mark —
// which is the whole point: a home-screen icon drawn separately from the
// browser tab icon is two different pictures of the same product.
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
const FLUTTER = join(WEB_DIR, '..', '..', 'app');
const ANDROID_RES = join(FLUTTER, 'android', 'app', 'src', 'main', 'res');
const IOS_ICONS = join(
  FLUTTER, 'ios', 'Runner', 'Assets.xcassets', 'AppIcon.appiconset');

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

// An adaptive icon is two layers, and Android composites them itself. The
// foreground is the glyph on transparency inside the guaranteed-visible middle
// (72 of 108 units); the background is a flat colour in XML. Shipping only a
// legacy `ic_launcher` instead is what puts a square tile inside the
// launcher's circular mask, which is how this looked next to every other app
// on the first attempt.
const foregroundSvg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512" role="img" aria-label="OpenDocScan">
  <g transform="translate(256 256) scale(0.60) translate(-256 -256)">
${glyph}
  </g>
</svg>
`;

// ---------------------------------------------------------------- rendering

async function png(page, svg, size, { opaque = false } = {}) {
  await page.setViewportSize({ width: size, height: size });
  const ground = opaque ? GROUND : 'transparent';
  await page.setContent(
    `<style>html,body{margin:0;padding:0;background:${ground}}svg{display:block;width:${size}px;height:${size}px}</style>${svg}`,
  );
  // App Store Connect rejects a 1024 marketing icon that carries an alpha
  // channel, so the iOS set is flattened onto the mark's own ground.
  return page.screenshot({ omitBackground: !opaque });
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

// --- the phone app ---------------------------------------------------------

// Android launcher icons. Square and round both point at the same drawing;
// the round one is masked by the launcher, and the mark's ground already
// bleeds to the edge, so it survives the crop.
const ANDROID_MIPMAPS = {
  'mipmap-mdpi': 48,
  'mipmap-hdpi': 72,
  'mipmap-xhdpi': 96,
  'mipmap-xxhdpi': 144,
  'mipmap-xxxhdpi': 192,
};
for (const [dir, size] of Object.entries(ANDROID_MIPMAPS)) {
  const target = join(ANDROID_RES, dir);
  mkdirSync(target, { recursive: true });
  // The legacy icon, for launchers older than API 26.
  writeFileSync(join(target, 'ic_launcher.png'), await png(page, maskableSvg, size));
  // The adaptive foreground. 108/48 times the nominal size, because an
  // adaptive icon is authored at 108dp for a 48dp slot.
  writeFileSync(
    join(target, 'ic_launcher_foreground.png'),
    await png(page, foregroundSvg, Math.round((size * 108) / 48)),
  );
}
written.push('and   mipmap-*/ic_launcher{,_foreground}.png'.padEnd(34) +
  `${Object.keys(ANDROID_MIPMAPS).length} densities`);

// The two-layer declaration, and the ground as a colour rather than a bitmap.
const anydpi = join(ANDROID_RES, 'mipmap-anydpi-v26');
mkdirSync(anydpi, { recursive: true });
const adaptive = `<?xml version="1.0" encoding="utf-8"?>
<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android">
    <background android:drawable="@color/ic_launcher_background" />
    <foreground android:drawable="@mipmap/ic_launcher_foreground" />
    <monochrome android:drawable="@mipmap/ic_launcher_foreground" />
</adaptive-icon>
`;
writeFileSync(join(anydpi, 'ic_launcher.xml'), adaptive);
writeFileSync(join(anydpi, 'ic_launcher_round.xml'), adaptive);

const values = join(ANDROID_RES, 'values');
mkdirSync(values, { recursive: true });
writeFileSync(
  join(values, 'ic_launcher_background.xml'),
  `<?xml version="1.0" encoding="utf-8"?>
<resources>
    <!-- The family tile ground. Generated by apps/web/scripts/make-assets.mjs;
         edit the mark there, not here. -->
    <color name="ic_launcher_background">${GROUND}</color>
</resources>
`,
);
written.push('and   mipmap-anydpi-v26 + values'.padEnd(34) + 'adaptive icon');

// iOS. The filenames are fixed by Contents.json, which Xcode reads literally —
// a missing one is a build warning and a blank icon on the home screen, and
// the 1024 marketing icon must have no alpha channel or App Store Connect
// rejects the upload.
const IOS_SIZES = [
  ['Icon-App-20x20@1x.png', 20], ['Icon-App-20x20@2x.png', 40],
  ['Icon-App-20x20@3x.png', 60], ['Icon-App-29x29@1x.png', 29],
  ['Icon-App-29x29@2x.png', 58], ['Icon-App-29x29@3x.png', 87],
  ['Icon-App-40x40@1x.png', 40], ['Icon-App-40x40@2x.png', 80],
  ['Icon-App-40x40@3x.png', 120], ['Icon-App-60x60@2x.png', 120],
  ['Icon-App-60x60@3x.png', 180], ['Icon-App-76x76@1x.png', 76],
  ['Icon-App-76x76@2x.png', 152], ['Icon-App-83.5x83.5@2x.png', 167],
  ['Icon-App-1024x1024@1x.png', 1024],
];
for (const [name, size] of IOS_SIZES) {
  // iOS applies its own corner mask, so it gets the square-ground drawing
  // rather than the rounded tile — a rounded icon inside iOS's mask shows a
  // pale halo at the corners.
  const data = await png(page, maskableSvg, size, { opaque: true });
  writeFileSync(join(IOS_ICONS, name), data);
}
written.push(`ios   AppIcon.appiconset`.padEnd(34) + `${IOS_SIZES.length} sizes`);

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
