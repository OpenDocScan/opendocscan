use image::{DynamicImage, ImageBuffer, Rgb, RgbaImage};
use imageproc::geometric_transformations::{warp_into, Interpolation, Projection};

/// Re-exported from `docscan-core`, which owns the shared geometry types, so
/// callers can name the corner type without a second import.
pub use docscan_core::Point;

/// The output size a quad should be rectified into, in pixels.
///
/// `warp_to_quad` takes the output dimensions as an argument because the
/// caller sometimes has an opinion — fitting a known paper size, or
/// capping memory on a huge capture. But most callers have no opinion and
/// just want the page at the size it was actually photographed, and
/// answering that badly is how a scan comes out subtly squashed.
///
/// Each dimension takes the longer of its two opposing edges. A page
/// photographed at an angle has one long edge and one foreshortened one;
/// the long edge is the one nearer the camera and therefore the one
/// carrying real detail, so sizing to it resamples up from the squashed
/// side rather than throwing away resolution from the sharp side.
pub fn natural_output_size(quad: [Point; 4]) -> (u32, u32) {
    let [tl, tr, br, bl] = quad;
    let width = edge_length(tl, tr).max(edge_length(bl, br));
    let height = edge_length(tl, bl).max(edge_length(tr, br));
    // A degenerate quad must still name a size a buffer can be allocated
    // for; one pixel is the smallest honest answer.
    (
        width.round().max(1.0) as u32,
        height.round().max(1.0) as u32,
    )
}

fn edge_length(a: Point, b: Point) -> f32 {
    ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt()
}

/// Warps the quadrilateral region `quad` (source-image coordinates, ordered
/// top-left, top-right, bottom-right, bottom-left) into a rectangular output
/// image of size `output_width x output_height`, correcting perspective.
///
/// Output pixels whose pre-image falls outside the source image are filled
/// with black. The transform is a pure function of its inputs: identical
/// arguments always produce a byte-identical output image.
///
/// Returns `None` if `quad` is degenerate — three collinear corners, or two
/// corners on top of each other — because no projection maps such a shape
/// onto a rectangle. That is a reachable input, not a programming error: the
/// corner handles are user-draggable, and dragging one onto another is an
/// ordinary thing to do, so callers get a value to handle rather than a
/// panic.
pub fn warp_to_quad(
    img: &DynamicImage,
    quad: [Point; 4],
    output_width: u32,
    output_height: u32,
) -> Option<DynamicImage> {
    let src = img.to_rgb8();
    let projection = page_projection(quad, output_width, output_height)?;

    let black = Rgb([0u8, 0, 0]);
    let mut out: ImageBuffer<Rgb<u8>, Vec<u8>> =
        ImageBuffer::from_pixel(output_width, output_height, black);
    warp_into(&src, &projection, Interpolation::Bilinear, black, &mut out);

    Some(DynamicImage::ImageRgb8(out))
}

