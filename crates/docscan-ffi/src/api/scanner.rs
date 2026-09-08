//! Scanning calls: what Dart asks of the core, and what comes back.

/// What a photograph turned out to be, once the core had decoded it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodedInfo {
    pub width: u32,
    pub height: u32,
}

/// Proves the bridge is wired: Dart called Rust, Rust answered, the string
/// survived the crossing. The first thing to check on a new device, because it
/// fails differently from every later problem — a missing library rather than
/// a wrong answer.
pub fn ping() -> String {
    "docscan-ffi-ok".to_string()
}

/// Decode a photograph far enough to report its size.
///
/// Returns `Result`, where the M1 plan specified a panic with a note to tighten
/// it before M2. Deferring it was not safe to do: the bytes reaching this
/// function are a photograph the user just took or picked, and the failure
/// cases are ordinary rather than exotic — a truncated file, an interrupted
/// share, or an iOS HEIC, which this build cannot decode at all. A panic
/// unwinding across an FFI boundary is undefined behaviour in the general case
/// and a hard crash of the whole app in the friendly case. `flutter_rust_bridge`
/// maps `Err` onto a Dart exception, so the cost of doing it properly is one
/// `?` and a `match` in the UI.
pub fn decode_dimensions(bytes: Vec<u8>) -> Result<DecodedInfo, String> {
    let img = docscan_core::load_image_from_bytes(&bytes).map_err(|e| e.to_string())?;
    Ok(DecodedInfo {
        width: img.width(),
        height: img.height(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ping_returns_a_fixed_string() {
        assert_eq!(ping(), "docscan-ffi-ok");
    }

    #[test]
    fn decode_dimensions_reports_a_pngs_size() {
        let img = image::DynamicImage::new_rgb8(8, 6);
        let mut bytes: Vec<u8> = Vec::new();
        img.write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .unwrap();

        let info = decode_dimensions(bytes).unwrap();

        assert_eq!(info.width, 8);
        assert_eq!(info.height, 6);
    }

    /// A phone camera hands over JPEG, not PNG. The workspace pins `image`
    /// with `default-features = false`, so this is the test that fails the day
    /// someone trims the feature list to what the *browser* build needs.
    #[test]
    fn decode_dimensions_reports_a_jpegs_size() {
        let img = image::DynamicImage::new_rgb8(64, 48);
        let mut bytes: Vec<u8> = Vec::new();
        img.write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Jpeg,
        )
        .unwrap();

        let info = decode_dimensions(bytes).unwrap();

        assert_eq!((info.width, info.height), (64, 48));
    }

    /// The case the plan deferred. Bytes that are not an image come back as an
    /// error Dart can catch, rather than unwinding across the boundary.
    #[test]
    fn decode_dimensions_reports_an_error_rather_than_panicking() {
        let result = decode_dimensions(b"this is not a photograph".to_vec());

        assert!(result.is_err());
        assert!(!result.unwrap_err().is_empty(), "the message reaches Dart");
    }

    /// An empty pick — a share sheet that handed over nothing — is the same
    /// path, and must not be a special case anywhere above this.
    #[test]
    fn decode_dimensions_refuses_an_empty_buffer() {
        assert!(decode_dimensions(Vec::new()).is_err());
    }
}
