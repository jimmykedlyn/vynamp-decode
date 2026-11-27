//! MP4 / ISO base media file format demuxer (`.m4a`, `.m4b`, `.mp4`).
//!
//! Exposes the first audio track (`hdlr` = `soun`) whose sample entry is ALAC; any other
//! audio sample entry is `Unsupported("mp4 codec")`. Each MP4 sample is one packet, with
//! the timing of the sample tables (`stts`, `stsc`, `stsz`/`stz2`, `stco`/`co64`).
//!
//! Notes:
//!
//! - The whole `moov` box is read into memory; its size is bounded by
//!   `Limits::max_metadata_bytes` (`Limit(Metadata)`), the total length by
//!   `Limits::max_frames` (`Limit(Frames)`).
//! - The media timescale (`mdhd`) must equal the sample rate of the ALAC configuration,
//!   so that sample durations are frames; otherwise `Unsupported("mp4 timescale")`.
//! - The edit list is not applied. `Track::delay` and `Track::padding` stay 0; a non-identity
//!   edit list makes `Track::gapless` `Unknown` instead of `Exact`.
//! - Chapters come from the Nero `moov/udta/chpl` box when present, else from the
//!   QuickTime text track that the audio track names in `tref/chap`. FFmpeg writes both
//!   for `-f ipod`, but anchors the first chapter of the text track at 0, so `chpl` is
//!   the one that keeps the declared start.
#![warn(missing_docs)]

mod boxes;
mod table;

use boxes::Bx;
use table::Sample;
use vynamp_decode_core::{
    ByteReader, CancelToken, Chapter, CodecId, Demuxer, Error, GaplessKind, LimitKind, Mode,
    Options, Packet, Result, SeekTo, Source, Track,
};

/// Length of an `ALACSpecificConfig`.
const ALAC_CONFIG_LEN: usize = 24;

/// Detects an MP4 file: 100 when the first box is `ftyp`, else 0.
pub fn probe_mp4(header: &[u8]) -> u8 {
    if header.len() >= 8 && &header[4..8] == b"ftyp" {
        100
    } else {
        0
    }
}

/// Demuxer for MP4 files with one ALAC audio track.
pub struct Mp4Demuxer {
    src: Box<dyn Source>,
    tracks: Vec<Track>,
    samples: Vec<Sample>,
    /// Index of the next sample to return.
    next: usize,
    chapters: Vec<Chapter>,
    cancel: CancelToken,
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

/// Walks the top-level boxes and returns the payload of `moov` and its source offset.
fn read_moov(src: &mut dyn Source, opts: &Options) -> Result<(Vec<u8>, u64)> {
    loop {
        opts.cancel.check()?;
        let start = src.position();
        let mut head = [0u8; 8];
        let got = read_full(src, &mut head)?;
        if got == 0 {
            return Err(Error::Invalid {
                what: "mp4 moov missing",
                offset: start,
            });
        }
        if got < head.len() {
            return Err(Error::Invalid {
                what: "unexpected eof",
                offset: src.position(),
            });
        }
        let mut r = ByteReader::new(src);
        let size = match u32::from_be_bytes([head[0], head[1], head[2], head[3]]) {
            0 => None,
            1 => Some(r.u64_be()?),
            s => Some(u64::from(s)),
        };
        let header = r.position() - start;
        let body = match size {
            Some(s) if s < header => {
                return Err(Error::Invalid {
                    what: "mp4 box size",
                    offset: start,
                });
            }
            Some(s) => Some(s - header),
            None => src.len().map(|l| l.saturating_sub(start + header)),
        };
        if &head[4..8] == b"moov" {
            let len = body.ok_or(Error::Unsupported("mp4 moov of unknown size"))?;
            if len > u64::from(opts.limits.max_metadata_bytes) {
                return Err(Error::Limit(LimitKind::Metadata));
            }
            let offset = src.position();
            if src.len().is_some_and(|l| offset + len > l) {
                return Err(Error::Invalid {
                    what: "unexpected eof",
                    offset: src.len().unwrap_or(offset),
                });
            }
            // `len` is at most `max_metadata_bytes`, a u32.
            let mut moov = vec![0u8; len as usize];
            ByteReader::new(src).read_exact(&mut moov)?;
            return Ok((moov, offset));
        }
        match body {
            Some(len) => ByteReader::new(src).skip(len)?,
            None => {
                return Err(Error::Invalid {
                    what: "mp4 moov missing",
                    offset: start,
                });
            }
        }
    }
}

fn invalid(what: &'static str, b: &Bx<'_>) -> Error {
    Error::Invalid {
        what,
        offset: b.offset,
    }
}

/// `tkhd` track id.
fn track_id(trak: &Bx<'_>) -> Result<u32> {
    let tkhd = trak.child(b"tkhd")?.ok_or(invalid("mp4 tkhd", trak))?;
    let mut c = tkhd.cur();
    let skip = if c.version()? == 1 { 16 } else { 8 };
    c.skip(skip)?;
    c.u32()
}

/// `mdhd` timescale.
fn timescale(mdia: &Bx<'_>) -> Result<u32> {
    let mdhd = mdia.child(b"mdhd")?.ok_or(invalid("mp4 mdhd", mdia))?;
    let mut c = mdhd.cur();
    let skip = if c.version()? == 1 { 16 } else { 8 };
    c.skip(skip)?;
    let ts = c.u32()?;
    if ts == 0 {
        return Err(invalid("mp4 timescale", &mdhd));
    }
    Ok(ts)
}

/// `hdlr` handler type.
fn handler(mdia: &Bx<'_>) -> Result<[u8; 4]> {
    let hdlr = mdia.child(b"hdlr")?.ok_or(invalid("mp4 hdlr", mdia))?;
    let mut c = hdlr.cur();
    c.version()?;
    c.skip(4)?;
    c.fourcc()
}

/// `mdia/minf/stbl`.
fn stbl<'a>(mdia: &Bx<'a>) -> Result<Bx<'a>> {
    let minf = mdia.child(b"minf")?.ok_or(invalid("mp4 minf", mdia))?;
    minf.child(b"stbl")?.ok_or(invalid("mp4 stbl", &minf))
}

