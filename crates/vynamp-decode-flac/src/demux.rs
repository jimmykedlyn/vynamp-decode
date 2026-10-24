//! FLAC demuxer: opening, frame sync, packetization and seeking.
//!
//! A frame starts at a sync code whose header passes the CRC-8 and agrees with
//! STREAMINFO. It ends where the next such header begins, provided the bytes
//! in between pass the frame's CRC-16; a candidate boundary that fails the
//! CRC-16 (a header-like byte pattern inside the frame body) is skipped. A
//! frame whose CRC-16 never holds is corrupt.
//!
//! Strict mode turns corruption into errors. Resilient mode replaces a corrupt
//! frame by a silence packet (empty `data`, `frames` from the header), fills
//! missing samples between valid headers the same way, and reports each case
//! with `Event::CorruptFrame`.

use crate::frame::{FrameHeader, MAX_HEADER, parse_frame_header_with};
use crate::metadata::read_metadata;
use crate::{RawBlock, SeekPoint, StreamInfo};
use vynamp_decode_core::{
    ByteReader, CodecId, Demuxer, Error, Event, GaplessKind, LimitKind, Mode, Options, Packet,
    Result, SeekTo, Source, Track, crc16, skip_id3v2,
};

/// Read size while streaming frames.
const READ_CHUNK: usize = 64 * 1024;
/// Read size while probing for sync during a seek.
const PROBE_CHUNK: usize = 4 * 1024;
/// Largest frame accepted when STREAMINFO does not give a maximum frame size.
const HARD_CAP: usize = 16 << 20;
/// Header-like boundaries examined per frame before it is declared corrupt.
const MAX_CANDIDATES: usize = 4;
/// Bisection stops when the byte range is this small; a frame scan finishes the job.
const BISECT_MIN: u64 = 4096;
/// Queued events beyond this many are dropped.
const MAX_EVENTS: usize = 4096;
/// Silence per gap is capped at this many maximum-size blocks; beyond that the
/// timeline jumps.
const GAP_FILL_BLOCKS: u64 = 8;
/// Length of an ID3v1 tag at the end of a file.
const ID3V1_LEN: usize = 128;

/// Demuxer for native FLAC streams.
///
/// `Packet::data` holds one frame from its sync code through its CRC-16
/// footer. It may carry trailing bytes after the footer (for example a zero
/// fill that the CRC-16 cannot tell apart from the frame), so a decoder must
/// read the footer at the position it parsed and ignore anything after it.
pub struct FlacDemuxer {
    src: Box<dyn Source>,
    opts: Options,
    tracks: Vec<Track>,
    info: StreamInfo,
    seek_points: Vec<SeekPoint>,
    blocks: Vec<RawBlock>,
    first_frame: u64,
    /// Bytes read from the source; `buf[head..]` is unconsumed.
    buf: Vec<u8>,
    head: usize,
    /// Absolute source position of `buf[0]`.
    buf_off: u64,
    /// The source has no more bytes after `buf`.
    eof: bool,
    /// Size of one source read.
    chunk: usize,
    /// Sample number expected at the next frame.
    next_pts: u64,
    /// Absolute position where the previous frame ended.
    last_end: u64,
    /// No more packets (end of data or trailer reached).
    done: bool,
    /// Absolute position where an ID3v1/APEv2 trailer begins (`u64::MAX` = none).
    audio_end: u64,
    /// Blocking strategy of the stream, learned from the first confirmed frame.
    blocking: Option<bool>,
    /// Pre-2007 variable block size layout: "fixed" headers carry sample numbers.
    legacy_vbr: bool,
    /// Sample number of the stream's first frame in the original numbering; every
    /// timeline position the demuxer reports is relative to it.
    origin: u64,
    /// The `Clamped` event for samples beyond STREAMINFO's total was already queued.
    clamped: bool,
    /// Where Resilient mode started skipping unparseable bytes.
    skipped: Option<u64>,
    /// Silence still owed for the gap in front of the frame at `head`.
    fill_left: u64,
    /// Sample number of that frame: the timeline continues there once the fill is done.
    fill_resume: u64,
    events: Vec<Event>,
}

impl FlacDemuxer {
    /// The parsed STREAMINFO block.
    pub fn stream_info(&self) -> &StreamInfo {
        &self.info
    }

    /// Seek table entries without placeholders (offsets are from the first frame).
    pub fn seek_points(&self) -> &[SeekPoint] {
        &self.seek_points
    }