/// [`warp_to_quad`] for callers already holding RGBA, staying in RGBA.
///
/// Same projection, same interpolation, same guarantees — the only
/// difference is which buffers get allocated. The `DynamicImage` version
/// above converts its input to RGB and its caller then converts the result
/// back, which on a twelve-megapixel capture is two full-image passes and
/// two large allocations spent arriving at pixels that were already in the
/// right layout. The browser is that caller: canvas hands over RGBA and
/// wants RGBA back, and the alpha channel it carries is a constant 255 in
/// both directions.
///
/// Pixels whose pre-image falls outside the source are filled with opaque
/// black, matching [`warp_to_quad`]; a transparent fill would look right
/// on a canvas and then print as a black margin anyway once the page is
/// flattened into a PDF.
pub fn warp_rgba_to_quad(
    src: &RgbaImage,
    quad: [Point; 4],
    output_width: u32,
    output_height: u32,
) -> Option<RgbaImage> {
    // Asked for its own sake: it is the authority on whether these four
    // corners describe a page at all, and reusing it means the fast path
    // below accepts and rejects exactly the same quads the general one
    // does, rather than having its own opinion about degeneracy.
    page_projection(quad, output_width, output_height)?;
    let map = InverseMap::for_quad(quad, output_width, output_height)?;

    let (src_width, src_height) = src.dimensions();
    let src_raw = src.as_raw();
    let stride = src_width as usize * 4;
    // The last row and column can never be the *top-left* of a bilinear
    // sample — the sample needs a neighbour on each side — which is the
    // bound `imageproc` enforces by comparing against `width` and
    // `height` after adding one. Hoisted out of the loop as a float,
    // because that comparison happens several million times.
    let max_u = (src_width as f32) - 1.0;
    let max_v = (src_height as f32) - 1.0;

    let mut out: Vec<u8> = vec![0; output_width as usize * output_height as usize * 4];

    for (y, row) in out.chunks_exact_mut(output_width as usize * 4).enumerate() {
        // The source coordinate is a ratio of two functions that are each
        // affine in x, so walking a row means adding a constant to three
        // accumulators rather than evaluating the projection from scratch.
        // They are `f64` deliberately: at `f32` the error from two
        // thousand accumulated additions reaches a noticeable fraction of
        // a pixel by the right-hand edge, and a rectified page that
        // smears toward one side is precisely the defect this whole
        // pipeline exists to remove.
        let (mut nu, mut nv, mut den) = map.row_start(y as f64);

        for px in row.as_chunks_mut::<4>().0 {
            let inv = 1.0 / den;
            let u = (nu * inv) as f32;
            let v = (nv * inv) as f32;
            nu += map.du;
            nv += map.dv;
            den += map.dd;

            // Positive comparisons, so a NaN coordinate — reachable from a
            // near-degenerate quad — falls through to the black fill
            // instead of indexing on garbage.
            if !(u >= 0.0 && u < max_u && v >= 0.0 && v < max_v) {
                px[3] = 255;
                continue;
            }

            let x0 = u as usize;
            let y0 = v as usize;
            let fx = u - x0 as f32;
            let fy = v - y0 as f32;

            let top = y0 * stride + x0 * 4;
            let bottom = top + stride;
            let (w_tl, w_tr) = ((1.0 - fx) * (1.0 - fy), fx * (1.0 - fy));
            let (w_bl, w_br) = ((1.0 - fx) * fy, fx * fy);

            for c in 0..3 {
                let value = src_raw[top + c] as f32 * w_tl
                    + src_raw[top + 4 + c] as f32 * w_tr
                    + src_raw[bottom + c] as f32 * w_bl
                    + src_raw[bottom + 4 + c] as f32 * w_br;
                // Rounded once, at the end. The general path this
                // replaces blends horizontally, truncates to `u8`,
                // blends horizontally again, truncates again, then
                // blends vertically and truncates a third time —
                // `imageproc`'s `Clamp<f32> for u8` is a bare `as u8`,
                // which floors. Three floors bias every interpolated
                // pixel downwards by up to one and a half levels, so
                // this is not merely different from the old behaviour,
                // it is the correction of it.
                px[c] = value.round().clamp(0.0, 255.0) as u8;
            }
            px[3] = 255;
        }
    }

    RgbaImage::from_raw(output_width, output_height, out)
}

/// The output-pixel-to-source-pixel map, in the form the warp loop wants.
///
/// A projective map sends `(x, y)` to `(nu/den, nv/den)` where all three
/// of `nu`, `nv` and `den` are affine in `x` and `y`. Storing it this way
/// — the value at the start of a row, plus the constant each accumulator
/// gains per step along it — is what lets the inner loop cost three
/// additions and one reciprocal instead of a full matrix evaluation.
struct InverseMap {
    /// Coefficients of `nu`, `nv` and `den` as `(x, y, 1)` respectively.
    u: [f64; 3],
    v: [f64; 3],
    d: [f64; 3],
    du: f64,
    dv: f64,
    dd: f64,
}

