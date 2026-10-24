//! Error types.

/// Result alias using the crate [`Error`].
pub type Result<T> = core::result::Result<T, Error>;

/// Which configured limit was exceeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitKind {
    /// Total byte size.
    Bytes,
    /// Total frame count.
    Frames,
    /// Channel count.
    Channels,
    /// Sample rate.
    SampleRate,
    /// Metadata size.
    Metadata,
    /// Nesting depth.
    Depth,
}

/// Errors produced while reading or decoding.
///
/// There is no end-of-file variant: the end of a stream is `Ok(false)` from
/// `Demuxer::next_packet` and `Reader::read`, and truncated data is
/// `Invalid { what: "unexpected eof", .. }`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// An I/O error of the given kind.
    Io(std::io::ErrorKind),
    /// The input is malformed.
    Invalid {
        /// What was found to be invalid.
        what: &'static str,
        /// Absolute source position where the problem was detected.
        offset: u64,
    },
    /// The input uses a feature that is not supported.
    Unsupported(&'static str),
    /// A configured limit was exceeded.
    Limit(LimitKind),
    /// The operation was cancelled.
    Cancelled,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::Io(k) => write!(f, "i/o error: {k}"),
            Error::Invalid { what, offset } => write!(f, "invalid {what} at offset {offset}"),
            Error::Unsupported(w) => write!(f, "unsupported: {w}"),
            Error::Limit(k) => write!(f, "limit exceeded: {k:?}"),
            Error::Cancelled => write!(f, "cancelled"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e.kind())
    }
}
