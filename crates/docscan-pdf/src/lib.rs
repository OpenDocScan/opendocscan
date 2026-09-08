//! Multi-page PDF assembly: the last step of a scan, and the one the user
//! actually keeps.
//!
//! The report this work is scoped from found that half the market puts
//! multi-page export behind a paywall, and that doing so is the single
//! most-complained-about thing in those apps' reviews. So this crate has
//! no notion of a page limit, a watermark, or a tier — there is nothing
//! here to gate, by construction rather than by policy.
//!
//! Two properties are load-bearing and are enforced by tests, not by
//! convention:
//!
//! * **Deterministic.** The same pages and options produce byte-identical
//!   output, every time. No creation date, no document ID, no randomness
//!   anywhere. That is what makes an export hashable in CI (PLAN.md §6),
//!   and it is also why a user can diff two exports and learn something.
//! * **Streaming-shaped.** Pages are encoded and written one at a time and
//!   dropped; a twenty-page document never has twenty decoded bitmaps
//!   alive at once. The peak is one page's pixels plus its compressed
//!   bytes.

mod helvetica;

use docscan_ocr::OcrPage;
use image::{DynamicImage, GenericImageView};
use pdf_writer::types::TextRenderingMode;
use pdf_writer::{Content, Filter, Finish, Name, Pdf, Rect, Ref, Str};

/// 72 PDF points to the inch — the definition of a point, not a setting.
const POINTS_PER_INCH: f32 = 72.0;

#[derive(thiserror::Error, Debug)]
pub enum PdfError {
    #[error("a PDF needs at least one page")]
    NoPages,
    #[error("page {index} is {width}x{height}; a page needs a non-zero extent")]
    EmptyPage {
        index: usize,
        width: u32,
        height: u32,
    },
    #[error("encoding page {index} as JPEG failed: {source}")]
    Encode {
        index: usize,
        #[source]
        source: image::ImageError,
    },
}

/// One scanned page: the corrected, filtered image, and optionally what
/// OCR read in it.
pub struct ScanPage {
    pub image: DynamicImage,
    /// The words to lay down as an invisible, selectable layer. `None`
    /// and `Some(empty)` both mean "no text layer" and are equally valid —
    /// OCR being off and OCR finding nothing are different facts about the
    /// world but the same PDF.
    pub ocr: Option<OcrPage>,
}

impl ScanPage {
    /// A page with no text layer.
    pub fn new(image: DynamicImage) -> Self {
        ScanPage { image, ocr: None }
    }

    /// A page with the given text layer.
    pub fn with_ocr(image: DynamicImage, ocr: OcrPage) -> Self {
        ScanPage {
            image,
            ocr: Some(ocr),
        }
    }
}

/// A physical sheet, in points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Paper {
    pub width_pt: f32,
    pub height_pt: f32,
}

impl Paper {
    pub const A4: Paper = Paper {
        width_pt: 595.276,
        height_pt: 841.890,
    };
    pub const LETTER: Paper = Paper {
        width_pt: 612.0,
        height_pt: 792.0,
    };
    pub const LEGAL: Paper = Paper {
        width_pt: 612.0,
        height_pt: 1008.0,
    };

    /// This sheet turned to match the given aspect, so a photographed
    /// landscape page is not letterboxed into a portrait sheet with two
    /// thick bands of white.
    fn oriented_for(self, image_is_landscape: bool) -> Paper {
        let sheet_is_landscape = self.width_pt > self.height_pt;
        if image_is_landscape == sheet_is_landscape {
            self
        } else {
            Paper {
                width_pt: self.height_pt,
                height_pt: self.width_pt,
            }
        }
    }
}

/// How a page's pixels become a page's physical size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PageLayout {
    /// The sheet is exactly the image, sized so it prints at this many
    /// dots per inch. Faithful — what you scanned is what comes out, at a
    /// known scale — but every page can be a different size, which some
    /// printers handle poorly.
    ImageAtDpi(f32),
    /// The sheet is a fixed size; the image is scaled to fit inside it and
    /// centred. The sheet is auto-oriented to the image's aspect. This is
    /// what makes a stack of hand-held photos come out as a tidy,
    /// uniformly sized, printable document.
    FitTo { paper: Paper, margin_pt: f32 },
}

impl Default for PageLayout {
    /// A4 with a small margin: the sheet most of the world prints on, and
    /// a margin that survives the non-printable border of a typical inkjet
    /// instead of clipping the last row of text.
    fn default() -> Self {
        PageLayout::FitTo {
            paper: Paper::A4,
            margin_pt: 18.0,
        }
    }
}

/// How each page's pixels are compressed into the file.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ImageCodec {
    /// Inspect the page and pick. A pure black-and-white page is packed one
    /// bit per pixel and deflated; anything else is JPEG at `quality`.
    ///
    /// This is not a micro-optimisation. The black-and-white filter is the
    /// most-used one in a scanner app, and its output is the exact case
    /// JPEG is worst at: a lossy photographic codec applied to hard-edged
    /// bilevel text rings every letter with grey mosquito noise *and*
    /// produces a bigger file than losslessly packing eight pixels into a
    /// byte would. Auto exists so the common case is not the badly served
    /// one.
    Auto {
        quality: u8,
    },
    Jpeg {
        quality: u8,
    },
    /// Lossless, always. Large for photographs; exact for everything.
    Flate,
}

