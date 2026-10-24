//! MPEG audio demuxer: sync, packetization and exact seeking.
//!
//! A stream is a run of frames. A position is accepted as a frame start when its header
//! parses, agrees with the stream (version, layer, sample rate, mono or not) and the header
//! at the end of the frame does too, or the data ends there (the false-sync guard). Free
//! format streams (bitrate index 0) have no length in the header: it is measured from the
//! distance between two headers with the same layer, version and rate, and then applies to
//! every frame, plus the padding slot.
//!
//! Strict mode turns a lost sync into `Invalid { what: "mpeg sync" }` and a truncated final
//! frame into `Invalid { what: "unexpected eof" }`. Resilient mode looks for the next
//! confirmed frame, reports the lost span as one `Event::CorruptFrame` (the number of lost
//! frames estimated from the span and the last frame length), delivers at most eight frames
//! of silence for it (packets with empty `data`) and moves the timeline over the rest.
//!
//! Seeking is exact. The index of a frame is only known by counting from a known point, so
//! `seek` counts frames from the nearest checkpoint (recorded while reading) and records
//! new ones on the way. Constant bitrate streams whose Info header agrees with the audio
//! size get a checkpoint from arithmetic instead, verified against the headers there.

use crate::header::{ChannelMode, FrameHeader, MpegVersion, parse_header};
use crate::vbr::{VbrInfo, parse_vbr};
use vynamp_decode_core::{
    CodecId, Demuxer, Error, Event, GaplessKind, Mode, Options, Packet, Result, SeekTo, Source,
    Track, find_trailers, skip_id3v2,
};

/// Read size while streaming frames.
const READ_CHUNK: usize = 64 * 1024;
/// Size of the window searched for sync words.
const SCAN_WINDOW: usize = 16 * 1024;
/// Queued events beyond this many are dropped.
const MAX_EVENTS: usize = 4096;
/// Silence per gap is capped at this many frames; beyond that the timeline jumps.
const GAP_FILL_FRAMES: u64 = 8;
/// Longest free-format frame searched for (bytes). A 640 kbit/s Layer II stream at 32 kHz has
/// frames of 2880 bytes; the search never looks further than this plus a header.
const MAX_FREE_FRAME: usize = 4096;
/// How far past the start of the audio the first frame is searched for.
const MAX_SYNC_SCAN: u64 = 1 << 20;
/// Layer III bit reservoir: a frame can need main data from up to this many frames earlier
/// (at most 511 bytes, which spans two frames at the lowest bitrates). Seeks start decoding
/// this many frames before the target.
const PREROLL_LAYER3: u64 = 2;
/// Layers I and II have no reservoir; one frame warms the synthesis filterbank up.
const PREROLL_OTHER: u64 = 1;
/// One seek checkpoint is recorded per this many frames.
const ANCHOR_STEP: u64 = 64;
/// Checkpoints kept at most.
const MAX_ANCHORS: usize = 1 << 16;
/// The MP3 decoder's own delay in samples (the polyphase filterbank plus the IMDCT overlap).
const MP3_DECODER_DELAY: u32 = 529;

/// A frame accepted at some position.
#[derive(Clone, Copy)]
struct Cand {
    h: FrameHeader,
    len: usize,
    /// Free-format length without padding, measured.
    free_base: Option<usize>,
}

/// Constant bitrate arithmetic: frame `i` starts about `i * bytes_per_frame_x` bytes after
/// the first one.
#[derive(Clone, Copy)]
struct Cbr {
    bitrate: u32,
    sample_rate: u32,
    /// 48, 144 or 72: bytes per frame = this * kbit/s * 1000 / sample rate.
    coefficient: u32,
    slot: usize,
}

impl Cbr {
    fn estimate(&self, idx: u64) -> u64 {
        let num = u128::from(idx) * u128::from(self.coefficient) * u128::from(self.bitrate) * 1000;
        (num / u128::from(self.sample_rate)) as u64
    }
}

enum Walk {
    /// Decoding starts at `pos`, which holds frame `idx`.
    Landed { pos: u64, idx: u64 },
    /// The data ends at `pos` after `total` frames.
    End { pos: u64, total: u64 },
}

