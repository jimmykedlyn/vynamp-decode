//! MPEG audio elementary stream demuxer (ISO/IEC 11172-3 and 13818-3): `.mp1`, `.mp2`
//! and `.mp3` files of Layer I, II and III in MPEG-1, 2 and 2.5.
//!
//! `open_mpa` skips ID3v2 tags, ignores trailing tags (ID3v1, APEv2, Lyrics3v2), and finds
//! the first confirmed frame: a valid header whose successor is also valid and agrees on
//! version, layer and sample rate (the false-sync guard). Free-format streams (bitrate
//! index 0) get their frame length measured. A Xing/Info header (with an optional LAME tag)
//! or a VBRI header in the first Layer III frame sets `Track::frames` and the gapless numbers
//! and is not delivered as a packet. See [`open_mpa`] for the arithmetic.
//!
//! `next_packet` yields one packet per frame with `pts = frame index * samples per frame`;
//! `data` is the whole frame including its header. In `Mode::Strict` a lost sync or a
//! truncated last frame is an error (bytes after the last frame that hold no further
//! frame end the stream, as in FLAC; a Xing/VBRI frame count that is neither the number of frames found
//! nor that number plus one (some encoders count the header frame) is
//! `Invalid { what: "mpeg frame count" }`); in `Mode::Resilient` the lost span becomes silence
//! packets (empty `data`, at most eight per gap) and one `Event::CorruptFrame`.
//!
//! `seek` is exact and returns a start before the target. For Layer III the start is computed:
//! the `main_data_begin` of the goal frame and of the two frames before it (the overlap of the
//! inverse MDCT and the filterbank history of the goal's first granule) say how many bytes of earlier
//! main data they need; the start is the earliest frame whose successors' main data covers all three
//! (at most 32 frames back), so that the output from the goal on is identical to a decode from the
//! start of the stream. For Layers I and II it is one frame earlier (the synthesis filterbank
//! needs a frame to warm up). `SeekTo::preroll` is the number of samples to discard after
//! decoding from there.
#![warn(missing_docs)]

mod demux;
mod header;
mod vbr;

pub use demux::{MpaDemuxer, open_mpa};
pub use header::{ChannelMode, FrameHeader, MpegVersion, parse_header};
pub use vbr::{LameInfo, VbrInfo, VbrKind, parse_vbr};

/// Longest stretch of `header` searched for the first frame.
const PROBE_SCAN: usize = 4096;

/// Detects MPEG audio in `header` (the first bytes after any ID3v2 tag).
///
/// Looks for a valid frame header and follows the chain of frame lengths: three consistent
/// consecutive frames (same version, layer and sample rate) give 90, two give 60, one gives
/// 30. A lone header found away from the start, with room for its successor, or a free-format
/// header (which has no length to follow) found there, gives 0: random data contains such headers
/// by chance.
pub fn probe_mpa(header: &[u8]) -> u8 {
    let mut best = 0u8;
    for start in 0..header.len().min(PROBE_SCAN) {
        if header[start] != 0xFF {
            continue;
        }
        let Some(first) = header
            .get(start..start + 4)
            .and_then(|b| parse_header(b.try_into().ok()?))
        else {
            continue;
        };
        let mut count = 1;
        let mut room = false;
        let (mut h, mut pos) = (first, start);
        while h.frame_len != 0 {
            pos += h.frame_len;
            let Some(b) = header.get(pos..pos + 4) else {
                room = header.len() >= pos + 4;
                break;
            };
            match parse_header([b[0], b[1], b[2], b[3]]) {
                Some(n)
                    if n.version == first.version
                        && n.layer == first.layer
                        && n.sample_rate == first.sample_rate =>
                {
                    count += 1;
                    h = n;
                }
                _ => {
                    room = true;
                    break;
                }
            }
        }
        let score = match count {
            1 if start > 0 && (room || first.frame_len == 0) => 0,
            1 => 30,
            2 => 60,
            _ => 90,
        };
        best = best.max(score);
        if best == 90 {
            break;
        }
    }
    best
}
