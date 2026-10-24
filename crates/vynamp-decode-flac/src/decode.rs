//! FLAC frame decoder: subframes, residuals and channel decorrelation.

use crate::StreamInfo;
use crate::frame::{ChannelAssignment, parse_frame_header};
use crate::metadata::STREAMINFO_LEN;
use crate::subframe::decode_subframe;
use vynamp_decode_core::{
    AudioBuffer, BitReader, CodecId, Decoder, Error, Event, LimitKind, Limits, Md5, Mode, Options,
    Packet, Result, SampleFormat, Track,
};

/// Decodes FLAC packets (one frame each, as produced by `FlacDemuxer`) into planar audio.
///
/// Samples are not rescaled: a stream of `n` bits per sample is stored at that depth
/// in the buffer, which is `U8` up to 8 bits, `I16` up to 16, `I24` up to 24 and `I32`
/// above. An empty packet is a Resilient silence packet of `packet.frames` frames
/// (clamped to the maximum block size and checked against the frame limit). In
/// `Mode::Resilient` a frame that fails to decode is replaced by silence of its length
/// and reported as `Event::CorruptFrame`; in `Mode::Strict` the error is returned.
/// The decoder trusts the demuxer's CRC-16 check and ignores bytes after the frame.
///
/// When STREAMINFO carries a nonzero MD5 and total sample count, and the stream is decoded
/// from sample 0 without a seek or a silence packet, the decoded samples are hashed and
/// compared with it once `total_samples` have been decoded. A mismatch is an
/// `Invalid { what: "flac md5" }` error from that `decode` call in `Mode::Strict`, and an
/// `Event::ChecksumMismatch` (see `drain_events`) in `Mode::Resilient`.
pub struct FlacDecoder {
    info: StreamInfo,
    format: SampleFormat,
    channels: u16,
    /// One scratch plane per channel, `max_block` samples each.
    planes: Vec<Vec<i64>>,
    mode: Mode,
    limits: Limits,
    md5: Md5Check,
    events: Vec<Event>,
}

/// State of the STREAMINFO MD5 verification.
struct Md5Check {
    /// Whether samples are still being hashed.
    active: bool,
    /// Whether the first packet has been seen (it must start at sample 0).
    started: bool,
    hasher: Md5,
    /// Frames hashed so far.
    hashed: u64,
    /// Bytes per sample in the hash input.
    width: usize,
    /// Reusable byte buffer for one packet's hash input.
    scratch: Vec<u8>,
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
        let verify = info.md5 != [0; 16] && info.total_samples > 0;
        let width = usize::from(info.bits_per_sample).div_ceil(8);
        let mut scratch = Vec::new();
        if verify {
            let bytes = samples * width;
            limits.check_bytes(bytes as u64)?;
            scratch
                .try_reserve_exact(bytes)
                .map_err(|_| Error::Limit(LimitKind::Bytes))?;
        }
        Ok(Self {
            mode: opts.mode,
            limits: opts.limits,
            md5: Md5Check {
                active: verify,
                started: false,
                hasher: Md5::new(),
                hashed: 0,
                width,
                scratch,
            },
            events: Vec::new(),
            format: format_for(info.bits_per_sample),
            channels: u16::from(info.channels),
            info,
            planes,
        })
    }

    /// Fills `out` with silence for `frames` frames starting at `pts`, after clamping
    /// the count to the maximum block size and checking the frame limit.
    fn silence(&self, pts: u64, frames: u32, out: &mut AudioBuffer) -> Result<()> {
        let frames = frames.min(u32::from(self.info.max_block));
        self.limits
            .check_frames(pts.saturating_add(u64::from(frames)))?;
        out.reset(self.format, self.channels, frames as usize)?;
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

impl FlacDecoder {
    /// Hashes the `frames` samples just decoded into `planes` and, on reaching
    /// `total_samples`, compares the digest with STREAMINFO's.
    fn verify(&mut self, frames: usize, offset: u64) -> Result<()> {
        // RFC 9639, section 8.2: "The MD5 checksum is computed over the audio samples ...
        // interleaved ... each sample as a little-endian signed integer of ceil(bits per
        // sample / 8) bytes" (for 8-bit audio this is the signed value, not the U8 offset
        // form stored in the output buffer).
        let m = &mut self.md5;
        m.scratch.clear();
        for i in 0..frames {
            for plane in &self.planes {
                m.scratch
                    .extend_from_slice(&plane[i].to_le_bytes()[..m.width]);
            }
        }
        m.hasher.update(&m.scratch);
        m.hashed += frames as u64;
        match m.hashed.cmp(&self.info.total_samples) {
            std::cmp::Ordering::Less => Ok(()),
            // More samples than declared: the digest cannot be compared.
            std::cmp::Ordering::Greater => {
                m.active = false;
                Ok(())
            }
            std::cmp::Ordering::Equal => {
                m.active = false;
                if std::mem::take(&mut m.hasher).finish() == self.info.md5 {
                    return Ok(());
                }
                match self.mode {
                    Mode::Strict => Err(Error::Invalid {
                        what: "flac md5",
                        offset,
                    }),
                    Mode::Resilient => {
                        self.events
                            .push(Event::ChecksumMismatch { what: "flac md5" });
                        Ok(())
                    }
                }
            }
        }
    }

    fn decode_packet(&mut self, packet: &Packet, out: &mut AudioBuffer) -> Result<()> {
        if packet.data.is_empty() {
            return self.silence(packet.pts, packet.frames, out);
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
        if self.md5.active {
            self.verify(frames, packet.offset)?;
        }
        Ok(())
    }
}

impl Decoder for FlacDecoder {
    fn decode(&mut self, packet: &Packet, out: &mut AudioBuffer) -> Result<()> {
        // Only a stream decoded from its very first sample can match the MD5.
        if !self.md5.started {
            self.md5.started = true;
            self.md5.active &= packet.pts == 0;
        }
        // A silence packet stands for lost samples.
        self.md5.active &= !packet.data.is_empty();
        let result = self.decode_packet(packet, out);
        if result.is_err() {
            self.md5.active = false;
        }
        match result {
            // Resilient: a frame that cannot be decoded becomes silence of its length.
            Err(Error::Invalid { .. }) if self.mode == Mode::Resilient => {
                let frames = parse_frame_header(&packet.data, &self.info)
                    .map_or(packet.frames, |h| h.block_size);
                self.silence(packet.pts, frames, out)?;
                self.events.push(Event::CorruptFrame {
                    pts: packet.pts,
                    offset: packet.offset,
                    frames: out.frames() as u32,
                });
                Ok(())
            }
            other => other,
        }
    }

    fn reset(&mut self) {
        self.md5.active = false;
    }

    fn drain_events(&mut self, out: &mut Vec<Event>) {
        out.append(&mut self.events);
    }
}
