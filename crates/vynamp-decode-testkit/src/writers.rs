//! Container writers that build test files from raw interleaved samples.

use vynamp_decode_core::{PcmEncoding, PcmSpec};

/// WAV container flavour produced by [`wav`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WavVariant {
    /// Plain RIFF/WAVE with a 16-byte `fmt ` chunk.
    Riff,
    /// RIFF/WAVE with a `WAVE_FORMAT_EXTENSIBLE` `fmt ` chunk.
    Extensible,
    /// RF64 with a `ds64` chunk.
    Rf64,
    /// BW64 with a `ds64` chunk.
    Bw64,
    /// Sony Wave64.
    Wave64,
    /// RIFF/WAVE whose `data` size is 0xFFFFFFFF (streamed output).
    RiffStreaming,
}

type Guid = [u8; 16];

const fn guid(d1: u32, d2: u16, d3: u16, d4: [u8; 8]) -> Guid {
    let a = d1.to_le_bytes();
    let b = d2.to_le_bytes();
    let c = d3.to_le_bytes();
    [
        a[0], a[1], a[2], a[3], b[0], b[1], c[0], c[1], d4[0], d4[1], d4[2], d4[3], d4[4], d4[5],
        d4[6], d4[7],
    ]
}

const TAIL: [u8; 8] = [0x8C, 0xD1, 0x00, 0xC0, 0x4F, 0x8E, 0xDB, 0x8A];
const GUID_RIFF: Guid = guid(
    0x6666_6972,
    0x912E,
    0x11CF,
    [0xA5, 0xD6, 0x28, 0xDB, 0x04, 0xC1, 0x00, 0x00],
);
const GUID_WAVE: Guid = guid(0x6576_6177, 0xACF3, 0x11D3, TAIL);
const GUID_FMT: Guid = guid(0x2074_6D66, 0xACF3, 0x11D3, TAIL);
const GUID_DATA: Guid = guid(0x6174_6164, 0xACF3, 0x11D3, TAIL);

fn subformat(tag: u16) -> Guid {
    guid(
        u32::from(tag),
        0x0000,
        0x0010,
        [0x80, 0x00, 0x00, 0xaa, 0x00, 0x38, 0x9b, 0x71],
    )
}

/// Container width in bits: `spec.bits` rounded up to whole bytes (at least 8).
fn container_bits(spec: PcmSpec) -> u16 {
    u16::from(spec.bits).div_ceil(8).max(1) * 8
}

fn fmt_body(spec: PcmSpec, rate: u32, channels: u16, extensible: bool) -> Vec<u8> {
    let tag: u16 = if spec.encoding == PcmEncoding::Float {
        3
    } else {
        1
    };
    let bits = container_bits(spec);
    let block_align = channels.wrapping_mul(bits / 8);
    let mut v = Vec::new();
    v.extend((if extensible { 0xFFFEu16 } else { tag }).to_le_bytes());
    v.extend(channels.to_le_bytes());
    v.extend(rate.to_le_bytes());
    v.extend(rate.wrapping_mul(u32::from(block_align)).to_le_bytes());
    v.extend(block_align.to_le_bytes());
    v.extend(bits.to_le_bytes());
    if extensible {
        v.extend(22u16.to_le_bytes());
        v.extend(u16::from(spec.valid_bits).to_le_bytes());
        let mask: u32 = if channels >= 32 {
            u32::MAX
        } else {
            (1u32 << channels) - 1
        };
        v.extend(mask.to_le_bytes());
        v.extend(subformat(tag));
    }
    v
}

fn riff_chunk(out: &mut Vec<u8>, id: &[u8; 4], size: u32, body: &[u8]) {
    out.extend(id);
    out.extend(size.to_le_bytes());
    out.extend(body);
    if body.len() % 2 == 1 {
        out.push(0);
    }
}

fn w64_chunk(out: &mut Vec<u8>, id: &Guid, body: &[u8]) {
    out.extend(id);
    out.extend((body.len() as u64 + 24).to_le_bytes());
    out.extend(body);
    out.resize(out.len().next_multiple_of(8), 0);
}