    /// Non-padding metadata blocks in file order, STREAMINFO first.
    pub fn metadata_blocks(&self) -> &[RawBlock] {
        &self.blocks
    }

    /// Absolute byte offset of the first audio frame.
    pub fn first_frame_offset(&self) -> u64 {
        self.first_frame
    }

    fn strict(&self) -> bool {
        self.opts.mode == Mode::Strict
    }

    fn abs(&self, idx: usize) -> u64 {
        self.buf_off + idx as u64
    }

    fn event(&mut self, e: Event) {
        if self.events.len() < MAX_EVENTS {
            self.events.push(e);
        }
    }

    /// Discards the buffer and continues reading at absolute position `off`.
    fn reposition(&mut self, off: u64) -> Result<()> {
        self.src.seek(off)?;
        self.buf.clear();
        self.head = 0;
        self.buf_off = off;
        self.eof = false;
        self.last_end = off;
        self.fill_left = 0;
        Ok(())
    }

    /// Drops the consumed prefix of the buffer. Invalidates buffer indices.
    fn compact(&mut self) {
        if self.head > 0 {
            self.buf.drain(..self.head);
            self.buf_off += self.head as u64;
            self.head = 0;
        }
    }

    /// Reads until `buf` holds at least `upto` bytes or the source ends.
    fn ensure(&mut self, upto: usize) -> Result<()> {
        while self.buf.len() < upto && !self.eof {
            let room = self
                .audio_end
                .saturating_sub(self.buf_off + self.buf.len() as u64);
            if room == 0 {
                self.eof = true;
                break;
            }
            let old = self.buf.len();
            let want = self.chunk.min(usize::try_from(room).unwrap_or(usize::MAX));
            self.buf.resize(old + want, 0);
            match self.src.read(&mut self.buf[old..]) {
                Ok(0) => {
                    self.buf.truncate(old);
                    self.eof = true;
                }
                Ok(n) => self.buf.truncate(old + n),
                Err(e) => {
                    self.buf.truncate(old);
                    return Err(e);
                }
            }
        }
        Ok(())
    }

    /// The valid frame header at buffer index `idx`, if there is one.
    fn header_at(&mut self, idx: usize) -> Result<Option<FrameHeader>> {
        self.ensure(idx + MAX_HEADER)?;
        let Some(rest) = self.buf.get(idx..) else {
            return Ok(None);
        };
        let rest = &rest[..rest.len().min(MAX_HEADER)];
        let blocking = self.blocking;
        let origin = self.origin;
        Ok(parse_frame_header_with(rest, &self.info, self.legacy_vbr)
            .ok()
            .filter(|h| blocking.is_none_or(|b| b == h.blocking_variable))
            .and_then(|mut h| {
                h.first_sample = h.first_sample.checked_sub(origin)?;
                Some(h)
            }))
    }

    /// Moves `head` to the next 0xFF byte at or after it; `false` at the end of data.
    fn advance_to_ff(&mut self) -> Result<bool> {
        loop {
            self.opts.cancel.check()?;
            self.ensure(self.head + 1)?;
            if self.head >= self.buf.len() {
                return Ok(false);
            }
            if let Some(i) = self.buf[self.head..].iter().position(|&b| b == 0xFF) {
                self.head += i;
                return Ok(true);
            }
            self.head = self.buf.len();
            if self.head >= READ_CHUNK {
                self.compact();
            }
        }
    }

    /// Records that bytes are being skipped and steps past the one at `head`.
    fn skip_one(&mut self) -> Result<bool> {
        if self.skipped.is_none() {
            self.skipped = Some(self.abs(self.head));
        }
        self.head += 1;
        self.advance_to_ff()
    }

    /// A header with a block size below the STREAMINFO minimum is only valid in
    /// the last frame of the stream.
    fn plausible(&mut self, h: &FrameHeader) -> Result<bool> {
        if h.block_size >= u32::from(self.info.min_block) {
            return Ok(true);
        }
        let loc = self.locate(h)?;
        Ok(loc.crc_ok && self.eof && loc.end == self.buf.len())
    }

