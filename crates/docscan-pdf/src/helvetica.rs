//! Glyph advance widths for Helvetica, from Adobe's Core 14 AFM metrics,
//! in 1/1000 em.
//!
//! These exist so the invisible text layer can be *stretched to fit* the
//! box OCR reported. Without real widths there is no way to know how wide
//! a word would naturally be, so there is no way to compute the horizontal
//! scale that makes the invisible glyphs line up with the visible ink —
//! and a text layer that does not line up is one where dragging to select
//! a word highlights its neighbour.
//!
//! Only the printable ASCII range is tabulated. Helvetica's WinAnsi upper
//! half is far less predictable per-glyph, and OCR output above U+007F is
//! rare enough in the v1 English models that a flat estimate there costs
//! nothing measurable; see [`width`].

/// Advance widths for `' '` (0x20) through `'~'` (0x7E), in 1/1000 em.
const ASCII_WIDTHS: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278,
    278, // 0x20-0x2F
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556, // 0x30-0x39 digits
    278, 278, 584, 584, 584, 556, 1015, // 0x3A-0x40
    667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, // 0x41-0x4D
    722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, // 0x4E-0x5A
    278, 278, 278, 469, 556, 333, // 0x5B-0x60
    556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, // 0x61-0x6D
    556, 556, 556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, // 0x6E-0x7A
    334, 260, 334, 584, // 0x7B-0x7E
];

/// A stand-in advance for characters outside the tabulated range.
///
/// 556 is Helvetica's lowercase-`e` width — the most common letter in
/// English text, so the estimate errs towards the shape of real prose
/// rather than towards an extreme.
const FALLBACK_WIDTH: u16 = 556;

/// This character's advance width in 1/1000 em.
pub fn width(byte: u8) -> f32 {
    let w = if (0x20..=0x7E).contains(&byte) {
        ASCII_WIDTHS[(byte - 0x20) as usize]
    } else {
        FALLBACK_WIDTH
    };
    f32::from(w)
}

/// How wide this string would be set in Helvetica at `size` points, if
/// nothing stretched it.
pub fn text_width(bytes: &[u8], size: f32) -> f32 {
    bytes.iter().map(|&b| width(b)).sum::<f32>() / 1000.0 * size
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_covers_exactly_printable_ascii() {
        assert_eq!(ASCII_WIDTHS.len(), (0x7E - 0x20) + 1);
    }

    #[test]
    fn known_afm_widths_land_on_the_right_entries() {
        // Spot-checks against Adobe's published Helvetica.afm. These catch
        // an off-by-one in the table far more reliably than its length
        // does: a table shifted by one still has 95 entries.
        assert_eq!(width(b' '), 278.0);
        assert_eq!(width(b'0'), 556.0);
        assert_eq!(width(b'@'), 1015.0);
        assert_eq!(width(b'A'), 667.0);
        assert_eq!(width(b'W'), 944.0);
        assert_eq!(width(b'a'), 556.0);
        assert_eq!(width(b'i'), 222.0);
        assert_eq!(width(b'm'), 833.0);
        assert_eq!(width(b'~'), 584.0);
    }

    #[test]
    fn characters_outside_the_table_get_the_fallback() {
        assert_eq!(width(0x1F), f32::from(FALLBACK_WIDTH));
        assert_eq!(width(0xE9), f32::from(FALLBACK_WIDTH));
    }

    #[test]
    fn a_string_at_one_em_measures_the_sum_of_its_advances() {
        // "AA" at 1000pt is 2 x 667/1000 x 1000 = 1334pt.
        assert!((text_width(b"AA", 1000.0) - 1334.0).abs() < 0.01);
    }

    #[test]
    fn the_empty_string_is_zero_wide() {
        assert_eq!(text_width(b"", 12.0), 0.0);
    }
}
