//! FLAC demuxer (RFC 9639): metadata, frame packetization and exact seek.
//!
//! `open_flac` skips a leading ID3v2 tag, checks the `fLaC` marker and parses
//! the metadata blocks. STREAMINFO must come first and is validated; the total
//! size of the non-padding blocks is bounded by `Limits::max_metadata_bytes`,
//! and padding is skipped without being read. Seek table placeholders are
//! dropped. A zero `total_samples` means the length is unknown
//! (`Track::frames == None`).
//!
//! `next_packet` yields one packet per frame (header through CRC-16), found by
//! sync code, header CRC-8 and agreement with STREAMINFO, and checked with the
//! frame CRC-16. In `Mode::Strict` a bad CRC, garbage between frames, a gap in
//! the sample numbers or a stream that ends before `total_samples` is an
//! error. In `Mode::Resilient` a corrupt frame, and any samples missing
//! between two valid frames, become silence packets (empty `data`, `frames`
//! set) and each is reported as `Event::CorruptFrame`. `seek` is exact: it
//! returns the first sample of the frame holding the target and the preroll
//! to discard, using the seek table when it checks out and bisection over the
//! file otherwise. Only an ID3v1 tag is recognised after the last frame.
#![warn(missing_docs)]

mod demux;
mod frame;
mod metadata;

pub use demux::{FlacDemuxer, open_flac};
pub use frame::{ChannelAssignment, FrameHeader};
pub use metadata::{BlockKind, RawBlock, SeekPoint, StreamInfo};

/// Detects a FLAC stream marker in `header`: `fLaC` at offset 0 gives 100.
pub fn probe_flac(header: &[u8]) -> u8 {
    if header.starts_with(b"fLaC") { 100 } else { 0 }
}
