//! Compressed packet.

/// A unit of compressed (or raw) data read from a container.
#[derive(Debug, Default)]
pub struct Packet {
    /// Track the packet belongs to.
    pub track: u32,
    /// Presentation timestamp in frames.
    pub pts: u64,
    /// Number of frames the packet decodes to.
    pub frames: u32,
    /// Packet payload.
    pub data: Vec<u8>,
}