/// Builds a WAV file of the given `variant` around `interleaved_bytes`
/// (little-endian samples, already interleaved).
pub fn wav(
    spec: PcmSpec,
    rate: u32,
    channels: u16,
    interleaved_bytes: &[u8],
    variant: WavVariant,
) -> Vec<u8> {
    let data = interleaved_bytes;
    let mut out = Vec::new();
    if variant == WavVariant::Wave64 {
        out.extend(GUID_RIFF);
        out.extend(0u64.to_le_bytes());
        out.extend(GUID_WAVE);
        w64_chunk(&mut out, &GUID_FMT, &fmt_body(spec, rate, channels, false));
        w64_chunk(&mut out, &GUID_DATA, data);
        let total = out.len() as u64;
        out[16..24].copy_from_slice(&total.to_le_bytes());
        return out;
    }
    let (magic, extensible) = match variant {
        WavVariant::Rf64 => (b"RF64", false),
        WavVariant::Bw64 => (b"BW64", false),
        WavVariant::Extensible => (b"RIFF", true),
        _ => (b"RIFF", false),
    };
    let big = matches!(variant, WavVariant::Rf64 | WavVariant::Bw64);
    out.extend(magic);
    out.extend(0u32.to_le_bytes());
    out.extend(b"WAVE");
    if big {
        let frame_bytes = (u64::from(channels) * u64::from(container_bits(spec) / 8)).max(1);
        let mut ds = Vec::new();
        ds.extend(0u64.to_le_bytes()); // RIFF size, patched below
        ds.extend((data.len() as u64).to_le_bytes());
        ds.extend((data.len() as u64 / frame_bytes).to_le_bytes());
        ds.extend(0u32.to_le_bytes());
        riff_chunk(&mut out, b"ds64", 28, &ds);
    }
    let fmt = fmt_body(spec, rate, channels, extensible);
    riff_chunk(&mut out, b"fmt ", fmt.len() as u32, &fmt);
    let data_size = if big || variant == WavVariant::RiffStreaming {
        u32::MAX
    } else {
        data.len() as u32
    };
    riff_chunk(&mut out, b"data", data_size, data);
    let total = out.len() as u64 - 8;
    if big {
        out[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        out[20..28].copy_from_slice(&total.to_le_bytes());
    } else {
        out[4..8].copy_from_slice(&(total as u32).to_le_bytes());
    }
    out
}

/// Encodes a positive integer as an IEEE 754 80-bit extended number (big endian).
///
/// Zero is encoded as all zero bytes.
pub fn extended_80(value: u32) -> [u8; 10] {
    let mut out = [0u8; 10];
    if value == 0 {
        return out;
    }
    let e = 31 - value.leading_zeros();
    let exp = 16383 + e as u16;
    let mantissa = u64::from(value) << (63 - e);
    out[..2].copy_from_slice(&exp.to_be_bytes());
    out[2..].copy_from_slice(&mantissa.to_be_bytes());
    out
}

fn iff_chunk(out: &mut Vec<u8>, id: &[u8; 4], body: &[u8]) {
    out.extend(id);
    out.extend((body.len() as u32).to_be_bytes());
    out.extend(body);
    if body.len() % 2 == 1 {
        out.push(0);
    }
}

/// Builds an AIFF file (`compression == None`) or AIFF-C file around
/// `interleaved_bytes` (samples already interleaved and in the byte order
/// the file format requires). The `COMM` sample size is `spec.valid_bits`;
/// the frame count is derived from the data length.
pub fn aiff(
    spec: PcmSpec,
    rate: u32,
    channels: u16,
    interleaved_bytes: &[u8],
    compression: Option<[u8; 4]>,
) -> Vec<u8> {
    let frame_bytes = (usize::from(channels) * usize::from(container_bits(spec) / 8)).max(1);
    let frames = (interleaved_bytes.len() / frame_bytes) as u32;
    let mut out = Vec::new();
    out.extend(b"FORM");
    out.extend(0u32.to_be_bytes());
    out.extend(if compression.is_some() {
        b"AIFC"
    } else {
        b"AIFF"
    });
    if compression.is_some() {
        iff_chunk(&mut out, b"FVER", &0xA280_5140u32.to_be_bytes());
    }
    let mut comm = Vec::new();
    comm.extend(channels.to_be_bytes());
    comm.extend(frames.to_be_bytes());
    comm.extend(u16::from(spec.valid_bits).to_be_bytes());
    comm.extend(extended_80(rate));
    if let Some(c) = compression {
        comm.extend(c);
        comm.extend([0u8, 0]); // empty pstring, padded to even length
    }
    iff_chunk(&mut out, b"COMM", &comm);
    let mut ssnd = Vec::new();
    ssnd.extend(0u32.to_be_bytes()); // offset
    ssnd.extend(0u32.to_be_bytes()); // block size
    ssnd.extend(interleaved_bytes);
    iff_chunk(&mut out, b"SSND", &ssnd);
    let total = (out.len() - 8) as u32;
    out[4..8].copy_from_slice(&total.to_be_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use vynamp_decode_core::Endian;

    #[test]
    fn riff_layout() {
        let spec = PcmSpec {
            encoding: PcmEncoding::Signed,
            bits: 16,
            valid_bits: 16,
            endian: Endian::Little,
        };
        let w = wav(spec, 8000, 1, &[1, 2, 3, 4], WavVariant::Riff);
        assert_eq!(&w[0..4], b"RIFF");
        let riff_size = u32::from_le_bytes(w[4..8].try_into().unwrap()) as usize;
        assert_eq!(riff_size, w.len() - 8);
        assert_eq!(&w[36..40], b"data");
        assert_eq!(&w[44..], &[1, 2, 3, 4]);
    }

    #[test]
    fn aiff_layout() {
        let spec = PcmSpec {
            encoding: PcmEncoding::Signed,
            bits: 16,
            valid_bits: 16,
            endian: Endian::Big,
        };
        let a = aiff(spec, 44100, 1, &[1, 2, 3, 4], None);
        assert_eq!(&a[0..4], b"FORM");
        assert_eq!(&a[8..12], b"AIFF");
        assert_eq!(&a[12..16], b"COMM");
        assert_eq!(&a[28..38], &[0x40, 0x0E, 0xAC, 0x44, 0, 0, 0, 0, 0, 0]);
        assert_eq!(
            u32::from_be_bytes(a[4..8].try_into().unwrap()) as usize,
            a.len() - 8
        );
        assert_eq!(&a[a.len() - 4..], &[1, 2, 3, 4]);
        assert_eq!(&aiff(spec, 8000, 1, &[], Some(*b"NONE"))[8..12], b"AIFC");
    }
}
