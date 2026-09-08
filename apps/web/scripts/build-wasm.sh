#!/usr/bin/env bash
# Builds crates/docscan-wasm for wasm32-unknown-unknown and generates the
# wasm-bindgen JS glue into apps/web/src/wasm/. There is no bundler in this
# app — the generated module is loaded directly as an ES module — so this
# script is the entire build step.
#
# Two knobs, both measured rather than assumed (see apps/web/BENCHMARKS.md):
#
#   DOCSCAN_SIMD=0   skip the 128-bit SIMD build (see below)
#   DOCSCAN_PROFILE  cargo profile, default wasm-release
set -euo pipefail

export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:$PATH"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WEB_DIR="$(dirname "$SCRIPT_DIR")"
WORKSPACE_DIR="$(dirname "$(dirname "$WEB_DIR")")"
OUT_DIR="$WEB_DIR/src/wasm"

: "${CARGO_TARGET_DIR:=$WORKSPACE_DIR/target}"
: "${DOCSCAN_PROFILE:=wasm-release}"
: "${DOCSCAN_SIMD:=0}"
export CARGO_TARGET_DIR

# SIMD is off by default, and that is a measurement, not an oversight.
# WebAssembly SIMD is available everywhere this app runs (Chrome 91,
# Firefox 89, Safari 16.4), and the image passes here look like exactly
# the shape that should benefit — long runs of independent byte
# arithmetic. It buys nothing: every operation lands within noise of the
# scalar build, two of them slightly behind it, for 2.2KB more over the
# wire. The reason is visible once you look at what the loops actually
# do — a lookup table indexed by each byte, and a bilinear warp gathering
# four scattered pixels per output — and neither is a vector operation,
# because both are indirection. Turn it back on with DOCSCAN_SIMD=1 and
# re-run apps/web/bench/wasm-bench.mjs if that ever stops being true.
RUSTFLAGS="${RUSTFLAGS:-}"
if [ "$DOCSCAN_SIMD" = "1" ]; then
  RUSTFLAGS="$RUSTFLAGS -C target-feature=+simd128"
fi
export RUSTFLAGS

cargo build -p docscan-wasm \
  --target wasm32-unknown-unknown \
  --profile "$DOCSCAN_PROFILE" \
  --manifest-path "$WORKSPACE_DIR/Cargo.toml"

wasm-bindgen \
  --target web \
  --no-typescript \
  --out-dir "$OUT_DIR" \
  --out-name docscan \
  "$CARGO_TARGET_DIR/wasm32-unknown-unknown/$DOCSCAN_PROFILE/docscan_wasm.wasm"

# Prefer the version pinned in package.json over whatever a machine
# happens to have, so two developers get byte-identical output.
WASM_OPT="$WEB_DIR/node_modules/binaryen/bin/wasm-opt"
[ -x "$WASM_OPT" ] || WASM_OPT="$(command -v wasm-opt || true)"

if [ -n "$WASM_OPT" ] && [ -x "$WASM_OPT" ]; then
  RAW=$(wc -c < "$OUT_DIR/docscan_bg.wasm")
  OPT_FLAGS=(-O3 --enable-bulk-memory --enable-nontrapping-float-to-int)
  [ "$DOCSCAN_SIMD" = "1" ] && OPT_FLAGS+=(--enable-simd)
  # `-O3`, not `-Oz`. This module is fetched once and then served from the
  # service worker cache forever, so a few tens of kilobytes cost one
  # download; the pixel loops inside it run on every frame of a live
  # camera preview on a phone.
  "$WASM_OPT" "${OPT_FLAGS[@]}" -o "$OUT_DIR/docscan_bg.wasm.tmp" "$OUT_DIR/docscan_bg.wasm"
  mv "$OUT_DIR/docscan_bg.wasm.tmp" "$OUT_DIR/docscan_bg.wasm"
  NEW=$(wc -c < "$OUT_DIR/docscan_bg.wasm")
  echo "wasm-opt: $RAW -> $NEW bytes"
else
  echo "wasm-opt not found — skipping (works, just larger and slower)"
fi

# Precompressed siblings, so the dev server and any static host can serve
# the small one. Brotli roughly halves what gzip manages on wasm.
for FILE in "$OUT_DIR/docscan_bg.wasm" "$OUT_DIR/docscan.js"; do
  gzip -9 -kf "$FILE"
  if command -v brotli >/dev/null 2>&1; then
    brotli -f -q 11 -o "$FILE.br" "$FILE"
  fi
done

SIZE=$(wc -c < "$OUT_DIR/docscan_bg.wasm")
GZ=$(wc -c < "$OUT_DIR/docscan_bg.wasm.gz")
BR=$( [ -f "$OUT_DIR/docscan_bg.wasm.br" ] && wc -c < "$OUT_DIR/docscan_bg.wasm.br" || echo 0)
echo "wasm build complete (simd=$DOCSCAN_SIMD, profile=$DOCSCAN_PROFILE)"
echo "  docscan_bg.wasm  $SIZE bytes  (gzip $GZ, brotli $BR)"