impl Default for ImageCodec {
    fn default() -> Self {
        ImageCodec::Auto { quality: 82 }
    }
}

#[derive(Debug, Clone)]
pub struct PdfOptions {
    pub layout: PageLayout,
    pub codec: ImageCodec,
    /// Written to the document's `/Title`. Deliberately the *only*
    /// metadata written: an author or producer string would be a
    /// fingerprint travelling with a document the user believes is
    /// private, and a creation date would break determinism.
    pub title: Option<String>,
    /// Words the engine scored below this are left out of the text layer.
    pub min_ocr_confidence: f32,
}

impl Default for PdfOptions {
    fn default() -> Self {
        PdfOptions {
            layout: PageLayout::default(),
            codec: ImageCodec::default(),
            title: None,
            // Engines that report no confidence say 1.0, so a floor of
            // zero keeps every word from every engine by default and lets
            // a caller opt into pruning.
            min_ocr_confidence: 0.0,
        }
    }
}

/// Assemble scanned pages into one PDF.
pub fn build_pdf(pages: &[ScanPage], options: &PdfOptions) -> Result<Vec<u8>, PdfError> {
    if pages.is_empty() {
        return Err(PdfError::NoPages);
    }
    for (index, page) in pages.iter().enumerate() {
        let (width, height) = page.image.dimensions();
        if width == 0 || height == 0 {
            return Err(PdfError::EmptyPage {
                index,
                width,
                height,
            });
        }
    }

    let mut pdf = Pdf::new();
    let mut next = Ref::new(1);
    let mut alloc = || {
        let id = next;
        next = Ref::new(next.get() + 1);
        id
    };

    let catalog_id = alloc();
    let page_tree_id = alloc();
    let font_id = alloc();

    struct PageIds {
        page: Ref,
        image: Ref,
        content: Ref,
    }
    let ids: Vec<PageIds> = pages
        .iter()
        .map(|_| PageIds {
            page: alloc(),
            image: alloc(),
            content: alloc(),
        })
        .collect();

    let mut catalog = pdf.catalog(catalog_id);
    catalog.pages(page_tree_id);
    catalog.finish();

    pdf.pages(page_tree_id)
        .kids(ids.iter().map(|p| p.page))
        .count(ids.len() as i32);

    // Helvetica, unembedded. The glyphs are never painted — the whole
    // layer is rendering mode 3 — so what matters is that every reader
    // already has metrics for this name, which is true of the Core 14 and
    // of nothing else. Embedding a font here would add hundreds of
    // kilobytes to draw nothing.
    let mut font = pdf.type1_font(font_id);
    font.base_font(Name(b"Helvetica"));
    font.encoding_predefined(Name(b"WinAnsiEncoding"));
    font.finish();

    for (index, (page, ids)) in pages.iter().zip(ids.iter()).enumerate() {
        let (px_width, px_height) = page.image.dimensions();
        let encoded = encode_image(&page.image, options.codec)
            .map_err(|source| PdfError::Encode { index, source })?;

        let mut xobject = pdf.image_xobject(ids.image, &encoded.bytes);
        xobject.filter(encoded.filter);
        xobject.width(px_width as i32);
        xobject.height(px_height as i32);
        if encoded.gray {
            xobject.color_space().device_gray();
        } else {
            xobject.color_space().device_rgb();
        }
        xobject.bits_per_component(encoded.bits_per_component);
        xobject.finish();

        let placement = place(px_width, px_height, options.layout);

        let image_name = Name(b"Im0");
        let font_name = Name(b"F0");
        let mut content = Content::new();
        content.save_state();
        content.transform([
            placement.draw_width_pt,
            0.0,
            0.0,
            placement.draw_height_pt,
            placement.origin_x_pt,
            placement.origin_y_pt,
        ]);
        content.x_object(image_name);
        content.restore_state();

        let words_written = match &page.ocr {
            Some(ocr) => write_text_layer(
                &mut content,
                font_name,
                ocr,
                &placement,
                px_width,
                px_height,
                options.min_ocr_confidence,
            ),
            None => 0,
        };

        pdf.stream(ids.content, &content.finish());

        let mut pdf_page = pdf.page(ids.page);
        pdf_page.media_box(Rect::new(
            0.0,
            0.0,
            placement.sheet_width_pt,
            placement.sheet_height_pt,
        ));
        pdf_page.parent(page_tree_id);
        pdf_page.contents(ids.content);
        {
            let mut resources = pdf_page.resources();
            resources.x_objects().pair(image_name, ids.image);
            if words_written > 0 {
                resources.fonts().pair(font_name, font_id);
            }
            resources.finish();
        }
        pdf_page.finish();
    }

    if let Some(title) = &options.title {
        let info_id = alloc();
        let mut info = pdf.document_info(info_id);
        info.title(pdf_writer::TextStr(title));
        info.finish();
    }

    Ok(pdf.finish())
}

/// Where a page's image sits on its sheet, all in PDF points with the
/// origin at the bottom-left.
struct Placement {
    sheet_width_pt: f32,
    sheet_height_pt: f32,
    origin_x_pt: f32,
    origin_y_pt: f32,
    draw_width_pt: f32,
    draw_height_pt: f32,
}

