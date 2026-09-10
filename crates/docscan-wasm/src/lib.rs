//! The scanning core, addressable from a browser tab.
//!
//! This is glue and nothing else: every decision about how a page is
//! found, rectified, filtered, or written lives in the crates below it.
//! What this layer adds is the marshalling — raw RGBA in, raw RGBA or PDF
//! bytes out — and two browser-specific concerns the core has no business
//! knowing about: that a live camera preview must be analysed at a size
//! that keeps up with the frame rate, and that a phone tab has a memory
//! ceiling a 12-megapixel capture can walk straight through.
//!
//! Nothing here touches the network, and nothing here *can*: the crate
//! has no HTTP client, and the only imports are `wasm_bindgen`'s own
//! plumbing. That is checkable by reading this file, which is the point —
//! the product's central promise should not require trusting a privacy
//! policy when it can be read off the dependency list instead.

use docscan_core::Point;
use docscan_ocr::{OcrPage, OcrWord};
use docscan_pdf::{ImageCodec, PageLayout, Paper, PdfOptions, ScanPage};
use image::{DynamicImage, GrayImage, RgbaImage};
use serde::Deserialize;
use wasm_bindgen::prelude::*;

/// Longest edge, in pixels, that document detection is allowed to work at.
///
/// Detection runs Canny plus a contour trace, which is superlinear in
/// pixel count, and the live preview calls it several times a second. A
/// page's corners are a large-scale feature — they survive downscaling
/// intact, which is exactly why detection can be cheap — so working at
/// this size and scaling the answer back up costs nothing measurable in
/// accuracy and is the difference between a preview that tracks the page
/// and one that lurches a second behind the camera.
const DETECT_WORKING_EDGE: u32 = 800;

/// Longest edge, in pixels, a rectified page may have by default.
///
/// A 12MP phone capture rectifies to roughly 4000px on its long edge; at
/// four bytes a pixel that is a ~50MB RGBA buffer, and a page tray holding
/// several of those is how a mobile tab gets killed by the OS with no
/// error anyone can catch. 2400px on the long edge is about 300dpi across
/// an A4 sheet — the resolution at which scanned text stops improving to
/// the eye — so this ceiling costs nothing a reader would notice.
const DEFAULT_MAX_PAGE_EDGE: u32 = 2400;

#[wasm_bindgen(start)]
pub fn start() {
    // Turns a Rust panic into a readable console message instead of an
    // opaque `unreachable executed` trap.
    console_error_panic_hook::set_once();
}

/// A raster image crossing the boundary: RGBA8, row-major, no padding —
/// the exact layout of a canvas `ImageData`, so JS can hand it straight to
/// `putImageData` without a repack.
#[wasm_bindgen]
pub struct RasterImage {
    data: Vec<u8>,
    width: u32,
    height: u32,
}

#[wasm_bindgen]
impl RasterImage {
    #[wasm_bindgen(getter)]
    pub fn width(&self) -> u32 {
        self.width
    }

    #[wasm_bindgen(getter)]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// The pixels, moved out rather than copied.
    ///
    /// Takes `self` by value because these buffers are tens of megabytes
    /// and a getter that quietly cloned one would double the peak memory
    /// of every page in the tray. Calling it consumes the image, which is
    /// what the caller wanted anyway.
    #[wasm_bindgen(js_name = intoData)]
    pub fn into_data(self) -> Vec<u8> {
        self.data
    }
}

