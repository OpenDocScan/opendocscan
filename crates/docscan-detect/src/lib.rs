use image::{DynamicImage, GrayImage};
use imageproc::contours::find_contours;
use imageproc::edges::canny;
use imageproc::geometry::convex_hull;
use imageproc::point::Point as IPoint;

/// Re-exported from `docscan-core`, which owns the shared geometry types, so
/// callers can name the corner type without a second import.
pub use docscan_core::Point;

/// Upper bound on the number of convex-hull vertices handed to the exact
/// maximum-area-quadrilateral search in [`max_area_quad`].
///
/// That search is cubic in the vertex count, and a noisy real-world contour
/// can yield a hull with hundreds of vertices, so bigger hulls are thinned
/// first (see [`thin_hull`]). This is a compute budget, not a geometric
/// tuning knob: any value comfortably above 4 leaves the true corners of a
/// page-shaped hull untouched, because thinning always sacrifices the
/// flattest vertex it can find and a real corner is never the flattest.
const MAX_HULL_VERTICES: usize = 32;

/// Smallest fraction of the frame a candidate quad may cover and still be
/// reported as a document.
///
/// Without it every image has a "best" quad: an image of nothing but noise
/// still yields contours, and the largest quad among them — a few dozen
/// pixels across — is returned with exactly the same confidence as a real
/// page. A minimum area turns "the largest candidate" into "a candidate
/// large enough to be a photographed document", which is the actual
/// question, and lets the detector answer `None`.
///
/// 2% is deliberately permissive, chosen from measurements rather than
/// taste. The ceiling is the smallest labeled fixture in this crate's
/// harness, at 4.7% of its frame — a user photographing a receipt at arm's
/// length is a real case this must not reject, so the threshold has to stay
/// well under that. The floor is what noise produces: the largest quad an
/// edge-tangle yields is a roughly fixed *absolute* size (a few thousand
/// pixels), so on a real capture it lands at ~1% of frame and shrinks
/// further as resolution grows.
///
/// That framing is also the honest limit of this filter: because the noise
/// floor is absolute and this bound is relative, the two only separate at
/// capture resolutions. On a thumbnail-sized frame a noise tangle can clear
/// 2%. This is a coarse plausibility gate on images from a camera, not a
/// document classifier.
const MIN_QUAD_AREA_FRACTION: f64 = 0.02;

/// Smallest ratio of a candidate quad's shortest side to its longest.
///
/// Area alone does not say whether a shape is a page. Run over real
/// documents rather than synthetic ones, the largest quad in a frame is
/// very often a sliver: a ruled line across a table, or — worse — a
/// near-collapsed shape stretched along the frame's diagonal, which has a
/// large area and no width. Two of six real documents produced one, and
/// what the user got back was a hundred-pixel strip of a table in the
/// first case and a black triangle in the second.
///
/// A document is not a sliver. Even a long till receipt photographed end
/// to end is around five to one, so this bound is set at eight to one:
/// loose enough that no real page is near it, tight enough that the two
/// failures above are both well outside. It is measured on the quad's
/// own edges rather than on its bounding box, so a page lying diagonally
/// across the frame is judged by its shape and not by its orientation.
const MIN_QUAD_SIDE_RATIO: f64 = 0.125;

/// Finds the quadrilateral of the largest document-like region in `img`.
///
/// Returns its four corners ordered top-left, top-right, bottom-right,
/// bottom-left — the input contract of `docscan-transform::warp_to_quad`.
///
/// Returns `None` when the image contains no plausible document: either no
/// contour reduces to a quadrilateral at all, or the best one is too small
/// a fraction of the frame to be one (see [`MIN_QUAD_AREA_FRACTION`]).
pub fn find_document_quad(img: &DynamicImage) -> Option<[Point; 4]> {
    find_document_quad_luma(&img.to_luma8())
}