/// Demuxer for MPEG audio elementary streams (`.mp1`, `.mp2`, `.mp3`).
///
/// `Packet::data` is a whole frame including its 4-byte header (and CRC); a decoder reads
/// the header from it. Layer III frames depend on the bit reservoir of earlier frames, so
/// packets must be delivered in order; after a seek the first packets are preroll (see
/// [`SeekTo::preroll`]).
pub struct MpaDemuxer {
    src: Box<dyn Source>,
    opts: Options,
    tracks: Vec<Track>,
    /// Header of the first audio frame: the stream parameters.
    ref_h: FrameHeader,
    /// Free-format frame length without padding.
    free_base: Option<usize>,
    vbr: Option<VbrInfo>,
    cbr: Option<Cbr>,
    first_frame: u64,
    /// Absolute position where the audio ends (before any trailer); `u64::MAX` if unknown.
    audio_end: u64,
    /// Bytes read from the source: `buf[0]` is at `buf_off`, the source position is `buf`'s end.
    buf: Vec<u8>,
    buf_off: u64,
    /// The source has no bytes after `buf`.
    eof: bool,
    /// Absolute position of the next frame to deliver.
    pos: u64,
    /// Index of that frame.
    idx: u64,
    /// Length of the last frame seen, for estimating lost frames.
    last_len: usize,
    done: bool,
    /// Seek checkpoints (frame index, position), sorted.
    anchors: Vec<(u64, u64)>,
    /// Silence packets still owed, the index of the next one, and where to resume.
    fill_left: u64,
    fill_idx: u64,
    fill_offset: u64,
    resume_pos: u64,
    resume_idx: u64,
    events: Vec<Event>,
    events_dropped: u64,
}

impl MpaDemuxer {
    /// The VBR header found in the first frame, if any.
    pub fn vbr_info(&self) -> Option<&VbrInfo> {
        self.vbr.as_ref()
    }

    /// The measured frame length (without padding) of a free-format stream.
    pub fn free_format_len(&self) -> Option<usize> {
        self.free_base
    }

    /// Absolute byte offset of the first audio frame (after any VBR header frame).
    pub fn first_frame_offset(&self) -> u64 {
        self.first_frame
    }

    /// Whether seeks use constant-bitrate arithmetic for their checkpoints (an Info header
    /// that agrees with the size of the audio) instead of counting from the start.
    pub fn uses_cbr_index(&self) -> bool {
        self.cbr.is_some()
    }

    fn strict(&self) -> bool {
        self.opts.mode == Mode::Strict
    }

    fn spf(&self) -> u64 {
        u64::from(self.ref_h.samples)
    }

    fn event(&mut self, e: Event) {
        if self.events.len() < MAX_EVENTS {
            self.events.push(e);
        } else {
            self.events_dropped += 1;
        }
    }

    /// The bytes at `pos..pos + n`, clamped to the end of the audio and of the source.
    /// Shorter than `n` only at the end.
    fn window(&mut self, pos: u64, n: usize) -> Result<&[u8]> {
        let want_end = pos.saturating_add(n as u64).min(self.audio_end);
        if pos >= want_end {
            return Ok(&[]);
        }
        let buf_end = self.buf_off + self.buf.len() as u64;
        if pos < self.buf_off || pos > buf_end {
            self.buf.clear();
            self.buf_off = pos;
            self.eof = false;
            self.src.seek(pos)?;
        } else if self.buf.len() > 4 * READ_CHUNK && pos > self.buf_off {
            // Forget what lies behind the reader.
            self.buf.drain(..(pos - self.buf_off) as usize);
            self.buf_off = pos;
        }
        while self.buf_off + (self.buf.len() as u64) < want_end && !self.eof {
            self.opts.cancel.check()?;
            let at = self.buf.len();
            self.buf.resize(at + READ_CHUNK, 0);
            let got = self.src.read(&mut self.buf[at..]);
            let got = match got {
                Ok(n) => n,
                Err(e) => {
                    self.buf.truncate(at);
                    return Err(e);
                }
            };
            self.buf.truncate(at + got);
            if got == 0 {
                self.eof = true;
            }
        }
        let start = (pos - self.buf_off) as usize;
        let end = ((want_end - self.buf_off) as usize).min(self.buf.len());
        Ok(&self.buf[start.min(end)..end])
    }

