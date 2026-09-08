# OpenDocScan for the web

The scanning core, running in a browser tab. Photograph a document, get a
clean multi-page PDF with searchable text — with no account, no watermark,
no page limit, and nothing uploaded.

## Why a web app at all

The competitive study this app is scoped from surveyed ten scanner
products and found no web version among them, concluding the form was a
dead end: *"浏览器插件/Web站运行在电脑端，无法调用手机摄像头"* — browser apps run
on desktops and so cannot reach a phone camera.

That has not been true for years. `getUserMedia({ facingMode: "environment" })`
opens the rear camera on any modern mobile browser, and every step after
it — finding the page, straightening it, filtering it, reading it, writing
the PDF — is arithmetic over a pixel buffer with no platform surface at
all. So the same Rust core that targets iOS and Android compiles to
`wasm32-unknown-unknown` unchanged — **114KB** over the wire, brotli.

What that buys, beyond reach:

- **Nothing to install.** The Microsoft Lens retirement window the study
  identifies runs out before an app-store review queue does.
- **The privacy claim becomes checkable.** This is the only form where a
  sceptical user can verify "nothing is uploaded" themselves: open
  DevTools, watch the Network tab, scan something. After the app loads,
  there are no requests. On a native app that same claim is a promise.

## Running it

```bash
./scripts/build-wasm.sh     # compiles the Rust core to WASM
npm run serve               # any static server will do
```

Then open `http://127.0.0.1:8765`. There is no build step for the HTML,
CSS, or JS: they are served as written.

Deploying is copying this directory to any static host that speaks HTTPS.
There is no server-side component, and there is nothing for one to do.

### Testing on a real phone

`npm run serve` binds to loopback, so a phone cannot reach it — and
reaching it by LAN address over plain HTTP does not help either, because
`getUserMedia` is gated on the page being a **secure context** and
`http://192.168.x.x` is not one. Both Safari and Chrome go further than
the spec requires here: they refuse camera access on any origin whose
certificate failed to validate, *including one the user clicked through*.
So "visit anyway" past a certificate warning gets you the app with a dead
shutter button, which is a confusing thing to debug.

```bash
npm run serve:lan
```

That binds to every interface, generates a `serverAuth` certificate
covering this machine's LAN address on first run, and prints the exact
steps to install and trust it on iOS and Android. Trusting it is the part
that is not optional.

Two things that are not this app's fault but look like it:

- Guest Wi-Fi networks frequently block device-to-device traffic
  entirely, so the phone cannot reach the laptop at all.
- The certificate covers the LAN address it saw at generation time. If
  DHCP moves the machine, delete `.certs/` and re-run.

Everything except the camera works fine on plain HTTP over the LAN —
"Import images" exercises detection, correction, filters, OCR, and export,
which is the whole pipeline.

## What it does

| | |
|---|---|
| Camera capture | Live preview with the detected page outlined as you move |
| Import | One image goes to the corner editor; a batch is detected and added without stopping to ask |
| Edge detection | Automatic, with four draggable corners to correct it |
| Perspective correction | Sized to the page's near edge, so nothing sharp is thrown away |
| Filters | Photo, Enhance, Black & white, plus brightness and rotation |
| Multi-page | Reorder, delete, add — no limit |
| OCR | English, on-device, producing an invisible selectable text layer |
| Library | Stored in this browser, full-text searchable across every scan |
| Export | PDF to the OS share sheet, or a download |
| Offline | Works with no connection once loaded |

## How the privacy claim is enforced

Not by policy — by three mechanisms, in order of how hard they are to
subvert:

1. **The core cannot reach the network.** `docscan-wasm` has no HTTP
   client and no networking dependency. Read its `Cargo.toml`.
2. **The OCR engine is vendored.** tesseract.js fetches its WASM core and
   language model from a CDN by default, which would mean a third party
   learning each time you scanned something. Every byte of it is served
   from this app's own origin instead — see `vendor/tesseract/`.
3. **The service worker refuses cross-origin requests outright.** Every
   request the page makes passes through `sw.js`, and any request to
   another origin is answered with a 403 rather than forwarded. If a
   future dependency ever tried to phone home, it would fail loudly
   instead of succeeding quietly.

The end-to-end test asserts this too: it records every request the app
makes and fails if any of them leaves the origin.

## Testing

```bash
npm run serve &     # the tests need a server on :8765
npm test
```

The suite drives a real Chromium through the real UI: it draws a
synthetic photographed page — a white sheet skewed on a dark desk, with
text on it — imports it, checks the rectified result has the *sheet's*
aspect rather than the *photo's*, adds a second page, exports, and then
parses the resulting PDF for its page count, its title, its invisible
text-layer marker, and the words OCR actually read. It finishes by
searching the library for text that only OCR could have supplied.

The Rust side is tested separately and needs no browser:

```bash
cargo test --workspace
```

## Speed

The pixel work — detection, rectification, filtering, encoding, PDF
assembly — runs in a Web Worker, so nothing it does can freeze the
interface. Measured across one complete scan of a 12-megapixel photo,
including six drags of the brightness slider: **zero** long tasks, against
five totalling 796ms before, the worst of them a 428ms freeze.

The core itself got between 2x and 20x faster at the same time, and the
module got three times smaller over the wire. `apps/web/BENCHMARKS.md` has
the numbers, the two build flags that were decided by measuring rather
than by taste, and the three benchmarks that produced them:

    npm run serve                      # in one terminal
    npm run bench                      # per-operation, in the browser
    node bench/main-thread.mjs         # how long the UI is frozen
    cargo run -p docscan-wasm --example bench --release

## Known limits

- **OCR is English only.** The bundled model is `eng`; other languages
  need their `.traineddata` added to `vendor/tesseract/lang/`.
- **The invisible text layer is Latin-script only.** It uses an
  unembedded Helvetica with WinAnsi encoding, which cannot carry CJK,
  Cyrillic, or Greek. Words it cannot encode are dropped from the layer
  rather than mangled — the page image is unaffected. Supporting those
  scripts means embedding a font with a `/ToUnicode` map, which is the
  same change on both sides and is worth making once a model that reads
  them ships.
- **The library lives in this browser.** Not in an account, not on a
  server, and not on your other devices. Clearing site data clears it.
  The app asks for persistent storage and tells you plainly whether the
  browser granted it.