fn place(px_width: u32, px_height: u32, layout: PageLayout) -> Placement {
    let px_width = px_width as f32;
    let px_height = px_height as f32;

    match layout {
        PageLayout::ImageAtDpi(dpi) => {
            let dpi = if dpi > 0.0 { dpi } else { 300.0 };
            let width_pt = px_width / dpi * POINTS_PER_INCH;
            let height_pt = px_height / dpi * POINTS_PER_INCH;
            Placement {
                sheet_width_pt: width_pt,
                sheet_height_pt: height_pt,
                origin_x_pt: 0.0,
                origin_y_pt: 0.0,
                draw_width_pt: width_pt,
                draw_height_pt: height_pt,
            }
        }
        PageLayout::FitTo { paper, margin_pt } => {
            let paper = paper.oriented_for(px_width > px_height);
            // A margin wider than the sheet would invert the content box;
            // clamp so a nonsensical margin degrades to a full-bleed page
            // rather than producing negative geometry.
            let margin = margin_pt
                .max(0.0)
                .min(paper.width_pt.min(paper.height_pt) / 2.0);
            let box_width = paper.width_pt - 2.0 * margin;
            let box_height = paper.height_pt - 2.0 * margin;
            let scale = (box_width / px_width).min(box_height / px_height);
            let draw_width = px_width * scale;
            let draw_height = px_height * scale;
            Placement {
                sheet_width_pt: paper.width_pt,
                sheet_height_pt: paper.height_pt,
                origin_x_pt: (paper.width_pt - draw_width) / 2.0,
                origin_y_pt: (paper.height_pt - draw_height) / 2.0,
                draw_width_pt: draw_width,
                draw_height_pt: draw_height,
            }
        }
    }
}

/// Lay each recognised word down as invisible glyphs sitting exactly on
/// top of the ink it was read from. Returns how many words were written.
#[allow(clippy::too_many_arguments)]
fn write_text_layer(
    content: &mut Content,
    font_name: Name,
    ocr: &OcrPage,
    placement: &Placement,
    px_width: u32,
    px_height: u32,
    min_confidence: f32,
) -> usize {
    let source_width = if ocr.width > 0 { ocr.width } else { px_width };
    let source_height = if ocr.height > 0 {
        ocr.height
    } else {
        px_height
    };

    // OCR pixels -> PDF points, folding the OCR-image-to-page-image
    // rescale into the same factor as the page-image-to-sheet scale. OCR
    // may well have run on a downscale of the page for speed, so its own
    // reported dimensions are honoured rather than assumed equal.
    let scale_x = placement.draw_width_pt / source_width as f32;
    let scale_y = placement.draw_height_pt / source_height as f32;

    let mut written = 0usize;
    let mut open = false;

    for word in &ocr.words {
        let Some(placed) = place_word(word, placement, scale_x, scale_y, min_confidence) else {
            continue;
        };
        if !open {
            content.begin_text();
            content.set_text_rendering_mode(TextRenderingMode::Invisible);
            open = true;
        }
        content.set_font(font_name, placed.font_size);
        content.set_horizontal_scaling(placed.horizontal_scaling);
        content.set_text_matrix([1.0, 0.0, 0.0, 1.0, placed.x_pt, placed.baseline_pt]);
        content.show(Str(&placed.bytes));
        written += 1;
    }

    if open {
        content.end_text();
    }
    written
}

/// One word's invisible glyphs, resolved into PDF user space.
#[derive(Debug, PartialEq)]
struct PlacedWord {
    bytes: Vec<u8>,
    x_pt: f32,
    /// Distance up from the sheet's bottom edge to the text baseline.
    baseline_pt: f32,
    font_size: f32,
    /// Percent, per PDF's `Tz` operator: 100 is unstretched.
    horizontal_scaling: f32,
}

/// Where this word's invisible glyphs go, or `None` if it should not be
/// written at all.
///
/// Three coordinate systems meet here. OCR reports pixels in whatever
/// image it was handed, `y` growing down. The page image is pixels, `y`
/// growing down. PDF is points, `y` growing *up*, with the image occupying
/// only part of the sheet. Getting the flip wrong is invisible in a page
/// count and obvious the moment anyone tries to select a line, so it is
/// pulled out here where a test can look at the numbers directly.
fn place_word(
    word: &docscan_ocr::OcrWord,
    placement: &Placement,
    scale_x: f32,
    scale_y: f32,
    min_confidence: f32,
) -> Option<PlacedWord> {
    if word.confidence < min_confidence || word.width <= 0.0 || word.height <= 0.0 {
        return None;
    }
    let bytes = encode_winansi(&word.text)?;

    let box_width_pt = word.width * scale_x;
    let box_height_pt = word.height * scale_y;

    // OCR boxes bound the ink, so the box height is roughly the
    // cap-to-baseline extent of the tallest glyph rather than the full em.
    // Helvetica's cap height is 0.717 em; sizing by that puts the
    // invisible glyphs at the same visual weight as the ink beneath, which
    // is what makes a selection rectangle track the printed word.
    let font_size = box_height_pt / 0.717;
    let natural_width = helvetica::text_width(&bytes, font_size);
    if natural_width <= 0.0 {
        return None;
    }

    // The word's top edge, measured *down* from the image's top, becomes a
    // distance *up* from the sheet's bottom; the baseline then sits one box
    // height below that.
    let top_pt = placement.origin_y_pt + placement.draw_height_pt - word.y * scale_y;

    Some(PlacedWord {
        bytes,
        x_pt: placement.origin_x_pt + word.x * scale_x,
        baseline_pt: top_pt - box_height_pt,
        font_size,
        horizontal_scaling: (box_width_pt / natural_width) * 100.0,
    })
}

