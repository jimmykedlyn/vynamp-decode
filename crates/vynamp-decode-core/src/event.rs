//! Diagnostic events reported while decoding.

/// Something noteworthy that a demuxer or decoder recovered from.
///
/// In `Mode::Strict` these conditions are errors; in `Mode::Resilient` they
/// are recovered from and reported here instead. Drain them with
/// `Reader::take_events`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A corrupt frame was replaced or skipped.
    CorruptFrame {
        /// Timeline position of the frame, in frames.
        pts: u64,
        /// Absolute source byte position of the frame.
        offset: u64,
        /// Number of frames affected.
        frames: u32,
    },
    /// A declared size was clamped to what the data can hold.
    Clamped {
        /// Which size was clamped.
        what: &'static str,
        /// The declared (or unbounded) value.
        declared: u64,
        /// The value used instead.
        actual: u64,
    },
    /// A checksum did not match.
    ChecksumMismatch {
        /// Which checksum, e.g. `"flac md5"`.
        what: &'static str,
    },
    /// Conflicting gapless information was found (reserved).
    GaplessConflict {
        /// Which sources of information conflict.
        what: &'static str,
    },
}
