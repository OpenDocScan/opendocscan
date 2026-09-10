# Where the time goes

Three benchmarks, for three different questions.

| | `cargo run -p docscan-wasm --example bench --release` | `npm run bench` | `node apps/web/bench/main-thread.mjs` |
|---|---|---|---|
| runs | native, on the host | WebAssembly, in Chromium | the whole app, in Chromium |
| good for | comparing two implementations of a pass | choosing build flags, and absolute numbers | whether it *feels* fast |
| why | native preserves the *ratio* and iterates in seconds | the compiler backend is the thing under test | speed and responsiveness are different questions |

The two browser ones need a server on `:8765` (`npm run serve`); point them
at another build with `BASE_URL`. The first two print the same operation
names, so a row in one can be read against a row in the other.

## How long the interface is frozen

`performance` calls a stretch where the browser could not respond to
anything a *long task*. Summing them across one real scan — import a
12-megapixel photo, confirm the corners, drag the brightness slider six
times, add the page — is the closest a script gets to what a person
notices.

| | before | after |
|---|---:|---:|
| wall clock, import to page in tray | 1,759 ms | 1,263 ms |
| long tasks | 5 | **0** |
| total time the UI could not respond | 796 ms | **0 ms** |
| longest single freeze | 428 ms | **0 ms** |

The 428ms freeze was one filter pass at full resolution. On a phone it
would have been well over a second — during which the brightness slider
does not move, because the thread that would move it is busy.

Nothing was made faster to achieve the zero; the work was moved. That is
worth saying plainly, because the two are easy to confuse: the core still
spends real milliseconds filtering a page, it just no longer spends them
where the interface lives.

## The numbers that mattered

Chromium, Apple Silicon, synthetic pages at the sizes the app really uses:
a 640x480 live preview frame, a 12-megapixel capture, and an 1800x2400
rectified page.

| operation | before | after | |
|---|---:|---:|---:|
| `detectQuad` 640x480 — every preview frame | 51.9 ms | 15.3 ms | 3.4x |
| `detectQuad` 3024x4032 — every capture | 415.8 ms | 81.5 ms | 5.1x |
| `rectify` 12MP to 2400px | 310.8 ms | 112.1 ms | 2.8x |
| `applyFilter original` + brightness | 73.6 ms | 7.8 ms | 9.4x |
| `applyFilter bw` + brightness | 160.2 ms | 9.7 ms | 16.5x |
| `applyFilter enhance` + brightness | 270.6 ms | 13.4 ms | 20.2x |
| `applyFilter enhance` 825x1100 — slider preview | 56.3 ms | 2.7 ms | 20.9x |
| 5-page A4 PDF export | 856.3 ms | 393.4 ms | 2.2x |
| module over the wire | 357,874 B | 113,987 B | 3.1x |

A desktop browser is roughly three to five times faster than a mid-range
phone at this kind of work, so read every number as "and several times
this, on the device someone is actually scanning with".

## The two build flags, and what deciding them looked like

**`opt-level`.** The profile asked for `"z"`, which is the obvious choice
for a module that has to travel down a phone connection. It was the wrong
one, and not by a little:

| | brotli | `detectQuad` 12MP | 5-page export |
|---|---:|---:|---:|
| `opt-level = "z"` | 104,782 B | 552.6 ms | 596.6 ms |
| `opt-level = 3` | 113,987 B | 78.3 ms | 394.0 ms |

Nine kilobytes, once, against seven times slower detection on every
capture for the life of the install. The module is fetched once and then
served from the service worker cache; the loops inside it run several
times a second while a camera preview is open.

**SIMD.** Enabled, then measured, then turned off. Every operation landed
within noise of the scalar build and two were slightly behind it, for
2.2KB more. The loops look vectorisable and are not: the filters are a
lookup table indexed by each byte, and the warp gathers four scattered
source pixels per output pixel. Both are indirection, which is the one
thing a vector unit cannot do. `DOCSCAN_SIMD=1` re-enables it if a future
compiler disagrees.

## What made the difference

- **The filters stopped allocating.** `enhance` sorted a freshly allocated
  copy of every pixel to read two percentiles off it — `n log n` and four
  megabytes to find two numbers. A 256-bin histogram answers the same
  question in one pass and one kilobyte. Then, because contrast stretching
  and brightness are both per-channel maps, the two compose into a single
  256-entry lookup table applied in one in-place pass, instead of two
  full-image allocations.
- **The buffers stopped being copied.** The entry points took `&[u8]` and
  immediately called `to_vec()`, so every frame crossed the boundary
  twice. Taking `Vec<u8>` means `wasm_bindgen`'s copy out of the JS heap is
  the only one — 48MB a capture, not 96MB.
- **Detection stopped building colour images.** It discards colour as its
  first act, so it now gets the luma plane directly and downscales one
  channel rather than four.
- **The warp stopped converting.** Rectify was RGBA to RGB, warp, RGB back
  to RGBA — two full-image passes to arrive at the layout it started in.
  It is now a hand-written projective warp straight from RGBA, stepping
  the source coordinate incrementally along each row rather than
  evaluating the projection per pixel. It is also, incidentally, more
  accurate: the general path truncates to `u8` three times per pixel.
- **The PDF encoder stopped asking three times.** "Is it bilevel", "is it
  achromatic" and "what is its luma plane" were three full-image walks,
  two of whose allocations were discarded whichever way the answer went.
  They are one walk that exits early on the first coloured pixel.
- **The pixel work left the main thread.** Detection, rectification and
  filtering run in a Web Worker, so a slow frame no longer freezes the
  camera preview or the slider being dragged.

## A note on measuring

The first version of these numbers showed detection getting *slower*. It
had not: a `brew list` left running in the background was taking half the
machine. Every number above comes from an idle machine, and the before
column is built from a `git worktree` of the previous commit so both sides
are measured on the same hardware in the same minute.

## The filters became spatial, 10 September 2026

Enhance and Black & white were global — one histogram and one lookup table for
the page, one cutoff at luma 128 for every pixel. That is correct for evenly
lit input and useless on a photograph, because a shadow supplies both ends of
the histogram itself and leaves the stretch nearly an identity.

Measured on the same page rendered flat, and again with an ordinary hand-held
lighting gradient:

| | flat render | shadowed photo |
|---|---|---|
| original | 94.4% paper | 32.7% paper |
| Enhance, global | 94.4% | **35.8%** |
| Enhance, flattened first | 94.8% | **94.8%** |
| B&W, fixed threshold | 96.0% paper / 4.0% ink | **84.4% / 15.6%** |
| B&W, Sauvola | 94.3% / 5.7% | **94.4% / 5.6%** |

15.6% ink on a text page is the shadowed corner turning solid black.

Both new passes share one summed-area table, so a window mean is four lookups
whatever the radius. What it costs, in the browser:

    applyFilter enhance 825x1100      24.5 ms   <- the interactive preview size
    applyFilter bw      1800x2400     58.8 ms
    applyFilter enhance 1800x2400    116.8 ms

The interactive path is the first line: the app derives a preview capped at
1100px and filters that while the brightness slider moves. Full resolution runs
once, on confirm, in the worker. The whole-scan measurement is unchanged at
**zero long tasks**.

The module grew 318,780 -> 321,190 bytes, 138,027 -> 139,161 gzipped.
