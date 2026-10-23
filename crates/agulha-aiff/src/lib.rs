//! AIFF and AIFF-C demuxer.
//!
//! Exposes a single PCM track (id 0) split into packets of up to
//! [`PACKET_FRAMES`] frames.
//!
//! Notes:
//!
//! - Chunks may appear in any order (`SSND` before `COMM` is parsed normally);
//!   the first `COMM` and first `SSND` win, other chunks are skipped. Chunk
//!   payloads are padded to an even size.
//! - The frame count is the smaller of the `COMM` count and what the `SSND`
//!   chunk (after its `offset`) can hold. If the `COMM` count exceeds what the
//!   chunk holds, `Mode::Strict` fails to open with
//!   `Invalid { what: "comm frames exceed ssnd" }` (offset of the `SSND` chunk
//!   header) while `Mode::Resilient` clamps to the chunk. If the data size is
//!   not a multiple of the frame size, the trailing partial frame is dropped.
//! - When `SSND` precedes `COMM`, the walk must skip over the sample data to
//!   find `COMM`; if the `SSND` size reaches past the end of the source, opening
//!   fails with `Invalid { what: "unexpected eof" }` even in `Mode::Resilient`.
//! - The data length comes from the `SSND` chunk header, so a file that ends
//!   early is detected while reading: `Mode::Strict` fails with
//!   `Invalid { what: "unexpected eof" }`, `Mode::Resilient` returns a short
//!   final packet of whole frames and then ends the stream.
//! - Sample sizes that are not a multiple of 8 use a container rounded up to
//!   whole bytes; `valid_bits` keeps the declared size.
#![warn(missing_docs)]

mod comm;
#[doc(hidden)]
pub mod extended;

use agulha_core::{
    ByteReader, CancelToken, CodecId, Demuxer, Error, LimitKind, Mode, Options, Packet, Result,
    SeekTo, Source, Track,
};
use comm::{COMM_READ, Comm};

/// Maximum frames per packet.
pub const PACKET_FRAMES: u64 = 4096;

/// Detects an AIFF or AIFF-C signature in `header` (the first bytes of the file).
///
/// Returns 100 for `FORM` with form type `AIFF` or `AIFC`, else 0.
pub fn probe_aiff(header: &[u8]) -> u8 {
    if header.len() >= 12 && &header[..4] == b"FORM" && matches!(&header[8..12], b"AIFF" | b"AIFC")
    {
        100
    } else {
        0
    }
}

