// The scanning core, running where it cannot stutter the interface.
//
// Everything in this file is pixels: detection, rectification, filtering,
// rotation, encoding, PDF assembly. None of it belongs on the main thread,
// because all of it is measured in tens or hundreds of milliseconds on a
// phone and the main thread is simultaneously trying to keep a camera
// preview at sixty frames a second and follow a finger dragging a corner.
//
// Two rules make that worth doing rather than merely tidy:
//
// * **Buffers move, they are never copied.** Every pixel buffer crossing
//   in or out is a transferable, so a 48MB capture changes owner rather
//   than being duplicated. The cost is that the sender loses it — which is
//   why the second rule exists.
// * **Images the interface keeps are held here, not there.** The frame
//   being cropped and the page being filtered live in `slots`. The UI
//   sends one of them across once and then refers to it by name, so
//   dragging a brightness slider sends four bytes per nudge instead of
//   twenty megabytes.

import init, {
  detectQuad as wasmDetectQuad,
  rectify as wasmRectify,
  applyFilter as wasmApplyFilter,
  PdfBuilder,
} from './wasm/docscan.js';

const ready = init();

// Images retained on behalf of the interface, by name. Small and
// deliberately so: `crop`, `filter-source`, `filter-preview`. Anything
// else would be a cache, and a cache of multi-megabyte buffers on a phone
// is a way to get the tab killed.
const slots = new Map();

// One canvas reused for every rotation and every encode. Allocating an
// `OffscreenCanvas` per operation is a real cost — each is a drawing
// surface the browser has to set up — and these operations happen in
// bursts.
let scratch = null;
function scratchContext(width, height) {
  if (!scratch) scratch = new OffscreenCanvas(width, height);
  if (scratch.width !== width || scratch.height !== height) {
    scratch.width = width;
    scratch.height = height;
  }
  return scratch.getContext('2d', { willReadFrequently: true });
}

// An image argument is either pixels sent with the message or the name of
// one already here. Resolving both to the same shape in one place is what
// lets every operation below take either without caring.
function resolve(request) {
  if (request.slot !== undefined) {
    const held = slots.get(request.slot);
    if (!held) throw new Error(`no image is being held as "${request.slot}"`);
    return held;
  }
  return { pixels: request.pixels, width: request.width, height: request.height };
}

function rotated(image, quarterTurns) {
  const turns = ((quarterTurns % 4) + 4) % 4;
  if (turns === 0) return image;

  const swap = turns % 2 === 1;
  const width = swap ? image.height : image.width;
  const height = swap ? image.width : image.height;

  // Drawn rather than transposed by hand: `drawImage` on a rotated
  // context is the browser's own blit, and beating it in JavaScript is not
  // a fight worth picking.
  const source = new OffscreenCanvas(image.width, image.height);
  source
    .getContext('2d')
    .putImageData(new ImageData(new Uint8ClampedArray(image.pixels), image.width, image.height), 0, 0);

  const ctx = scratchContext(width, height);
  ctx.save();
  ctx.translate(width / 2, height / 2);
  ctx.rotate((turns * Math.PI) / 2);
  ctx.drawImage(source, -image.width / 2, -image.height / 2);
  ctx.restore();

  const data = ctx.getImageData(0, 0, width, height);
  return { pixels: data.data, width, height };
}

/// A `RasterImage` from the core, unwrapped. `intoData` moves the pixels
/// out rather than copying them, so it is called exactly once, here, and
/// the raster never escapes.
function unwrap(raster) {
  const { width, height } = raster;
  return { pixels: raster.intoData(), width, height };
}

