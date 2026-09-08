//! The three scan filters, written as passes over a raw RGBA buffer.
//!
//! Everything a filter does to a document scan is, at bottom, one of two
//! shapes:
//!
//! * a **per-channel map** — brightness and contrast stretching both
//!   answer "what does the byte 137 become?" the same way everywhere in
//!   the image, so the answer can be computed 256 times up front and read
//!   back per byte; and
//! * a **per-pixel decision** — binarisation reads all three channels to
//!   get a luma, so it cannot be a per-channel map, but its *output* is
//!   only ever one of two colours, which can also be precomputed.
//!
//! Writing them that way matters more than it looks. These run inside a
//! phone browser on a page that can be 4 megapixels, while the user drags
//! a brightness slider and expects the preview to follow their thumb. The
//! straightforward version — convert to luma, sort every pixel to find
//! percentiles, allocate an output image, do float arithmetic per channel,
//! then allocate again to apply brightness — spends most of its time on
//! allocation and on recomputing, four million times, an answer that
//! depends on nothing but the byte in front of it.
//!
//! So the entry points here take `&mut [u8]` and rewrite it in place. The
//! `DynamicImage` wrappers below them are kept because they are the
//! honest way to express the operation on the desktop side and because
//! the tests read better against them, but the browser calls the slices.

use image::DynamicImage;

/// Fixed luma threshold used by [`to_black_and_white`]. Chosen as the
/// midpoint of the 0-255 luma range; a reasonable default for document
/// scans without per-image calibration.
const BW_THRESHOLD: u8 = 128;

/// Rec. 709 luma weights, in fixed point.
///
/// These are the weights the `image` crate uses for the same conversion.
/// Doing the arithmetic in integers rather than calling it keeps the
/// result bit-identical across targets — a promise this pipeline makes
/// elsewhere and should not quietly drop in its innermost loop — and
/// agrees with `image`'s float version on all but about 0.4% of colours,
/// where the two land one level apart purely from float rounding.
const LUMA_R: u32 = 2126;
const LUMA_G: u32 = 7152;
const LUMA_B: u32 = 722;
const LUMA_DIV: u32 = 10000;
/// Added before the divide so it rounds instead of truncating. See
/// [`luma`] for why that is worth a constant.
const LUMA_HALF: u32 = LUMA_DIV / 2;

/// The fraction of pixels [`stretch_lut`] pushes past pure black and pure
/// white respectively. Clipping the extremes is what makes the stretch
/// useful on a real photograph: a single specular highlight or one dark
/// speck would otherwise define the whole range and the correction would
/// do almost nothing.
const CLIP_LOW: f64 = 0.01;
const CLIP_HIGH: f64 = 0.99;

/// The grey a colour reads as, rounded rather than truncated.
///
/// The rounding is not cosmetic. Truncating biases every value down by
/// half a level, and *how much* it loses depends on the fractional part,
/// which is a function of the colour — so truncation is not a constant
/// offset but colour-correlated noise of up to one level. Edge detection
/// downstream reads precisely the kind of small local differences that
/// noise manufactures, and this is shared with the detector's luma pass
/// (`docscan-wasm` calls it) so the two cannot drift apart on what grey a
/// colour is.
#[inline]
pub fn luma(r: u8, g: u8, b: u8) -> u8 {
    ((LUMA_R * r as u32 + LUMA_G * g as u32 + LUMA_B * b as u32 + LUMA_HALF) / LUMA_DIV) as u8
}

/// A precomputed per-channel colour map: `lut[v]` is what the byte `v`
/// becomes.
///
/// This is the whole optimisation in one type. Any composition of
/// per-channel operations — stretch then brighten, in this crate's case —
/// is itself a per-channel operation, so composing them costs 256 steps
/// once instead of one step per byte, and the pixel loop that applies the
/// result never does arithmetic at all.
pub type Lut = [u8; 256];

/// The identity map, optionally shifted by `brightness`.
pub fn brightness_lut(brightness: i32) -> Lut {
    let mut lut = [0u8; 256];
    for (value, out) in lut.iter_mut().enumerate() {
        *out = (value as i32 + brightness).clamp(0, 255) as u8;
    }
    lut
}

/// Counts how many pixels fall in each of the 256 luma buckets.
///
/// This replaces sorting every pixel to read percentiles off the sorted
/// array. Both answer the same question, but a histogram is one linear
/// pass into one kilobyte, where the sort was `n log n` over a freshly
/// allocated copy of every pixel in the image — four megabytes of
/// allocation and roughly twenty million comparisons on a full page, to
/// find two numbers.
///
/// `rgba` is read four bytes at a time; a trailing partial pixel, which a
/// well-formed buffer never has, is ignored.
pub fn luma_histogram(rgba: &[u8]) -> [u32; 256] {
    let mut hist = [0u32; 256];
    for &[r, g, b, _] in rgba.as_chunks::<4>().0 {
        hist[luma(r, g, b) as usize] += 1;
    }
    hist
}

