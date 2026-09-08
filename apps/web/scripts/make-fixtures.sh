#!/usr/bin/env bash
# Render the screenshot fixtures, from a document that may be published.
#
# This exists because of a mistake worth not repeating. The screenshots for
# this app are captured by driving the real pipeline over real files, and the
# real files on this machine are a CV, two invoices, a bank statement, a
# company deck and a customer's furniture drawing with their name and address
# in the title block. A capture run pointed at that directory put the
# customer's drawing straight into a published document, and it was caught by
# looking at the PNG rather than by any check.
#
# So there are now two fixture directories and they are not interchangeable:
#
#   /tmp/docscan-real     private. Adversarial input for `inspect` runs whose
#                         findings are reported as numbers, never as pictures.
#   /tmp/docscan-public   this script's output. Bitcoin's whitepaper, which is
#                         published, unowned and text-heavy enough to exercise
#                         detection, rectification, filtering and OCR.
#
# The marker file this writes is what `capture.mjs` checks for. A directory of
# someone's private documents will never have one, so the guard cannot be
# satisfied by accident — only by running this script.
set -euo pipefail

SOURCE="${1:-$HOME/Downloads/bitcoin.pdf}"
OUT="${2:-/tmp/docscan-public}"

test -f "$SOURCE" || {
  echo "No source PDF at $SOURCE." >&2
  echo "Pass one that is safe to publish: ./scripts/make-fixtures.sh <file.pdf> [outdir]" >&2
  exit 1
}

rm -rf "$OUT"
mkdir -p "$OUT"

# 150 dpi: enough that OCR has real glyph detail to read, small enough that a
# nine-shot capture run stays quick. Named page-1..page-5 so capture.mjs's sort
# is the document's own page order.
#
# PyMuPDF rather than sips, which renders only the first page of a PDF and
# succeeds silently doing it — five identical fixtures, and nothing says so.
SOURCE="$SOURCE" OUT="$OUT" python3 -c '
import os

import fitz  # PyMuPDF

src, out = os.environ["SOURCE"], os.environ["OUT"]
doc = fitz.open(src)
matrix = fitz.Matrix(150 / 72, 150 / 72)
for n in range(min(5, doc.page_count)):
    # alpha=False gives a white ground. A scanner s input is a sheet of paper,
    # and an alpha-zero background composites to black in the detector.
    pix = doc[n].get_pixmap(matrix=matrix, alpha=False)
    pix.save(f"{out}/page-{n + 1}.png")
    print(f"  page-{n + 1}.png  {pix.width}x{pix.height}")
'

printf '%s\n' \
  "Rendered from $(basename "$SOURCE") by scripts/make-fixtures.sh." \
  "Safe to publish: every page here comes from that document and nothing else." \
  > "$OUT/PUBLISHABLE"

echo
echo "Wrote $(ls "$OUT"/*.png | wc -l | tr -d ' ') pages to $OUT"