/// [`find_document_quad`], given the luma plane directly.
///
/// Detection never looks at colour: the first thing it did with the image
/// it was handed was throw two thirds of it away. Callers that already
/// have a luma plane — or that can produce one more cheaply than by
/// materialising a full colour image first — should say so, and skip both
/// the conversion and the colour buffer it needs. On the browser's capture
/// path that is the difference between resizing four channels of a
/// twelve-megapixel frame and resizing one.
pub fn find_document_quad_luma(gray: &GrayImage) -> Option<[Point; 4]> {
    let (width, height) = gray.dimensions();
    let min_area = f64::from(width) * f64::from(height) * MIN_QUAD_AREA_FRACTION;
    let edges = canny(gray, 50.0, 100.0);
    let contours = find_contours::<i32>(&edges);

    contours
        .iter()
        .filter(|c| c.points.len() >= 4)
        .filter_map(|c| corner_quad(&c.points))
        .filter(is_page_shaped)
        .map(|quad| (polygon_area(&quad), quad))
        .filter(|(area, _)| *area >= min_area)
        .max_by(|(a, _), (b, _)| a.total_cmp(b))
        .map(|(_, quad)| quad)
}

/// Whether a quad is a plausible photograph of a document, as opposed to
/// a sliver that merely happens to be the largest thing in the frame.
///
/// See [`MIN_QUAD_SIDE_RATIO`] for why this is not a matter of taste: run
/// over documents rather than fixtures, the unfiltered detector's answer
/// is wrong often enough to be the app's most visible defect.
fn is_page_shaped(quad: &[Point; 4]) -> bool {
    let sides: [f64; 4] = [
        side_length(quad[0], quad[1]),
        side_length(quad[1], quad[2]),
        side_length(quad[2], quad[3]),
        side_length(quad[3], quad[0]),
    ];
    let longest = sides.iter().copied().fold(0.0, f64::max);
    let shortest = sides.iter().copied().fold(f64::INFINITY, f64::min);

    // A quad with a zero-length side is not merely thin, it is degenerate,
    // and the projection that would flatten it does not exist.
    longest > 0.0 && shortest / longest >= MIN_QUAD_SIDE_RATIO
}

fn side_length(a: Point, b: Point) -> f64 {
    f64::from(b.0 - a.0).hypot(f64::from(b.1 - a.1))
}

/// Reduces one traced contour to the four corners of the shape it outlines.
///
/// The contour is first replaced by its convex hull — a sheet of paper is
/// convex, so this discards concavities caused by edge noise, shadows and
/// gaps in the Canny response without moving the true corners — and the
/// hull is then reduced to its inscribed quadrilateral of maximum area.
///
/// Maximum inscribed area is the right objective because for a hull that
/// really is a (digitised) quadrilateral the maximum is attained exactly at
/// its four corners: pulling any vertex inwards along the hull strictly
/// loses area. That is what makes this robust where the previous
/// "topmost / bottommost / leftmost / rightmost point" heuristic was not —
/// those four extremes coincide pairwise as soon as a page is close to
/// axis-aligned (the top-left corner is both the topmost and the leftmost
/// point), silently collapsing the detected quad into a triangle.
fn corner_quad(points: &[IPoint<i32>]) -> Option<[Point; 4]> {
    let mut hull: Vec<(f64, f64)> = convex_hull(points.to_vec())
        .iter()
        .map(|p| (p.x as f64, p.y as f64))
        .collect();
    if hull.len() < 4 {
        return None;
    }
    thin_hull(&mut hull, MAX_HULL_VERTICES);
    Some(order_clockwise_from_top_left(max_area_quad(&hull)?))
}

/// Drops hull vertices until at most `target` remain, each time removing the
/// one whose removal costs the least area.
///
/// Vertices sitting on the staircase of a digitised straight edge cost a
/// sub-pixel sliver and go first; a true corner costs the whole triangle
/// spanned by its two neighbours, which only grows as the neighbours move
/// apart, so corners survive.
fn thin_hull(hull: &mut Vec<(f64, f64)>, target: usize) {
    while hull.len() > target {
        let n = hull.len();
        let mut victim = 0;
        let mut lowest_cost = f64::INFINITY;
        for i in 0..n {
            let cost = triangle_area(hull[(i + n - 1) % n], hull[i], hull[(i + 1) % n]);
            if cost < lowest_cost {
                lowest_cost = cost;
                victim = i;
            }
        }
        hull.remove(victim);
    }
}