    /// Finds the frame header at `head`. Garbage is an error in Strict mode and
    /// skipped in Resilient mode. `None` means the audio data has ended.
    fn sync_head(&mut self) -> Result<Option<FrameHeader>> {
        loop {
            self.opts.cancel.check()?;
            if self.head >= READ_CHUNK {
                self.compact();
            }
            self.ensure(self.head + MAX_HEADER)?;
            if self.head >= self.buf.len() {
                return Ok(None);
            }
            if let Some(h) = self.header_at(self.head)?
                && self.plausible(&h)?
            {
                return Ok(Some(h));
            }
            let total = self.info.total_samples;
            if total != 0 && self.next_pts >= total {
                // Everything declared has been delivered: what follows is a trailer.
                self.done = true;
                return Ok(None);
            }
            if self.strict() {
                return Err(Error::Invalid {
                    what: "flac sync",
                    offset: self.abs(self.head),
                });
            }
            if !self.skip_one()? {
                return Ok(None);
            }
        }
    }

    /// Whether `buf[start..end]` is a frame whose last two bytes are its CRC-16.
    fn crc_ok(&self, start: usize, end: usize) -> bool {
        if end < start + 3 || end > self.buf.len() {
            return false;
        }
        let frame = &self.buf[start..end];
        let n = frame.len();
        crc16(&frame[..n - 2]).to_be_bytes() == frame[n - 2..]
    }

    /// Finds the end of the frame with header `h` at `head`.
    fn locate(&mut self, h: &FrameHeader) -> Result<Located> {
        let start = self.head;
        let hard = self.info.max_frame == 0;
        let cap = if hard {
            HARD_CAP
        } else {
            self.info.max_frame as usize
        };
        let too_long = |first: Option<usize>| match first {
            Some(end) => Ok(Located { end, crc_ok: false }),
            None if hard => Err(Error::Limit(LimitKind::Bytes)),
            None => Ok(Located {
                end: start + h.header_len,
                crc_ok: false,
            }),
        };
        let mut first: Option<usize> = None;
        let mut tried = 0;
        let mut j = start + h.header_len + 2;
        loop {
            self.opts.cancel.check()?;
            if j - start > cap {
                return too_long(first);
            }
            self.ensure(j + 2)?;
            let found = self
                .buf
                .get(j..)
                .and_then(|s| s.iter().position(|&b| b == 0xFF));
            let Some(i) = found else {
                if self.eof {
                    break;
                }
                j = self.buf.len().max(j);
                continue;
            };
            j += i;
            if j - start > cap {
                return too_long(first);
            }
            if self.header_at(j)?.is_some() {
                if self.crc_ok(start, j) {
                    return Ok(Located {
                        end: j,
                        crc_ok: true,
                    });
                }
                first.get_or_insert(j);
                tried += 1;
                if tried >= MAX_CANDIDATES {
                    return too_long(first);
                }
            }
            j += 1;
        }
        // No further header: the frame runs to the end of the data (before any trailer).
        let len = self.buf.len();
        if self.crc_ok(start, len) {
            return Ok(Located {
                end: len,
                crc_ok: true,
            });
        }
        Ok(Located {
            end: first.unwrap_or(len),
            crc_ok: false,
        })
    }

    fn consume_to(&mut self, end: usize) {
        self.head = end;
        self.last_end = self.abs(end);
    }

    /// End of data: a stream that stops before its declared length is an error in Strict mode.
    fn finish(&mut self) -> Result<bool> {
        let total = self.info.total_samples;
        let skipped = self.skipped.take();
        if total == 0 || self.next_pts >= total {
            if let Some(offset) = skipped
                && !self.strict()
            {
                let pts = self.next_pts;
                self.event(Event::CorruptFrame {
                    pts,
                    offset,
                    frames: 0,
                });
            }
        } else {
            let offset = self.abs(self.head);
            if self.strict() {
                return Err(Error::Invalid {
                    what: "unexpected eof",
                    offset,
                });
            }
            let frames = u32::try_from(total - self.next_pts).unwrap_or(u32::MAX);
            let pts = self.next_pts;
            self.event(Event::CorruptFrame {
                pts,
                offset,
                frames,
            });
            self.next_pts = total;
        }
        self.done = true;
        Ok(false)
    }

    /// Resilient: a silence packet, reported with a `CorruptFrame` event.
    fn silence(&mut self, out: &mut Packet, pts: u64, frames: u32, offset: u64) {
        self.silence_packet(out, pts, frames, offset);
        self.event(Event::CorruptFrame {
            pts,
            offset,
            frames,
        });
    }

