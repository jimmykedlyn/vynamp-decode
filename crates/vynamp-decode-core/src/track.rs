//! Track description.

use crate::PcmSpec;

/// Identifies the codec of a track.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodecId {
    /// Raw PCM with the given layout.
    Pcm(PcmSpec),
    /// FLAC.
    Flac,
    /// MPEG-1 Audio Layer I.
    Mp1,
    /// MPEG-1 Audio Layer II.
    Mp2,
    /// MPEG-1 Audio Layer III.
    Mp3,
}

/// How reliable a track's `delay`, `padding` and `frames` are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GaplessKind {
    /// Exact by construction (lossless formats).
    Exact,
    /// Taken from encoder metadata.
    FromMetadata,
    /// Not known.
    Unknown,
}

/// An audio track inside a container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    /// Track identifier.
    pub id: u32,
    /// Codec of the track.
    pub codec: CodecId,
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// Channel count.
    pub channels: u16,
    /// Channel position mask (0 if unspecified).
    pub channel_mask: u32,
    /// Total frames, if known.
    pub frames: Option<u64>,
    /// Encoder delay in frames to skip at the start.
    pub delay: u32,
    /// Encoder padding in frames to drop at the end.
    pub padding: u32,
    /// Reliability of `delay`, `padding` and `frames`.
    pub gapless: GaplessKind,
    /// Codec-specific setup data.
    pub codec_private: Vec<u8>,
}