/// The first sample entry of `stsd`.
fn sample_entry<'a>(stbl: &Bx<'a>) -> Result<Bx<'a>> {
    let stsd = stbl.child(b"stsd")?.ok_or(invalid("mp4 stsd", stbl))?;
    stsd.children(8)?
        .into_iter()
        .next()
        .ok_or(invalid("mp4 stsd", &stsd))
}

/// The `ALACSpecificConfig` of an `alac` audio sample entry.
fn alac_config(entry: &Bx<'_>) -> Result<Vec<u8>> {
    // SampleEntry (8 bytes) + AudioSampleEntry fields (20 bytes) precede the child boxes.
    let inner = entry
        .children(28)?
        .into_iter()
        .find(|b| &b.kind == b"alac")
        .ok_or(invalid("mp4 alac config", entry))?;
    match inner.body.get(4..4 + ALAC_CONFIG_LEN) {
        Some(cfg) => Ok(cfg.to_vec()),
        None => Err(invalid("mp4 alac config", &inner)),
    }
}

/// Whether the edit list of `trak` is absent or plays the media unchanged from its start.
fn identity_edit(trak: &Bx<'_>, movie_ts: Option<u32>, media_ts: u32, total: u64) -> Result<bool> {
    let Some(edts) = trak.child(b"edts")? else {
        return Ok(true);
    };
    let Some(elst) = edts.child(b"elst")? else {
        return Ok(true);
    };
    let mut c = elst.cur();
    let v1 = c.version()? == 1;
    let n = c.count(if v1 { 20 } else { 12 })?;
    if n == 0 {
        return Ok(true);
    }
    let (Some(movie_ts), 1) = (movie_ts, n) else {
        return Ok(false);
    };
    let (seg, media_time) = if v1 {
        (c.u64()?, c.u64()? as i64)
    } else {
        (u64::from(c.u32()?), i64::from(c.u32()? as i32))
    };
    let rate = c.u32()?;
    // Same length within one tick of the movie timescale.
    let seg = u128::from(seg) * u128::from(media_ts);
    let media = u128::from(total) * u128::from(movie_ts);
    Ok(media_time == 0 && rate == 0x0001_0000 && seg.abs_diff(media) < u128::from(media_ts))
}

/// `mvhd` timescale, if the box is there.
fn movie_timescale(moov: &Bx<'_>) -> Result<Option<u32>> {
    let Some(mvhd) = moov.child(b"mvhd")? else {
        return Ok(None);
    };
    let mut c = mvhd.cur();
    let skip = if c.version()? == 1 { 16 } else { 8 };
    c.skip(skip)?;
    Ok(Some(c.u32()?).filter(|&t| t != 0))
}

