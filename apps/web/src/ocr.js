// Offline text recognition.
//
// Every byte of the engine — worker, WASM core, and the English model —
// is served from this app's own origin (see apps/web/vendor/tesseract).
// tesseract.js would otherwise fetch its core and language data from a
// CDN on first use, which would mean an app promising that nothing leaves
// your device quietly telling a third party each time you scanned
// something. The paths below are not a convenience; they are the promise.
//
// The engine is ~6MB, so it is loaded on first use rather than at startup,
// and `precache` exists so a user can deliberately pull it down while they
// still have a connection instead of discovering the need offline.

const WORKER_PATH = '../vendor/tesseract/worker.min.js';
const CORE_PATH = '../vendor/tesseract/core';
const LANG_PATH = '../vendor/tesseract/lang';
const LANG = 'eng';

let workerPromise = null;

// Which core file tesseract will ask for, so `precache` can warm the right
// one. WASM SIMD has been in every major engine since 2021; the non-SIMD
// build is carried for the tail that predates it.
function coreFile() {
  const simd = wasmSimdSupported();
  return `${CORE_PATH}/tesseract-core${simd ? '-simd' : ''}-lstm.wasm.js`;
}

function wasmSimdSupported() {
  try {
    // The shortest valid module containing a SIMD instruction (v128.const).
    // Validating it is how you ask an engine whether it speaks SIMD without
    // the answer depending on a user-agent string.
    return WebAssembly.validate(
      new Uint8Array([
        0, 97, 115, 109, 1, 0, 0, 0, 1, 5, 1, 96, 0, 1, 123, 3, 2, 1, 0, 10, 10, 1, 8, 0, 65, 0,
        253, 15, 253, 98, 11,
      ]),
    );
  } catch {
    return false;
  }
}

async function getWorker(onProgress) {
  if (workerPromise) return workerPromise;

  workerPromise = (async () => {
    const module = await import('../vendor/tesseract/tesseract.esm.min.js');
    // The ESM build exports the whole namespace as its default and nothing
    // by name; the UMD build does the opposite. Accepting either means a
    // future swap between the two is a file copy rather than a bug that
    // only shows up the first time someone turns OCR on.
    const createWorker = module.createWorker ?? module.default?.createWorker;
    if (typeof createWorker !== 'function') {
      throw new Error('the vendored OCR engine does not expose createWorker');
    }

    // OEM 1 is LSTM-only, which is what the vendored `-lstm` core builds
    // contain. Asking for the legacy engine would send it looking for a
    // core that is deliberately not shipped.
    const options = {
      workerPath: new URL(WORKER_PATH, import.meta.url).href,
      corePath: new URL(CORE_PATH, import.meta.url).href,
      langPath: new URL(LANG_PATH, import.meta.url).href,
      // The model ships gzipped, and saying so stops the worker looking
      // for an uncompressed one first and taking a 404 on the chin.
      gzip: true,
    };
    // Set only when there is one. tesseract.js reserves a callback slot
    // for every function-shaped option it is handed and then invokes it
    // across the worker boundary; a `logger: undefined` reserves the slot
    // and fills it with nothing, and every progress tick throws.
    if (onProgress) options.logger = onProgress;

    return createWorker(LANG, 1, options);
  })().catch((error) => {
    // A failed load must not poison every later attempt — the usual cause
    // is a transient one, and the user's next scan deserves a fresh try.
    workerPromise = null;
    throw error;
  });

  return workerPromise;
}

export function isEngineLoaded() {
  return workerPromise !== null;
}

// Pull the engine into the service worker's cache while a connection is
// available, so the first offline scan is not the moment it is missed.
export async function precache(onProgress) {
  const assets = [
    new URL('../vendor/tesseract/tesseract.esm.min.js', import.meta.url).href,
    new URL(WORKER_PATH, import.meta.url).href,
    new URL(coreFile(), import.meta.url).href,
    new URL(`${LANG_PATH}/${LANG}.traineddata.gz`, import.meta.url).href,
  ];

  let done = 0;
  for (const url of assets) {
    await fetch(url, { cache: 'force-cache' });
    done += 1;
    onProgress?.(done / assets.length);
  }
}

// Read one page.
//
// Returns the shape `docscan-pdf` wants: words with pixel boxes, plus the
// size of the image they were measured against — which is deliberately not
// assumed to be the page's own size, because recognition runs on a
// downscale.
export async function recognize(blob, { maxEdge = 1600, onProgress } = {}) {
  const worker = await getWorker(onProgress);

  // Tesseract wants roughly 300dpi-equivalent text and gains nothing from
  // more, while cost climbs with pixel count. Recognising a 2400px page at
  // 1600px is several times faster at the same accuracy on printed text.
  const bitmap = await createImageBitmap(blob);
  let source = blob;
  let width = bitmap.width;
  let height = bitmap.height;

  try {
    const longest = Math.max(width, height);
    if (longest > maxEdge) {
      const scale = maxEdge / longest;
      width = Math.max(1, Math.round(width * scale));
      height = Math.max(1, Math.round(height * scale));
      const canvas = document.createElement('canvas');
      canvas.width = width;
      canvas.height = height;
      canvas.getContext('2d').drawImage(bitmap, 0, 0, width, height);
      source = await new Promise((resolve) => canvas.toBlob(resolve, 'image/png'));
    }
  } finally {
    bitmap.close();
  }

  const { data } = await worker.recognize(source, {}, { blocks: true });

  const words = [];
  for (const word of collectWords(data)) {
    const text = (word.text ?? '').trim();
    if (!text) continue;
    const box = word.bbox;
    if (!box) continue;
    words.push({
      text,
      x: box.x0,
      y: box.y0,
      width: box.x1 - box.x0,
      height: box.y1 - box.y0,
      // Tesseract scores 0-100; the core speaks 0-1.
      confidence: (word.confidence ?? 100) / 100,
    });
  }

  return { words, width, height, text: words.map((w) => w.text).join(' ') };
}

// Tesseract v5 returns words nested inside blocks/paragraphs/lines, and
// only populates the flat `data.words` array for some configurations.
// Walking the tree covers both rather than depending on which.
function collectWords(data) {
  if (Array.isArray(data.words) && data.words.length > 0) return data.words;

  const words = [];
  for (const block of data.blocks ?? []) {
    for (const paragraph of block.paragraphs ?? []) {
      for (const line of paragraph.lines ?? []) {
        words.push(...(line.words ?? []));
      }
    }
  }
  return words;
}

export async function shutdown() {
  if (!workerPromise) return;
  const worker = await workerPromise.catch(() => null);
  workerPromise = null;
  await worker?.terminate?.();
}