    fn silence_packet(&self, out: &mut Packet, pts: u64, frames: u32, offset: u64) {
        out.track = 0;
        out.pts = pts;
        out.frames = frames;
        out.offset = offset;
        out.data.clear();
    }

    /// The next chunk of the silence owed for a gap (the gap was reported when it was found).
    fn fill_chunk(&mut self, out: &mut Packet) {
        let n = self.fill_left.min(u64::from(self.info.max_block)) as u32;
        let (pts, at) = (self.next_pts, self.last_end);
        self.silence_packet(out, pts, n, at);
        self.fill_left -= u64::from(n);
        self.next_pts += u64::from(n);
        if self.fill_left == 0 {
            self.next_pts = self.fill_resume;
        }
    }

    /// First frame header at or after `from` and before `until`, found by sync search.
    fn probe(&mut self, from: u64, until: u64) -> Result<Option<(u64, FrameHeader)>> {
        self.reposition(from)?;
        loop {
            self.opts.cancel.check()?;
            if self.head >= READ_CHUNK {
                self.compact();
            }
            if self.abs(self.head) >= until {
                return Ok(None);
            }
            self.ensure(self.head + MAX_HEADER)?;
            if self.head >= self.buf.len() {
                return Ok(None);
            }
            if let Some(h) = self.header_at(self.head)? {
                // Only a header that starts a CRC-16 valid frame counts.
                let loc = self.locate(&h)?;
                if loc.crc_ok && self.plausible(&h)? {
                    self.blocking.get_or_insert(h.blocking_variable);
                    return Ok(Some((self.abs(self.head), h)));
                }
            }
            self.head += 1;
            if !self.advance_to_ff()? {
                return Ok(None);
            }
        }
    }

    /// Byte offset and first sample of a frame at or before `target`, found by bisection.
    fn bisect(&mut self, target: u64) -> Result<(u64, u64)> {
        let Some(len) = self.src.len() else {
            return Ok((self.first_frame, 0));
        };
        let (mut lo, mut lo_pts, mut hi) = (self.first_frame, 0u64, len);
        for _ in 0..64 {
            self.opts.cancel.check()?;
            if hi.saturating_sub(lo) <= BISECT_MIN {
                break;
            }
            let mid = lo + (hi - lo) / 2;
            match self.probe(mid, hi)? {
                Some((at, h)) if h.first_sample <= target => {
                    lo = at;
                    lo_pts = h.first_sample;
                }
                _ => hi = mid,
            }
        }
        Ok((lo, lo_pts))
    }

    /// Where to start scanning for `target`: a verified seek point, else bisection.
    fn seek_start(&mut self, target: u64) -> Result<(u64, u64)> {
        if target == 0 {
            return Ok((self.first_frame, 0));
        }
        let point = self
            .seek_points
            .iter()
            .filter(|p| p.sample <= target)
            .max_by_key(|p| p.sample)
            .copied();
        self.chunk = PROBE_CHUNK;
        let result = self.seek_start_probing(target, point);
        self.chunk = READ_CHUNK;
        result
    }

    fn seek_start_probing(&mut self, target: u64, point: Option<SeekPoint>) -> Result<(u64, u64)> {
        if let Some(p) = point
            && let Some(off) = self.first_frame.checked_add(p.offset)
        {
            self.reposition(off)?;
            let ok = self
                .header_at(0)?
                .is_some_and(|h| h.first_sample == p.sample);
            if ok {
                return Ok((off, p.sample));
            }
        }
        self.bisect(target)
    }

    /// Scans frames from `off` (whose first sample is `pts`) to the one holding
    /// `target` and leaves the demuxer positioned at its start.
    fn scan_to(&mut self, target: u64, mut off: u64, mut pts: u64) -> Result<SeekTo> {
        let mut restarted = off == self.first_frame;
        'restart: loop {
            self.reposition(off)?;
            self.done = false;
            self.next_pts = pts;
            loop {
                self.opts.cancel.check()?;
                let Some(h) = self.sync_head()? else {
                    self.done = true;
                    return Ok(SeekTo {
                        frame: self.next_pts,
                        preroll: 0,
                    });
                };
                if h.first_sample > target {
                    if !restarted {
                        // The shortcut overshot: start over from the beginning.
                        restarted = true;
                        off = self.first_frame;
                        pts = 0;
                        continue 'restart;
                    }
                    self.next_pts = h.first_sample;
                    return Ok(SeekTo {
                        frame: h.first_sample,
                        preroll: 0,
                    });
                }
                let end_pts = h.first_sample.saturating_add(u64::from(h.block_size));
                if target < end_pts {
                    self.next_pts = h.first_sample;
                    return Ok(SeekTo {
                        frame: h.first_sample,
                        preroll: target - h.first_sample,
                    });
                }
                let at = self.abs(self.head);
                let loc = self.locate(&h)?;
                if !loc.crc_ok && self.strict() {
                    return Err(Error::Invalid {
                        what: "flac frame crc",
                        offset: at,
                    });
                }
                if loc.crc_ok {
                    self.blocking.get_or_insert(h.blocking_variable);
                }
                self.consume_to(loc.end);
                self.next_pts = end_pts;
            }
        }
    }
}