/// Demuxer for AIFF and AIFF-C files.
pub struct AiffDemuxer {
    src: Box<dyn Source>,
    tracks: Vec<Track>,
    cancel: CancelToken,
    mode: Mode,
    /// Bytes per frame.
    block: u64,
    data_start: u64,
    total: u64,
    /// Frames already returned (position of the next packet).
    next_frame: u64,
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

/// Location of the `SSND` sample data.
#[derive(Clone, Copy)]
struct Ssnd {
    /// Absolute position of the chunk header.
    header: u64,
    data_start: u64,
    data_len: u64,
}

/// Opens an AIFF or AIFF-C file.
pub fn open_aiff(mut src: Box<dyn Source>, opts: &Options) -> Result<AiffDemuxer> {
    opts.cancel.check()?;
    if let Some(len) = src.len() {
        opts.limits.check_bytes(len)?;
    }
    let src_len = src.len();
    let (comm, ssnd) = {
        let mut r = ByteReader::new(src.as_mut());
        walk(&mut r, src_len, opts)?
    };
    let block = comm.block();
    let total = u64::from(comm.frames).min(ssnd.data_len / block);
    if total < u64::from(comm.frames) && opts.mode == Mode::Strict {
        return Err(Error::Invalid {
            what: "comm frames exceed ssnd",
            offset: ssnd.header,
        });
    }
    opts.limits.check_frames(total)?;
    src.seek(ssnd.data_start)?;
    let track = Track {
        id: 0,
        codec: CodecId::Pcm(comm.spec),
        sample_rate: comm.sample_rate,
        channels: comm.channels,
        channel_mask: 0,
        frames: Some(total),
        delay: 0,
        padding: 0,
        codec_private: Vec::new(),
    };
    Ok(AiffDemuxer {
        src,
        tracks: vec![track],
        cancel: opts.cancel.clone(),
        mode: opts.mode,
        block,
        data_start: ssnd.data_start,
        total,
        next_frame: 0,
    })
}

/// Reads the `FORM` header and walks chunks until both `COMM` and `SSND` are known.
fn walk(r: &mut ByteReader<'_>, src_len: Option<u64>, opts: &Options) -> Result<(Comm, Ssnd)> {
    let magic = r.fourcc()?;
    let _form_size = r.u32_be()?;
    let form = r.fourcc()?;
    if &magic != b"FORM" {
        return Err(Error::Invalid {
            what: "aiff signature",
            offset: 0,
        });
    }
    let aifc = match &form {
        b"AIFF" => false,
        b"AIFC" => true,
        _ => {
            return Err(Error::Invalid {
                what: "form type",
                offset: 8,
            });
        }
    };

    let mut comm: Option<Comm> = None;
    let mut ssnd: Option<Ssnd> = None;
    loop {
        opts.cancel.check()?;
        let start = r.position();
        if src_len.is_some_and(|l| start >= l) {
            return Err(Error::Invalid {
                what: if comm.is_none() {
                    "missing comm chunk"
                } else {
                    "missing ssnd chunk"
                },
                offset: start,
            });
        }
        let id = r.fourcc()?;
        let size = u64::from(r.u32_be()?);
        let body = r.position();
        match &id {
            b"COMM" if comm.is_none() => {
                if size > u64::from(opts.limits.max_metadata_bytes) {
                    return Err(Error::Limit(LimitKind::Metadata));
                }
                let n = size.min(COMM_READ as u64) as usize;
                let mut buf = [0u8; COMM_READ];
                r.read_exact(&mut buf[..n])?;
                comm = Some(comm::parse(&buf[..n], size, aifc, body, &opts.limits)?);
            }
            b"SSND" if ssnd.is_none() => {
                if size < 8 {
                    return Err(Error::Invalid {
                        what: "ssnd size",
                        offset: start,
                    });
                }
                let offset = u64::from(r.u32_be()?);
                let _block_size = r.u32_be()?;
                let data_len = (size - 8).checked_sub(offset).ok_or(Error::Invalid {
                    what: "ssnd offset",
                    offset: start,
                })?;
                ssnd = Some(Ssnd {
                    header: start,
                    data_start: body + 8 + offset,
                    data_len,
                });
            }
            _ => {}
        }
        // Done as soon as both are known, without skipping the chunk just read
        // (only an SSND that precedes COMM has its data skipped, below).
        if let (Some(c), Some(s)) = (comm, ssnd) {
            return Ok((c, s));
        }
        // Next chunk: payload padded to an even size.
        let invalid_size = Error::Invalid {
            what: "chunk size",
            offset: start,
        };
        let next = body
            .checked_add(size)
            .and_then(|p| p.checked_add(size & 1))
            .ok_or(invalid_size.clone())?;
        let skip = next.checked_sub(r.position()).ok_or(invalid_size)?;
        r.skip(skip)?;
    }
}

impl Demuxer for AiffDemuxer {
    fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    fn next_packet(&mut self, out: &mut Packet) -> Result<bool> {
        self.cancel.check()?;
        let n = self
            .total
            .saturating_sub(self.next_frame)
            .min(PACKET_FRAMES);
        if n == 0 {
            return Ok(false);
        }
        let want = (n * self.block) as usize;
        out.data.clear();
        out.data.resize(want, 0);
        let got = read_full(self.src.as_mut(), &mut out.data)?;
        let whole = got as u64 / self.block;
        if got < want {
            if self.mode == Mode::Strict {
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
        out.frames = whole as u32;
        self.next_frame += whole;
        Ok(true)
    }

    fn seek(&mut self, track: u32, frame: u64) -> Result<SeekTo> {
        if track != 0 {
            return Err(Error::Invalid {
                what: "track",
                offset: self.src.position(),
            });
        }
        let frame = frame.min(self.total);
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
}