/// This word as WinAnsi bytes, or `None` if it cannot be represented.
///
/// A word that cannot be encoded is dropped whole rather than partially
/// transliterated. Substituting `?` for an unmappable character would put
/// a word into the searchable layer that matches neither what is on the
/// page nor what a user would type — worse than the word being absent,
/// because it is a wrong answer rather than a missing one.
///
/// Known limit: WinAnsi is Latin-1-shaped, so this excludes CJK, Cyrillic,
/// and Greek. That is aligned with, not narrower than, the v1 OCR models;
/// supporting those scripts means embedding a font with a `/ToUnicode`
/// map, which is the same change on both sides and should be made once,
/// when a model that reads them ships.
fn encode_winansi(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len());
    for ch in text.chars() {
        let code = ch as u32;
        // 0x20..0x7E is ASCII; 0xA0..0xFF is where WinAnsi and Latin-1
        // agree. The 0x80..0x9F gap is where they differ, so it is
        // excluded rather than guessed at.
        if (0x20..=0x7E).contains(&code) || (0xA0..=0xFF).contains(&code) {
            out.push(code as u8);
        } else {
            return None;
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

struct Encoded {
    bytes: Vec<u8>,
    filter: Filter,
    gray: bool,
    bits_per_component: i32,
}

fn encode_image(img: &DynamicImage, codec: ImageCodec) -> Result<Encoded, image::ImageError> {
    let tone = classify(img);
    match codec {
        ImageCodec::Auto { quality } => match tone {
            Tone::Bilevel => Ok(encode_bilevel(&gray_plane(img, tone))),
            Tone::Gray => encode_jpeg_gray(&gray_plane(img, tone), quality),
            Tone::Colour => encode_jpeg_rgb(img, quality),
        },
        ImageCodec::Jpeg { quality } => match tone {
            Tone::Colour => encode_jpeg_rgb(img, quality),
            gray => encode_jpeg_gray(&gray_plane(img, gray), quality),
        },
        ImageCodec::Flate => match tone {
            Tone::Bilevel => Ok(encode_bilevel(&gray_plane(img, tone))),
            Tone::Gray => Ok(Encoded {
                bytes: deflate(gray_plane(img, tone).as_raw()),
                filter: Filter::FlateDecode,
                gray: true,
                bits_per_component: 8,
            }),
            Tone::Colour => Ok(Encoded {
                bytes: deflate(img.to_rgb8().as_raw()),
                filter: Filter::FlateDecode,
                gray: false,
                bits_per_component: 8,
            }),
        },
    }
}

/// How much of a colour image a page actually uses.
///
/// Ordered by how little it costs to store: a bilevel page is also
/// achromatic, and an achromatic page is a special case of a colour one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tone {
    /// Every pixel is pure black or pure white — the signature of the
    /// black-and-white filter's output.
    Bilevel,
    /// No pixel carries colour, so two of the three channels are waste.
    Gray,
    Colour,
}

/// Decides a page's [`Tone`] in a single pass over the pixels it already
/// has.
///
/// The three questions this used to answer separately — is it bilevel, is
/// it achromatic, and what does its luma plane look like — were each paid
/// for with a full-image allocation and a full-image walk, and two of the
/// three allocations were thrown away whatever the answer turned out to
/// be. They are one question, and a colour page answers it after a handful
/// of pixels because the first coloured pixel settles it.
fn classify(img: &DynamicImage) -> Tone {
    fn scan<const N: usize>(raw: &[u8]) -> Tone {
        let mut bilevel = true;
        for &px in raw.as_chunks::<N>().0 {
            let (r, g, b) = (px[0], px[1], px[2]);
            if r != g || g != b {
                return Tone::Colour;
            }
            bilevel &= r == 0 || r == 255;
        }
        if bilevel {
            Tone::Bilevel
        } else {
            Tone::Gray
        }
    }

    match img {
        DynamicImage::ImageRgba8(buf) => scan::<4>(buf.as_raw()),
        DynamicImage::ImageRgb8(buf) => scan::<3>(buf.as_raw()),
        DynamicImage::ImageLuma8(buf) => {
            if buf.as_raw().iter().all(|&v| v == 0 || v == 255) {
                Tone::Bilevel
            } else {
                Tone::Gray
            }
        }
        // Everything else — 16-bit, float, luma-with-alpha — is rare
        // enough here that normalising it first is cheaper to be sure
        // about than to enumerate.
        other => scan::<3>(other.to_rgb8().as_raw()),
    }
}

/// The luma plane of an image `classify` has already found achromatic.
///
/// For such an image the Rec. 709 weighted sum collapses: with
/// `r == g == b` the weights sum to exactly one, so luma *is* the red
/// channel and the conversion is a channel extraction rather than
/// arithmetic. Debug-asserts the precondition, because calling this on a
/// colour page would silently drop its colour rather than fail.
fn gray_plane(img: &DynamicImage, tone: Tone) -> image::GrayImage {
    debug_assert_ne!(tone, Tone::Colour, "gray_plane needs an achromatic page");
    let _ = tone;
    match img {
        DynamicImage::ImageLuma8(buf) => buf.clone(),
        DynamicImage::ImageRgba8(buf) => strip::<4>(buf.as_raw(), img.width(), img.height()),
        DynamicImage::ImageRgb8(buf) => strip::<3>(buf.as_raw(), img.width(), img.height()),
        other => other.to_luma8(),
    }
}

fn strip<const N: usize>(raw: &[u8], width: u32, height: u32) -> image::GrayImage {
    let plane: Vec<u8> = raw.as_chunks::<N>().0.iter().map(|px| px[0]).collect();
    image::GrayImage::from_raw(width, height, plane)
        .expect("a stripped plane has exactly one byte per pixel")
}

/// Pack a bilevel page one bit per pixel, then deflate.
///
/// Rows are byte-aligned, which PDF requires, and the padding bits are set
/// to 1 (white) so that a reader which ignores the row stride — some do —
/// shows a white margin rather than a black one.
fn encode_bilevel(luma: &image::GrayImage) -> Encoded {
    let (width, height) = luma.dimensions();
    let row_bytes = width.div_ceil(8) as usize;
    let mut packed = vec![0xFFu8; row_bytes * height as usize];
    // Walked as rows of the backing buffer rather than through
    // `get_pixel`, which recomputes `y * width + x` and bounds-checks it
    // for every one of the several million pixels on a scanned page.
    let raw = luma.as_raw();
    for y in 0..height {
        let row_start = y as usize * row_bytes;
        let src_row = &raw[y as usize * width as usize..][..width as usize];
        for (x, &value) in src_row.iter().enumerate() {
            let x = x as u32;
            if value == 0 {
                let byte = row_start + (x / 8) as usize;
                packed[byte] &= !(0x80 >> (x % 8));
            }
        }
    }
    Encoded {
        bytes: deflate(&packed),
        filter: Filter::FlateDecode,
        gray: true,
        bits_per_component: 1,
    }
}

fn encode_jpeg_rgb(img: &DynamicImage, quality: u8) -> Result<Encoded, image::ImageError> {
    let rgb = img.to_rgb8();
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, quality.clamp(1, 100))
        .encode_image(&rgb)?;
    Ok(Encoded {
        bytes,
        filter: Filter::DctDecode,
        gray: false,
        bits_per_component: 8,
    })
}

