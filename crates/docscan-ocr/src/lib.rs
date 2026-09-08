//! Offline text recognition: the engine seam, and the page geometry an
//! engine produces.
//!
//! This crate deliberately contains no recognition model. It owns the
//! *shape* of the answer — where on the page each word sits — so that
//! `docscan-pdf` can lay an invisible, selectable text layer over a scan
//! without knowing or caring which engine read it (PLAN.md §2, "OCR engine
//! seam"). A `tract`/ONNX backend on mobile and a WASM backend in the
//! browser are then two implementations of one trait rather than two
//! parallel pipelines.
//!
//! Coordinates are in **image pixels**, origin top-left, `y` growing
//! downwards — the same convention as [`docscan_core::Point`] and the
//! `image` crate's buffers. PDF's origin is bottom-left, but that flip is
//! `docscan-pdf`'s business: an engine should never have to think in PDF
//! user space to report what it saw.

use image::DynamicImage;

#[derive(thiserror::Error, Debug)]
pub enum OcrError {
    /// The engine loaded but could not read this image.
    #[error("recognition failed: {0}")]
    Recognition(String),
    /// The engine could not start — a missing or corrupt model, usually.
    #[error("engine unavailable: {0}")]
    Unavailable(String),
}

/// One recognised word and the box it occupies, in image pixels.
///
/// Word-level rather than line- or character-level because that is the
/// granularity every mainstream engine agrees on, and it is the
/// granularity a PDF text layer wants: a word is the unit a reader
/// double-clicks to select and the unit a search matches.
#[derive(Debug, Clone, PartialEq)]
pub struct OcrWord {
    pub text: String,
    /// Left edge, in image pixels.
    pub x: f32,
    /// Top edge, in image pixels (`y` grows downwards).
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// Engine confidence in `0.0..=1.0`. Engines that don't report one
    /// should say `1.0` rather than invent a number — a caller filtering
    /// on confidence should drop words the engine doubted, not words the
    /// engine declined to score.
    pub confidence: f32,
}

/// Everything an engine read from one page.
///
/// `width`/`height` are the dimensions of the image the words were found
/// in, carried alongside them so a consumer can rescale the boxes to a
/// different output size without having to still be holding the source
/// image.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct OcrPage {
    pub words: Vec<OcrWord>,
    pub width: u32,
    pub height: u32,
}

impl OcrPage {
    /// The page's text as a single string, words space-joined in the order
    /// the engine reported them.
    ///
    /// This is what a full-text index stores. It is deliberately not an
    /// attempt to reconstruct layout — no line breaks are inferred, since
    /// guessing them wrongly corrupts the index in a way that is invisible
    /// until a search silently fails to match.
    pub fn text(&self) -> String {
        self.words
            .iter()
            .map(|w| w.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Drop words the engine was unsure of.
    ///
    /// A low-confidence word in an *invisible* layer is worse than no word:
    /// the reader never sees that it is wrong, so it silently pollutes
    /// search results and copy-paste.
    pub fn filter_confidence(&self, min: f32) -> OcrPage {
        OcrPage {
            words: self
                .words
                .iter()
                .filter(|w| w.confidence >= min)
                .cloned()
                .collect(),
            width: self.width,
            height: self.height,
        }
    }
}

/// A recogniser. One method, because recognition is one question.
///
/// Implementations must not touch the network — the whole product promise
/// rests on that (PLAN.md "Global Constraints"), and an engine that
/// silently fetches a model on first use would break it in exactly the
/// place nobody looks.
pub trait OcrEngine {
    fn recognize(&self, image: &DynamicImage) -> Result<OcrPage, OcrError>;
}

/// An engine that reads nothing, successfully.
///
/// Not a placeholder for a real engine — it is the honest answer for a
/// build with OCR compiled out or switched off in settings. Returning an
/// empty page rather than an error lets the export path stay one path:
/// `docscan-pdf` lays down an empty text layer, which is a PDF with no
/// text layer, which is exactly right.
pub struct NoOcr;

impl OcrEngine for NoOcr {
    fn recognize(&self, image: &DynamicImage) -> Result<OcrPage, OcrError> {
        Ok(OcrPage {
            words: Vec::new(),
            width: image.width(),
            height: image.height(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(text: &str, confidence: f32) -> OcrWord {
        OcrWord {
            text: text.into(),
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
            confidence,
        }
    }

    #[test]
    fn joins_words_into_indexable_text() {
        let page = OcrPage {
            words: vec![word("hello", 1.0), word("world", 1.0)],
            width: 100,
            height: 100,
        };
        assert_eq!(page.text(), "hello world");
    }

    #[test]
    fn filtering_keeps_words_at_exactly_the_threshold() {
        // A word scored exactly at the caller's floor met the bar they set;
        // dropping it would make the threshold mean something other than
        // what it reads as.
        let page = OcrPage {
            words: vec![word("sure", 0.9), word("maybe", 0.5)],
            width: 100,
            height: 100,
        };
        let kept = page.filter_confidence(0.9);
        assert_eq!(kept.words.len(), 1);
        assert_eq!(kept.words[0].text, "sure");
    }

    #[test]
    fn filtering_preserves_page_dimensions() {
        // The boxes that survive are still in the coordinate space of the
        // original image, so the page must keep saying what that space was.
        let page = OcrPage {
            words: vec![word("gone", 0.1)],
            width: 640,
            height: 480,
        };
        let kept = page.filter_confidence(0.9);
        assert!(kept.words.is_empty());
        assert_eq!((kept.width, kept.height), (640, 480));
    }

    #[test]
    fn the_disabled_engine_reports_the_image_it_declined_to_read() {
        let img = DynamicImage::new_rgb8(320, 240);
        let page = NoOcr.recognize(&img).unwrap();
        assert!(page.words.is_empty());
        assert_eq!((page.width, page.height), (320, 240));
        assert_eq!(page.text(), "");
    }
}
