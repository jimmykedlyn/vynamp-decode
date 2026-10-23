//! Raw PCM decoder.
#![warn(missing_docs)]

use vynamp_decode_core::{
    AudioBuffer, CodecId, Decoder, Endian, Error, Packet, PcmEncoding, Result, SampleFormat, Track,
};

#[derive(Debug, Clone, Copy)]
enum Kind {
    U8,
    S8,
    I16,
    I24,
    I32,
    F32,
    F64,
}

impl Kind {
    fn format(self) -> SampleFormat {
        match self {
            Kind::U8 | Kind::S8 => SampleFormat::U8,
            Kind::I16 => SampleFormat::I16,
            Kind::I24 => SampleFormat::I24,
            Kind::I32 => SampleFormat::I32,
            Kind::F32 => SampleFormat::F32,
            Kind::F64 => SampleFormat::F64,
        }
    }
}

/// Decoder for raw PCM packets (interleaved, any supported width and byte order).
#[derive(Debug)]
pub struct PcmDecoder {
    kind: Kind,
    big: bool,
    channels: u16,
    bytes_per_sample: usize,
    bytes_per_frame: usize,
}

impl PcmDecoder {
    /// Creates a decoder for `track`.
    ///
    /// Returns `Unsupported("pcm spec")` if the track is not PCM, has zero
    /// channels, or uses an encoding/width combination that is not supported.
    pub fn new(track: &Track) -> Result<Self> {
        let unsupported = Error::Unsupported("pcm spec");
        let CodecId::Pcm(spec) = &track.codec else {
            return Err(unsupported);
        };
        let kind = match (spec.encoding, spec.bits) {
            (PcmEncoding::Unsigned, 8) => Kind::U8,
            (PcmEncoding::Signed, 8) => Kind::S8,
            (PcmEncoding::Signed, 16) => Kind::I16,
            (PcmEncoding::Signed, 24) => Kind::I24,
            (PcmEncoding::Signed, 32) => Kind::I32,
            (PcmEncoding::Float, 32) => Kind::F32,
            (PcmEncoding::Float, 64) => Kind::F64,
            _ => return Err(unsupported),
        };
        let bytes_per_sample = usize::from(spec.bits / 8);
        let bytes_per_frame = usize::from(track.channels)
            .checked_mul(bytes_per_sample)
            .filter(|&n| n > 0)
            .ok_or(unsupported)?;
        Ok(Self {
            kind,
            big: spec.endian == Endian::Big,
            channels: track.channels,
            bytes_per_sample,
            bytes_per_frame,
        })
    }
}

/// Reads sample `f` of channel `c` from interleaved `data`, calling `conv` on its bytes.
macro_rules! deinterleave {
    ($self:ident, $data:ident, $out:ident, $plane:ident, $n:literal, $conv:expr) => {{
        let conv = $conv;
        let step = $self.bytes_per_frame;
        for c in 0..usize::from($self.channels) {
            let plane = $out.$plane(c).ok_or(Error::Unsupported("pcm spec"))?;
            for (dst, frame) in plane.iter_mut().zip($data.chunks_exact(step)) {
                let at = c * $self.bytes_per_sample;
                let mut b = [0u8; $n];
                b.copy_from_slice(&frame[at..at + $n]);
                *dst = conv(b, $self.big);
            }
        }
    }};
}

impl Decoder for PcmDecoder {
    fn decode(&mut self, packet: &Packet, out: &mut AudioBuffer) -> Result<()> {
        let frames = packet.frames as usize;
        let invalid = Error::Invalid {
            what: "pcm packet size",
            offset: 0,
        };
        let expected = frames
            .checked_mul(self.bytes_per_frame)
            .ok_or(invalid.clone())?;
        if packet.data.len() != expected {
            return Err(invalid);
        }
        out.reset(self.kind.format(), self.channels, frames)?;
        let data = &packet.data[..];
        match self.kind {
            Kind::U8 => deinterleave!(self, data, out, plane_u8_mut, 1, |b: [u8; 1], _| b[0]),
            Kind::S8 => {
                deinterleave!(self, data, out, plane_u8_mut, 1, |b: [u8; 1], _| b[0]
                    ^ 0x80)
            }
            Kind::I16 => deinterleave!(self, data, out, plane_i16_mut, 2, |b, big| {
                if big {
                    i16::from_be_bytes(b)
                } else {
                    i16::from_le_bytes(b)
                }
            }),
            Kind::I24 => deinterleave!(self, data, out, plane_i32_mut, 3, |b: [u8; 3], big| {
                // Place the 24 bits in the top of an i32, then shift back arithmetically.
                let v = if big {
                    i32::from_be_bytes([b[0], b[1], b[2], 0])
                } else {
                    i32::from_le_bytes([0, b[0], b[1], b[2]])
                };
                v >> 8
            }),
            Kind::I32 => deinterleave!(self, data, out, plane_i32_mut, 4, |b, big| {
                if big {
                    i32::from_be_bytes(b)
                } else {
                    i32::from_le_bytes(b)
                }
            }),
            Kind::F32 => deinterleave!(self, data, out, plane_f32_mut, 4, |b, big| {
                if big {
                    f32::from_be_bytes(b)
                } else {
                    f32::from_le_bytes(b)
                }
            }),
            Kind::F64 => deinterleave!(self, data, out, plane_f64_mut, 8, |b, big| {
                if big {
                    f64::from_be_bytes(b)
                } else {
                    f64::from_le_bytes(b)
                }
            }),
        }
        Ok(())
    }

    fn reset(&mut self) {}
}