/// Find the page in this frame.
///
/// Returns the four corners as `[x0, y0, x1, y1, x2, y2, x3, y3]` —
/// top-left, top-right, bottom-right, bottom-left — in the coordinate
/// space of the frame that was passed in, or `null` if the frame holds
/// nothing page-shaped. `null` is an ordinary answer, not a failure: it is
/// what the camera sees while it is being pointed at a desk.
#[wasm_bindgen(js_name = detectQuad)]
pub fn detect_quad(rgba: &[u8], width: u32, height: u32) -> Option<Vec<f32>> {
    // Straight to luma, without ever materialising a colour image.
    // Detection is a shape question and discards colour immediately, so
    // the three buffers the obvious route allocates — an RGBA copy of the
    // frame, a downscaled RGBA copy, and the luma plane taken from it —
    // are two buffers and one pass more than the answer needs. On a
    // twelve-megapixel capture the difference is resampling one channel
    // instead of four.
    let frame = luma_from_rgba(rgba, width, height)?;

    // Detect small, report big. The scale factor is applied to the answer
    // rather than to the image the caller holds, so the corners come back
    // in the caller's own coordinates and no one downstream has to
    // remember that a downscale happened.
    let (working, scale) = downscale_luma_to_edge(frame, DETECT_WORKING_EDGE);
    let quad = docscan_detect::find_document_quad_luma(&working)?;

    Some(
        quad.iter()
            .flat_map(|&(x, y)| [x / scale, y / scale])
            .collect(),
    )
}

/// Flatten the quadrilateral `corners` out of this frame into a
/// rectangular page.
///
/// `max_edge` caps the result's longer side; pass 0 for the default
/// ceiling. The natural size — what the page measures at the resolution it
/// was actually photographed — is computed first and only reduced if it
/// exceeds the cap, so a receipt shot close up is not upscaled to fill it.
#[wasm_bindgen]
pub fn rectify(
    rgba: Vec<u8>,
    width: u32,
    height: u32,
    corners: &[f32],
    max_edge: u32,
) -> Result<RasterImage, JsError> {
    let frame = rgba_from_owned(rgba, width, height)
        .ok_or_else(|| JsError::new("frame dimensions do not match the pixel buffer"))?;
    let quad = quad_from_slice(corners).map_err(|e| JsError::new(&e))?;

    let (natural_width, natural_height) = docscan_transform::natural_output_size(quad);
    let cap = if max_edge == 0 {
        DEFAULT_MAX_PAGE_EDGE
    } else {
        max_edge
    };
    let (out_width, out_height) = cap_to_edge(natural_width, natural_height, cap);

    let warped = docscan_transform::warp_rgba_to_quad(&frame, quad, out_width, out_height)
        .ok_or_else(|| JsError::new("those four corners do not form a page to flatten"))?;

    Ok(RasterImage {
        width: warped.width(),
        height: warped.height(),
        data: warped.into_raw(),
    })
}

/// Apply a scan filter, then a brightness offset.
///
/// The order is deliberate and is the order the UI presents: the filter
/// decides what kind of image this is, and brightness is the user nudging
/// the result. Brightening first and then thresholding to black and white
/// would let the slider silently erase faint text instead of revealing it.
#[wasm_bindgen(js_name = applyFilter)]
pub fn apply_filter(
    mut rgba: Vec<u8>,
    width: u32,
    height: u32,
    filter: &str,
    brightness: i32,
) -> Result<RasterImage, JsError> {
    if !dimensions_match(rgba.len(), width, height) {
        return Err(JsError::new(
            "image dimensions do not match the pixel buffer",
        ));
    }

    // Filter and brightness are applied together, in one pass over the
    // buffer the caller handed us, which is also the buffer we hand back.
    // The order the UI presents is preserved exactly — the filter decides
    // what kind of image this is, and brightness is the user nudging the
    // result — but for the two per-channel filters that order is a
    // property of how the lookup table is *built*, not of how many times
    // the image is walked. Brightening first and then thresholding to
    // black and white would let the slider silently erase faint text
    // instead of revealing it, so `bw` composes the two the same way.
    match filter {
        "original" => {
            if brightness != 0 {
                docscan_filters::apply_lut_rgba(
                    &mut rgba,
                    &docscan_filters::brightness_lut(brightness),
                );
            }
        }
        // Both spatial, because both are applied to a photograph of a page
        // rather than to a scan of one. The lighting is part of the input here,
        // and neither a single lookup table nor a single cutoff can see it.
        "bw" => docscan_filters::binarize_adaptive_rgba(&mut rgba, width, height, brightness),
        "enhance" => docscan_filters::enhance_page_rgba(&mut rgba, width, height, brightness),
        other => return Err(JsError::new(&format!("unknown filter: {other}"))),
    }

    Ok(RasterImage {
        data: rgba,
        width,
        height,
    })
}