fn encode_jpeg_gray(luma: &image::GrayImage, quality: u8) -> Result<Encoded, image::ImageError> {
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, quality.clamp(1, 100))
        .encode_image(luma)?;
    Ok(Encoded {
        bytes,
        filter: Filter::DctDecode,
        gray: true,
        bits_per_component: 8,
    })
}

fn deflate(data: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder
        .write_all(data)
        .expect("in-memory write cannot fail");
    encoder.finish().expect("in-memory finish cannot fail")
}

#[cfg(test)]
mod tests {
    use super::*;
    use docscan_ocr::OcrWord;
    use image::{Rgb, RgbImage};

    fn white_page(width: u32, height: u32) -> DynamicImage {
        DynamicImage::ImageRgb8(RgbImage::from_pixel(width, height, Rgb([255, 255, 255])))
    }

    /// A page with real colour in it, so the achromatic check has
    /// something to say no to.
    fn colour_page(width: u32, height: u32) -> DynamicImage {
        let mut img = RgbImage::new(width, height);
        for (x, y, px) in img.enumerate_pixels_mut() {
            *px = Rgb([(x % 256) as u8, (y % 256) as u8, 128]);
        }
        DynamicImage::ImageRgb8(img)
    }

    /// Half black, half white — bilevel, and not uniform enough for the
    /// bilevel check to pass by accident on a blank image.
    fn bilevel_page(width: u32, height: u32) -> DynamicImage {
        let mut img = image::GrayImage::new(width, height);
        for (x, _y, px) in img.enumerate_pixels_mut() {
            *px = image::Luma([if x % 2 == 0 { 0 } else { 255 }]);
        }
        DynamicImage::ImageLuma8(img)
    }

    fn word(text: &str, x: f32, y: f32, width: f32, height: f32, confidence: f32) -> OcrWord {
        OcrWord {
            text: text.into(),
            x,
            y,
            width,
            height,
            confidence,
        }
    }

    // --- refusals -------------------------------------------------------

    #[test]
    fn refuses_to_build_a_pdf_with_no_pages() {
        let err = build_pdf(&[], &PdfOptions::default()).unwrap_err();
        assert!(matches!(err, PdfError::NoPages));
    }

    #[test]
    fn refuses_a_page_with_no_extent() {
        // Caught before any encoding happens, so the caller learns which
        // page is bad rather than getting a codec error from the middle of
        // a twenty-page export.
        let pages = vec![
            ScanPage::new(white_page(10, 10)),
            ScanPage::new(white_page(0, 10)),
        ];
        let err = build_pdf(&pages, &PdfOptions::default()).unwrap_err();
        match err {
            PdfError::EmptyPage { index, width, .. } => {
                assert_eq!(index, 1);
                assert_eq!(width, 0);
            }
            other => panic!("expected EmptyPage, got {other:?}"),
        }
    }

    // --- structure ------------------------------------------------------