    fn header_at(&mut self, pos: u64) -> Result<Option<FrameHeader>> {
        let w = self.window(pos, 4)?;
        Ok(w.try_into().ok().and_then(parse_header))
    }

    /// Whether `h` belongs to the stream described by `r`.
    fn consistent(r: &FrameHeader, h: &FrameHeader) -> bool {
        r.version == h.version
            && r.layer == h.layer
            && r.sample_rate == h.sample_rate
            && (r.channel_mode == ChannelMode::Mono) == (h.channel_mode == ChannelMode::Mono)
    }

    /// Checks that a whole frame of `len` bytes starts at `pos` and that the next header
    /// agrees with `h` (or the data ends exactly there).
    fn chain_ok(&mut self, pos: u64, len: usize, h: &FrameHeader) -> Result<bool> {
        if self.window(pos, len)?.len() != len {
            return Ok(false);
        }
        let next = pos + len as u64;
        let w = self.window(next, 4)?;
        match w.len() {
            0 => Ok(true),
            4 => Ok(parse_header([w[0], w[1], w[2], w[3]]).is_some_and(|n| {
                Self::consistent(h, &n) && (n.bitrate_kbps == 0) == (h.bitrate_kbps == 0)
            })),
            _ => Ok(false),
        }
    }

    /// Measures the length of a free-format frame (without padding) from the distance to the
    /// next header with the same parameters, which must itself be followed by another.
    fn measure_free(&mut self, pos: u64, h: &FrameHeader) -> Result<Option<usize>> {
        let slot = h.slot_size();
        let pad = usize::from(h.padding) * slot;
        let window = self.window(pos, MAX_FREE_FRAME + 4 + pad)?.to_vec();
        let min = h.min_frame_len();
        for d in (min + pad).max(slot)..window.len().saturating_sub(3) {
            if window[d] != 0xFF || window[d + 1] & 0xE0 != 0xE0 {
                continue;
            }
            let Some(n) = parse_header([window[d], window[d + 1], window[d + 2], window[d + 3]])
            else {
                continue;
            };
            let base = d - pad;
            if n.bitrate_kbps != 0
                || !Self::consistent(h, &n)
                || !base.is_multiple_of(slot)
                || base > MAX_FREE_FRAME
            {
                continue;
            }
            // The header after it must be where the measured length puts it.
            let after = pos + d as u64 + (base + usize::from(n.padding) * slot) as u64;
            let w = self.window(after, 4)?;
            let ok = match w.len() {
                0 => true,
                4 => parse_header([w[0], w[1], w[2], w[3]])
                    .is_some_and(|a| a.bitrate_kbps == 0 && Self::consistent(h, &a)),
                _ => false,
            };
            if ok {
                return Ok(Some(base));
            }
        }
        Ok(None)
    }

    /// Whether a confirmed frame starts at `pos`. `reference` is the stream's header and
    /// free-format length once they are known; without it the frame defines the stream.
    fn candidate(
        &mut self,
        pos: u64,
        reference: Option<(FrameHeader, Option<usize>)>,
        verify: bool,
    ) -> Result<Option<Cand>> {
        let Some(h) = self.header_at(pos)? else {
            return Ok(None);
        };
        let mut free_base = None;
        if let Some((r, free)) = reference {
            if !Self::consistent(&r, &h) || (h.bitrate_kbps == 0) != free.is_some() {
                return Ok(None);
            }
            free_base = free;
        } else if h.bitrate_kbps == 0 {
            free_base = self.measure_free(pos, &h)?;
            if free_base.is_none() {
                return Ok(None);
            }
        }
        let len = match free_base {
            Some(base) => base + usize::from(h.padding) * h.slot_size(),
            None => h.frame_len,
        };
        if len < h.min_frame_len() || (verify && !self.chain_ok(pos, len, &h)?) {
            return Ok(None);
        }
        Ok(Some(Cand { h, len, free_base }))
    }