const operations = {
  hold({ slot, pixels, width, height }) {
    slots.set(slot, { pixels, width, height });
    return { result: { width, height }, transfer: [] };
  },

  /// Holds a downscaled copy of one held image under a second name.
  ///
  /// The filter view needs both a full-resolution page and a preview of
  /// it, and deriving the second from the first here means the
  /// full-resolution buffer never travels to the main thread and back
  /// just to be shrunk.
  derive({ from, to, maxEdge }) {
    const image = resolve({ slot: from });
    const longest = Math.max(image.width, image.height);
    if (maxEdge <= 0 || longest <= maxEdge) {
      slots.set(to, { pixels: image.pixels.slice(), width: image.width, height: image.height });
      return { result: { width: image.width, height: image.height }, transfer: [] };
    }

    const scale = maxEdge / longest;
    const width = Math.max(1, Math.round(image.width * scale));
    const height = Math.max(1, Math.round(image.height * scale));

    const source = new OffscreenCanvas(image.width, image.height);
    source
      .getContext('2d')
      .putImageData(
        new ImageData(new Uint8ClampedArray(image.pixels), image.width, image.height),
        0,
        0,
      );
    const ctx = scratchContext(width, height);
    ctx.clearRect(0, 0, width, height);
    ctx.imageSmoothingQuality = 'high';
    ctx.drawImage(source, 0, 0, width, height);

    const data = ctx.getImageData(0, 0, width, height);
    slots.set(to, { pixels: data.data, width, height });
    return { result: { width, height }, transfer: [] };
  },

  release({ slot }) {
    slots.delete(slot);
    return { result: {}, transfer: [] };
  },

  releaseAll() {
    slots.clear();
    return { result: {}, transfer: [] };
  },

  detect(request) {
    const { pixels, width, height } = resolve(request);
    const flat = wasmDetectQuad(pixels, width, height);
    const quad = flat
      ? [
          { x: flat[0], y: flat[1] },
          { x: flat[2], y: flat[3] },
          { x: flat[4], y: flat[5] },
          { x: flat[6], y: flat[7] },
        ]
      : null;
    return { result: { quad, width, height }, transfer: [] };
  },

  rectify(request) {
    const { pixels, width, height } = resolve(request);
    const flat = new Float32Array(8);
    request.corners.forEach((corner, i) => {
      flat[i * 2] = corner.x;
      flat[i * 2 + 1] = corner.y;
    });

    // No defensive copy of a held slot. `wasm_bindgen` copies the array
    // into wasm memory and leaves the JavaScript one alone, so a slot
    // survives being operated on however many times — and copying it
    // first, which looks like the careful thing to do, would duplicate
    // twenty megabytes to protect against nothing.
    const out = unwrap(wasmRectify(pixels, width, height, flat, request.maxEdge ?? 0));
    return { result: out, transfer: [out.pixels.buffer] };
  },

  filter(request) {
    const source = resolve(request);
    const turned = rotated(source, request.rotation ?? 0);
    const out = unwrap(
      wasmApplyFilter(turned.pixels, turned.width, turned.height, request.name, request.brightness),
    );
    return { result: out, transfer: [out.pixels.buffer] };
  },

  async encode(request) {
    const image = resolve(request);
    let { pixels, width, height } = image;

    // Thumbnails are the only caller that asks for a size, and asking the
    // canvas to do the downscale keeps the resampling on the GPU path
    // rather than in a hand-rolled loop.
    const cap = request.maxEdge ?? 0;
    const longest = Math.max(width, height);
    const source = new OffscreenCanvas(width, height);
    source
      .getContext('2d')
      .putImageData(new ImageData(new Uint8ClampedArray(pixels), width, height), 0, 0);

    let canvas = source;
    if (cap > 0 && longest > cap) {
      const scale = cap / longest;
      width = Math.max(1, Math.round(width * scale));
      height = Math.max(1, Math.round(height * scale));
      canvas = new OffscreenCanvas(width, height);
      const ctx = canvas.getContext('2d');
      ctx.imageSmoothingQuality = 'high';
      ctx.drawImage(source, 0, 0, width, height);
    }

    const blob = await canvas.convertToBlob({ type: request.type, quality: request.quality });
    return { result: { blob, width, height }, transfer: [] };
  },

  async pdf({ pages, options }) {
    // Built one page at a time, and each page's pixels dropped before the
    // next is decoded — the difference between a twenty-page export
    // working on a phone and the tab being killed with no error to catch.
    let builder = new PdfBuilder(
      options.paper,
      options.margin,
      options.quality,
      options.title ?? undefined,
    );
    try {
      for (const page of pages) {
        const bitmap = await createImageBitmap(page.blob);
        try {
          const ctx = scratchContext(bitmap.width, bitmap.height);
          ctx.drawImage(bitmap, 0, 0);
          const data = ctx.getImageData(0, 0, bitmap.width, bitmap.height);
          builder.addPage(
            data.data,
            bitmap.width,
            bitmap.height,
            page.ocr ? JSON.stringify(page.ocr) : undefined,
          );
        } finally {
          bitmap.close();
        }
      }
      const bytes = builder.finish();
      // `finish` takes the builder by value on the Rust side, so the glue
      // has already nulled the handle. Clearing the local reference at the
      // same moment is what keeps the cleanup below from freeing a pointer
      // that is already gone — which fails loudly, at the end of an
      // export, having done all the work.
      builder = null;
      return { result: { bytes }, transfer: [bytes.buffer] };
    } finally {
      builder?.free();
    }
  },
};

self.onmessage = async (event) => {
  const { id, op, request } = event.data;
  try {
    await ready;
    const handler = operations[op];
    if (!handler) throw new Error(`unknown scanner operation: ${op}`);
    const { result, transfer } = await handler(request);
    self.postMessage({ id, ok: true, result }, transfer);
  } catch (error) {
    // Errors cross as plain strings: an `Error` survives structured
    // cloning but a `JsError` thrown out of wasm-bindgen does not.
    self.postMessage({ id, ok: false, error: error?.message ?? String(error) });
  }
};
