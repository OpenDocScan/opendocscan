//! Timings for the operations the browser waits on, at the sizes it
//! actually uses them. Run with:
//!
//!     cargo run -p docscan-wasm --example bench --release
//!
//! This is a native binary, not the wasm build, so the absolute numbers
//! are optimistic — wasm runs perhaps 1.5-2x slower. What it measures
//! reliably is the *ratio* between two implementations of the same pass,
//! which is what an optimisation has to move.

use image::{DynamicImage, ImageBuffer, Rgba};
use std::time::Instant;

/// A synthetic photographed page: a bright sheet on a dark desk, with
/// text-like marks. Deterministic, so runs are comparable.
fn photo(width: u32, height: u32) -> DynamicImage {
    let mut buf: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::new(width, height);
    let (w, h) = (width as f32, height as f32);
    for (x, y, px) in buf.enumerate_pixels_mut() {
        let (fx, fy) = (x as f32, y as f32);
        // A skewed sheet occupying the middle ~70% of the frame.
        let sheet = fx > w * 0.12 + fy * 0.04
            && fx < w * 0.88 + fy * 0.04
            && fy > h * 0.10
            && fy < h * 0.90;
        let v = if !sheet {
            40 + ((x * 7 + y * 13) % 20) as u8
        } else if (y % 40) < 6 && (x % 160) < 120 {
            60 + ((x * 3) % 30) as u8 // "text"
        } else {
            200 + ((x + y) % 30) as u8 // paper, deliberately not pure white
        };
        *px = Rgba([v, v, v.saturating_add(4), 255]);
    }
    DynamicImage::ImageRgba8(buf)
}

fn time<T>(label: &str, runs: u32, mut f: impl FnMut() -> T) {
    // One untimed pass so allocator warm-up is not charged to the first run.
    let _ = f();
    let start = Instant::now();
    for _ in 0..runs {
        std::hint::black_box(f());
    }
    let per = start.elapsed().as_secs_f64() * 1000.0 / f64::from(runs);
    println!("{label:<44} {per:>8.1} ms");
}

fn main() {
    // The three sizes the app works at: a live preview frame, a capture,
    // and a rectified page.
    let preview = photo(640, 480);
    let capture = photo(3024, 4032);
    let page = photo(1800, 2400);

    let preview_rgba = preview.to_rgba8().into_raw();
    let capture_rgba = capture.to_rgba8().into_raw();
    let page_rgba = page.to_rgba8().into_raw();

    // The owning entry points below are handed a `Vec` because that is
    // what `wasm_bindgen` builds when it copies the caller's array into
    // our heap — exactly once. This bench has no `wasm_bindgen`, so it
    // clones to supply one, and that clone is charged to every timing
    // below. Measure it here so it can be subtracted.
    println!("--- baseline (bench-only overhead) ---");
    time("clone 1800x2400 rgba buffer", 20, || page_rgba.clone());
    time("clone 3024x4032 rgba buffer", 10, || capture_rgba.clone());

    println!("--- detect (live preview path) ---");
    time("detectQuad 640x480", 20, || {
        docscan_wasm::detect_quad(&preview_rgba, 640, 480)
    });

    println!("--- detect (capture path) ---");
    time("detectQuad 3024x4032", 5, || {
        docscan_wasm::detect_quad(&capture_rgba, 3024, 4032)
    });

    println!("--- rectify ---");
    let corners: Vec<f32> = vec![400.0, 420.0, 2650.0, 560.0, 2600.0, 3600.0, 360.0, 3480.0];
    time("rectify 12MP -> 2400px", 5, || {
        docscan_wasm::rectify(capture_rgba.clone(), 3024, 4032, &corners, 2400).ok()
    });

    println!("--- filters (full page) ---");
    for filter in ["original", "bw", "enhance"] {
        time(&format!("applyFilter {filter} 1800x2400"), 10, || {
            docscan_wasm::apply_filter(page_rgba.clone(), 1800, 2400, filter, 0).ok()
        });
        time(&format!("applyFilter {filter} +brightness"), 10, || {
            docscan_wasm::apply_filter(page_rgba.clone(), 1800, 2400, filter, 20).ok()
        });
    }

    println!("--- filters (preview size) ---");
    let prev = photo(825, 1100);
    let prev_rgba = prev.to_rgba8().into_raw();
    time("applyFilter enhance 825x1100", 20, || {
        docscan_wasm::apply_filter(prev_rgba.clone(), 825, 1100, "enhance", 10).ok()
    });

    println!("--- pdf export ---");
    time("PdfBuilder 5 x 1800x2400 -> A4", 3, || {
        let mut b = docscan_wasm::PdfBuilder::new("a4", 18.0, 82, None).unwrap();
        for _ in 0..5 {
            b.add_page(page_rgba.clone(), 1800, 2400, None).unwrap();
        }
        b.finish().ok()
    });
}