    /// A confirmed frame of the stream at `pos`.
    fn frame_at(&mut self, pos: u64) -> Result<Option<(FrameHeader, usize)>> {
        let reference = Some((self.ref_h, self.free_base));
        Ok(self.candidate(pos, reference, false)?.map(|c| (c.h, c.len)))
    }

    /// The first confirmed frame at or after `from` and before `until`.
    fn scan(
        &mut self,
        from: u64,
        until: u64,
        reference: Option<(FrameHeader, Option<usize>)>,
    ) -> Result<Option<(u64, Cand)>> {
        let mut q = from;
        while q < until {
            self.opts.cancel.check()?;
            let w = self.window(q, SCAN_WINDOW)?;
            if w.len() < 4 {
                return Ok(None);
            }
            let found = w[..w.len() - 1]
                .windows(2)
                .position(|p| p[0] == 0xFF && p[1] & 0xE0 == 0xE0);
            let Some(i) = found else {
                // Keep the last byte: it may start a sync word.
                q += w.len() as u64 - 1;
                continue;
            };
            let at = q + i as u64;
            if at >= until {
                return Ok(None);
            }
            if let Some(c) = self.candidate(at, reference, true)? {
                return Ok(Some((at, c)));
            }
            q = at + 1;
        }
        Ok(None)
    }

    fn resync(&mut self, from: u64) -> Result<Option<u64>> {
        let reference = Some((self.ref_h, self.free_base));
        Ok(self.scan(from, u64::MAX, reference)?.map(|(at, _)| at))
    }

    /// Number of frames a span of `bytes` stands for, at least one.
    fn lost_frames(&self, bytes: u64) -> u64 {
        let len = self.last_len.max(1) as u64;
        ((bytes + len / 2) / len).max(1)
    }

    fn anchor(&mut self, idx: u64, pos: u64) {
        if idx.is_multiple_of(ANCHOR_STEP)
            && self.anchors.len() < MAX_ANCHORS
            && self.anchors.last().is_none_or(|&(i, _)| i < idx)
        {
            self.anchors.push((idx, pos));
        }
    }

    fn start_silence(&mut self, count: u64, offset: u64, resume_pos: u64, resume_idx: u64) {
        self.fill_left = count;
        self.fill_idx = self.idx;
        self.fill_offset = offset;
        self.resume_pos = resume_pos;
        self.resume_idx = resume_idx;
    }

    fn emit_silence(&mut self, out: &mut Packet) {
        let spf = self.spf();
        out.track = 0;
        out.pts = self.fill_idx * spf;
        out.frames = self.ref_h.samples;
        out.offset = self.fill_offset;
        out.data.clear();
        self.fill_idx += 1;
        self.fill_left -= 1;
        if self.fill_left == 0 {
            self.pos = self.resume_pos;
            self.idx = self.resume_idx;
        }
    }

    /// Counts frames from the nearest checkpoint (or `from`) up to the frame that a decoder
    /// has to start with, `start`, verifying that frame `goal >= start` exists.
    fn walk(&mut self, from: Option<(u64, u64)>, start: u64, goal: u64) -> Result<Walk> {
        let (mut idx, mut pos) = match from {
            Some((i, p)) if i <= start => (i, p),
            _ => {
                let at = self.anchors.partition_point(|&(i, _)| i <= start);
                self.anchors[at - 1]
            }
        };
        let mut land: Option<(u64, u64)> = None;
        let mut counted = 0u32;
        loop {
            if idx >= start && land.is_none() {
                land = Some((pos, idx));
            }
            if idx > goal {
                let (pos, idx) = land.unwrap_or((pos, idx));
                return Ok(Walk::Landed { pos, idx });
            }
            counted += 1;
            if counted.is_multiple_of(1024) {
                self.opts.cancel.check()?;
            }
            if let Some((_, len)) = self.frame_at(pos)? {
                if idx == goal {
                    let (pos, idx) = land.unwrap_or((pos, idx));
                    return Ok(Walk::Landed { pos, idx });
                }
                self.last_len = len;
                pos += len as u64;
                idx += 1;
                self.anchor(idx, pos);
                continue;
            }
            // No frame here: the end of the audio, or lost sync.
            let rest = self.window(pos, 4)?.len();
            if rest == 0 {
                return Ok(Walk::End { pos, total: idx });
            }
            if self.strict() {
                return Err(Error::Invalid {
                    what: if rest < 4 {
                        "unexpected eof"
                    } else {
                        "mpeg sync"
                    },
                    offset: pos,
                });
            }
            match self.resync(pos + 1)? {
                Some(q) => {
                    idx += self.lost_frames(q - pos);
                    pos = q;
                }
                None => {
                    let end = self.data_end(pos)?;
                    return Ok(Walk::End {
                        pos: end,
                        total: idx,
                    });
                }
            }
        }
    }

