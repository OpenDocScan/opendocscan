use image::DynamicImage;
use std::path::Path;

/// A location in image coordinates: `(x, y)` in pixels, with `y` increasing
/// downwards to match `image`'s buffer layout.
///
/// This lives in `docscan-core` — the crate that owns the shared types
/// (PLAN.md §2) — so that a quad produced by `docscan-detect` and consumed
/// by `docscan-transform` is literally the same type rather than two
/// structurally-equal aliases that happen to agree today.
pub type Point = (f32, f32);

#[derive(thiserror::Error, Debug)]
pub enum CoreError {
    #[error("image decode/encode failed: {0}")]
    Image(#[from] image::ImageError),
}

pub fn load_image(path: &Path) -> Result<DynamicImage, CoreError> {
    Ok(image::open(path)?)
}

pub fn save_image(img: &DynamicImage, path: &Path) -> Result<(), CoreError> {
    img.save(path)?;
    Ok(())
}

/// Decode an image that is already in memory.
///
/// The mobile bridge hands over the bytes of a photograph, not a path to one:
/// on Android a camera plugin's result and a picked file both arrive as a byte
/// array, and writing them to a temporary file just so `load_image` can read
/// them back would add an IO round trip and a cleanup obligation to every
/// capture. The web build has the same shape for the same reason.
pub fn load_image_from_bytes(bytes: &[u8]) -> Result<DynamicImage, CoreError> {
    Ok(image::load_from_memory(bytes)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::GenericImageView;

    /// The bridge's decode path, exercised without touching the filesystem.
    #[test]
    fn loads_an_image_from_in_memory_bytes() {
        let original = DynamicImage::new_rgb8(4, 4);
        let mut bytes: Vec<u8> = Vec::new();
        original
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();

        let loaded = load_image_from_bytes(&bytes).unwrap();

        assert_eq!(original.dimensions(), loaded.dimensions());
    }

    /// A photograph that arrived truncated must come back as an error rather
    /// than as a zero-sized image the UI would render as a blank page.
    #[test]
    fn refuses_bytes_that_are_not_an_image() {
        assert!(load_image_from_bytes(b"not an image at all").is_err());
    }

    #[test]
    fn round_trips_a_png_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.png");

        let original = DynamicImage::new_rgb8(4, 4);
        save_image(&original, &path).unwrap();
        let loaded = load_image(&path).unwrap();

        assert_eq!(original.dimensions(), loaded.dimensions());
    }
}