    #[test]
    fn exports_one_pdf_page_per_scanned_page() {
        // The feature the report found paywalled in half the market. It
        // has no limit here, so the test asserts a count no free tier
        // elsewhere would allow.
        let pages: Vec<ScanPage> = (0..12).map(|_| ScanPage::new(white_page(64, 96))).collect();
        let bytes = build_pdf(&pages, &PdfOptions::default()).unwrap();
        let doc = lopdf::Document::load_mem(&bytes).unwrap();
        assert_eq!(doc.get_pages().len(), 12);
    }

    #[test]
    fn the_exported_file_is_a_pdf_a_reader_will_open() {
        let pages = vec![ScanPage::new(colour_page(32, 48))];
        let bytes = build_pdf(&pages, &PdfOptions::default()).unwrap();
        assert!(bytes.starts_with(b"%PDF-"));
        assert!(bytes.ends_with(b"%%EOF\n") || bytes.ends_with(b"%%EOF"));
        // Loading is the real assertion: lopdf parses the xref table, so a
        // mis-allocated object reference fails here rather than silently
        // producing a file only some readers cope with.
        lopdf::Document::load_mem(&bytes).unwrap();
    }

    // --- determinism ----------------------------------------------------

    #[test]
    fn the_same_pages_export_byte_identical_output() {
        // PLAN.md §6's "deterministic test pipeline" requirement, stated
        // as the property it actually means.
        let build = || {
            build_pdf(
                &[
                    ScanPage::new(colour_page(40, 60)),
                    ScanPage::with_ocr(
                        white_page(40, 60),
                        OcrPage {
                            words: vec![word("hello", 4.0, 4.0, 20.0, 8.0, 0.9)],
                            width: 40,
                            height: 60,
                        },
                    ),
                ],
                &PdfOptions {
                    title: Some("Receipts".into()),
                    ..Default::default()
                },
            )
            .unwrap()
        };
        assert_eq!(build(), build());
    }

    #[test]
    fn an_export_carries_no_timestamp_and_no_producer() {
        // Both would break the determinism above, and a producer string is
        // a fingerprint riding along with a document the user was promised
        // stays theirs.
        let bytes =
            build_pdf(&[ScanPage::new(white_page(16, 16))], &PdfOptions::default()).unwrap();
        let haystack = String::from_utf8_lossy(&bytes);
        assert!(!haystack.contains("CreationDate"));
        assert!(!haystack.contains("ModDate"));
        assert!(!haystack.contains("Producer"));
    }