    /// The position where the data ends, searching forward from `pos`.
    fn data_end(&mut self, mut pos: u64) -> Result<u64> {
        loop {
            self.opts.cancel.check()?;
            let n = self.window(pos, SCAN_WINDOW)?.len();
            if n == 0 {
                return Ok(pos);
            }
            pos += n as u64;
        }
    }

    /// A verified checkpoint for frame `idx` from constant-bitrate arithmetic.
    fn cbr_anchor(&mut self, idx: u64) -> Result<Option<(u64, u64)>> {
        let Some(cbr) = self.cbr else {
            return Ok(None);
        };
        let est = self.first_frame.saturating_add(cbr.estimate(idx));
        let reach = 2 * cbr.slot as u64;
        for delta in 0..=reach {
            for pos in [est.checked_add(delta), est.checked_sub(delta)]
                .into_iter()
                .flatten()
            {
                let reference = Some((self.ref_h, self.free_base));
                let Some(c) = self.candidate(pos, reference, true)? else {
                    continue;
                };
                if c.h.bitrate_kbps != cbr.bitrate {
                    continue;
                }
                // Two frames in step make a look-alike unlikely.
                let next = pos + c.len as u64;
                if self.window(next, 4)?.len() == 4
                    && self.candidate(next, reference, true)?.is_none()
                {
                    continue;
                }
                return Ok(Some((idx, pos)));
            }
        }
        Ok(None)
    }
}

