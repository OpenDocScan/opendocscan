// The scanning core, addressed from the main thread.
//
// Every decision here about what a scan *is* lives in Rust, and every
// pixel it touches lives in a Web Worker (see `scanner-worker.js`). What
// remains in this file is the request/response plumbing between them, and
// the small amount of drawing the main thread genuinely has to do because
// only it can see the DOM.
//
// The functions are all async. That is not incidental — it is the whole
// point of the arrangement, and the reason a slow frame can no longer
// freeze a camera preview or a corner being dragged.

const worker = new Worker(new URL('./scanner-worker.js', import.meta.url), {
  type: 'module',
});

let nextId = 1;
const pending = new Map();

worker.onmessage = (event) => {
  const { id, ok, result, error } = event.data;
  const settle = pending.get(id);
  if (!settle) return;
  pending.delete(id);
  if (ok) settle.resolve(result);
  else settle.reject(new Error(error));
};

worker.onerror = (event) => {
  // A worker that failed to start never answers, so every outstanding
  // request has to be failed rather than left hanging — a spinner that
  // never stops is worse than an error message.
  const failure = new Error(event.message ?? 'the scanning engine failed to start');
  for (const [, settle] of pending) settle.reject(failure);
  pending.clear();
};

function call(op, request = {}, transfer = []) {
  const id = nextId;
  nextId += 1;
  return new Promise((resolve, reject) => {
    pending.set(id, { resolve, reject });
    worker.postMessage({ id, op, request }, transfer);
  });
}

/// Wakes the worker and waits for the wasm module to be live, so the
/// first real operation is not also a cold start.
export function initScanner() {
  return call('releaseAll');
}

// --- getting pixels in and out of the DOM ---------------------------------

// A canvas kept alive between calls.
//
// Allocating a fresh canvas per frame is what makes live detection
// stutter on a phone: each one is a GPU-backed surface the compositor has
// to set up and tear down, thirty times a second.
const scratch = document.createElement('canvas');
const scratchCtx = scratch.getContext('2d', { willReadFrequently: true });

function drawToScratch(source, width, height) {
  if (scratch.width !== width || scratch.height !== height) {
    scratch.width = width;
    scratch.height = height;
  }
  scratchCtx.drawImage(source, 0, 0, width, height);
  return scratchCtx.getImageData(0, 0, width, height);
}

// Pixels out of anything the browser can draw: a video element, an
// ImageBitmap, an image. `maxEdge` caps the longer side.
export function imageDataFrom(source, sourceWidth, sourceHeight, maxEdge = 0) {
  let width = sourceWidth;
  let height = sourceHeight;

  if (maxEdge > 0) {
    const longest = Math.max(width, height);
    if (longest > maxEdge) {
      const scale = maxEdge / longest;
      width = Math.max(1, Math.round(width * scale));
      height = Math.max(1, Math.round(height * scale));
    }
  }

  return drawToScratch(source, width, height);
}

export async function blobToImageData(blob, maxEdge = 0) {
  const bitmap = await createImageBitmap(blob);
  try {
    return imageDataFrom(bitmap, bitmap.width, bitmap.height, maxEdge);
  } finally {
    // Bitmaps hold decoded pixels outside the JS heap, so the collector
    // has no idea how expensive they are and is in no hurry.
    bitmap.close();
  }
}

function toImageData({ pixels, width, height }) {
  return new ImageData(new Uint8ClampedArray(pixels.buffer, pixels.byteOffset, pixels.length), width, height);
}

// --- images the interface keeps -------------------------------------------
//
// Naming an image and leaving it in the worker, rather than passing it
// back and forth, is what makes dragging a brightness slider cost four
// bytes a nudge instead of twenty megabytes.

/// Hands `imageData` to the worker under `slot`. The buffer is
/// transferred, so the caller's `ImageData` is detached afterwards and
/// must not be read again — draw it to a canvas first if it also needs to
/// be shown.
export function hold(slot, imageData) {
  return call('hold', { slot, pixels: imageData.data, width: imageData.width, height: imageData.height }, [
    imageData.data.buffer,
  ]);
}

/// Holds a downscaled copy of `from` under the name `to`, without the
/// full-resolution buffer ever coming back to this thread. Returns the
/// derived size.
export function derive(from, to, maxEdge) {
  return call('derive', { from, to, maxEdge });
}

export function release(slot) {
  return call('release', { slot });
}

// --- operations -----------------------------------------------------------

// The page in this frame, as `{x, y}` corners in the frame's own
// coordinates, or `null` if there is nothing page-shaped in it.
//
// Takes either an `ImageData` — whose buffer is transferred and therefore
// consumed — or `{ slot }` naming an image already held.
export async function detectQuad(image) {
  const { quad } = await call(...request('detect', image));
  return quad;
}

// The whole frame, for when detection finds nothing and the user still
// wants to keep what they photographed.
export function fullFrameQuad(width, height) {
  return [
    { x: 0, y: 0 },
    { x: width, y: 0 },
    { x: width, y: height },
    { x: 0, y: height },
  ];
}

export async function rectify(image, corners, maxEdge = 0) {
  const [op, base, transfer] = request('rectify', image);
  return toImageData(await call(op, { ...base, corners, maxEdge }, transfer));
}

export async function applyFilter(image, name, brightness, rotation = 0) {
  const [op, base, transfer] = request('filter', image);
  return toImageData(await call(op, { ...base, name, brightness, rotation }, transfer));
}

/// Encode to a `Blob`, optionally downscaling first — which is how
/// thumbnails are made without a second full-size buffer on the main
/// thread.
export async function encodeImage(image, type = 'image/jpeg', quality = 0.92, maxEdge = 0) {
  const [op, base, transfer] = request('encode', image);
  const { blob } = await call(op, { ...base, type, quality, maxEdge }, transfer);
  return blob;
}

// Assemble finished pages into a PDF.
//
// The pages cross as `Blob`s, which structured cloning passes by
// reference rather than copying, so the decode, the pixel read and the
// JPEG re-encode all happen in the worker. The main thread's whole
// contribution to an export is the list.
export async function exportPdf(pages, { paper, margin, quality, title }) {
  const { bytes } = await call('pdf', {
    pages: pages.map((page) => ({ blob: page.blob, ocr: page.ocr })),
    options: { paper, margin, quality, title },
  });
  return new Blob([bytes], { type: 'application/pdf' });
}

/// Normalises the "an ImageData, or the name of a held image" argument
/// every operation above accepts into the message shape and the transfer
/// list that go with it.
function request(op, image) {
  if (image instanceof ImageData) {
    return [
      op,
      { pixels: image.data, width: image.width, height: image.height },
      [image.data.buffer],
    ];
  }
  return [op, { slot: image.slot }, []];
}
