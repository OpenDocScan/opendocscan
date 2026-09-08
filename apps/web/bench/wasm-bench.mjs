// Times the core's operations where they actually run: inside a browser's
// WebAssembly engine, on the buffer shapes the app hands it.
//
// The Rust bench (`cargo run -p docscan-wasm --example bench --release`)
// is the right tool for comparing two implementations of a pass, because
// it is fast to iterate on and native code preserves the ratio. It is the
// wrong tool for choosing build flags, because the whole question there
// is what a different compiler backend does with the same source. So this
// exists, and it is the one that decides `opt-level` and SIMD.
//
// Run with:  node apps/web/bench/wasm-bench.mjs      (server on :8765)

import { chromium } from 'playwright';

const BASE = process.env.BASE_URL ?? 'http://127.0.0.1:8765';
const RUNS = Number(process.env.RUNS ?? 7);

// Kept in step with crates/docscan-wasm/examples/bench.rs, so a number
// here can be read against a number there.
const IN_PAGE = async ({ base, runs }) => {
  const wasm = await import(`${base}/src/wasm/docscan.js`);
  await wasm.default();

  const photo = (width, height) => {
    const data = new Uint8ClampedArray(width * height * 4);
    for (let y = 0; y < height; y += 1) {
      for (let x = 0; x < width; x += 1) {
        const sheet =
          x > width * 0.12 + y * 0.04 &&
          x < width * 0.88 + y * 0.04 &&
          y > height * 0.1 &&
          y < height * 0.9;
        let v;
        if (!sheet) v = 40 + ((x * 7 + y * 13) % 20);
        else if (y % 40 < 6 && x % 160 < 120) v = 60 + ((x * 3) % 30);
        else v = 200 + ((x + y) % 30);
        const i = (y * width + x) * 4;
        data[i] = v;
        data[i + 1] = v;
        data[i + 2] = Math.min(255, v + 4);
        data[i + 3] = 255;
      }
    }
    return data;
  };

  const time = (label, count, fn) => {
    fn();
    const start = performance.now();
    for (let i = 0; i < count; i += 1) fn();
    return { label, ms: (performance.now() - start) / count };
  };

  const preview = photo(640, 480);
  const capture = photo(3024, 4032);
  const page = photo(1800, 2400);
  const small = photo(825, 1100);
  const corners = new Float32Array([400, 420, 2650, 560, 2600, 3600, 360, 3480]);

  const results = [];
  results.push(time('detectQuad 640x480', runs * 3, () => wasm.detectQuad(preview, 640, 480)));
  results.push(time('detectQuad 3024x4032', runs, () => wasm.detectQuad(capture, 3024, 4032)));
  results.push(
    time('rectify 12MP -> 2400px', runs, () => {
      // Each call consumes its buffer, so each run needs its own copy —
      // exactly as the app does, where the pixels come from a canvas.
      wasm.rectify(capture.slice(), 3024, 4032, corners, 2400).intoData();
    }),
  );
  for (const filter of ['original', 'bw', 'enhance']) {
    results.push(
      time(`applyFilter ${filter} 1800x2400`, runs, () => {
        wasm.applyFilter(page.slice(), 1800, 2400, filter, 20).intoData();
      }),
    );
  }
  results.push(
    time('applyFilter enhance 825x1100', runs * 3, () => {
      wasm.applyFilter(small.slice(), 825, 1100, 'enhance', 10).intoData();
    }),
  );
  results.push(
    time('PdfBuilder 5 x 1800x2400 -> A4', Math.max(1, Math.round(runs / 2)), () => {
      const builder = new wasm.PdfBuilder('a4', 18, 82, undefined);
      for (let i = 0; i < 5; i += 1) builder.addPage(page.slice(), 1800, 2400, undefined);
      builder.finish();
    }),
  );

  const simd = WebAssembly.validate(
    new Uint8Array([
      0, 97, 115, 109, 1, 0, 0, 0, 1, 5, 1, 96, 0, 1, 123, 3, 2, 1, 0, 10, 10, 1, 8, 0, 65, 0,
      253, 15, 253, 98, 11,
    ]),
  );
  return { results, simd };
};

const browser = await chromium.launch();
const page = await browser.newPage();
page.on('console', (message) => {
  if (message.type() === 'error') console.error('  [page]', message.text());
});
// Any origin under the app is fine; the module is imported by URL.
await page.goto(`${BASE}/index.html`);
// The app's own service worker would serve a cached copy of the module,
// which is the wrong thing to measure when the point is to compare builds.
await page.evaluate(async () => {
  const registrations = await navigator.serviceWorker?.getRegistrations?.();
  await Promise.all((registrations ?? []).map((r) => r.unregister()));
});

const { results, simd } = await page.evaluate(IN_PAGE, { base: BASE, runs: RUNS });
await browser.close();

console.log(`wasm bench — ${BASE} — SIMD available in engine: ${simd}`);
for (const { label, ms } of results) {
  console.log(`${label.padEnd(38)} ${ms.toFixed(1).padStart(8)} ms`);
}