/// Exhaustively finds the maximum-area quadrilateral whose corners are
/// vertices of the convex polygon `hull` (given in cyclic order).
///
/// Every quadrilateral splits along a diagonal into two triangles that share
/// it, so each candidate is enumerated by its diagonal `(i, j)` and then
/// completed independently on both sides by the vertex that spans the most
/// area. The returned corners keep the hull's cyclic order, so they always
/// describe a simple (non-self-intersecting) polygon.
fn max_area_quad(hull: &[(f64, f64)]) -> Option<[(f64, f64); 4]> {
    let n = hull.len();
    if n < 4 {
        return None;
    }

    let mut best_area = f64::NEG_INFINITY;
    let mut best = None;
    for i in 0..n {
        // Leave at least one vertex free on each side of the diagonal.
        for offset in 2..n - 1 {
            let j = (i + offset) % n;
            let (near_area, k) = widest_apex(hull, i, j);
            let (far_area, l) = widest_apex(hull, j, i);
            if near_area + far_area > best_area {
                best_area = near_area + far_area;
                best = Some([hull[i], hull[k], hull[j], hull[l]]);
            }
        }
    }
    best
}

/// Of the vertices strictly between `from` and `to` (walking forward around
/// the hull), the one forming the largest triangle with them, and its area.
///
/// Callers guarantee that arc is non-empty.
fn widest_apex(hull: &[(f64, f64)], from: usize, to: usize) -> (f64, usize) {
    let n = hull.len();
    let mut best = (f64::NEG_INFINITY, from);
    let mut idx = (from + 1) % n;
    while idx != to {
        let area = triangle_area(hull[from], hull[idx], hull[to]);
        if area > best.0 {
            best = (area, idx);
        }
        idx = (idx + 1) % n;
    }
    best
}

/// Rotates a cyclically-ordered quad into the top-left, top-right,
/// bottom-right, bottom-left order that `docscan-transform` expects.
///
/// The corners already arrive in cyclic order, so only the winding direction
/// and the starting vertex are open questions; fixing them by rotation
/// (rather than assigning each corner a role independently) keeps the
/// polygon simple no matter how the page is oriented.
fn order_clockwise_from_top_left(mut quad: [(f64, f64); 4]) -> [Point; 4] {
    // Image coordinates are y-down, so clockwise on screen is the positive
    // shoelace direction.
    if signed_area(&quad) < 0.0 {
        quad.reverse();
    }
    let start = (0..4)
        .min_by(|&a, &b| (quad[a].0 + quad[a].1).total_cmp(&(quad[b].0 + quad[b].1)))
        .unwrap_or(0);

    let mut ordered = [(0.0f32, 0.0f32); 4];
    for (offset, corner) in ordered.iter_mut().enumerate() {
        let (x, y) = quad[(start + offset) % 4];
        *corner = (x as f32, y as f32);
    }
    ordered
}

