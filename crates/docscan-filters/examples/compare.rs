//! Run the shipped filters over an image and write the results out, so the
//! output can be looked at rather than reasoned about.
//!
//!   cargo run -p docscan-filters --release --example compare -- <in.png> <outdir>

fn main() {
    let mut args = std::env::args().skip(1);
    let input = args.next().expect("usage: compare <in.png> <outdir>");
    let outdir = args.next().unwrap_or_else(|| ".".into());

    let img = image::open(&input).expect("open").to_rgba8();
    let (w, h) = img.dimensions();
    println!("  input {w}x{h}");

    let mut enhanced = img.clone();
    docscan_filters::enhance_page_rgba(&mut enhanced, w, h, 0);
    enhanced.save(format!("{outdir}/enhance.png")).unwrap();

    let mut bw = img.clone();
    docscan_filters::binarize_adaptive_rgba(&mut bw, w, h, 0);
    bw.save(format!("{outdir}/bw.png")).unwrap();

    // What the global stretch actually chose, which is the whole story.
    let hist = docscan_filters::luma_histogram(&img);
    let total: u64 = hist.iter().map(|&c| u64::from(c)).sum();
    let mut seen = 0u64;
    let (mut p1, mut p99) = (0u32, 255u32);
    for (v, &c) in hist.iter().enumerate() {
        seen += u64::from(c);
        if p1 == 0 && seen as f64 > total as f64 * 0.01 {
            p1 = v as u32;
        }
        if seen as f64 > total as f64 * 0.99 {
            p99 = v as u32;
            break;
        }
    }
    println!("  global stretch maps luma {p1}..{p99} onto 0..255");

    // What the spatial pass costs, since filtering re-runs on every drag of
    // the brightness slider.
    for name in ["flatten", "enhance_page", "binarize_adaptive"] {
        let mut buf = img.clone();
        let t = std::time::Instant::now();
        match name {
            "flatten" => docscan_filters::flatten_illumination_rgba(&mut buf, w, h),
            "enhance_page" => docscan_filters::enhance_page_rgba(&mut buf, w, h, 0),
            _ => docscan_filters::binarize_adaptive_rgba(&mut buf, w, h, 0),
        }
        println!(
            "  {name:<18} {:>6.1} ms",
            t.elapsed().as_secs_f64() * 1000.0
        );
    }

    for (name, im) in [("original", &img), ("enhance", &enhanced), ("bw", &bw)] {
        let hist = docscan_filters::luma_histogram(im);
        let total: u64 = hist.iter().map(|&c| u64::from(c)).sum();
        let paper = hist[200..].iter().map(|&c| u64::from(c)).sum::<u64>();
        let ink = hist[..60].iter().map(|&c| u64::from(c)).sum::<u64>();
        let mean: f64 = hist
            .iter()
            .enumerate()
            .map(|(v, &c)| v as f64 * c as f64)
            .sum::<f64>()
            / total as f64;
        println!(
            "  {name:<9} mean {mean:6.1}   paper(>200) {:5.1}%   ink(<60) {:5.1}%",
            paper as f64 / total as f64 * 100.0,
            ink as f64 / total as f64 * 100.0,
        );
    }
}
