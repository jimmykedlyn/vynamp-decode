//! `COMM` chunk parsing.

use crate::extended;
use agulha_core::{Endian, Error, Limits, PcmEncoding, PcmSpec, Result};

/// Bytes of the `COMM` payload that are inspected (AIFF-C header without the name).
pub(crate) const COMM_READ: usize = 22;

/// Decoded `COMM` chunk.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Comm {
    pub spec: PcmSpec,
    pub sample_rate: u32,
    pub channels: u16,
    /// Frame count declared by the chunk.
    pub frames: u32,
}

impl Comm {
    /// Bytes per frame.
    pub fn block(&self) -> u64 {
        u64::from(self.channels) * u64::from(self.spec.bits / 8)
    }
}

fn be16(b: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([b[at], b[at + 1]])
}

fn be32(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// Parses the leading bytes of a `COMM` payload found at absolute `offset`.
/// `payload_len` is the declared size of the whole payload.
pub(crate) fn parse(
    body: &[u8],
    payload_len: u64,
    aifc: bool,
    offset: u64,
    limits: &Limits,
) -> Result<Comm> {
    let invalid = |what| Error::Invalid { what, offset };
    let at_offset = |e: Error| match e {
        Error::Invalid { what, .. } => Error::Invalid { what, offset },
        e => e,
    };
    let min = if aifc { COMM_READ } else { 18 };
    if payload_len < min as u64 || body.len() < min {
        return Err(invalid("comm size"));
    }
    let channels = be16(body, 0);
    let frames = be32(body, 2);
    let sample_bits = be16(body, 6);
    let mut ext = [0u8; 10];
    ext.copy_from_slice(&body[8..18]);
    let sample_rate = extended::to_u32(ext).map_err(at_offset)?;
    let compression: [u8; 4] = if aifc {
        [body[18], body[19], body[20], body[21]]
    } else {
        *b"NONE"
    };

    let bytes = sample_bits.div_ceil(8);
    // Integer PCM in a container just wide enough for the declared sample size.
    let integer = |endian| -> Result<PcmSpec> {
        match sample_bits {
            0 => Err(invalid("sample size")),
            1..=32 => Ok(PcmSpec {
                encoding: PcmEncoding::Signed,
                bits: (bytes * 8) as u8,
                valid_bits: sample_bits as u8,
                endian,
            }),
            _ => Err(Error::Unsupported("sample width")),
        }
    };
    // Integer PCM whose container width is fixed by the compression type.
    let fixed = |width: u16, endian| -> Result<PcmSpec> {
        if sample_bits == 0 || bytes * 8 != width {
            return Err(invalid("sample size"));
        }
        integer(endian)
    };
    let float = |width: u16| -> Result<PcmSpec> {
        if sample_bits != width {
            return Err(invalid("sample size"));
        }
        Ok(PcmSpec {
            encoding: PcmEncoding::Float,
            bits: width as u8,
            valid_bits: width as u8,
            endian: Endian::Big,
        })
    };
    let spec = match &compression {
        b"NONE" | b"twos" => integer(Endian::Big)?,
        b"sowt" => integer(Endian::Little)?,
        b"in24" => fixed(24, Endian::Big)?,
        b"in32" => fixed(32, Endian::Big)?,
        b"42ni" => fixed(24, Endian::Little)?,
        b"23ni" => fixed(32, Endian::Little)?,
        b"fl32" | b"FL32" => float(32)?,
        b"fl64" | b"FL64" => float(64)?,
        b"raw " => {
            if sample_bits == 0 || sample_bits > 8 {
                return Err(invalid("sample size"));
            }
            PcmSpec {
                encoding: PcmEncoding::Unsigned,
                bits: 8,
                valid_bits: sample_bits as u8,
                endian: Endian::Big,
            }
        }
        _ => return Err(Error::Unsupported("aiff compression type")),
    };
    limits.check_channels(channels).map_err(at_offset)?;
    limits.check_sample_rate(sample_rate).map_err(at_offset)?;
    Ok(Comm {
        spec,
        sample_rate,
        channels,
        frames,
    })
}