impl FlacDemuxer {
    /// Rebases the timeline of a stream whose first frame does not start at sample 0.
    ///
    /// Such a stream is a cut-out of a longer one (leading frames removed, metadata kept).
    /// When the first bytes after the metadata are a valid frame with a nonzero first
    /// sample, that sample becomes the origin: positions are reported relative to it, the
    /// track length becomes the declared total minus the origin, and `Event::TimelineRebased`
    /// is queued. The stream is no longer complete, so the track's STREAMINFO copy has its
    /// MD5 zeroed (disabling verification) and the seek table, whose entries describe the
    /// original file, is dropped.
    fn detect_origin(&mut self) -> Result<()> {
        let first = self.first_frame;
        self.reposition(first)?;
        let origin = match self.header_at(0)? {
            Some(h) if h.first_sample > 0 => {
                let loc = self.locate(&h)?;
                loc.crc_ok.then_some(h.first_sample)
            }
            _ => None,
        };
        self.blocking = None;
        self.reposition(first)?;
        let Some(origin) = origin else {
            return Ok(());
        };
        self.origin = origin;
        self.seek_points.clear();
        let total = self.info.total_samples;
        self.info.total_samples = total.saturating_sub(origin);
        let track = &mut self.tracks[0];
        track.frames = (self.info.total_samples != 0).then_some(self.info.total_samples);
        if let Some(md5) = track.codec_private.get_mut(18..34) {
            md5.fill(0);
        }
        self.event(Event::TimelineRebased { origin });
        Ok(())
    }

    /// Recognises the pre-2007 variable block size layout.
    ///
    /// RFC 9639 appendix B.1: before July 2007 variable block size streams were not
    /// flagged in the frame header, and decoders told them apart by comparing the
    /// minimum and maximum block size of STREAMINFO. The coded number of such a frame
    /// is the sample number although the header says "fixed". Because a fixed stream
    /// can also have a smaller minimum (its short last frame), the layout is only
    /// accepted when the second frame's number equals the first frame's block size
    /// (a frame number would be 1). Streams with equal minimum and maximum are never
    /// examined.
    fn detect_legacy_vbr(&mut self) -> Result<()> {
        if self.info.min_block == self.info.max_block {
            return Ok(());
        }
        let first = self.first_frame;
        self.reposition(first)?;
        let found = (|| -> Result<bool> {
            let Some(h0) = self.header_at(0)? else {
                return Ok(false);
            };
            if h0.blocking_variable || h0.first_sample != 0 {
                return Ok(false);
            }
            let loc = self.locate(&h0)?;
            if !loc.crc_ok {
                return Ok(false);
            }
            self.ensure(loc.end + MAX_HEADER)?;
            let Some(rest) = self.buf.get(loc.end..) else {
                return Ok(false);
            };
            let rest = &rest[..rest.len().min(MAX_HEADER)];
            Ok(
                parse_frame_header_with(rest, &self.info, true).is_ok_and(|h1| {
                    !h1.blocking_variable && h1.first_sample == u64::from(h0.block_size)
                }),
            )
        })();
        self.legacy_vbr = match found {
            Ok(b) => b,
            Err(e @ Error::Cancelled) => return Err(e),
            Err(_) => false,
        };
        self.reposition(first)?;
        self.blocking = None;
        Ok(())
    }
}

/// End of a frame found by [`FlacDemuxer::locate`].
struct Located {
    /// Buffer index one past the frame.
    end: usize,
    /// The frame passes its CRC-16.
    crc_ok: bool,
}

