//! `fmt ` chunk parsing shared by RIFF/RF64/BW64 and Wave64.

use vynamp_decode_core::{Endian, Error, Limits, PcmEncoding, PcmSpec, Result};

/// Bytes of the `fmt ` payload that are inspected (WAVEFORMATEXTENSIBLE).
pub(crate) const FMT_READ: usize = 40;

const TAG_PCM: u16 = 1;
const TAG_FLOAT: u16 = 3;
const TAG_EXTENSIBLE: u16 = 0xFFFE;

/// Decoded `fmt ` chunk.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Fmt {
    pub spec: PcmSpec,
    pub sample_rate: u32,
    pub channels: u16,
    pub channel_mask: u32,
    /// Bytes per frame (`channels * bits / 8`).
    pub block_align: u16,
}

fn le16(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn le32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// Format tag from a SubFormat GUID of the form `XXXXXXXX-0000-0010-8000-00aa00389b71`.
fn subformat_tag(g: &[u8]) -> Option<u16> {
    const TAIL: [u8; 12] = [
        0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xaa, 0x00, 0x38, 0x9b, 0x71,
    ];
    if g[4..16] != TAIL || g[2] != 0 || g[3] != 0 {
        return None;
    }
    Some(le16(g, 0))
}

/// Parses the leading bytes of a `fmt ` payload found at absolute `offset`.
/// `payload_len` is the declared size of the whole payload.
pub(crate) fn parse_fmt(
    body: &[u8],
    payload_len: u64,
    offset: u64,
    limits: &Limits,
) -> Result<Fmt> {
    let invalid = |what| Error::Invalid { what, offset };
    let at_offset = |e: Error| match e {
        Error::Invalid { what, .. } => Error::Invalid { what, offset },
        e => e,
    };
    if payload_len < 16 || body.len() < 16 {
        return Err(invalid("fmt size"));
    }
    let mut tag = le16(body, 0);
    let channels = le16(body, 2);
    let sample_rate = le32(body, 4);
    let block_align = le16(body, 12);
    let bits = le16(body, 14);
    let mut valid_bits = bits;
    let mut channel_mask = 0;
    if tag == TAG_EXTENSIBLE {
        if payload_len < 40 || body.len() < FMT_READ {
            return Err(invalid("fmt size"));
        }
        valid_bits = le16(body, 18);
        channel_mask = le32(body, 20);
        tag = subformat_tag(&body[24..40]).ok_or(Error::Unsupported("wave subformat"))?;
        if valid_bits == 0 {
            valid_bits = bits;
        }
        if valid_bits > bits {
            return Err(invalid("valid bits"));
        }
    }
    let encoding = match (tag, bits) {
        (TAG_PCM, 8) => PcmEncoding::Unsigned,
        (TAG_PCM, 16 | 24 | 32) => PcmEncoding::Signed,
        (TAG_FLOAT, 32 | 64) => PcmEncoding::Float,
        (TAG_PCM | TAG_FLOAT, _) => return Err(Error::Unsupported("sample width")),
        _ => return Err(Error::Unsupported("format tag")),
    };
    limits.check_channels(channels).map_err(at_offset)?;
    limits.check_sample_rate(sample_rate).map_err(at_offset)?;
    if u32::from(block_align) != u32::from(channels) * u32::from(bits / 8) {
        return Err(invalid("block align"));
    }
    Ok(Fmt {
        spec: PcmSpec {
            encoding,
            bits: bits as u8,
            valid_bits: valid_bits as u8,
            endian: Endian::Little,
        },
        sample_rate,
        channels,
        channel_mask,
        block_align,
    })
}