/// The contrast-stretching map implied by `hist`, composed with a
/// `brightness` shift.
///
/// Returns `None` when there is nothing to stretch — an empty image, or
/// one whose 1st and 99th percentiles coincide, which is a flat image and
/// where the stretch would be a division by zero. The caller should fall
/// back to [`brightness_lut`]; returning `None` rather than silently
/// substituting it keeps "this image has no usable spread" distinguishable
/// from "this image was stretched".
pub fn stretch_lut(hist: &[u32; 256], brightness: i32) -> Option<Lut> {
    let total: u64 = hist.iter().map(|&c| u64::from(c)).sum();
    if total == 0 {
        return None;
    }

    // The same two percentile *indices* the sort-based version took, found
    // by walking the cumulative histogram instead of indexing a sorted
    // array. `low_idx` counts from the bottom and `high_idx` from the top,
    // matching `values[(len * 0.01) as usize]` and
    // `values[min(len - 1, (len * 0.99) as usize)]` on the same data.
    let low_rank = (total as f64 * CLIP_LOW) as u64;
    let high_rank = ((total as f64 * CLIP_HIGH) as u64).min(total - 1);

    let mut low = 0u8;
    let mut high = 255u8;
    let mut seen = 0u64;
    let mut have_low = false;
    for (value, &count) in hist.iter().enumerate() {
        if count == 0 {
            continue;
        }
        let next = seen + u64::from(count);
        if !have_low && next > low_rank {
            low = value as u8;
            have_low = true;
        }
        if next > high_rank {
            high = value as u8;
            break;
        }
        seen = next;
    }

    if high <= low {
        return None;
    }

    let (low, high) = (f32::from(low), f32::from(high));
    let mut lut = [0u8; 256];
    for (value, out) in lut.iter_mut().enumerate() {
        let stretched = (value as f32 - low) / (high - low) * 255.0;
        let stretched = stretched.round().clamp(0.0, 255.0) as i32;
        *out = (stretched + brightness).clamp(0, 255) as u8;
    }
    Some(lut)
}

/// Rewrites every colour channel of `rgba` through `lut`, leaving alpha
/// untouched.
pub fn apply_lut_rgba(rgba: &mut [u8], lut: &Lut) {
    for px in rgba.as_chunks_mut::<4>().0 {
        px[0] = lut[px[0] as usize];
        px[1] = lut[px[1] as usize];
        px[2] = lut[px[2] as usize];
    }
}

/// Binarises `rgba` in place: each pixel becomes black or white by luma,
/// then `brightness` shifts the two results.
///
/// The shift is applied to the two possible outputs rather than to the
/// pixels, which is both faster and the behaviour the UI wants — the
/// filter decides what kind of image this is and the slider then nudges
/// the result, so brightening can lift a black-and-white scan off pure
/// black without ever changing which pixels were called black.
pub fn binarize_rgba(rgba: &mut [u8], brightness: i32) {
    let dark = brightness.clamp(0, 255) as u8;
    let light = (255 + brightness).clamp(0, 255) as u8;
    for px in rgba.as_chunks_mut::<4>().0 {
        let value = if luma(px[0], px[1], px[2]) >= BW_THRESHOLD {
            light
        } else {
            dark
        };
        px[0] = value;
        px[1] = value;
        px[2] = value;
    }
}

/// Contrast-stretch `rgba` in place, then shift it by `brightness`.
///
/// One histogram pass and one lookup pass, whatever the image size.
pub fn enhance_rgba(rgba: &mut [u8], brightness: i32) {
    let hist = luma_histogram(rgba);
    let lut = stretch_lut(&hist, brightness).unwrap_or_else(|| brightness_lut(brightness));
    apply_lut_rgba(rgba, &lut);
}

/// Binarizes the image for document-scan readability: pixels are mapped to
/// pure black or pure white based on a fixed luma threshold (not just
/// grayscale/desaturation).
pub fn to_black_and_white(img: &DynamicImage) -> DynamicImage {
    let mut rgba = img.to_rgba8();
    binarize_rgba(&mut rgba, 0);
    DynamicImage::ImageRgba8(rgba)
}

/// Shifts brightness by `delta` (positive brightens, negative darkens),
/// clamping each channel to the valid [0, 255] range.
pub fn adjust_brightness(img: &DynamicImage, delta: i32) -> DynamicImage {
    let mut rgba = img.to_rgba8();
    apply_lut_rgba(&mut rgba, &brightness_lut(delta));
    DynamicImage::ImageRgba8(rgba)
}