/// One word as the browser's OCR engine reported it.
#[derive(Deserialize)]
struct WordJson {
    text: String,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    #[serde(default = "full_confidence")]
    confidence: f32,
}

fn full_confidence() -> f32 {
    1.0
}

#[derive(Deserialize)]
struct OcrJson {
    words: Vec<WordJson>,
    width: u32,
    height: u32,
}

/// Assembles pages into a PDF one at a time.
///
/// A builder rather than a single call taking an array, because the array
/// version means every page's pixels are alive in JS *and* in Rust at the
/// same moment. Here each page is handed over, encoded, and its RGBA
/// dropped before the next one arrives — which is the difference between
/// a twenty-page export working on a phone and the tab being killed.
#[wasm_bindgen]
pub struct PdfBuilder {
    pages: Vec<ScanPage>,
    options: PdfOptions,
}

#[wasm_bindgen]
impl PdfBuilder {
    /// `paper` is `"a4"`, `"letter"`, `"legal"`, or `"original"` — the
    /// last meaning each sheet is the page itself at `dpi`, rather than a
    /// fixed size.
    #[wasm_bindgen(constructor)]
    pub fn new(
        paper: &str,
        margin_pt: f32,
        quality: u8,
        title: Option<String>,
    ) -> Result<PdfBuilder, JsError> {
        let layout = match paper {
            "a4" => PageLayout::FitTo {
                paper: Paper::A4,
                margin_pt,
            },
            "letter" => PageLayout::FitTo {
                paper: Paper::LETTER,
                margin_pt,
            },
            "legal" => PageLayout::FitTo {
                paper: Paper::LEGAL,
                margin_pt,
            },
            "original" => PageLayout::ImageAtDpi(300.0),
            other => return Err(JsError::new(&format!("unknown paper size: {other}"))),
        };

        Ok(PdfBuilder {
            pages: Vec::new(),
            options: PdfOptions {
                layout,
                codec: ImageCodec::Auto { quality },
                // An empty title is the user not having named the
                // document, which should write no title at all rather than
                // an empty string.
                title: title.filter(|t| !t.trim().is_empty()),
                ..Default::default()
            },
        })
    }

    /// Add one page, optionally with the text OCR read in it.
    ///
    /// `ocr_json` carries the words in the coordinate space of whatever
    /// image the engine was given, which is frequently a downscale of this
    /// one — the PDF layer rescales them, so the caller does not have to.
    #[wasm_bindgen(js_name = addPage)]
    pub fn add_page(
        &mut self,
        rgba: Vec<u8>,
        width: u32,
        height: u32,
        ocr_json: Option<String>,
    ) -> Result<(), JsError> {
        let img = rgba_from_owned(rgba, width, height)
            .map(DynamicImage::ImageRgba8)
            .ok_or_else(|| JsError::new("page dimensions do not match the pixel buffer"))?;

        let ocr = match ocr_json {
            Some(json) => Some(parse_ocr(&json, width, height).map_err(|e| JsError::new(&e))?),
            None => None,
        };

        self.pages.push(ScanPage { image: img, ocr });
        Ok(())
    }

    #[wasm_bindgen(getter, js_name = pageCount)]
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Write the document out. Consumes the builder — the pages are moved
    /// into the writer rather than copied.
    pub fn finish(self) -> Result<Vec<u8>, JsError> {
        docscan_pdf::build_pdf(&self.pages, &self.options).map_err(|e| JsError::new(&e.to_string()))
    }
}

/// Validation returns plain `String` errors rather than `JsError`, and the
/// boundary functions convert. `JsError` cannot be constructed off a wasm
/// target, so a helper that built one directly would be a helper no host
/// test could ever call — and these are exactly the functions worth
/// testing, since they are where a JS caller's bad input is caught.
fn parse_ocr(json: &str, page_width: u32, page_height: u32) -> Result<OcrPage, String> {
    let parsed: OcrJson =
        serde_json::from_str(json).map_err(|e| format!("bad OCR payload: {e}"))?;

    Ok(OcrPage {
        words: parsed
            .words
            .into_iter()
            .map(|w| OcrWord {
                text: w.text,
                x: w.x,
                y: w.y,
                width: w.width,
                height: w.height,
                confidence: w.confidence,
            })
            .collect(),
        // An engine that did not say what it measured against was
        // measuring against this page.
        width: if parsed.width > 0 {
            parsed.width
        } else {
            page_width
        },
        height: if parsed.height > 0 {
            parsed.height
        } else {
            page_height
        },
    })
}