/// A chapter title: strict mode rejects invalid UTF-8, resilient mode replaces it.
fn title(bytes: &[u8], offset: u64, mode: Mode) -> Result<String> {
    match (std::str::from_utf8(bytes), mode) {
        (Ok(s), _) => Ok(s.to_owned()),
        (Err(_), Mode::Resilient) => Ok(String::from_utf8_lossy(bytes).into_owned()),
        (Err(_), Mode::Strict) => Err(Error::Invalid {
            what: "mp4 chapter title",
            offset,
        }),
    }
}

/// `t` ticks of `scale` per second, as frames at `rate`, rounded to nearest.
fn to_frames(t: u64, scale: u64, rate: u32) -> u64 {
    let f = (u128::from(t) * u128::from(rate) + u128::from(scale / 2)) / u128::from(scale);
    u64::try_from(f).unwrap_or(u64::MAX)
}

/// Chapters of the Nero `chpl` box (start times in units of 100 ns).
fn chpl_chapters(chpl: &Bx<'_>, rate: u32, mode: Mode) -> Result<Vec<Chapter>> {
    let mut c = chpl.cur();
    if c.version()? == 1 {
        c.skip(4)?;
    }
    let n = c.u8()?;
    let mut out = Vec::with_capacity(usize::from(n));
    for _ in 0..n {
        let start = c.u64()?;
        let len = c.u8()?;
        let at = c.offset();
        let text = c.take(usize::from(len))?;
        out.push(Chapter {
            title: title(text, at, mode)?,
            start_frame: to_frames(start, 10_000_000, rate),
        });
    }
    Ok(out)
}

