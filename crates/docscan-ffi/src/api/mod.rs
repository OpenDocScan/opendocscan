//! The surface Dart calls.
//!
//! Kept deliberately small. Each function translates bytes and numbers across
//! the boundary and delegates immediately; the moment one of them starts
//! deciding something, that decision belongs in a core crate where it can be
//! tested without a phone.

pub mod scanner;

/// Runs once, before anything else crosses the bridge.
///
/// `setup_default_user_utils` is what routes a Rust panic into the Dart console
/// instead of losing it. Without it a panic on a phone is a silent stop with no
/// message anywhere — which is a bad way to spend an afternoon, and the reason
/// `decode_dimensions` returns a `Result` rather than relying on this.
#[flutter_rust_bridge::frb(init)]
pub fn init_app() {
    flutter_rust_bridge::setup_default_user_utils();
}
