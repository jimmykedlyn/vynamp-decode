//! Demuxer and decoder traits.

use crate::{AudioBuffer, Packet, Result, Track};

/// Result of a seek.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeekTo {
    /// Frame at which the next packet starts.
    pub frame: u64,
    /// Frames to discard after decoding to reach the requested position.
    pub preroll: u64,
}

/// Splits a container into packets.
pub trait Demuxer: Send {
    /// Tracks found in the container.
    fn tracks(&self) -> &[Track];
    /// Reads the next packet into `out`; returns `false` at end of stream.
    fn next_packet(&mut self, out: &mut Packet) -> Result<bool>;
    /// Seeks `track` towards `frame`.
    fn seek(&mut self, track: u32, frame: u64) -> Result<SeekTo>;
}

/// Decodes packets into audio.
pub trait Decoder: Send {
    /// Decodes `packet` into `out`.
    fn decode(&mut self, packet: &Packet, out: &mut AudioBuffer) -> Result<()>;
    /// Clears internal state (e.g. after a seek).
    fn reset(&mut self);
}
