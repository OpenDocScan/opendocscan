#!/usr/bin/env python3
"""Strip the CDN fallbacks out of the vendored tesseract.js.

tesseract.js is designed to fetch its worker, its core and its language
data from jsDelivr unless told otherwise. `src/ocr.js` tells it otherwise,
and the service worker refuses cross-origin requests outright, so nothing
reaches a CDN today. But the URLs are still in the shipped bytes as `x ||
"https://cdn.jsdelivr.net/..."` fallbacks — one dropped option away from
being live, in the one product whose whole claim is that nothing leaves
the device.

So they are rewritten to same-origin paths here. A missing file then 404s
against this app's own origin, which is the right failure: loud, local,
and impossible to mistake for a successful upload.

Run after re-vendoring tesseract.js, and note that `npm test` fails if you
forget — the end-to-end suite greps the shipped files for third-party
hostnames.

    python3 scripts/deorbit-tesseract.py
"""

import pathlib
import sys

VENDOR = pathlib.Path(__file__).resolve().parent.parent / "vendor" / "tesseract"

# Each fallback is a prefix the library concatenates a version or language
# onto, so the replacement has to be a prefix too, and the shape of what
# gets appended decides where it should point.
REWRITES = [
    ("https://cdn.jsdelivr.net/npm/tesseract.js-core@v", "./core/v"),
    ("https://cdn.jsdelivr.net/npm/@tesseract.js-data/", "./lang/"),
    ("https://cdn.jsdelivr.net/npm/tesseract.js@v", "./v"),
]


def main() -> int:
    if not VENDOR.is_dir():
        print(f"no vendored tesseract at {VENDOR}", file=sys.stderr)
        return 1

    changed = 0
    for path in sorted(VENDOR.rglob("*.js")):
        text = path.read_text(encoding="utf-8")
        original = text
        for cdn, local in REWRITES:
            text = text.replace(cdn, local)
        if text != original:
            path.write_text(text, encoding="utf-8")
            print(f"rewrote {path.relative_to(VENDOR.parent.parent)}")
            changed += 1

    print(f"{changed} file(s) changed" if changed else "nothing to rewrite")
    return 0


if __name__ == "__main__":
    sys.exit(main())
