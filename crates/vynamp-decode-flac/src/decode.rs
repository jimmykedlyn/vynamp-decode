//! FLAC frame decoder: subframes, residuals and channel decorrelation.

use crate::StreamInfo;
use crate::frame::{ChannelAssignment, parse_frame_header};
use crate::metadata::STREAMINFO_LEN;
use crate::subframe::decode_subframe;
use vynamp_decode_core::{
    AudioBuffer, BitReader, CodecId, Decoder, Error, LimitKind, Limits, Options, Packet, Result,
    SampleFormat, Track,
};

/// Decodes FLAC packets (one frame each, as produced by `FlacDemuxer`) into planar audio.
///
/// Samples are not rescaled: a stream of `n` bits per sample is stored at that depth
/// in the buffer, which is `U8` up to 8 bits, `I16` up to 16, `I24` up to 24 and `I32`
/// above. An empty packet is a Resilient silence packet of `packet.frames` frames.
/// The decoder trusts the demuxer's CRC-16 check and ignores bytes after the frame.
pub struct FlacDecoder {
    info: StreamInfo,
    format: SampleFormat,
    channels: u16,
    /// One scratch plane per channel, `max_block` samples each.
    planes: Vec<Vec<i64>>,
}

/// Shifts the offset of an `Invalid` error by `base` (the absolute position the
/// error's relative offset counts from).
fn at(e: Error, base: u64) -> Error {
    match e {
        Error::Invalid { what, offset } => Error::Invalid {
            what,
            offset: base.saturating_add(offset),
        },
        other => other,
    }
}

fn format_for(bps: u8) -> SampleFormat {
    match bps {
        0..=8 => SampleFormat::U8,
        9..=16 => SampleFormat::I16,
        17..=24 => SampleFormat::I24,
        _ => SampleFormat::I32,
    }
}

/// Copies `src` into `dst` converted with `conv`; returns whether every sample was in `lo..=hi`.
fn store<T>(dst: &mut [T], src: &[i64], lo: i64, hi: i64, conv: impl Fn(i64) -> T) -> bool {
    let mut bad = false;
    for (d, &s) in dst.iter_mut().zip(src) {
        bad |= (s < lo) | (s > hi);
        *d = conv(s);
    }
    !bad
}

impl FlacDecoder {
    /// Creates a decoder from a FLAC track (its `codec_private` is the STREAMINFO body)
    /// with default limits.
    ///
    /// # Errors
    /// `Unsupported` for another codec; `Invalid` for a malformed STREAMINFO.
    pub fn new(track: &Track) -> Result<Self> {
        Self::with_options(track, &Options::default())
    }

    /// Like [`new`](Self::new), checking the scratch size against `opts.limits`.
    ///
    /// # Errors
    /// As [`new`](Self::new), plus `Limit` if the channel count or scratch memory exceeds the limits.
    pub fn with_options(track: &Track, opts: &Options) -> Result<Self> {
        if track.codec != CodecId::Flac {
            return Err(Error::Unsupported("not a flac track"));
        }
        let body: &[u8; STREAMINFO_LEN] = track
            .codec_private
            .get(..STREAMINFO_LEN)
            .and_then(|b| b.try_into().ok())
            .ok_or(Error::Invalid {
                what: "flac streaminfo",
                offset: 0,
            })?;
        let info = StreamInfo::parse(body, 0)?;
        let limits: &Limits = &opts.limits;
        limits.check_channels(u16::from(info.channels))?;
        let samples = usize::from(info.max_block) * usize::from(info.channels);
        limits.check_bytes((samples * size_of::<i64>()) as u64)?;
        let mut planes = Vec::new();
        planes
            .try_reserve_exact(usize::from(info.channels))
            .map_err(|_| Error::Limit(LimitKind::Bytes))?;
        for _ in 0..info.channels {
            let mut p = Vec::new();
            p.try_reserve_exact(usize::from(info.max_block))
                .map_err(|_| Error::Limit(LimitKind::Bytes))?;
            p.resize(usize::from(info.max_block), 0i64);
            planes.push(p);
        }
        Ok(Self {
            format: format_for(info.bits_per_sample),
            channels: u16::from(info.channels),
            info,
            planes,
        })
    }

