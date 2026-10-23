//! Core types shared by all agulha crates.
#![warn(missing_docs)]

mod bytes;
mod cancel;
mod error;
mod limits;
mod source;

pub use bytes::ByteReader;
pub use cancel::{CancelSource, CancelToken};
pub use error::{Error, LimitKind, Result};
pub use limits::{Limits, Mode, Options};
pub use source::{FileSource, SliceSource, Source, VecSource};
