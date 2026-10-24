//! Demuxer and decoder traits.

use crate::{AudioBuffer, Event, Packet, Result, Track};

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
    /// Moves pending diagnostic events into `out`. The default reports none.
    fn drain_events(&mut self, _out: &mut Vec<Event>) {}
    /// Returns, and resets, the number of events dropped because the demuxer's own
    /// queue was full. The default drops none.
    fn take_events_dropped(&mut self) -> u64 {
        0
    }
}

/// Decodes packets into audio.
pub trait Decoder: Send {
    /// Decodes `packet` into `out`.
    fn decode(&mut self, packet: &Packet, out: &mut AudioBuffer) -> Result<()>;
    /// Clears internal state (e.g. after a seek).
    fn reset(&mut self);
    /// Moves pending diagnostic events into `out`. The default reports none.
    fn drain_events(&mut self, _out: &mut Vec<Event>) {}
}