impl InverseMap {
    /// Builds the map sending the `output_width` x `output_height`
    /// rectangle onto `quad`.
    ///
    /// This is the *inverse* of the transform a reader would write down —
    /// the warp pulls each output pixel from the source rather than
    /// pushing source pixels out, which is what makes every output pixel
    /// get written exactly once and no gaps appear between them — so it is
    /// built in that direction rather than built forwards and inverted.
    ///
    /// Uses Heckbert's closed form for the unit square onto a quad, with
    /// the two scale factors folded into the coefficients so the caller
    /// can pass raw pixel coordinates. Returns `None` if the quad is
    /// degenerate enough to make the coefficients non-finite.
    fn for_quad(quad: [Point; 4], output_width: u32, output_height: u32) -> Option<InverseMap> {
        let [(x0, y0), (x1, y1), (x2, y2), (x3, y3)] = quad.map(|(x, y)| (x as f64, y as f64));

        let (sx, sy) = (x0 - x1 + x2 - x3, y0 - y1 + y2 - y3);
        let (dx1, dx2) = (x1 - x2, x3 - x2);
        let (dy1, dy2) = (y1 - y2, y3 - y2);

        // A quad whose opposite edges stay parallel maps affinely, and the
        // projective branch's denominator vanishes for exactly that case.
        let (g, h) = if sx == 0.0 && sy == 0.0 {
            (0.0, 0.0)
        } else {
            let den = dx1 * dy2 - dx2 * dy1;
            if den == 0.0 {
                return None;
            }
            ((sx * dy2 - dx2 * sy) / den, (dx1 * sy - sx * dy1) / den)
        };

        let (a, b, c) = (x1 - x0 + g * x1, x3 - x0 + h * x3, x0);
        let (d, e, f) = (y1 - y0 + g * y1, y3 - y0 + h * y3, y0);

        // Fold the unit-square scaling in, so the loop can index in pixels.
        let (inv_w, inv_h) = (
            1.0 / f64::from(output_width),
            1.0 / f64::from(output_height),
        );
        let map = InverseMap {
            u: [a * inv_w, b * inv_h, c],
            v: [d * inv_w, e * inv_h, f],
            d: [g * inv_w, h * inv_h, 1.0],
            du: a * inv_w,
            dv: d * inv_w,
            dd: g * inv_w,
        };

        let finite = map
            .u
            .iter()
            .chain(map.v.iter())
            .chain(map.d.iter())
            .all(|value| value.is_finite());
        finite.then_some(map)
    }

    /// The three accumulators at `x == 0` of row `y`.
    #[inline]
    fn row_start(&self, y: f64) -> (f64, f64, f64) {
        (
            self.u[1] * y + self.u[2],
            self.v[1] * y + self.v[2],
            self.d[1] * y + self.d[2],
        )
    }
}

