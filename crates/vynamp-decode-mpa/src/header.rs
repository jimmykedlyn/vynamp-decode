//! MPEG audio frame header (ISO/IEC 11172-3 section 2.4.1, ISO/IEC 13818-3).
//!
//! The 32-bit header, most significant bit first:
//!
//! ```text
//! AAAAAAAA AAABBCCD EEEEFFGH IIJJKLMM
//! A sync (11 bits, all ones)        B version (00 = 2.5, 01 reserved, 10 = 2, 11 = 1)
//! C layer (01 = III, 10 = II, 11 = I, 00 reserved)   D protection (0 = CRC follows)
//! E bitrate index (0 = free format, 15 forbidden)    F sample rate index (3 reserved)
//! G padding   H private   I channel mode   J mode extension   K copyright   L original
//! M emphasis
//! ```

/// MPEG version of a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MpegVersion {
    /// MPEG-1 (32, 44.1 and 48 kHz).
    V1,
    /// MPEG-2 (16, 22.05 and 24 kHz).
    V2,
    /// MPEG-2.5 (8, 11.025 and 12 kHz), an unofficial extension.
    V25,
}

/// Channel mode of a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelMode {
    /// Two independent-coded channels.
    Stereo,
    /// Joint stereo (intensity and/or mid/side).
    JointStereo,
    /// Two unrelated channels.
    DualChannel,
    /// One channel.
    Mono,
}

/// A parsed frame header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameHeader {
    /// MPEG version.
    pub version: MpegVersion,
    /// Layer, 1 to 3.
    pub layer: u8,
    /// A 16-bit CRC follows the header (protection bit 0).
    pub crc: bool,
    /// Bitrate in kbit/s; 0 means free format.
    pub bitrate_kbps: u32,
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// The padding bit: one extra slot in this frame.
    pub padding: bool,
    /// Channel mode.
    pub channel_mode: ChannelMode,
    /// Raw two-bit mode extension field.
    pub mode_ext: u8,
    /// Raw two-bit emphasis field. The reserved value 2 is kept, not rejected.
    pub emphasis: u8,
    /// Frame length in bytes including the header, or 0 for free format (the length
    /// is then a property of the stream, see [`FrameHeader::slot_size`]).
    pub frame_len: usize,
    /// Samples per channel in the frame: 384 (Layer I), 1152, or 576 (Layer III of
    /// MPEG-2 and 2.5).
    pub samples: u32,
}

/// Bitrates in kbit/s by index 1 to 14. Rows: V1 L1, V1 L2, V1 L3, V2/2.5 L1, V2/2.5 L2 and L3.
const BITRATES: [[u32; 14]; 5] = [
    [
        32, 64, 96, 128, 160, 192, 224, 256, 288, 320, 352, 384, 416, 448,
    ],
    [
        32, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384,
    ],
    [
        32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
    ],
    [
        32, 48, 56, 64, 80, 96, 112, 128, 144, 160, 176, 192, 224, 256,
    ],
    [8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160],
];

/// Parses a 4-byte frame header; `None` for a bad sync word or a reserved/forbidden value
/// (version 01, layer 00, bitrate index 15, sample rate index 3).
pub fn parse_header(b: [u8; 4]) -> Option<FrameHeader> {
    if b[0] != 0xFF || b[1] & 0xE0 != 0xE0 {
        return None;
    }
    let version = match (b[1] >> 3) & 3 {
        0 => MpegVersion::V25,
        2 => MpegVersion::V2,
        3 => MpegVersion::V1,
        _ => return None,
    };
    let layer = match (b[1] >> 1) & 3 {
        1 => 3,
        2 => 2,
        3 => 1,
        _ => return None,
    };
    let br_idx = usize::from(b[2] >> 4);
    if br_idx == 15 {
        return None;
    }
    let rate_idx = usize::from((b[2] >> 2) & 3);
    if rate_idx == 3 {
        return None;
    }
    let sample_rate = match version {
        MpegVersion::V1 => [44100, 48000, 32000][rate_idx],
        MpegVersion::V2 => [22050, 24000, 16000][rate_idx],
        MpegVersion::V25 => [11025, 12000, 8000][rate_idx],
    };
    let row = match (version, layer) {
        (MpegVersion::V1, 1) => 0,
        (MpegVersion::V1, 2) => 1,
        (MpegVersion::V1, _) => 2,
        (_, 1) => 3,
        _ => 4,
    };
    let bitrate_kbps = if br_idx == 0 {
        0
    } else {
        BITRATES[row][br_idx - 1]
    };
    let samples = match (layer, version) {
        (1, _) => 384,
        (3, MpegVersion::V2 | MpegVersion::V25) => 576,
        _ => 1152,
    };
    let padding = b[2] & 2 != 0;
    let channel_mode = match b[3] >> 6 {
        0 => ChannelMode::Stereo,
        1 => ChannelMode::JointStereo,
        2 => ChannelMode::DualChannel,
        _ => ChannelMode::Mono,
    };
    // Bytes per frame = coefficient * bitrate / sample rate, plus one slot of padding.
    // Layer I counts in 4-byte slots, the other layers in bytes.
    let coefficient: u32 = match (layer, version) {
        (1, _) => 48,
        (3, MpegVersion::V2 | MpegVersion::V25) => 72,
        _ => 144,
    };
    let slot = if layer == 1 { 4 } else { 1 };
    let frame_len = if bitrate_kbps == 0 {
        0
    } else {
        // At most 48 * 448000 / 8000 fits comfortably; the product is below 2^32.
        let body = (coefficient * bitrate_kbps * 1000 / sample_rate) as usize;
        let body = body / slot * slot;
        body + usize::from(padding) * slot
    };
    Some(FrameHeader {
        version,
        layer,
        crc: b[1] & 1 == 0,
        bitrate_kbps,
        sample_rate,
        padding,
        channel_mode,
        mode_ext: (b[3] >> 4) & 3,
        emphasis: b[3] & 3,
        frame_len,
        samples,
    })
}

impl FrameHeader {
    /// Size of one padding slot in bytes: 4 for Layer I, 1 otherwise.
    pub fn slot_size(&self) -> usize {
        if self.layer == 1 { 4 } else { 1 }
    }

    /// Number of audio channels.
    pub fn channels(&self) -> u16 {
        if self.channel_mode == ChannelMode::Mono {
            1
        } else {
            2
        }
    }

    /// Size of the Layer III side information (0 for the other layers): 17 or 32 bytes in
    /// MPEG-1 (mono or not), 9 or 17 in MPEG-2 and 2.5.
    pub fn side_info_len(&self) -> usize {
        if self.layer != 3 {
            return 0;
        }
        match (self.version, self.channel_mode == ChannelMode::Mono) {
            (MpegVersion::V1, true) => 17,
            (MpegVersion::V1, false) => 32,
            (_, true) => 9,
            (_, false) => 17,
        }
    }

    /// Smallest plausible frame length: the header, the CRC if present and, for Layer III,
    /// the side information. Shorter lengths (which a free-format distance measurement can
    /// produce from look-alike bytes) cannot be real frames.
    pub fn min_frame_len(&self) -> usize {
        4 + if self.crc { 2 } else { 0 } + self.side_info_len().max(4)
    }
}