/// Opens an MPEG audio elementary stream.
///
/// Skips ID3v2 tags and ignores trailing tags, finds the first confirmed frame (searching up
/// to 1 MiB for it), reads a Xing/Info/VBRI header from it if there is one (that frame is then
/// not audio and is not delivered) and derives the track.
///
/// Gapless numbers: with a LAME tag whose CRC is valid, `delay = encoder_delay + 529` and
/// `padding = lame padding - 529` (the decoder delay of 529 samples lies at the start of the
/// output, so it counts as delay and comes off the padding), `frames` is the Xing frame count
/// times the samples per frame, and `gapless` is `FromMetadata`: the trimmed length
/// `frames - delay - padding` is the length of the encoded PCM. Without a LAME tag a Layer
/// III track has `delay = 529` and `gapless = Unknown`; Layer I and II tracks have `delay = 0`
/// (their decoder delay is set where the decoder is) and `Unknown`.
///
/// # Errors
/// `Invalid { what: "mpeg sync" }` if no frame is found, limit errors, source errors.
pub fn open_mpa(mut src: Box<dyn Source>, opts: &Options) -> Result<MpaDemuxer> {
    opts.cancel.check()?;
    if let Some(len) = src.len() {
        opts.limits.check_bytes(len)?;
    }
    let start = skip_id3v2(src.as_mut(), &opts.limits)?;
    let (audio_end, _trailers) = find_trailers(src.as_mut(), start)?;
    src.seek(start)?;
    let placeholder = parse_header([0xFF, 0xFB, 0x90, 0x00]).ok_or(Error::Unsupported("header"))?;
    let mut d = MpaDemuxer {
        src,
        opts: opts.clone(),
        tracks: Vec::new(),
        ref_h: placeholder,
        free_base: None,
        vbr: None,
        cbr: None,
        first_frame: start,
        audio_end,
        buf: Vec::new(),
        buf_off: start,
        eof: false,
        pos: start,
        idx: 0,
        last_len: 0,
        done: false,
        anchors: Vec::new(),
        fill_left: 0,
        fill_idx: 0,
        fill_offset: 0,
        resume_pos: 0,
        resume_idx: 0,
        events: Vec::new(),
        events_dropped: 0,
    };
    let until = start.saturating_add(MAX_SYNC_SCAN);
    let Some((first, cand)) = d.scan(start, until, None)? else {
        return Err(Error::Invalid {
            what: "mpeg sync",
            offset: start,
        });
    };
    d.ref_h = cand.h;
    d.free_base = cand.free_base;
    let frame = d.window(first, cand.len)?.to_vec();
    let mut first_frame = first;
    if let Some(info) = parse_vbr(&frame) {
        first_frame = first + cand.len as u64;
        d.vbr = Some(info);
        // The stream parameters come from the audio, not from the header frame.
        let reference = Some((cand.h, cand.free_base));
        if let Some((_, next)) = d.scan(first_frame, first_frame + 1, reference)? {
            d.ref_h = next.h;
        }
    }
    d.first_frame = first_frame;
    d.pos = first_frame;
    d.last_len = cand.len;
    d.anchors.push((0, first_frame));
    let h = d.ref_h;
    opts.limits.check_sample_rate(h.sample_rate)?;
    opts.limits.check_channels(h.channels())?;
    let spf = u64::from(h.samples);
    let mut frames = None;
    if let Some(n) = d.vbr.as_ref().and_then(|v| v.frames) {
        let total = u64::from(n).saturating_mul(spf);
        opts.limits.check_frames(total)?;
        frames = Some(total);
    }
    let mut delay = if h.layer == 3 { MP3_DECODER_DELAY } else { 0 };
    let mut padding = 0;
    let mut gapless = GaplessKind::Unknown;
    if h.layer == 3
        && let Some(total) = frames
        && let Some(lame) = d.vbr.as_ref().and_then(|v| v.lame.as_ref())
        && lame.crc_ok
    {
        let dl = u32::from(lame.encoder_delay) + MP3_DECODER_DELAY;
        let pd = u32::from(lame.padding).saturating_sub(MP3_DECODER_DELAY);
        if u64::from(dl) + u64::from(pd) <= total {
            delay = dl;
            padding = pd;
            gapless = GaplessKind::FromMetadata;
        }
    }
    d.tracks.push(Track {
        id: 0,
        codec: match h.layer {
            1 => CodecId::Mp1,
            2 => CodecId::Mp2,
            _ => CodecId::Mp3,
        },
        sample_rate: h.sample_rate,
        channels: h.channels(),
        channel_mask: 0,
        frames,
        delay,
        padding,
        gapless,
        codec_private: Vec::new(),
    });
    d.cbr = d.detect_cbr()?;
    Ok(d)
}

impl MpaDemuxer {
    /// Constant bitrate arithmetic is used only when an Info header (CBR writers' tag) gives a
    /// frame count, the stream is not free format, and the audio size agrees with that count
    /// at the bitrate of the first audio frame to within a few slots.
    fn detect_cbr(&mut self) -> Result<Option<Cbr>> {
        use crate::vbr::VbrKind;
        let h = self.ref_h;
        let end = self.audio_end;
        let Some(v) = self.vbr.as_ref() else {
            return Ok(None);
        };
        let Some(frames) = v.frames else {
            return Ok(None);
        };
        if v.kind != VbrKind::Info || h.bitrate_kbps == 0 || end == u64::MAX || h.layer != 3 {
            return Ok(None);
        }
        let coefficient = match h.version {
            MpegVersion::V1 => 144,
            _ => 72,
        };
        let cbr = Cbr {
            bitrate: h.bitrate_kbps,
            sample_rate: h.sample_rate,
            coefficient,
            slot: h.slot_size(),
        };
        let expected = cbr.estimate(u64::from(frames));
        let actual = end.saturating_sub(self.first_frame);
        let slack = 4 * h.slot_size() as u64;
        Ok((expected.abs_diff(actual) <= slack).then_some(cbr))
    }
}

