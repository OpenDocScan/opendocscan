//! Runs the real pipeline over real images and prints what it made of them.
//!
//! Not a test and not a fixture: the point is to take files that were not
//! made for this program, put them through the same functions the browser
//! calls, and read the output adversarially. Fixtures agree with whatever
//! produced them, which is exactly why they pass while real input breaks.
//!
//!     cargo run --release -p docscan-wasm --example inspect -- FILE...
//!
//! For each file it reports what detection found, what the quad implies
//! about the page, what each filter did to the histogram, and what the
//! PDF came out weighing — the numbers that would look wrong if a stage
//! had quietly stopped working.

use std::path::Path;
use std::time::Instant;

fn main() {
    let files: Vec<String> = std::env::args().skip(1).collect();
    if files.is_empty() {
        eprintln!("usage: inspect FILE...");
        std::process::exit(2);
    }

    let mut failures = 0;
    for path in &files {
        if let Err(error) = inspect(Path::new(path)) {
            println!("  !! {error}");
            failures += 1;
        }
        println!();
    }

    println!("{} file(s), {failures} could not be read", files.len());
}

fn inspect(path: &Path) -> Result<(), String> {
    println!("=== {}", path.display());

    let decoded = Instant::now();
    let img = image::open(path).map_err(|e| format!("{e}"))?;
    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();
    let pixels = rgba.into_raw();
    let megapixels = (width as f64 * height as f64) / 1e6;
    println!(
        "  {width}x{height}  {megapixels:.1} MP  decoded in {:?}",
        decoded.elapsed()
    );

    let started = Instant::now();
    let quad = docscan_wasm::detect_quad(&pixels, width, height);
    let detect_ms = started.elapsed().as_secs_f64() * 1000.0;

    let corners = match &quad {
        Some(flat) => {
            let corners: Vec<(f32, f32)> = flat
                .as_chunks::<2>()
                .0
                .iter()
                .map(|&[x, y]| (x, y))
                .collect();
            // The fraction of the frame the page covers, and how far from
            // a rectangle it is. A detector that has quietly started
            // returning the whole frame shows up here as 100% and 0
            // degrees of skew, which looks like success and is not.
            let area = shoelace(&corners);
            let coverage = area / (width as f64 * height as f64) * 100.0;
            let skew = max_corner_deviation(&corners);
            println!(
                "  detect      {detect_ms:6.1} ms   page found, {coverage:.0}% of frame, corners off square by up to {skew:.0} deg"
            );
            for (i, (x, y)) in corners.iter().enumerate() {
                print!(
                    "{}({x:.0},{y:.0})",
                    if i == 0 { "              " } else { " " }
                );
            }
            println!();
            corners
        }
        None => {
            println!("  detect      {detect_ms:6.1} ms   no page found — falling back to the whole frame");
            vec![
                (0.0, 0.0),
                (width as f32, 0.0),
                (width as f32, height as f32),
                (0.0, height as f32),
            ]
        }
    };

    let flat: Vec<f32> = corners.iter().flat_map(|&(x, y)| [x, y]).collect();
    let started = Instant::now();
    let page = docscan_wasm::rectify(pixels.clone(), width, height, &flat, 2400)
        .map_err(|_| "rectify refused these corners".to_string())?;
    let rectify_ms = started.elapsed().as_secs_f64() * 1000.0;
    let (page_w, page_h) = (page.width(), page.height());
    let page_pixels = page.into_data();
    println!(
        "  rectify     {rectify_ms:6.1} ms   {page_w}x{page_h}, aspect {:.2}",
        page_w as f64 / page_h as f64
    );

    for filter in ["original", "bw", "enhance"] {
        let started = Instant::now();
        let out = docscan_wasm::apply_filter(page_pixels.clone(), page_w, page_h, filter, 0)
            .map_err(|_| format!("filter {filter} failed"))?;
        let ms = started.elapsed().as_secs_f64() * 1000.0;
        let data = out.into_data();
        let hist = docscan_filters::luma_histogram(&data);
        let total: u64 = hist.iter().map(|&c| u64::from(c)).sum();
        let mean: f64 = hist
            .iter()
            .enumerate()
            .map(|(v, &c)| v as f64 * c as f64)
            .sum::<f64>()
            / total as f64;
        // How much of the page is nearly white. A scan of a document
        // should be mostly paper; a number near zero means the filter
        // has darkened the page rather than cleaned it.
        let paper: f64 =
            hist[230..].iter().map(|&c| u64::from(c)).sum::<u64>() as f64 / total as f64 * 100.0;
        let ink: f64 =
            hist[..40].iter().map(|&c| u64::from(c)).sum::<u64>() as f64 / total as f64 * 100.0;
        println!(
            "  {filter:<10}  {ms:6.1} ms   mean luma {mean:5.1}, {paper:5.1}% paper, {ink:5.1}% ink"
        );

        if filter == "bw" {
            let stray = data
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|px| px[0] != 0 && px[0] != 255)
                .count();
            println!("              bw pixels that are neither black nor white: {stray}");
        }
    }

    // Written out so the pipeline's opinion can be looked at rather than
    // inferred from statistics. Every real defect found by this example
    // was visible in one of these and merely *suspicious* in the numbers.
    if let Ok(dir) = std::env::var("INSPECT_OUT") {
        let stem = path.file_stem().unwrap_or_default().to_string_lossy();
        let out = format!("{dir}/{stem}-rectified.png");
        if let Some(buf) = image::RgbaImage::from_raw(page_w, page_h, page_pixels.clone()) {
            let _ = buf.save(&out);
            println!("              wrote {out}");
        }
    }

    let started = Instant::now();
    let mut builder = docscan_wasm::PdfBuilder::new("a4", 18.0, 82, None)
        .map_err(|_| "PdfBuilder refused a4".to_string())?;
    builder
        .add_page(page_pixels, page_w, page_h, None)
        .map_err(|_| "addPage refused the rectified page".to_string())?;
    let pdf = builder.finish().map_err(|_| "finish failed".to_string())?;
    let ms = started.elapsed().as_secs_f64() * 1000.0;
    println!(
        "  pdf         {ms:6.1} ms   {} KB, starts {:?}",
        pdf.len() / 1024,
        std::str::from_utf8(&pdf[..5]).unwrap_or("??")
    );
    Ok(())
}

fn shoelace(corners: &[(f32, f32)]) -> f64 {
    let n = corners.len();
    let mut sum = 0.0;
    for i in 0..n {
        let (x0, y0) = corners[i];
        let (x1, y1) = corners[(i + 1) % n];
        sum += x0 as f64 * y1 as f64 - x1 as f64 * y0 as f64;
    }
    sum.abs() / 2.0
}

/// The largest amount, in degrees, by which any interior angle of the quad
/// differs from a right angle — a single number for "how tilted was this".
fn max_corner_deviation(corners: &[(f32, f32)]) -> f64 {
    let n = corners.len();
    (0..n)
        .map(|i| {
            let prev = corners[(i + n - 1) % n];
            let here = corners[i];
            let next = corners[(i + 1) % n];
            let a = (prev.0 - here.0, prev.1 - here.1);
            let b = (next.0 - here.0, next.1 - here.1);
            let dot = a.0 as f64 * b.0 as f64 + a.1 as f64 * b.1 as f64;
            let mag = ((a.0 as f64).hypot(a.1 as f64)) * ((b.0 as f64).hypot(b.1 as f64));
            if mag == 0.0 {
                return 90.0;
            }
            ((dot / mag).clamp(-1.0, 1.0).acos().to_degrees() - 90.0).abs()
        })
        .fold(0.0, f64::max)
}