/// Projection mapping source-image locations (`quad`) to their rectified
/// locations in an `output_width` x `output_height` image.
///
/// `warp_into` inverts this internally to pull each output pixel from the
/// correct source location.
fn page_projection(quad: [Point; 4], output_width: u32, output_height: u32) -> Option<Projection> {
    let output_rect: [Point; 4] = [
        (0.0, 0.0),
        (output_width as f32, 0.0),
        (output_width as f32, output_height as f32),
        (0.0, output_height as f32),
    ];
    Projection::from_control_points(quad, output_rect)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The hand-written RGBA warp exists only to be faster than the
    /// general one; the moment it is also *different* it is a bug, not an
    /// optimisation. This pins the two together on a genuinely projective
    /// quad — one with no two edges parallel, so the perspective division
    /// is doing real work on every pixel and an error in the incremental
    /// stepping would show up as a drift across the row rather than a
    /// uniform offset.
    #[test]
    fn the_fast_rgba_warp_agrees_with_the_general_one() {
        let quad: [Point; 4] = [(31.0, 24.0), (505.0, 61.0), (470.0, 372.0), (58.0, 331.0)];
        let source = detailed_image(560, 420);

        let reference = warp_to_quad(&source, quad, 300, 240).unwrap().to_rgb8();
        let fast = warp_rgba_to_quad(&source.to_rgba8(), quad, 300, 240).unwrap();

        let mut worst = 0i16;
        let mut total = 0i64;
        let mut signed = 0i64;
        let mut count = 0i64;
        for (rgb, rgba) in reference.pixels().zip(fast.pixels()) {
            for channel in 0..3 {
                let delta = rgba.0[channel] as i16 - rgb.0[channel] as i16;
                worst = worst.max(delta.abs());
                total += i64::from(delta.abs());
                signed += i64::from(delta);
                count += 1;
            }
            assert_eq!(rgba.0[3], 255, "the warp must leave every page opaque");
        }

        // The two do not agree exactly, and the difference is not noise.
        // `imageproc` blends the four taps in three stages and truncates
        // to `u8` after each one, because its `Clamp<f32> for u8` is a
        // bare `as u8`; this warp accumulates all four taps and rounds
        // once. So the fast path should sit *above* the general one by
        // something under one level on average — which is what the signed
        // mean asserts, and what distinguishes "corrected the rounding"
        // from "sampled the wrong pixel".
        let mean_signed = signed as f64 / count as f64;
        assert!(
            (0.0..1.0).contains(&mean_signed),
            "expected a small upward correction, got a mean shift of {mean_signed}"
        );

        // A real disagreement about *where* a pixel comes from shows up
        // here as tens or hundreds, not as the two levels three floors can
        // account for.
        assert!(worst <= 2, "worst channel disagreement was {worst}");
        let mean_abs = total as f64 / count as f64;
        assert!(mean_abs < 1.0, "mean channel disagreement was {mean_abs}");
    }

    /// A pattern with detail at every scale, so a sampling error anywhere
    /// in the frame changes some pixel rather than being absorbed by a
    /// flat region.
    fn detailed_image(width: u32, height: u32) -> DynamicImage {
        let mut buf: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::new(width, height);
        for (x, y, px) in buf.enumerate_pixels_mut() {
            *px = Rgb([
                (x * 7 % 256) as u8,
                (y * 11 % 256) as u8,
                ((x ^ y) % 256) as u8,
            ]);
        }
        DynamicImage::ImageRgb8(buf)
    }

    /// The corners are user-draggable, so the fast path has to refuse the
    /// same collapsed quads the general one refuses rather than indexing
    /// on coordinates that came out infinite.
    #[test]
    fn the_fast_rgba_warp_refuses_a_degenerate_quad() {
        let source = detailed_image(64, 64);
        let collapsed: [Point; 4] = [(10.0, 10.0), (10.0, 10.0), (40.0, 40.0), (10.0, 40.0)];

        assert!(warp_rgba_to_quad(&source.to_rgba8(), collapsed, 32, 32).is_none());
    }
    use image::{GenericImageView, ImageBuffer, Rgb};
    use imageproc::drawing::draw_polygon_mut;
    use imageproc::point::Point as IPoint;

    /// Builds a `width x height` background-colored image with `quad` filled
    /// in a distinct foreground color. Equivalent in spirit to the helper in
    /// `docscan-detect`'s test module, but kept local so this crate has no
    /// dependency on `docscan-detect`.
    fn synthetic_quad_image(width: u32, height: u32, quad: [Point; 4]) -> image::DynamicImage {
        let mut buf = ImageBuffer::from_pixel(width, height, BACKGROUND);
        fill_quad(&mut buf, quad, FOREGROUND);
        image::DynamicImage::ImageRgb8(buf)
    }

    const BACKGROUND: Rgb<u8> = Rgb([20u8, 20, 20]);
    const FOREGROUND: Rgb<u8> = Rgb([230u8, 230, 230]);
    /// Stamped onto the page in [`rectifies_a_known_rotation_with_landmark_within_pixel_tolerance`].
    /// Distinguishable from both page and background by a channel test, so
    /// bilinear resampling can blur its edges without confusing the two.
    const LANDMARK: Rgb<u8> = Rgb([0u8, 0, 255]);

    fn fill_quad(buf: &mut ImageBuffer<Rgb<u8>, Vec<u8>>, quad: [Point; 4], color: Rgb<u8>) {
        let poly: Vec<IPoint<i32>> = quad
            .iter()
            .map(|&(x, y)| IPoint::new(x.round() as i32, y.round() as i32))
            .collect();
        draw_polygon_mut(buf, &poly, color);
    }

    fn assert_close(pixel: Rgb<u8>, expected: Rgb<u8>, tolerance: i16) {
        for c in 0..3 {
            let diff = (pixel.0[c] as i16 - expected.0[c] as i16).abs();
            assert!(
                diff <= tolerance,
                "channel {c}: {:?} not within {tolerance} of {:?}",
                pixel,
                expected
            );
        }
    }

    #[test]
    fn rectifies_a_skewed_quad_into_a_clean_rectangle() {
        // A deliberately skewed, non-axis-aligned quad: top-left, top-right,
        // bottom-right, bottom-left, in that order.
        let quad: [Point; 4] = [(60.0, 20.0), (350.0, 60.0), (300.0, 260.0), (40.0, 230.0)];
        let src = synthetic_quad_image(400, 300, quad);

        let (out_w, out_h) = (200u32, 300u32);
        let warped = warp_to_quad(&src, quad, out_w, out_h).expect("quad is non-degenerate");

        assert_eq!(warped.dimensions(), (out_w, out_h));

        let rgb = warped.to_rgb8();
        let foreground = FOREGROUND;
        let inset = 5i64;
        let corners = [
            (inset, inset),
            (out_w as i64 - 1 - inset, inset),
            (out_w as i64 - 1 - inset, out_h as i64 - 1 - inset),
            (inset, out_h as i64 - 1 - inset),
        ];
        for (x, y) in corners {
            let pixel = *rgb.get_pixel(x as u32, y as u32);
            assert_close(pixel, foreground, 30);
        }
    }

    #[test]
    fn warp_is_deterministic_for_identical_inputs() {
        let quad: [Point; 4] = [(60.0, 20.0), (350.0, 60.0), (300.0, 260.0), (40.0, 230.0)];
        let src = synthetic_quad_image(400, 300, quad);

        let first = warp_to_quad(&src, quad, 200, 300).expect("quad is non-degenerate");
        let second = warp_to_quad(&src, quad, 200, 300).expect("quad is non-degenerate");

        assert_eq!(first.to_rgb8().into_raw(), second.to_rgb8().into_raw());
    }

    /// Rotates `point` about `center` by `angle_deg`, in image coordinates
    /// (y down, so a positive angle turns clockwise on screen).
    fn rotate_about(center: Point, angle_deg: f32, point: Point) -> Point {
        let (sin, cos) = angle_deg.to_radians().sin_cos();
        let (dx, dy) = (point.0 - center.0, point.1 - center.1);
        (
            center.0 + dx * cos - dy * sin,
            center.1 + dx * sin + dy * cos,
        )
    }

    /// PLAN.md §6 asks this crate for "known-angle skew corrects to within N
    /// pixels". Checking a handful of corner pixels are the page colour does
    /// not answer that — a stub returning a solid rectangle would pass it —
    /// so this checks *where content lands*.
    ///
    /// A page of known size is rotated by a known angle, with an asymmetric
    /// landmark stamped at a known position on it, and rectified back. The
    /// landmark's bounding box in the output is then compared against the
    /// position the geometry demands. Because the landmark is off-centre in
    /// both axes, a mirrored, transposed, 180-degree-wrong or wrongly-scaled
    /// warp moves it and fails, rather than mapping it onto itself.
    #[test]
    fn rectifies_a_known_rotation_with_landmark_within_pixel_tolerance() {
        const PAGE_W: f32 = 240.0;
        const PAGE_H: f32 = 160.0;
        const ANGLE_DEG: f32 = 25.0;
        // One pixel of slack for rasterising the source polygons, one for
        // bilinear resampling, one spare. Any real directional or magnitude
        // error in the projection displaces the landmark by far more.
        const TOLERANCE: f32 = 3.0;

        // Page-local coordinates: (0,0) is the page's top-left corner,
        // (PAGE_W, PAGE_H) its bottom-right, before any rotation.
        let center = (200.0f32, 150.0f32);
        let to_source = |u: f32, v: f32| -> Point {
            let unrotated = (center.0 - PAGE_W / 2.0 + u, center.1 - PAGE_H / 2.0 + v);
            rotate_about(center, ANGLE_DEG, unrotated)
        };
        let local_quad = |min: (f32, f32), max: (f32, f32)| -> [Point; 4] {
            [
                to_source(min.0, min.1),
                to_source(max.0, min.1),
                to_source(max.0, max.1),
                to_source(min.0, max.1),
            ]
        };

        let landmark_min = (0.10 * PAGE_W, 0.10 * PAGE_H);
        let landmark_max = (0.30 * PAGE_W, 0.25 * PAGE_H);

        let page = local_quad((0.0, 0.0), (PAGE_W, PAGE_H));
        let mut buf = ImageBuffer::from_pixel(400, 300, BACKGROUND);
        fill_quad(&mut buf, page, FOREGROUND);
        fill_quad(&mut buf, local_quad(landmark_min, landmark_max), LANDMARK);
        let src = image::DynamicImage::ImageRgb8(buf);

        let (out_w, out_h) = (PAGE_W as u32, PAGE_H as u32);
        let out = warp_to_quad(&src, page, out_w, out_h)
            .expect("a rotated rectangle is non-degenerate")
            .to_rgb8();

        // The landmark is the only strongly-blue thing in the frame, so a
        // channel test survives the blur bilinear resampling puts on its
        // edges.
        let is_landmark = |p: &Rgb<u8>| p.0[2] > 128 && p.0[0] < 128 && p.0[1] < 128;
        let (mut min_x, mut min_y) = (f32::INFINITY, f32::INFINITY);
        let (mut max_x, mut max_y) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
        for (x, y, pixel) in out.enumerate_pixels() {
            if is_landmark(pixel) {
                min_x = min_x.min(x as f32);
                min_y = min_y.min(y as f32);
                max_x = max_x.max(x as f32);
                max_y = max_y.max(y as f32);
            }
        }
        assert!(
            min_x.is_finite(),
            "no landmark pixels in the rectified output at all"
        );

        for (label, got, want) in [
            ("min x", min_x, landmark_min.0),
            ("min y", min_y, landmark_min.1),
            ("max x", max_x, landmark_max.0),
            ("max y", max_y, landmark_max.1),
        ] {
            assert!(
                (got - want).abs() <= TOLERANCE,
                "landmark {label}: {got} is more than {TOLERANCE}px from the \
                 expected {want} (bbox was [{min_x}, {min_y}]..[{max_x}, {max_y}], \
                 expected [{}, {}]..[{}, {}])",
                landmark_min.0,
                landmark_min.1,
                landmark_max.0,
                landmark_max.1
            );
        }

        // The page fills the output edge to edge: nothing outside the page
        // may be pulled in, which a warp that under- or over-shoots would do.
        let inset = 3u32;
        for y in (inset..out_h - inset).step_by(7) {
            for x in (inset..out_w - inset).step_by(7) {
                let pixel = out.get_pixel(x, y);
                assert!(
                    is_landmark(pixel) || pixel.0[0] > 128,
                    "({x}, {y}) is neither page nor landmark but {pixel:?} — \
                     background leaked into the rectified page"
                );
            }
        }
    }

    /// Corner handles are user-draggable (M2), so a user can drag one corner
    /// onto another or flatten the quad into a line. No projection exists for
    /// those, and the answer must be a `None` to handle, not a panic.
    #[test]
    fn returns_none_for_a_degenerate_quad() {
        let src = synthetic_quad_image(
            100,
            100,
            [(10.0, 10.0), (90.0, 10.0), (90.0, 90.0), (10.0, 90.0)],
        );

        let degenerate: [(&str, [Point; 4]); 3] = [
            (
                "all four corners collinear",
                [(0.0, 0.0), (10.0, 10.0), (20.0, 20.0), (30.0, 30.0)],
            ),
            (
                "two corners dragged onto each other",
                [(10.0, 10.0), (90.0, 10.0), (90.0, 10.0), (10.0, 90.0)],
            ),
            (
                "all four corners in one place",
                [(50.0, 50.0), (50.0, 50.0), (50.0, 50.0), (50.0, 50.0)],
            ),
        ];

        for (label, quad) in degenerate {
            assert!(
                warp_to_quad(&src, quad, 50, 50).is_none(),
                "{label}: expected None for {quad:?}"
            );
        }
    }
}