/// Absolute position where an APEv2 and/or ID3v1 trailer starts (`u64::MAX` = none).
fn find_trailer(src: &mut dyn Source, first_frame: u64) -> Result<u64> {
    let Some(len) = src.len() else {
        return Ok(u64::MAX);
    };
    let read_at = |src: &mut dyn Source, pos: u64, n: usize| -> Result<Option<Vec<u8>>> {
        if pos < first_frame || pos + n as u64 > len {
            return Ok(None);
        }
        src.seek(pos)?;
        let mut v = vec![0u8; n];
        let mut got = 0;
        while got < n {
            match src.read(&mut v[got..])? {
                0 => return Ok(None),
                k => got += k,
            }
        }
        Ok(Some(v))
    };
    let mut end = len;
    if len >= ID3V1_LEN as u64
        && let Some(tag) = read_at(src, len - ID3V1_LEN as u64, 3)?
        && &tag[..] == b"TAG"
    {
        end = len - ID3V1_LEN as u64;
    }
    if end >= 32
        && let Some(f) = read_at(src, end - 32, 32)?
        && f.starts_with(b"APETAGEX")
    {
        let le = |i: usize| u32::from_le_bytes([f[i], f[i + 1], f[i + 2], f[i + 3]]);
        let (version, size, flags) = (le(8), u64::from(le(12)), le(20));
        // The size covers the items and the footer but not an optional header.
        let total = size + if flags & 0x8000_0000 != 0 { 32 } else { 0 };
        if (version == 1000 || version == 2000) && size >= 32 && total <= end - first_frame {
            end -= total;
        }
    }
    Ok(if end < len { end } else { u64::MAX })
}

/// Opens a FLAC stream, parsing its metadata blocks.
pub fn open_flac(mut src: Box<dyn Source>, opts: &Options) -> Result<FlacDemuxer> {
    opts.cancel.check()?;
    if let Some(len) = src.len() {
        opts.limits.check_bytes(len)?;
    }
    let marker = skip_id3v2(src.as_mut(), &opts.limits)?;
    let (info, seek_points, blocks, first_frame) = {
        let mut r = ByteReader::new(src.as_mut());
        if &r.fourcc()? != b"fLaC" {
            return Err(Error::Invalid {
                what: "flac signature",
                offset: marker,
            });
        }
        read_metadata(&mut r, &opts.limits)?
    };
    opts.limits.check_sample_rate(info.sample_rate)?;
    opts.limits.check_channels(u16::from(info.channels))?;
    let frames = (info.total_samples != 0).then_some(info.total_samples);
    if let Some(f) = frames {
        opts.limits.check_frames(f)?;
    }
    let codec_private = blocks.first().map(|b| b.data.clone()).unwrap_or_default();
    let track = Track {
        id: 0,
        codec: CodecId::Flac,
        sample_rate: info.sample_rate,
        channels: u16::from(info.channels),
        channel_mask: 0,
        frames,
        delay: 0,
        padding: 0,
        gapless: GaplessKind::Exact,
        codec_private,
    };
    let audio_end = find_trailer(src.as_mut(), first_frame)?;
    src.seek(first_frame)?;
    let mut d = FlacDemuxer {
        src,
        opts: opts.clone(),
        tracks: vec![track],
        info,
        seek_points,
        blocks,
        first_frame,
        buf: Vec::new(),
        head: 0,
        buf_off: first_frame,
        eof: false,
        chunk: READ_CHUNK,
        next_pts: 0,
        last_end: first_frame,
        done: false,
        audio_end,
        blocking: None,
        legacy_vbr: false,
        origin: 0,
        clamped: false,
        skipped: None,
        fill_left: 0,
        fill_resume: 0,
        events: Vec::new(),
    };
    d.detect_legacy_vbr()?;
    d.detect_origin()?;
    Ok(d)
}