/// Twice-the-triangle by cross product, halved — always non-negative.
fn triangle_area(a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> f64 {
    ((b.0 - a.0) * (c.1 - a.1) - (c.0 - a.0) * (b.1 - a.1)).abs() / 2.0
}

/// Shoelace area of a quad, signed by winding direction.
fn signed_area(quad: &[(f64, f64); 4]) -> f64 {
    let mut area = 0.0_f64;
    for i in 0..4 {
        let (x1, y1) = quad[i];
        let (x2, y2) = quad[(i + 1) % 4];
        area += x1 * y2 - x2 * y1;
    }
    area / 2.0
}

fn polygon_area(quad: &[Point; 4]) -> f64 {
    signed_area(&quad.map(|(x, y)| (x as f64, y as f64))).abs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgb};
    use imageproc::drawing::draw_polygon_mut;
    use imageproc::point::Point as IPoint;

    fn synthetic_page(width: u32, height: u32, quad: [Point; 4]) -> image::DynamicImage {
        let mut buf = ImageBuffer::from_pixel(width, height, Rgb([20u8, 20, 20]));
        let poly: Vec<IPoint<i32>> = quad
            .iter()
            .map(|&(x, y)| IPoint::new(x as i32, y as i32))
            .collect();
        draw_polygon_mut(&mut buf, &poly, Rgb([230, 230, 230]));
        image::DynamicImage::ImageRgb8(buf)
    }

    fn rasterize_quad(quad: &[Point; 4], width: u32, height: u32) -> Vec<bool> {
        let mut buf: ImageBuffer<image::Luma<u8>, Vec<u8>> =
            ImageBuffer::from_pixel(width, height, image::Luma([0]));
        let poly: Vec<IPoint<i32>> = quad
            .iter()
            .map(|&(x, y)| IPoint::new(x as i32, y as i32))
            .collect();
        imageproc::drawing::draw_polygon_mut(&mut buf, &poly, image::Luma([255]));
        buf.pixels().map(|p| p.0[0] > 0).collect()
    }

    fn quad_iou(a: &[Point; 4], b: &[Point; 4], width: u32, height: u32) -> f64 {
        let mask_a = rasterize_quad(a, width, height);
        let mask_b = rasterize_quad(b, width, height);
        let intersection = mask_a
            .iter()
            .zip(&mask_b)
            .filter(|(x, y)| **x && **y)
            .count();
        let union = mask_a
            .iter()
            .zip(&mask_b)
            .filter(|(x, y)| **x || **y)
            .count();
        intersection as f64 / union.max(1) as f64
    }

    /// A labeled synthetic fixture: known image dimensions plus the exact
    /// ground-truth quad that was rendered into it, so detector output can
    /// be scored against a known-correct answer.
    struct Fixture {
        name: &'static str,
        width: u32,
        height: u32,
        quad: [Point; 4],
    }

    /// Five distinct quad geometries, chosen to stress different aspects of
    /// the Canny-edges -> contours -> convex-hull -> maximum-area inscribed
    /// quadrilateral detector: a near-rectangle, a rotated rectangle, a
    /// strong trapezoid, a small quad with lots of background margin, and a
    /// quad hugging two edges of the frame. Coordinates are fixed (no RNG)
    /// for reproducibility.
    fn fixtures() -> [Fixture; 5] {
        [
            // Near-axis-aligned: a slightly skewed rectangle, close to a
            // plain axis-aligned page scan. (This is the fixture from
            // Task 1's single-case test, folded into the labeled set.)
            Fixture {
                name: "near_axis_aligned",
                width: 400,
                height: 300,
                quad: [(40.0, 30.0), (360.0, 45.0), (350.0, 270.0), (30.0, 260.0)],
            },
            // Rotated: an exact rectangle (300x200, centered) rotated by
            // 15 degrees, so all four corners are offset by a consistent
            // rotation angle rather than an arbitrary skew.
            Fixture {
                name: "rotated_15deg",
                width: 400,
                height: 300,
                quad: [(81.0, 15.0), (371.0, 92.0), (319.0, 285.0), (29.0, 208.0)],
            },
            // Trapezoidal: strong perspective, top edge much narrower than
            // the bottom edge (as if a page were photographed from a low
            // angle).
            Fixture {
                name: "trapezoidal_perspective",
                width: 400,
                height: 300,
                quad: [(150.0, 30.0), (250.0, 30.0), (370.0, 270.0), (30.0, 270.0)],
            },
            // Small-relative-size: the quad occupies a small fraction of
            // the full frame, with wide background margin on all sides.
            Fixture {
                name: "small_relative_size",
                width: 400,
                height: 300,
                quad: [
                    (160.0, 110.0),
                    (240.0, 115.0),
                    (235.0, 190.0),
                    (165.0, 185.0),
                ],
            },
            // Near-edge: the quad sits close to the top and left borders of
            // the frame, leaving only a few pixels of margin on those two
            // sides.
            Fixture {
                name: "near_edge",
                width: 400,
                height: 300,
                quad: [(6.0, 6.0), (340.0, 12.0), (330.0, 270.0), (12.0, 264.0)],
            },
        ]
    }

    /// Gate from the M0 plan: mean IoU across the labeled fixture set must
    /// be >= 0.9, and no single fixture may fall below a hard floor of
    /// 0.75 (so one bad outlier can't hide behind a good average).
    #[test]
    fn detects_synthetic_pages_within_iou_gates() {
        const MEAN_THRESHOLD: f64 = 0.9;
        const PER_FIXTURE_FLOOR: f64 = 0.75;

        let results: Vec<(&'static str, f64)> = fixtures()
            .iter()
            .map(|f| {
                let img = synthetic_page(f.width, f.height, f.quad);
                let detected = find_document_quad(&img)
                    .unwrap_or_else(|| panic!("fixture '{}': should detect a quad", f.name));
                let iou = quad_iou(&f.quad, &detected, f.width, f.height);
                (f.name, iou)
            })
            .collect();

        let mean_iou = results.iter().map(|(_, iou)| iou).sum::<f64>() / results.len() as f64;

        let below_floor: Vec<String> = results
            .iter()
            .filter(|(_, iou)| *iou < PER_FIXTURE_FLOOR)
            .map(|(name, iou)| format!("{name}={iou:.4}"))
            .collect();

        let summary: String = results
            .iter()
            .map(|(name, iou)| format!("{name}={iou:.4}"))
            .collect::<Vec<_>>()
            .join(", ");

        assert!(
            below_floor.is_empty(),
            "fixture(s) below the per-image IoU floor of {PER_FIXTURE_FLOOR}: {} (all results: {summary})",
            below_floor.join(", ")
        );

        assert!(
            mean_iou >= MEAN_THRESHOLD,
            "mean IoU {mean_iou:.4} below the {MEAN_THRESHOLD} threshold (all results: {summary})"
        );
    }

    /// The border pixels of an axis-aligned rectangle, as a contour would
    /// trace them.
    fn rectangle_outline(x0: i32, y0: i32, x1: i32, y1: i32) -> Vec<IPoint<i32>> {
        let mut points = Vec::new();
        for x in x0..=x1 {
            points.push(IPoint::new(x, y0));
            points.push(IPoint::new(x, y1));
        }
        for y in y0..=y1 {
            points.push(IPoint::new(x0, y));
            points.push(IPoint::new(x1, y));
        }
        points
    }

    /// Regression test for the corner-collapse bug this crate shipped with.
    /// On an axis-aligned rectangle the topmost point is also the leftmost
    /// point, so picking the four compass extremes independently returned a
    /// duplicated corner and a degenerate, triangular "quad".
    #[test]
    fn recovers_all_four_corners_of_an_axis_aligned_rectangle() {
        let outline = rectangle_outline(10, 20, 110, 80);
        let quad = corner_quad(&outline).expect("a rectangle outline is a quadrilateral");

        let mut corners = quad.to_vec();
        corners.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
        assert_eq!(
            corners,
            vec![(10.0, 20.0), (10.0, 80.0), (110.0, 20.0), (110.0, 80.0)],
            "detected quad: {quad:?}"
        );
    }

    /// `docscan-transform::warp_to_quad` documents its input as ordered
    /// top-left, top-right, bottom-right, bottom-left; detection has to
    /// hand it corners in that order even for a rotated page.
    #[test]
    fn returns_corners_ordered_from_the_top_left_clockwise() {
        let expected: [Point; 4] = [(81.0, 15.0), (371.0, 92.0), (319.0, 285.0), (29.0, 208.0)];
        let img = synthetic_page(400, 300, expected);
        let detected = find_document_quad(&img).expect("should detect a quad");

        for (i, (want, got)) in expected.iter().zip(&detected).enumerate() {
            assert!(
                (want.0 - got.0).abs() <= 2.0 && (want.1 - got.1).abs() <= 2.0,
                "corner {i}: expected ~{want:?} but got {got:?} (full quad: {detected:?})"
            );
        }
    }

    /// Generalisation guard: the five labeled fixtures are a fixed target, so
    /// this sweeps a rectangle through every orientation instead, catching any
    /// corner-finding rule that only works at the angles those fixtures happen
    /// to use. (The original extreme-point rule fails the 0- and 90-degree
    /// ends of this sweep outright.)
    #[test]
    fn detects_a_rectangle_at_every_orientation() {
        let (cx, cy, half_w, half_h) = (200.0f64, 150.0, 120.0, 80.0);

        for step in 0..=18 {
            let theta = (step as f64) * 5.0 * std::f64::consts::PI / 180.0;
            let (sin, cos) = theta.sin_cos();
            let corner = |dx: f64, dy: f64| -> Point {
                (
                    (cx + dx * cos - dy * sin) as f32,
                    (cy + dx * sin + dy * cos) as f32,
                )
            };
            let expected: [Point; 4] = [
                corner(-half_w, -half_h),
                corner(half_w, -half_h),
                corner(half_w, half_h),
                corner(-half_w, half_h),
            ];

            let img = synthetic_page(400, 300, expected);
            let detected = find_document_quad(&img)
                .unwrap_or_else(|| panic!("{}deg: should detect a quad", step * 5));
            let iou = quad_iou(&expected, &detected, 400, 300);

            assert!(
                iou >= 0.95,
                "{}deg: IoU {iou:.4} (expected {expected:?}, detected {detected:?})",
                step * 5
            );
        }
    }

    /// A document-free image: no shape is drawn into it, just a
    /// high-frequency per-pixel pattern derived from the coordinates by a
    /// cheap integer hash. Deterministic by construction (no RNG crate, no
    /// run-to-run variation) while still giving Canny plenty of edges to
    /// trace, which is exactly the situation the area filter exists for.
    fn noise_image(width: u32, height: u32) -> image::DynamicImage {
        let mut buf: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::new(width, height);
        for (x, y, pixel) in buf.enumerate_pixels_mut() {
            let hash = (x.wrapping_mul(73_856_093) ^ y.wrapping_mul(19_349_663))
                .wrapping_mul(2_654_435_761);
            let value = (hash >> 13) as u8;
            *pixel = Rgb([value, value, value]);
        }
        image::DynamicImage::ImageRgb8(buf)
    }

    /// Detection has to be able to say "no document here". Before the
    /// minimum-area filter it could not: any image with traceable contours
    /// produced a `Some(quad)`, and on a noise image that quad — a tangle of
    /// edges a few thousand pixels across — was reported exactly like a real
    /// page.
    ///
    /// Both sizes are camera-capture scale, which is the domain
    /// [`MIN_QUAD_AREA_FRACTION`] is defined over: the noise floor is a
    /// roughly fixed absolute area, so it only falls below a *relative*
    /// threshold once the frame is large enough to photograph a page with.
    #[test]
    fn reports_no_document_in_a_document_free_image() {
        for (width, height) in [(800u32, 600u32), (1200, 900)] {
            let img = noise_image(width, height);

            let detected = find_document_quad(&img);

            assert!(
                detected.is_none(),
                "{width}x{height}: expected no document in a noise image, got \
                 {detected:?} (covering {:.2}% of the frame)",
                detected.map_or(0.0, |q| polygon_area(&q) / f64::from(width * height)
                    * 100.0)
            );
        }
    }

    /// Found by running the detector over real documents rather than
    /// synthetic ones, where it is the most visible defect the app had.
    ///
    /// A scanned invoice has no border to find — the page *is* the frame —
    /// so the largest quad in it is whatever is ruled inside: here a table
    /// row. Returning it means the user imports an invoice and gets back a
    /// hundred-pixel strip. Returning nothing means the app falls back to
    /// the whole frame, which for a full-page scan is the right answer.
    #[test]
    fn a_ruled_table_row_is_not_a_page() {
        // A white page filling the frame, with one dark horizontal band
        // across it — the shape a table rule traces.
        let (width, height) = (1200u32, 1600u32);
        let mut buf = ImageBuffer::from_pixel(width, height, Rgb([250u8, 250, 250]));
        for y in 700..760u32 {
            for x in 100..1100u32 {
                buf.put_pixel(x, y, Rgb([30u8, 30, 30]));
            }
        }
        let img = DynamicImage::ImageRgb8(buf);

        let detected = find_document_quad(&img);

        assert!(
            detected.is_none(),
            "a 1000x60 band is 16:1 and must not be offered as a page, got {detected:?}"
        );
    }

    /// The same rule, stated directly, at the boundary either side.
    #[test]
    fn page_shape_admits_a_long_receipt_and_refuses_a_sliver() {
        let receipt: [Point; 4] = [(0.0, 0.0), (300.0, 0.0), (300.0, 1500.0), (0.0, 1500.0)];
        assert!(is_page_shaped(&receipt), "5:1 is a real receipt");

        let sliver: [Point; 4] = [(0.0, 0.0), (1400.0, 0.0), (1400.0, 100.0), (0.0, 100.0)];
        assert!(!is_page_shaped(&sliver), "14:1 is a table row");

        // The shape that produced a black triangle: two corners two pixels
        // apart, the opposite edge two thousand.
        let collapsed: [Point; 4] = [
            (11.0, 105.0),
            (1698.0, 2204.0),
            (1698.0, 2206.0),
            (11.0, 2201.0),
        ];
        assert!(
            !is_page_shaped(&collapsed),
            "a near-collapsed quad is not a page"
        );
    }

    /// A contour with no quadrilateral in it (here, a straight line) must be
    /// discarded rather than reported as a degenerate document.
    #[test]
    fn rejects_a_contour_that_has_no_quadrilateral() {
        let collinear: Vec<IPoint<i32>> = (0..20).map(|x| IPoint::new(x, 5)).collect();
        assert!(corner_quad(&collinear).is_none());
    }
}