/// Whether `len` bytes is exactly a `width` x `height` RGBA image.
///
/// Every entry point checks this before trusting the two numbers it was
/// told, because they arrive from JavaScript separately from the buffer
/// and nothing but this check stops a mismatched pair from being read as
/// pixels.
fn dimensions_match(len: usize, width: u32, height: u32) -> bool {
    if width == 0 || height == 0 {
        return false;
    }
    (width as usize)
        .checked_mul(height as usize)
        .and_then(|px| px.checked_mul(4))
        .is_some_and(|expected| len == expected)
}

/// The caller's buffer adopted as an image, without copying it.
///
/// `wasm_bindgen` has already copied these bytes once, out of the JS heap
/// and into ours, to make the `Vec`. Taking it by value rather than by
/// slice means that copy is the only one: a twelve-megapixel frame is
/// forty-eight megabytes, and doing it twice is both the time and — on a
/// phone, where it decides whether the tab survives — the memory that
/// matters.
fn rgba_from_owned(rgba: Vec<u8>, width: u32, height: u32) -> Option<RgbaImage> {
    if !dimensions_match(rgba.len(), width, height) {
        return None;
    }
    RgbaImage::from_raw(width, height, rgba)
}

/// The luma plane of an RGBA buffer, computed in one pass.
///
/// This is the plane `to_luma8` would have produced, without building a
/// colour image to ask for it. The weighting — and the rounding, which
/// matters more than it sounds — lives in `docscan-filters`, so the
/// detector and the filters cannot come to disagree about what grey a
/// colour is.
fn luma_from_rgba(rgba: &[u8], width: u32, height: u32) -> Option<GrayImage> {
    if !dimensions_match(rgba.len(), width, height) {
        return None;
    }
    let gray: Vec<u8> = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&[r, g, b, _]| docscan_filters::luma(r, g, b))
        .collect();
    GrayImage::from_raw(width, height, gray)
}

fn quad_from_slice(corners: &[f32]) -> Result<[Point; 4], String> {
    if corners.len() != 8 {
        return Err("a page has four corners, so this wants eight numbers".into());
    }
    if corners.iter().any(|v| !v.is_finite()) {
        return Err("corner coordinates must be finite numbers".into());
    }
    Ok([
        (corners[0], corners[1]),
        (corners[2], corners[3]),
        (corners[4], corners[5]),
        (corners[6], corners[7]),
    ])
}

/// Shrink `img` so its longer edge is at most `edge`, returning the image
/// and the factor its coordinates were multiplied by.
///
/// An image already inside the limit is returned untouched with a scale of
/// 1.0 — resampling it would cost time and lose detail to buy nothing.
fn downscale_luma_to_edge(img: GrayImage, edge: u32) -> (GrayImage, f32) {
    let (width, height) = (img.width(), img.height());
    let longest = width.max(height);
    if longest <= edge {
        return (img, 1.0);
    }
    let scale = edge as f32 / longest as f32;
    let target_width = ((width as f32 * scale).round() as u32).max(1);
    let target_height = ((height as f32 * scale).round() as u32).max(1);
    (
        image::imageops::resize(
            &img,
            target_width,
            target_height,
            image::imageops::FilterType::Triangle,
        ),
        // The real scale, recovered from the rounded dimensions, so
        // mapping corners back lands where the pixels actually are rather
        // than where the unrounded arithmetic said they would.
        target_width as f32 / width as f32,
    )
}