    fn silence(&self, frames: usize, out: &mut AudioBuffer) -> Result<()> {
        out.reset(self.format, self.channels, frames)?;
        for c in 0..usize::from(self.channels) {
            match self.format {
                SampleFormat::U8 => out.plane_u8_mut(c).map(|p| p.fill(128)),
                SampleFormat::I16 => out.plane_i16_mut(c).map(|p| p.fill(0)),
                _ => out.plane_i32_mut(c).map(|p| p.fill(0)),
            };
        }
        Ok(())
    }
}

impl Decoder for FlacDecoder {
    fn decode(&mut self, packet: &Packet, out: &mut AudioBuffer) -> Result<()> {
        if packet.data.is_empty() {
            return self.silence(packet.frames as usize, out);
        }
        let header =
            parse_frame_header(&packet.data, &self.info).map_err(|e| at(e, packet.offset))?;
        let frames = header.block_size as usize;
        let bps = u32::from(header.bits_per_sample);
        let base = packet.offset + header.header_len as u64;
        let mut r = BitReader::new(&packet.data[header.header_len..]);

        // Extra bit for the side channel of the stereo decorrelation modes.
        let (bps0, bps1) = match header.channel_assignment {
            ChannelAssignment::Independent(_) => (bps, bps),
            ChannelAssignment::LeftSide | ChannelAssignment::MidSide => (bps, bps + 1),
            ChannelAssignment::RightSide => (bps + 1, bps),
        };
        let channels = usize::from(self.channels);
        for (c, plane) in self.planes.iter_mut().enumerate().take(channels) {
            let depth = match c {
                0 => bps0,
                1 => bps1,
                _ => bps,
            };
            decode_subframe(&mut r, &mut plane[..frames], depth).map_err(|e| at(e, base))?;
        }

        if channels == 2 {
            let (a, b) = self.planes.split_at_mut(1);
            let (p0, p1) = (&mut a[0][..frames], &mut b[0][..frames]);
            match header.channel_assignment {
                ChannelAssignment::LeftSide => {
                    for (l, s) in p0.iter().zip(p1.iter_mut()) {
                        *s = l - *s;
                    }
                }
                ChannelAssignment::RightSide => {
                    for (s, r) in p0.iter_mut().zip(p1.iter()) {
                        *s += r;
                    }
                }
                ChannelAssignment::MidSide => {
                    for (m, s) in p0.iter_mut().zip(p1.iter_mut()) {
                        let mid = (*m << 1) | (*s & 1);
                        *m = (mid + *s) >> 1;
                        *s = (mid - *s) >> 1;
                    }
                }
                ChannelAssignment::Independent(_) => {}
            }
        }

        out.reset(self.format, self.channels, frames)?;
        let hi = (1i64 << (bps - 1)) - 1;
        let lo = -hi - 1;
        let mut ok = true;
        for (c, plane) in self.planes.iter().enumerate().take(channels) {
            let src = &plane[..frames];
            ok &= match self.format {
                SampleFormat::U8 => out
                    .plane_u8_mut(c)
                    .is_some_and(|d| store(d, src, lo, hi, |v| (v + 128) as u8)),
                SampleFormat::I16 => out
                    .plane_i16_mut(c)
                    .is_some_and(|d| store(d, src, lo, hi, |v| v as i16)),
                _ => out
                    .plane_i32_mut(c)
                    .is_some_and(|d| store(d, src, lo, hi, |v| v as i32)),
            };
        }
        if !ok {
            return Err(Error::Invalid {
                what: "flac sample range",
                offset: packet.offset,
            });
        }
        Ok(())
    }

    fn reset(&mut self) {}
}