/// Chapters of the QuickTime text track `id`: each sample is a big-endian u16 length and
/// that many bytes of text, starting at the sample time.
fn text_chapters(
    src: &mut dyn Source,
    traks: &[Bx<'_>],
    id: u32,
    rate: u32,
    opts: &Options,
    media_bytes: u64,
) -> Result<Vec<Chapter>> {
    let mut found = None;
    for t in traks {
        if track_id(t)? == id {
            found = Some(*t);
        }
    }
    let Some(trak) = found else {
        return match opts.mode {
            Mode::Strict => Err(Error::Invalid {
                what: "mp4 chapter track",
                offset: traks.first().map_or(0, |t| t.offset),
            }),
            Mode::Resilient => Ok(Vec::new()),
        };
    };
    let mdia = trak.child(b"mdia")?.ok_or(invalid("mp4 mdia", &trak))?;
    let scale = timescale(&mdia)?;
    let samples = table::samples(&stbl(&mdia)?, media_bytes, &opts.limits)?;
    let bytes: u64 = samples.iter().map(|s| u64::from(s.size)).sum();
    if bytes > u64::from(opts.limits.max_metadata_bytes) {
        return Err(Error::Limit(LimitKind::Metadata));
    }
    let mut out = Vec::with_capacity(samples.len());
    let mut buf = Vec::new();
    for s in &samples {
        opts.cancel.check()?;
        // `bytes` above bounds each size by `max_metadata_bytes`, a u32.
        buf.resize(s.size as usize, 0);
        src.seek(s.offset)?;
        ByteReader::new(src).read_exact(&mut buf)?;
        let bx = Bx {
            kind: *b"text",
            body: &buf,
            offset: s.offset,
        };
        let mut c = bx.cur();
        let len = c.u16()?;
        let at = c.offset();
        let text = c.take(usize::from(len))?;
        out.push(Chapter {
            title: title(text, at, opts.mode)?,
            start_frame: to_frames(s.pts, u64::from(scale), rate),
        });
    }
    Ok(out)
}

/// Opens an MP4 file and selects its first ALAC audio track.
pub fn open_mp4(mut src: Box<dyn Source>, opts: &Options) -> Result<Mp4Demuxer> {
    opts.cancel.check()?;
    if let Some(len) = src.len() {
        opts.limits.check_bytes(len)?;
    }
    let media_bytes = src.len().unwrap_or(u64::MAX);
    let (data, offset) = read_moov(src.as_mut(), opts)?;
    let moov = Bx {
        kind: *b"moov",
        body: &data,
        offset,
    };
    let traks: Vec<Bx<'_>> = moov
        .children(0)?
        .into_iter()
        .filter(|b| &b.kind == b"trak")
        .collect();
    let mut audio = None;
    for t in &traks {
        let Some(mdia) = t.child(b"mdia")? else {
            continue;
        };
        if &handler(&mdia)? == b"soun" {
            audio = Some((*t, mdia));
            break;
        }
    }
    let (trak, mdia) = audio.ok_or(Error::Unsupported("mp4 without audio track"))?;
    let id = track_id(&trak)?;
    let media_ts = timescale(&mdia)?;
    let stbl = stbl(&mdia)?;
    let entry = sample_entry(&stbl)?;
    if &entry.kind != b"alac" {
        return Err(Error::Unsupported("mp4 codec"));
    }
    let config = alac_config(&entry)?;
    let channels = u16::from(config[9]);
    let rate = u32::from_be_bytes([config[20], config[21], config[22], config[23]]);
    opts.limits.check_channels(channels)?;
    opts.limits.check_sample_rate(rate)?;
    if media_ts != rate {
        return Err(Error::Unsupported("mp4 timescale"));
    }
    let samples = table::samples(&stbl, media_bytes, &opts.limits)?;
    let total = samples.last().map_or(0, |s| s.pts + u64::from(s.duration));
    opts.limits.check_frames(total)?;
    let exact = identity_edit(&trak, movie_timescale(&moov)?, media_ts, total)?;

    let udta_chpl = match moov.child(b"udta")? {
        Some(udta) => udta.child(b"chpl")?,
        None => None,
    };
    let chap_ref = match trak.child(b"tref")? {
        Some(tref) => tref.child(b"chap")?,
        None => None,
    };
    let chapters = match (udta_chpl, chap_ref) {
        (Some(chpl), _) => chpl_chapters(&chpl, rate, opts.mode)?,
        (None, Some(chap)) => {
            let target = chap.cur().u32()?;
            text_chapters(src.as_mut(), &traks, target, rate, opts, media_bytes)?
        }
        (None, None) => Vec::new(),
    };

    let track = Track {
        id,
        codec: CodecId::Alac,
        sample_rate: rate,
        channels,
        channel_mask: 0,
        frames: Some(total),
        delay: 0,
        padding: 0,
        gapless: if exact {
            GaplessKind::Exact
        } else {
            GaplessKind::Unknown
        },
        codec_private: config,
    };
    Ok(Mp4Demuxer {
        src,
        tracks: vec![track],
        samples,
        next: 0,
        chapters,
        cancel: opts.cancel.clone(),
    })
}

impl Mp4Demuxer {
    fn total(&self) -> u64 {
        self.samples
            .last()
            .map_or(0, |s| s.pts + u64::from(s.duration))
    }
}

impl Demuxer for Mp4Demuxer {
    fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    fn next_packet(&mut self, out: &mut Packet) -> Result<bool> {
        self.cancel.check()?;
        let Some(&s) = self.samples.get(self.next) else {
            out.data.clear();
            return Ok(false);
        };
        let end = s.offset.checked_add(u64::from(s.size));
        if end.is_none_or(|e| self.src.len().is_some_and(|l| e > l)) {
            out.data.clear();
            return Err(Error::Invalid {
                what: "unexpected eof",
                offset: s.offset,
            });
        }
        out.data.clear();
        out.data.resize(s.size as usize, 0);
        self.src.seek(s.offset)?;
        if let Err(e) = ByteReader::new(self.src.as_mut()).read_exact(&mut out.data) {
            out.data.clear();
            return Err(e);
        }
        out.track = self.tracks[0].id;
        out.pts = s.pts;
        out.frames = s.duration;
        out.offset = s.offset;
        self.next += 1;
        Ok(true)
    }

    fn seek(&mut self, track: u32, frame: u64) -> Result<SeekTo> {
        if track != self.tracks[0].id {
            return Err(Error::Unsupported("track"));
        }
        let total = self.total();
        if frame >= total {
            self.next = self.samples.len();
            return Ok(SeekTo {
                frame: total,
                preroll: 0,
            });
        }
        // `frame < total` and the first sample starts at 0, so at least one sample starts
        // at or before `frame`.
        let i = self.samples.partition_point(|s| s.pts <= frame) - 1;
        self.next = i;
        let start = self.samples[i].pts;
        Ok(SeekTo {
            frame: start,
            preroll: frame - start,
        })
    }

    fn chapters(&self) -> &[Chapter] {
        &self.chapters
    }
}
