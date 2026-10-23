//! RIFF/WAVE, RF64/BW64 and Wave64 demuxer.
//!
//! Exposes a single PCM track (id 0) split into packets of up to
//! [`PACKET_FRAMES`] frames.
//!
//! Notes on malformed or streamed input:
//!
//! - A `data` chunk whose size is `0xFFFFFFFF` (plain RIFF), or `0` with more
//!   bytes after its header (RIFF and Wave64), has an unknown size. In
//!   `Mode::Strict` opening fails with `Invalid { what: "unknown data size" }`;
//!   in `Mode::Resilient` the track has `frames == None` and samples are read up
//!   to the container size declared in the file header when that is usable
//!   (so trailing `LIST`/`id3 ` chunks are not decoded as audio), else to the end
//!   of the source. A `data` chunk of size 0 that ends the file is a valid empty
//!   stream (`frames == Some(0)`).
//! - If the data size is not a multiple of the frame size, the trailing partial
//!   frame is dropped (common in real files).
#![warn(missing_docs)]

mod fmt;
mod w64;
mod wav;

use fmt::Fmt;
use vynamp_decode_core::{
    CancelToken, CodecId, Demuxer, Error, Event, GaplessKind, Mode, Options, Packet, Result,
    SeekTo, Source, Track,
};

/// Maximum frames per packet.
pub const PACKET_FRAMES: u64 = 4096;

/// Result of walking the container up to the start of sample data.
pub(crate) struct Layout {
    pub fmt: Fmt,
    /// Absolute position of the first sample byte.
    pub data_start: u64,
    /// Declared length of the data chunk in bytes, `None` if unknown.
    pub data_len: Option<u64>,
    /// Absolute end of the container as declared by its header, if usable.
    pub container_end: Option<u64>,
}

/// Detects a WAV-family signature in `header` (the first bytes of the file).
///
/// Returns 100 for a complete RIFF/RF64/BW64 `WAVE` or Wave64 signature, else 0.
pub fn probe_riff(header: &[u8]) -> u8 {
    let riff = header.len() >= 12
        && matches!(&header[..4], b"RIFF" | b"RF64" | b"BW64")
        && &header[8..12] == b"WAVE";
    let w64 =
        header.len() >= 40 && header[..16] == w64::GUID_RIFF && header[24..40] == w64::GUID_WAVE;
    if riff || w64 { 100 } else { 0 }
}

/// Demuxer for WAV-family files.
pub struct RiffDemuxer {
    src: Box<dyn Source>,
    tracks: Vec<Track>,
    cancel: CancelToken,
    mode: Mode,
    /// Bytes per frame.
    block: u64,
    data_start: u64,
    /// Total frames (declared, or derived from the source length when undeclared).
    total: Option<u64>,
    /// Frames already returned (position of the next packet).
    next_frame: u64,
    events: Vec<Event>,
}

fn read_full(src: &mut dyn Source, buf: &mut [u8]) -> Result<usize> {
    let mut done = 0;
    while done < buf.len() {
        let n = src.read(&mut buf[done..])?;
        if n == 0 {
            break;
        }
        done += n;
    }
    Ok(done)
}

/// Opens a RIFF/WAVE, RF64, BW64 or Wave64 file.
pub fn open_riff(mut src: Box<dyn Source>, opts: &Options) -> Result<RiffDemuxer> {
    opts.cancel.check()?;
    if let Some(len) = src.len() {
        opts.limits.check_bytes(len)?;
    }
    let mut head = [0u8; 16];
    let got = read_full(src.as_mut(), &mut head)?;
    if got < 12 {
        return Err(Error::Invalid {
            what: "unexpected eof",
            offset: got as u64,
        });
    }
    src.seek(0)?;
    let magic = [head[0], head[1], head[2], head[3]];
    let layout = if matches!(&magic, b"RIFF" | b"RF64" | b"BW64") {
        wav::parse(src.as_mut(), magic, opts)?
    } else if head == w64::GUID_RIFF {
        w64::parse(src.as_mut(), opts)?
    } else {
        return Err(Error::Invalid {
            what: "riff signature",
            offset: 0,
        });
    };
    let Layout {
        fmt,
        data_start,
        data_len,
        container_end,
    } = layout;
    let block = u64::from(fmt.block_align);
    let declared = data_len.map(|d| d / block);
    if let Some(f) = declared {
        opts.limits.check_frames(f)?;
    }
    let mut events = Vec::new();
    let total = declared.or_else(|| {
        let end = match (src.len(), container_end) {
            (Some(l), Some(c)) => Some(l.min(c)),
            (l, c) => l.or(c),
        };
        end.map(|e| e.saturating_sub(data_start) / block)
    });
    if declared.is_none()
        && opts.mode == Mode::Resilient
        && let (Some(l), Some(c)) = (src.len(), container_end)
        && c < l
    {
        events.push(Event::Clamped {
            what: "riff data size",
            declared: l.saturating_sub(data_start),
            actual: c.saturating_sub(data_start),
        });
    }
    if let Some(f) = total {
        opts.limits.check_frames(f)?;
    }
    src.seek(data_start)?;
    let track = Track {
        id: 0,
        codec: CodecId::Pcm(fmt.spec),
        sample_rate: fmt.sample_rate,
        channels: fmt.channels,
        channel_mask: fmt.channel_mask,
        frames: declared,
        delay: 0,
        padding: 0,
        gapless: GaplessKind::Exact,
        codec_private: Vec::new(),
    };
    Ok(RiffDemuxer {
        src,
        tracks: vec![track],
        cancel: opts.cancel.clone(),
        mode: opts.mode,
        block,
        data_start,
        total,
        next_frame: 0,
        events,
    })
}

impl Demuxer for RiffDemuxer {
    fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    fn next_packet(&mut self, out: &mut Packet) -> Result<bool> {
        self.cancel.check()?;
        let remaining = self.total.map(|t| t.saturating_sub(self.next_frame));
        let n = remaining.map_or(PACKET_FRAMES, |r| r.min(PACKET_FRAMES));
        if n == 0 {
            return Ok(false);
        }
        let want = (n * self.block) as usize;
        out.data.clear();
        out.data.resize(want, 0);
        let got = read_full(self.src.as_mut(), &mut out.data)?;
        let whole = got as u64 / self.block;
        if got < want {
            // A source of unknown length legitimately ends here; otherwise the
            // declared data is missing.
            if self.mode == Mode::Strict && self.tracks[0].frames.is_some() {
                out.data.clear();
                return Err(Error::Invalid {
                    what: "unexpected eof",
                    offset: self.src.position(),
                });
            }
            if whole == 0 {
                out.data.clear();
                return Ok(false);
            }
            out.data.truncate((whole * self.block) as usize);
        }
        out.track = 0;
        out.pts = self.next_frame;
        out.offset = self.src.position().saturating_sub(got as u64);
        out.frames = whole as u32;
        self.next_frame += whole;
        Ok(true)
    }

    fn seek(&mut self, track: u32, frame: u64) -> Result<SeekTo> {
        if track != 0 {
            return Err(Error::Unsupported("track"));
        }
        let frame = self.total.map_or(frame, |t| frame.min(t));
        let pos = frame
            .checked_mul(self.block)
            .and_then(|b| b.checked_add(self.data_start))
            .ok_or(Error::Invalid {
                what: "seek overflow",
                offset: self.src.position(),
            })?;
        self.src.seek(pos)?;
        self.next_frame = frame;
        Ok(SeekTo { frame, preroll: 0 })
    }

    fn drain_events(&mut self, out: &mut Vec<Event>) {
        out.append(&mut self.events);
    }
}