#[cfg(test)]
mod natural_size_tests {
    use super::*;

    #[test]
    fn an_axis_aligned_rectangle_keeps_its_own_dimensions() {
        let quad = [(10.0, 20.0), (410.0, 20.0), (410.0, 320.0), (10.0, 320.0)];
        assert_eq!(natural_output_size(quad), (400, 300));
    }

    #[test]
    fn a_foreshortened_page_is_sized_to_its_near_edge() {
        // Photographed at an angle: the bottom edge is nearer the camera
        // and 400px long, the top edge recedes to 200px. Sizing to 200
        // would discard half the detail the bottom of the page actually
        // has.
        let quad = [(100.0, 0.0), (300.0, 0.0), (400.0, 500.0), (0.0, 500.0)];
        let (width, _) = natural_output_size(quad);
        assert_eq!(width, 400);
    }

    #[test]
    fn a_rotated_square_measures_its_true_edge_not_its_bounding_box() {
        // A 45-degree square with 100px half-diagonals: each edge is
        // 100 * sqrt(2) ~= 141px, while the bounding box is 200px.
        let quad = [(100.0, 0.0), (200.0, 100.0), (100.0, 200.0), (0.0, 100.0)];
        let (width, height) = natural_output_size(quad);
        assert_eq!((width, height), (141, 141));
    }

    #[test]
    fn a_collapsed_quad_still_names_an_allocatable_size() {
        let quad = [(5.0, 5.0); 4];
        assert_eq!(natural_output_size(quad), (1, 1));
    }
}