/// `width x height` reduced so its longer side is at most `edge`, aspect
/// preserved. Never enlarges.
fn cap_to_edge(width: u32, height: u32, edge: u32) -> (u32, u32) {
    let longest = width.max(height);
    if longest <= edge || edge == 0 {
        return (width.max(1), height.max(1));
    }
    let scale = edge as f32 / longest as f32;
    (
        ((width as f32 * scale).round() as u32).max(1),
        ((height as f32 * scale).round() as u32).max(1),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_buffer_that_does_not_match_its_stated_size_is_rejected() {
        // The one place a JS caller can corrupt the core: claiming
        // dimensions the buffer cannot back. Every entry point routes
        // through this one check before it indexes anything, so this is
        // the test that keeps a mismatched pair from being read as pixels.
        assert!(dimensions_match(16, 2, 2));
        assert!(!dimensions_match(15, 2, 2));
        assert!(!dimensions_match(17, 2, 2));
        assert!(!dimensions_match(16, 0, 2));
        assert!(!dimensions_match(16, 2, 0));
    }

    #[test]
    fn a_dimension_pair_that_would_overflow_is_rejected_rather_than_wrapping() {
        assert!(!dimensions_match(16, u32::MAX, u32::MAX));
    }

    /// The detector's luma plane and the filters' threshold have to be
    /// the same function, or a page can be detected against one notion of
    /// grey and binarised against another.
    #[test]
    fn the_detector_and_the_filters_agree_on_grey() {
        let rgba: Vec<u8> = (0u32..256)
            .flat_map(|i| {
                let (r, g, b) = (i as u8, (i * 3) as u8, (i * 7) as u8);
                [r, g, b, 255]
            })
            .collect();

        let plane = luma_from_rgba(&rgba, 256, 1).unwrap();

        for (px, &grey) in rgba.as_chunks::<4>().0.iter().zip(plane.as_raw()) {
            assert_eq!(grey, docscan_filters::luma(px[0], px[1], px[2]));
        }
    }

    #[test]
    fn a_quad_needs_exactly_eight_finite_numbers() {
        assert!(quad_from_slice(&[0.0; 8]).is_ok());
        assert!(quad_from_slice(&[0.0; 6]).is_err());
        assert!(quad_from_slice(&[0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, f32::NAN]).is_err());
    }

    #[test]
    fn a_quad_arrives_in_corner_order() {
        let quad = quad_from_slice(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]).unwrap();
        assert_eq!(quad, [(1.0, 2.0), (3.0, 4.0), (5.0, 6.0), (7.0, 8.0)]);
    }

    #[test]
    fn an_image_already_small_enough_is_not_resampled() {
        let img = GrayImage::new(400, 300);
        let (out, scale) = downscale_luma_to_edge(img, 800);
        assert_eq!((out.width(), out.height()), (400, 300));
        assert_eq!(scale, 1.0);
    }

    #[test]
    fn downscaling_reports_the_scale_the_pixels_actually_got() {
        // Not the requested ratio — the one implied by the rounded output
        // size, or corners mapped back land a pixel or two off.
        let img = GrayImage::new(1999, 1000);
        let (out, scale) = downscale_luma_to_edge(img, 800);
        assert_eq!(out.width(), 800);
        assert_eq!(scale, 800.0 / 1999.0);
    }

    #[test]
    fn capping_a_page_never_enlarges_a_small_one() {
        // A receipt shot close up should stay its own size rather than
        // being upsampled to fill the ceiling.
        assert_eq!(cap_to_edge(600, 900, 2400), (600, 900));
    }

    #[test]
    fn capping_a_page_preserves_its_aspect() {
        let (width, height) = cap_to_edge(4000, 3000, 2400);
        assert_eq!(width, 2400);
        assert_eq!(height, 1800);
    }

    #[test]
    fn ocr_that_omits_its_reference_size_is_assumed_to_mean_this_page() {
        let page = parse_ocr(
            r#"{"words":[{"text":"hi","x":1,"y":2,"width":3,"height":4}],"width":0,"height":0}"#,
            640,
            480,
        )
        .unwrap();
        assert_eq!((page.width, page.height), (640, 480));
        // An engine that reports no score is not the same as one reporting
        // zero, so the default must keep the word.
        assert_eq!(page.words[0].confidence, 1.0);
    }

    #[test]
    fn a_malformed_ocr_payload_is_an_error_not_a_silent_empty_page() {
        assert!(parse_ocr("{not json", 10, 10).is_err());
    }
}