impl Demuxer for MpaDemuxer {
    fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    fn next_packet(&mut self, out: &mut Packet) -> Result<bool> {
        self.opts.cancel.check()?;
        if self.fill_left > 0 {
            self.emit_silence(out);
            return Ok(true);
        }
        if self.done {
            return Ok(false);
        }
        let spf = self.spf();
        let pos = self.pos;
        let rest = self.window(pos, 4)?.len();
        if rest == 0 {
            self.done = true;
            return Ok(false);
        }
        let Some((_, len)) = self.frame_at(pos)? else {
            if self.strict() {
                return Err(Error::Invalid {
                    what: if rest < 4 {
                        "unexpected eof"
                    } else {
                        "mpeg sync"
                    },
                    offset: pos,
                });
            }
            let lost_pts = self.idx * spf;
            let found = if rest < 4 {
                None
            } else {
                self.resync(pos + 1)?
            };
            let Some(q) = found else {
                // Nothing usable follows: report what is left once and stop.
                self.event(Event::CorruptFrame {
                    pts: lost_pts,
                    offset: pos,
                    frames: 0,
                });
                self.done = true;
                return Ok(false);
            };
            let lost = self.lost_frames(q - pos);
            let resume = self.idx + lost;
            self.event(Event::CorruptFrame {
                pts: lost_pts,
                offset: pos,
                frames: u32::try_from(lost.saturating_mul(spf)).unwrap_or(u32::MAX),
            });
            if self
                .opts
                .limits
                .check_frames(resume.saturating_mul(spf))
                .is_err()
            {
                self.done = true;
                return Ok(false);
            }
            self.start_silence(lost.min(GAP_FILL_FRAMES), pos, q, resume);
            self.emit_silence(out);
            return Ok(true);
        };
        let end_pts = (self.idx + 1).saturating_mul(spf);
        if let Err(e) = self.opts.limits.check_frames(end_pts) {
            if self.strict() {
                return Err(e);
            }
            self.event(Event::CorruptFrame {
                pts: self.idx * spf,
                offset: pos,
                frames: self.ref_h.samples,
            });
            self.done = true;
            return Ok(false);
        }
        let (pts, samples) = (self.idx * spf, self.ref_h.samples);
        let frame = self.window(pos, len)?;
        if frame.len() < len {
            // The last frame is cut short.
            if self.strict() {
                return Err(Error::Invalid {
                    what: "unexpected eof",
                    offset: pos,
                });
            }
            self.event(Event::CorruptFrame {
                pts: self.idx * spf,
                offset: pos,
                frames: self.ref_h.samples,
            });
            self.done = true;
            self.start_silence(1, pos, pos, self.idx + 1);
            self.emit_silence(out);
            return Ok(true);
        }
        out.track = 0;
        out.pts = pts;
        out.frames = samples;
        out.offset = pos;
        out.data.clear();
        out.data.extend_from_slice(frame);
        self.pos += len as u64;
        self.idx += 1;
        self.last_len = len;
        let (idx, pos) = (self.idx, self.pos);
        self.anchor(idx, pos);
        Ok(true)
    }

    fn seek(&mut self, track: u32, frame: u64) -> Result<SeekTo> {
        self.opts.cancel.check()?;
        if track != 0 {
            return Err(Error::Unsupported("track"));
        }
        let spf = self.spf();
        let goal = frame / spf;
        let preroll = if self.ref_h.layer == 3 {
            PREROLL_LAYER3
        } else {
            PREROLL_OTHER
        };
        let start = goal.saturating_sub(preroll);
        let from = self.cbr_anchor(start)?;
        let walked = self.walk(from, start, goal)?;
        self.fill_left = 0;
        match walked {
            Walk::Landed { pos, idx } => {
                self.pos = pos;
                self.idx = idx;
                self.done = false;
                let first = idx * spf;
                Ok(SeekTo {
                    frame: first,
                    preroll: frame.saturating_sub(first),
                })
            }
            Walk::End { pos, total } => {
                self.pos = pos;
                self.idx = total;
                self.done = true;
                Ok(SeekTo {
                    frame: total * spf,
                    preroll: 0,
                })
            }
        }
    }

    fn drain_events(&mut self, out: &mut Vec<Event>) {
        out.append(&mut self.events);
    }

    fn take_events_dropped(&mut self) -> u64 {
        std::mem::take(&mut self.events_dropped)
    }
}