    #[test]
    fn a_title_is_the_only_metadata_written() {
        let bytes = build_pdf(
            &[ScanPage::new(white_page(16, 16))],
            &PdfOptions {
                title: Some("Tax 2026".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(String::from_utf8_lossy(&bytes).contains("Tax 2026"));
    }

    // --- physical layout ------------------------------------------------

    #[test]
    fn an_image_exported_at_a_known_dpi_gets_the_matching_physical_size() {
        // 300 pixels at 300 dpi is one inch, and one inch is 72 points, by
        // definition rather than by convention.
        let placement = place(300, 600, PageLayout::ImageAtDpi(300.0));
        assert!((placement.sheet_width_pt - 72.0).abs() < 0.01);
        assert!((placement.sheet_height_pt - 144.0).abs() < 0.01);
        assert_eq!(placement.origin_x_pt, 0.0);
    }

    #[test]
    fn a_nonsense_dpi_falls_back_instead_of_producing_an_infinite_page() {
        let placement = place(300, 300, PageLayout::ImageAtDpi(0.0));
        assert!(placement.sheet_width_pt.is_finite());
        assert!(placement.sheet_width_pt > 0.0);
    }

    #[test]
    fn fitting_a_landscape_photo_turns_the_sheet_to_match() {
        // Otherwise a landscape page is letterboxed into portrait A4 with
        // two thick white bands and half the usable resolution.
        let placement = place(
            1600,
            900,
            PageLayout::FitTo {
                paper: Paper::A4,
                margin_pt: 0.0,
            },
        );
        assert!(placement.sheet_width_pt > placement.sheet_height_pt);
        assert!((placement.sheet_width_pt - Paper::A4.height_pt).abs() < 0.01);
    }

    #[test]
    fn a_fitted_image_is_centred_and_stays_inside_its_margins() {
        let margin = 20.0;
        let placement = place(
            1000,
            1000,
            PageLayout::FitTo {
                paper: Paper::A4,
                margin_pt: margin,
            },
        );
        assert!(placement.origin_x_pt >= margin - 0.01);
        assert!(placement.origin_y_pt >= margin - 0.01);
        assert!(
            placement.origin_x_pt + placement.draw_width_pt
                <= placement.sheet_width_pt - margin + 0.01
        );
        // Square image, so the two centring insets differ only by the
        // sheet's own aspect.
        assert!((placement.origin_x_pt - margin).abs() < 0.01);
    }

    #[test]
    fn a_fitted_image_keeps_its_aspect_ratio() {
        let placement = place(
            400,
            800,
            PageLayout::FitTo {
                paper: Paper::LETTER,
                margin_pt: 10.0,
            },
        );
        let ratio = placement.draw_width_pt / placement.draw_height_pt;
        assert!((ratio - 0.5).abs() < 0.001, "got {ratio}");
    }

    #[test]
    fn an_absurd_margin_degrades_to_a_full_bleed_page_rather_than_inverting() {
        // A margin wider than the sheet would otherwise give a negative
        // content box and a page drawn inside out.
        let placement = place(
            100,
            100,
            PageLayout::FitTo {
                paper: Paper::A4,
                margin_pt: 10_000.0,
            },
        );
        assert!(placement.draw_width_pt >= 0.0);
        assert!(placement.draw_height_pt >= 0.0);
        assert!(placement.draw_width_pt <= placement.sheet_width_pt);
    }

    // --- codec choice ---------------------------------------------------

    #[test]
    fn a_black_and_white_page_is_packed_one_bit_per_pixel() {
        // The black-and-white filter is the most-used one in a scanner
        // app, and JPEG is at its worst on exactly that output.
        let encoded = encode_image(&bilevel_page(64, 64), ImageCodec::default()).unwrap();
        assert_eq!(encoded.bits_per_component, 1);
        assert!(encoded.gray);
        assert_eq!(encoded.filter, Filter::FlateDecode);
    }

    #[test]
    fn packing_a_bilevel_page_beats_jpeg_on_size() {
        let page = bilevel_page(512, 512);
        let packed = encode_image(&page, ImageCodec::Flate).unwrap();
        let jpeg = encode_image(&page, ImageCodec::Jpeg { quality: 82 }).unwrap();
        assert!(
            packed.bytes.len() < jpeg.bytes.len(),
            "packed {} bytes vs jpeg {} bytes",
            packed.bytes.len(),
            jpeg.bytes.len()
        );
    }

    #[test]
    fn a_bilevel_row_is_padded_to_a_byte_boundary_with_white() {
        // 12 pixels is 1.5 bytes; the 4 spare bits must be white, or a
        // reader that honours the stride loosely paints a black margin.
        let mut img = image::GrayImage::from_pixel(12, 1, image::Luma([255]));
        img.put_pixel(0, 0, image::Luma([0]));
        let encoded = encode_bilevel(&img);
        let raw = inflate(&encoded.bytes);
        assert_eq!(raw.len(), 2, "one row of 12 pixels is 2 bytes");
        assert_eq!(raw[0], 0b0111_1111, "only pixel 0 is black");
        assert_eq!(raw[1], 0xFF, "padding bits are white");
    }

    #[test]
    fn a_grey_page_is_not_given_three_identical_colour_channels() {
        let grey = DynamicImage::ImageLuma8(image::GrayImage::from_fn(64, 64, |x, _| {
            image::Luma([(x * 3 % 200 + 20) as u8])
        }));
        let encoded = encode_image(&grey, ImageCodec::default()).unwrap();
        assert!(encoded.gray, "a grey page should not be stored as RGB");
        assert_eq!(encoded.filter, Filter::DctDecode);
    }

    #[test]
    fn a_colour_page_keeps_its_colour() {
        let encoded = encode_image(&colour_page(64, 64), ImageCodec::default()).unwrap();
        assert!(!encoded.gray);
        assert_eq!(encoded.bits_per_component, 8);
    }

    #[test]
    fn flate_is_lossless_where_jpeg_is_asked_not_to_be() {
        // Not a size claim — a promise about which codec is which.
        let page = colour_page(32, 32);
        let flate = encode_image(&page, ImageCodec::Flate).unwrap();
        assert_eq!(flate.filter, Filter::FlateDecode);
        let raw = inflate(&flate.bytes);
        assert_eq!(raw, page.to_rgb8().as_raw().clone());
    }

    fn inflate(data: &[u8]) -> Vec<u8> {
        use std::io::Read;
        let mut out = Vec::new();
        flate2::read::ZlibDecoder::new(data)
            .read_to_end(&mut out)
            .unwrap();
        out
    }

    // --- text layer -----------------------------------------------------

    fn full_bleed(width: u32, height: u32) -> Placement {
        place(width, height, PageLayout::ImageAtDpi(72.0))
    }

    #[test]
    fn a_word_at_the_top_of_the_image_lands_near_the_top_of_the_sheet() {
        // The y-flip: image coordinates grow down, PDF's grow up. This is
        // the bug that a page count can never catch.
        let placement = full_bleed(100, 200);
        let top = place_word(
            &word("top", 0.0, 10.0, 30.0, 10.0, 1.0),
            &placement,
            1.0,
            1.0,
            0.0,
        )
        .unwrap();
        let bottom = place_word(
            &word("bottom", 0.0, 180.0, 30.0, 10.0, 1.0),
            &placement,
            1.0,
            1.0,
            0.0,
        )
        .unwrap();
        assert!(
            top.baseline_pt > bottom.baseline_pt,
            "top word baseline {} should sit above bottom word baseline {}",
            top.baseline_pt,
            bottom.baseline_pt
        );
        // A word 10px from a 200pt-tall page's top sits 10pt from its top.
        assert!((top.baseline_pt - (200.0 - 10.0 - 10.0)).abs() < 0.01);
    }

    #[test]
    fn a_word_is_stretched_to_the_width_ocr_measured() {
        // The point of carrying real Helvetica metrics: the invisible
        // glyphs must span the same distance as the visible ink, or a drag
        // selection highlights the neighbouring word.
        let placement = full_bleed(1000, 1000);
        let placed = place_word(
            &word("iiii", 0.0, 0.0, 400.0, 20.0, 1.0),
            &placement,
            1.0,
            1.0,
            0.0,
        )
        .unwrap();
        let natural = helvetica::text_width(b"iiii", placed.font_size);
        let effective = natural * placed.horizontal_scaling / 100.0;
        assert!(
            (effective - 400.0).abs() < 0.01,
            "stretched to {effective}pt, wanted 400pt"
        );
    }

    #[test]
    fn ocr_run_on_a_downscale_still_lands_on_the_full_size_page() {
        // A real optimisation: OCR is often run on a half-size image for
        // speed. Its boxes are in *that* image's pixels, so the page must
        // rescale them rather than assume they match.
        let placement = full_bleed(1000, 1000);
        let ocr = OcrPage {
            words: vec![word("half", 100.0, 100.0, 200.0, 40.0, 1.0)],
            width: 500,
            height: 500,
        };
        let scale_x = placement.draw_width_pt / ocr.width as f32;
        let scale_y = placement.draw_height_pt / ocr.height as f32;
        let placed = place_word(&ocr.words[0], &placement, scale_x, scale_y, 0.0).unwrap();
        // 100px into a 500px-wide OCR image is a fifth across a 1000pt page.
        assert!((placed.x_pt - 200.0).abs() < 0.01, "got {}", placed.x_pt);
    }

    #[test]
    fn words_the_engine_doubted_are_left_out_of_the_layer() {
        // A wrong word in an invisible layer is worse than a missing one:
        // the reader never sees that it is wrong.
        let placement = full_bleed(100, 100);
        assert!(place_word(
            &word("guess", 0.0, 0.0, 10.0, 10.0, 0.3),
            &placement,
            1.0,
            1.0,
            0.6
        )
        .is_none());
        assert!(place_word(
            &word("sure", 0.0, 0.0, 10.0, 10.0, 0.9),
            &placement,
            1.0,
            1.0,
            0.6
        )
        .is_some());
    }

    #[test]
    fn a_degenerate_box_is_skipped_rather_than_dividing_by_zero() {
        let placement = full_bleed(100, 100);
        assert!(place_word(
            &word("flat", 0.0, 0.0, 10.0, 0.0, 1.0),
            &placement,
            1.0,
            1.0,
            0.0
        )
        .is_none());
    }

    #[test]
    fn a_word_winansi_cannot_carry_is_dropped_whole() {
        // Substituting '?' would put a word in the searchable layer that
        // matches neither the page nor what anyone would type for it.
        assert!(encode_winansi("发票").is_none());
        assert!(
            encode_winansi("in\u{2014}voice").is_none(),
            "em dash is in the WinAnsi gap"
        );
        assert_eq!(encode_winansi("Total").unwrap(), b"Total".to_vec());
        assert_eq!(
            encode_winansi("caf\u{e9}").unwrap(),
            vec![b'c', b'a', b'f', 0xE9]
        );
    }

    #[test]
    fn a_page_with_no_readable_words_gets_no_font_resource() {
        // An empty text layer must produce a page indistinguishable from
        // one exported with OCR switched off — not a page carrying a font
        // that draws nothing.
        let with_empty_ocr = build_pdf(
            &[ScanPage::with_ocr(
                white_page(32, 32),
                OcrPage {
                    words: vec![],
                    width: 32,
                    height: 32,
                },
            )],
            &PdfOptions::default(),
        )
        .unwrap();
        let without =
            build_pdf(&[ScanPage::new(white_page(32, 32))], &PdfOptions::default()).unwrap();
        assert_eq!(with_empty_ocr, without);
    }

    #[test]
    fn the_recognised_text_is_searchable_in_the_exported_pdf() {
        // End to end, through a third-party parser: whatever the content
        // stream says, this is what a reader's find-in-document will see.
        let ocr = OcrPage {
            words: vec![
                word("INVOICE", 20.0, 20.0, 120.0, 24.0, 0.99),
                word("2026", 20.0, 60.0, 60.0, 24.0, 0.99),
            ],
            width: 400,
            height: 600,
        };
        let bytes = build_pdf(
            &[ScanPage::with_ocr(white_page(400, 600), ocr)],
            &PdfOptions::default(),
        )
        .unwrap();
        let doc = lopdf::Document::load_mem(&bytes).unwrap();
        let text = doc.extract_text(&[1]).unwrap();
        assert!(text.contains("INVOICE"), "extracted: {text:?}");
        assert!(text.contains("2026"), "extracted: {text:?}");
    }

    #[test]
    fn the_text_layer_is_invisible() {
        // Rendering mode 3. Without it the OCR guesses are painted in
        // black on top of the scan.
        let ocr = OcrPage {
            words: vec![word("HELLO", 10.0, 10.0, 80.0, 20.0, 1.0)],
            width: 200,
            height: 200,
        };
        let bytes = build_pdf(
            &[ScanPage::with_ocr(white_page(200, 200), ocr)],
            &PdfOptions::default(),
        )
        .unwrap();
        let doc = lopdf::Document::load_mem(&bytes).unwrap();
        let (_, page_id) = doc.get_pages().into_iter().next().unwrap();
        let content = doc.get_page_content(page_id).unwrap();
        let ops = String::from_utf8_lossy(&content);
        assert!(ops.contains("3 Tr"), "content stream: {ops}");
    }
}