impl Demuxer for FlacDemuxer {
    fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    fn next_packet(&mut self, out: &mut Packet) -> Result<bool> {
        self.opts.cancel.check()?;
        if self.done {
            return Ok(false);
        }
        if self.fill_left > 0 {
            self.fill_chunk(out);
            return Ok(true);
        }
        loop {
            let Some(h) = self.sync_head()? else {
                return self.finish();
            };
            let start = self.head;
            let offset = self.abs(start);
            let skipped = self.skipped.take();
            if h.first_sample != self.next_pts {
                if self.strict() {
                    return Err(Error::Invalid {
                        what: "flac sample number",
                        offset,
                    });
                }
                // Only a frame that passes its CRC-16 can justify a gap or an overlap.
                let loc = self.locate(&h)?;
                if !loc.crc_ok {
                    self.skipped = skipped;
                    self.skip_one()?;
                    continue;
                }
                self.blocking.get_or_insert(h.blocking_variable);
                if h.first_sample > self.next_pts {
                    // Samples are missing before this frame: fill them with silence,
                    // up to a plausibility cap, then jump the timeline. The gap is
                    // reported once; the chunks are produced without locating the
                    // frame again.
                    let gap = h.first_sample - self.next_pts;
                    let total = self.info.total_samples;
                    let mut room = self.opts.limits.max_frames.saturating_sub(self.next_pts);
                    if total != 0 {
                        room = room.min(total.saturating_sub(self.next_pts));
                    }
                    let cap = GAP_FILL_BLOCKS * u64::from(self.info.max_block);
                    let fill = gap.min(room).min(cap);
                    self.event(Event::CorruptFrame {
                        pts: self.next_pts,
                        offset: self.last_end,
                        frames: u32::try_from(gap).unwrap_or(u32::MAX),
                    });
                    if fill == 0 {
                        self.next_pts = h.first_sample;
                        continue;
                    }
                    self.fill_left = fill;
                    self.fill_resume = h.first_sample;
                    self.fill_chunk(out);
                    return Ok(true);
                }
                // The frame lies before the current position: drop it.
                self.consume_to(loc.end);
                self.event(Event::CorruptFrame {
                    pts: h.first_sample,
                    offset,
                    frames: h.block_size,
                });
                continue;
            }
            let end_pts = h.first_sample.saturating_add(u64::from(h.block_size));
            if let Err(e) = self.opts.limits.check_frames(end_pts) {
                if self.strict() {
                    return Err(e);
                }
                // Resilient: the stream ends where the frame limit is exceeded.
                self.event(Event::CorruptFrame {
                    pts: h.first_sample,
                    offset,
                    frames: h.block_size,
                });
                self.done = true;
                return Ok(false);
            }
            let total = self.info.total_samples;
            if total != 0 && end_pts > total {
                if self.strict() {
                    return Err(Error::Invalid {
                        what: "flac total samples",
                        offset,
                    });
                }
                if !self.clamped {
                    self.clamped = true;
                    self.event(Event::Clamped {
                        what: "flac total samples",
                        declared: total,
                        actual: end_pts,
                    });
                }
            }
            let loc = self.locate(&h)?;
            // RFC 9639 section 4.1: only the last block may be shorter than 16 samples.
            if h.block_size < 16 && self.strict() && !(self.eof && loc.end == self.buf.len()) {
                return Err(Error::Invalid {
                    what: "flac block size",
                    offset,
                });
            }
            if let Some(from) = skipped
                && !self.strict()
            {
                // Unparseable bytes were skipped but no samples are missing.
                self.event(Event::CorruptFrame {
                    pts: self.next_pts,
                    offset: from,
                    frames: 0,
                });
            }
            if !loc.crc_ok {
                if self.strict() {
                    return Err(Error::Invalid {
                        what: "flac frame crc",
                        offset,
                    });
                }
                self.silence(out, h.first_sample, h.block_size, offset);
            } else {
                self.blocking.get_or_insert(h.blocking_variable);
                out.track = 0;
                out.pts = h.first_sample;
                out.frames = h.block_size;
                out.offset = offset;
                out.data.clear();
                out.data.extend_from_slice(&self.buf[start..loc.end]);
            }
            self.consume_to(loc.end);
            self.next_pts = end_pts;
            return Ok(true);
        }
    }

    fn seek(&mut self, track: u32, frame: u64) -> Result<SeekTo> {
        self.opts.cancel.check()?;
        if track != 0 {
            return Err(Error::Unsupported("track"));
        }
        let total = self.info.total_samples;
        let target = if total != 0 { frame.min(total) } else { frame };
        if total != 0
            && target >= total
            && let Some(len) = self.src.len()
        {
            self.reposition(len)?;
            self.eof = true;
            self.done = true;
            self.next_pts = total;
            return Ok(SeekTo {
                frame: total,
                preroll: 0,
            });
        }
        let (off, pts) = self.seek_start(target)?;
        self.scan_to(target, off, pts)
    }

    fn drain_events(&mut self, out: &mut Vec<Event>) {
        out.append(&mut self.events);
    }
}
