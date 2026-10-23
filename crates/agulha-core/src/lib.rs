//! Core types shared by all agulha crates.
#![warn(missing_docs)]

mod cancel;
mod error;
mod limits;

pub use cancel::{CancelSource, CancelToken};
pub use error::{Error, LimitKind, Result};
pub use limits::{Limits, Mode, Options};
