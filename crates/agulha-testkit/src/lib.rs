//! Test-only helpers shared by the agulha crates.
//!
//! agulha-core must never depend on this crate; the reverse is allowed.

pub mod alloc;
mod sha256;
pub mod signal;
pub mod writers;

pub use sha256::{hex, sha256};
