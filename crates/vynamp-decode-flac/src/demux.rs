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

use crate::frame::{FrameHeader, MAX_HEADER, parse_frame_header};
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
    /// Where Resilient mode started skipping unparseable bytes.
    skipped: Option<u64>,
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
        Ok(parse_frame_header(rest, &self.info)
            .ok()
            .filter(|h| blocking.is_none_or(|b| b == h.blocking_variable)))
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

    /// Resilient: a silence packet.
    fn silence(&mut self, out: &mut Packet, pts: u64, frames: u32, offset: u64) {
        out.track = 0;
        out.pts = pts;
        out.frames = frames;
        out.offset = offset;
        out.data.clear();
        self.event(Event::CorruptFrame {
            pts,
            offset,
            frames,
        });
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
    Ok(FlacDemuxer {
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
        skipped: None,
        events: Vec::new(),
    })
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
                    // Samples are missing before this frame: fill them with silence.
                    let gap = h.first_sample - self.next_pts;
                    let total = self.info.total_samples;
                    let mut room = self.opts.limits.max_frames.saturating_sub(self.next_pts);
                    if total != 0 {
                        room = room.min(total.saturating_sub(self.next_pts));
                    }
                    let fill = gap.min(room);
                    let (pts, at) = (self.next_pts, self.last_end);
                    if fill == 0 {
                        // Nothing more may be filled: report the loss and jump the timeline.
                        self.event(Event::CorruptFrame {
                            pts,
                            offset: at,
                            frames: u32::try_from(gap).unwrap_or(u32::MAX),
                        });
                        self.next_pts = h.first_sample;
                        continue;
                    }
                    let n = fill.min(u64::from(self.info.max_block)) as u32;
                    self.silence(out, pts, n, at);
                    self.next_pts += u64::from(n);
                    if fill < gap && u64::from(n) == fill {
                        // More was lost than the limits allow to fill: jump the timeline.
                        self.next_pts = h.first_sample;
                    }
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
            let loc = self.locate(&h)?;
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
