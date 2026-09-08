//! The mobile boundary.
//!
//! Everything here is glue. The rule from PLAN.md is that the core stays
//! platform-agnostic and testable without a mobile toolchain, so this crate
//! holds the `flutter_rust_bridge` annotations and nothing else — no image
//! processing, no decisions. If a function here does more than translate
//! between Dart's types and the core's, it is in the wrong crate.

pub mod api;
mod frb_generated;
