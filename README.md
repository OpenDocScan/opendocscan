# OpenDocScan

Photograph a document, get a real scan: straightened, cleaned, and searchable.

**[opendocscan.com](https://opendocscan.com)** · **[open the scanner](https://app.opendocscan.com)**

Point a phone at a page. OpenDocScan finds the edges of the sheet, corrects the
perspective, cleans the lighting, reads the text, and writes a multi-page PDF
you can search and select from. No account, no watermark, no page limit, and
nothing to install.

Everything runs in the browser tab. Nothing is uploaded — and unlike a native
scanner app, that is a claim you can check yourself: open the developer tools,
switch to the Network panel, and scan something. After the app has loaded,
there are no further requests.

## How the privacy claim is enforced

Not by policy. By three mechanisms, in order of how hard each is to subvert:

1. **The core cannot reach the network.** `docscan-wasm` has no HTTP client and
   no networking dependency. Read its `Cargo.toml`.
2. **The OCR engine is vendored.** tesseract.js fetches its WASM core and
   language model from a CDN by default, which would mean a third party
   learning each time you scanned something. Every byte is served from the
   app's own origin instead — see `apps/web/vendor/tesseract/`.
3. **The service worker refuses cross-origin requests outright.** Every request
   the page makes passes through `apps/web/sw.js`, and anything bound for
   another origin is answered with a 403 rather than forwarded. A future
   dependency that tried to phone home would fail loudly instead of succeeding
   quietly.

The end-to-end suite asserts it too: it records every request the app makes
while driving a complete scan and fails if any of them leaves the origin.

## Layout

```
crates/
  docscan-core        the pipeline the app drives
  docscan-detect      finding the page in a photograph
  docscan-transform   perspective correction
  docscan-filters     histograms and lookup tables
  docscan-ocr         text recognition glue
  docscan-pdf         PDF assembly, with an invisible text layer
  docscan-wasm        the wasm32-unknown-unknown boundary
apps/web              the app itself — no build step but the Rust one
```

The same core targets iOS and Android unchanged; the web build is
`wasm32-unknown-unknown` and 138 KB over the wire.

## Building and running

```sh
cd apps/web
npm install
npm run build      # compiles the Rust core to WebAssembly
npm run serve      # http://127.0.0.1:8765
```

There is no bundler. The HTML, CSS and JavaScript are served as written.

Testing a phone camera needs HTTPS, because `getUserMedia` is gated on a secure
context — `npm run serve:lan` generates a certificate for this machine's LAN
address and prints the steps to trust it. `apps/web/README.md` has the detail,
including two failure modes that look like app bugs and are not.

## Tests

```sh
cargo test --workspace     # 73 tests, no browser needed
cd apps/web && npm test    # drives a real Chromium through the real UI
```

The browser suite draws a synthetic photographed page, imports it, checks the
rectified result has the *sheet's* aspect rather than the *photo's*, exports a
PDF, and parses it for its page count, title, invisible-text marker and the
words OCR actually read. `apps/web/BENCHMARKS.md` covers performance, including
two optimisations that measurement rejected.

## Known limits

- **OCR is English only.** The bundled model is `eng`.
- **The invisible text layer is Latin-script only.** It uses unembedded
  Helvetica with WinAnsi encoding, so it cannot carry CJK, Cyrillic or Greek.
  Words it cannot encode are dropped from the searchable layer rather than
  mangled; the page image is unaffected.
- **The library lives in this browser.** Not in an account, not on a server,
  not on your other devices. Clearing site data clears it.

## Licence

MIT or Apache-2.0, at your option.