/// Adaptive contrast enhancement for scanned pages: stretches the luma
/// histogram so the effective darkest and lightest values expand toward
/// black/white, improving readability of low-contrast scans.
///
/// An image with no usable spread — no pixels at all, or a flat one — is
/// returned unchanged: there is no histogram to read percentiles from.
pub fn enhance(img: &DynamicImage) -> DynamicImage {
    let mut rgba = img.to_rgba8();
    let hist = luma_histogram(&rgba);
    match stretch_lut(&hist, 0) {
        Some(lut) => {
            apply_lut_rgba(&mut rgba, &lut);
            DynamicImage::ImageRgba8(rgba)
        }
        None => img.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, ImageBuffer, Rgb};

    /// Builds an RGB image of `width x height` where every pixel is the
    /// same gray value `v` (so channel value == luma).
    fn gray_image(width: u32, height: u32, v: u8) -> DynamicImage {
        DynamicImage::ImageRgb8(ImageBuffer::from_pixel(width, height, Rgb([v, v, v])))
    }

    fn average_luma(img: &DynamicImage) -> f64 {
        let luma = img.to_luma8();
        let sum: u64 = luma.pixels().map(|p| p.0[0] as u64).sum();
        sum as f64 / luma.pixels().len() as f64
    }

    #[test]
    fn binarizes_below_and_above_threshold_pixels_to_pure_black_and_white() {
        // 2x1 image: pixel 0 has luma clearly below BW_THRESHOLD (128),
        // pixel 1 clearly above it. Gray pixels so channel value == luma.
        let mut buf: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::new(2, 1);
        buf.put_pixel(0, 0, Rgb([50, 50, 50]));
        buf.put_pixel(1, 0, Rgb([200, 200, 200]));
        let img = DynamicImage::ImageRgb8(buf);

        let out = to_black_and_white(&img).to_rgb8();

        assert_eq!(*out.get_pixel(0, 0), Rgb([0, 0, 0]));
        assert_eq!(*out.get_pixel(1, 0), Rgb([255, 255, 255]));
    }

    #[test]
    fn increasing_brightness_raises_average_luma_by_approximately_delta() {
        let img = gray_image(5, 5, 128);
        let delta = 40;

        let out = adjust_brightness(&img, delta);

        let before = average_luma(&img);
        let after = average_luma(&out);
        let diff = after - before;
        assert!(
            (diff - delta as f64).abs() <= 5.0,
            "expected luma to increase by ~{delta}, got {diff}"
        );
    }

    #[test]
    fn brightening_a_near_white_image_clamps_at_255() {
        let img = gray_image(4, 4, 250);

        let out = adjust_brightness(&img, 40).to_rgb8();

        for pixel in out.pixels() {
            assert_eq!(*pixel, Rgb([255, 255, 255]));
        }
    }

    #[test]
    fn darkening_a_near_black_image_clamps_at_0() {
        let img = gray_image(4, 4, 5);

        let out = adjust_brightness(&img, -40).to_rgb8();

        for pixel in out.pixels() {
            assert_eq!(*pixel, Rgb([0, 0, 0]));
        }
    }

    #[test]
    fn enhance_stretches_a_narrow_luma_band_toward_black_and_white() {
        // Explicit low-contrast image: luma values confined to 100-140.
        let values: [u8; 9] = [100, 105, 110, 115, 120, 125, 130, 135, 140];
        let mut buf: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::new(values.len() as u32, 1);
        for (x, &v) in values.iter().enumerate() {
            buf.put_pixel(x as u32, 0, Rgb([v, v, v]));
        }
        let img = DynamicImage::ImageRgb8(buf);

        let out = enhance(&img);

        let (in_min, in_max) = (*values.iter().min().unwrap(), *values.iter().max().unwrap());
        let out_luma = out.to_luma8();
        let out_min = out_luma.pixels().map(|p| p.0[0]).min().unwrap();
        let out_max = out_luma.pixels().map(|p| p.0[0]).max().unwrap();

        assert!(
            out_min < in_min,
            "expected stretched min {out_min} < original min {in_min}"
        );
        assert!(
            out_max > in_max,
            "expected stretched max {out_max} > original max {in_max}"
        );
    }

    /// A zero-pixel image has no histogram, so the 99th-percentile index
    /// computation used to underflow (`len - 1` on `len == 0`) and panic
    /// before the flat-image guard could return. `image` permits zero
    /// dimensions, so this is constructible — and a filter panicking on a
    /// buffer it was handed is never the right answer.
    #[test]
    fn enhancing_a_zero_pixel_image_returns_it_unchanged() {
        for (width, height) in [(0u32, 0u32), (0, 8), (8, 0)] {
            let img = DynamicImage::ImageRgb8(ImageBuffer::new(width, height));

            let out = enhance(&img);

            assert_eq!(
                out.to_rgb8().dimensions(),
                (width, height),
                "{width}x{height}: dimensions should survive untouched"
            );
        }
    }
}
