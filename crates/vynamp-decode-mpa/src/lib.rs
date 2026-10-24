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
//! truncated last frame is an error; in `Mode::Resilient` the lost span becomes silence
//! packets (empty `data`, at most eight per gap) and one `Event::CorruptFrame`.
//!
//! `seek` is exact and returns a start before the target: two frames earlier for Layer III
//! (the bit reservoir reaches back at most 511 bytes, which is at most two frames at the
//! lowest bitrates) and one frame earlier for Layers I and II (the synthesis filterbank
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
/// 30. A lone header found away from the start, with room for its successor, gives 0: random
/// data contains such headers by chance.
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
            1 if start > 0 && room => 0,
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
