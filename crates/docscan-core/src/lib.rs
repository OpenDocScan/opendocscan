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

#[cfg(test)]
mod tests {
    use super::*;
    use image::GenericImageView;

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
