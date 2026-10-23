//! FLAC frame header (RFC 9639 section 9.1).

use crate::StreamInfo;
use vynamp_decode_core::{BitReader, Error, Result, crc8};

/// Longest possible frame header in bytes (4 fixed, 7 coded number, 2 + 2 extra, 1 CRC-8).
pub(crate) const MAX_HEADER: usize = 16;

/// How the channels of a frame are coded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelAssignment {
    /// Independent channels (the count is 1 to 8).
    Independent(u8),
    /// Left channel and side channel.
    LeftSide,
    /// Right channel and side channel.
    RightSide,
    /// Mid channel and side channel.
    MidSide,
}

/// A parsed and validated frame header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameHeader {
    /// Whether the stream uses variable block sizes (the coded number is a sample number).
    pub blocking_variable: bool,
    /// Block size in samples.
    pub block_size: u32,
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// Channel coding.
    pub channel_assignment: ChannelAssignment,
    /// Bits per sample.
    pub bits_per_sample: u8,
    /// Sample number of the first sample of the frame.
    pub first_sample: u64,
    /// Header length in bytes, including the CRC-8.
    pub header_len: usize,
}

/// Parses the frame header at the start of `bytes`.
///
/// Validates the sync code, the reserved bits and codes, the CRC-8 and the
/// agreement with `info` (sample rate, channel count, bit depth and maximum
/// block size). Error offsets are relative to `bytes`; a sync search treats any
/// error as "no header here". `bytes` may be longer than the header.
pub(crate) fn parse_frame_header(bytes: &[u8], info: &StreamInfo) -> Result<FrameHeader> {
    parse_frame_header_with(bytes, info, false)
}

/// Like [`parse_frame_header`]. With `legacy_vbr` a header that says "fixed" carries a
/// sample number, as written by encoders from before the blocking-strategy bit existed
/// (RFC 9639 appendix B.1).
pub(crate) fn parse_frame_header_with(
    bytes: &[u8],
    info: &StreamInfo,
    legacy_vbr: bool,
) -> Result<FrameHeader> {
    let bad = |what: &'static str, offset: usize| Error::Invalid {
        what,
        offset: offset as u64,
    };
    let [b0, b1, b2, b3, ..] = *bytes else {
        return Err(bad("flac header eof", 0));
    };
    if b0 != 0xFF || b1 & 0xFC != 0xF8 {
        return Err(bad("flac sync", 0));
    }
    // The bit after the 14-bit sync code and the lowest bit of byte 3 are reserved;
    // the lowest bit of byte 1 is the blocking strategy.
    if b1 & 0x02 != 0 || b3 & 0x01 != 0 {
        return Err(bad("flac reserved", 1));
    }
    let blocking_variable = b1 & 1 != 0;
    let bs_code = b2 >> 4;
    let sr_code = b2 & 0x0F;
    let ch_code = b3 >> 4;
    let bps_code = (b3 >> 1) & 0x07;

    let channel_assignment = match ch_code {
        0..=7 => ChannelAssignment::Independent(ch_code + 1),
        8 => ChannelAssignment::LeftSide,
        9 => ChannelAssignment::RightSide,
        10 => ChannelAssignment::MidSide,
        _ => return Err(bad("flac channel assignment", 3)),
    };
    let channels = match channel_assignment {
        ChannelAssignment::Independent(n) => n,
        _ => 2,
    };
    let bits_per_sample = match bps_code {
        0 => info.bits_per_sample,
        1 => 8,
        2 => 12,
        4 => 16,
        5 => 20,
        6 => 24,
        7 => 32,
        _ => return Err(bad("flac bit depth", 3)),
    };

    let mut r = BitReader::new(&bytes[4..]);
    let number = r.read_utf8_u64().map_err(|_| bad("flac coded number", 4))?;
    let mut pos = 4 + r.byte_pos();
    let byte = |pos: &mut usize| -> Result<u32> {
        let v = *bytes.get(*pos).ok_or(bad("flac header eof", *pos))?;
        *pos += 1;
        Ok(u32::from(v))
    };

    let block_size = match bs_code {
        0 => return Err(bad("flac block size", 2)),
        1 => 192,
        2..=5 => 576 << (bs_code - 2),
        6 => byte(&mut pos)? + 1,
        7 => (byte(&mut pos)? << 8 | byte(&mut pos)?) + 1,
        _ => 256 << (bs_code - 8),
    };
    let sample_rate = match sr_code {
        0 => info.sample_rate,
        1 => 88_200,
        2 => 176_400,
        3 => 192_000,
        4 => 8000,
        5 => 16_000,
        6 => 22_050,
        7 => 24_000,
        8 => 32_000,
        9 => 44_100,
        10 => 48_000,
        11 => 96_000,
        12 => byte(&mut pos)? * 1000,
        13 => byte(&mut pos)? << 8 | byte(&mut pos)?,
        14 => (byte(&mut pos)? << 8 | byte(&mut pos)?) * 10,
        _ => return Err(bad("flac sample rate", 2)),
    };
    let stored_crc = *bytes.get(pos).ok_or(bad("flac header eof", pos))?;
    if crc8(&bytes[..pos]) != stored_crc {
        return Err(bad("flac header crc", pos));
    }
    let header_len = pos + 1;

    if block_size > u32::from(info.max_block) {
        return Err(bad("flac block size", 2));
    }
    if sample_rate != info.sample_rate
        || channels != info.channels
        || bits_per_sample != info.bits_per_sample
    {
        return Err(bad("flac header mismatch", 0));
    }
    let first_sample = if blocking_variable {
        if number >> 36 != 0 {
            return Err(bad("flac coded number", 4));
        }
        number
    } else {
        if number >> 31 != 0 {
            return Err(bad("flac coded number", 4));
        }
        if legacy_vbr {
            number
        } else {
            // Fixed-size streams: every frame but the last holds `max_block` samples.
            number * u64::from(info.max_block)
        }
    };
    Ok(FrameHeader {
        blocking_variable,
        block_size,
        sample_rate,
        channel_assignment,
        bits_per_sample,
        first_sample,
        header_len,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> StreamInfo {
        StreamInfo {
            min_block: 4096,
            max_block: 4096,
            min_frame: 0,
            max_frame: 0,
            sample_rate: 44_100,
            channels: 2,
            bits_per_sample: 16,
            total_samples: 0,
            md5: [0; 16],
        }
    }

    /// Appends the CRC-8 to `h`.
    fn seal(mut h: Vec<u8>) -> Vec<u8> {
        h.push(crc8(&h));
        h
    }

    fn err(r: Result<FrameHeader>) -> &'static str {
        match r {
            Err(Error::Invalid { what, .. }) => what,
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn fixed_header_basic() {
        // sync, fixed; bs code 12 (4096), rate code 9 (44100); stereo, 16 bit; frame 0.
        let h = seal(vec![0xFF, 0xF8, 0xC9, 0x18, 0x00]);
        let p = parse_frame_header(&h, &info()).unwrap();
        assert_eq!(
            p,
            FrameHeader {
                blocking_variable: false,
                block_size: 4096,
                sample_rate: 44_100,
                channel_assignment: ChannelAssignment::Independent(2),
                bits_per_sample: 16,
                first_sample: 0,
                header_len: 6,
            }
        );
    }

    #[test]
    fn trailing_bytes_are_ignored() {
        let mut h = seal(vec![0xFF, 0xF8, 0xC9, 0x18, 0x05]);
        h.extend([0xAA; 20]);
        let p = parse_frame_header(&h, &info()).unwrap();
        assert_eq!(p.header_len, 6);
        // Fixed streams: frame number times the STREAMINFO block size.
        assert_eq!(p.first_sample, 5 * 4096);
    }

    #[test]
    fn variable_header_uses_sample_number() {
        // 0xF9 = variable; bs code 1 (192); coded number 100000 (4-byte utf8).
        let n = 100_000u32;
        let utf8 = [
            0xF0 | (n >> 18) as u8,
            0x80 | ((n >> 12) & 0x3F) as u8,
            0x80 | ((n >> 6) & 0x3F) as u8,
            0x80 | (n & 0x3F) as u8,
        ];
        let mut v = vec![0xFF, 0xF9, 0x19, 0x18];
        v.extend(utf8);
        let p = parse_frame_header(&seal(v), &info()).unwrap();
        assert!(p.blocking_variable);
        assert_eq!(p.block_size, 192);
        assert_eq!(p.first_sample, 100_000);
        assert_eq!(p.header_len, 9);
    }

    #[test]
    fn block_size_codes() {
        let cases = [
            (1u8, 192u32),
            (2, 576),
            (3, 1152),
            (4, 2304),
            (5, 4608),
            (8, 256),
            (9, 512),
            (10, 1024),
            (11, 2048),
            (12, 4096),
            (13, 8192),
            (14, 16_384),
            (15, 32_768),
        ];
        let mut i = info();
        i.max_block = 32_768;
        i.min_block = 192;
        for (code, size) in cases {
            let h = seal(vec![0xFF, 0xF9, code << 4 | 9, 0x18, 0x00]);
            assert_eq!(
                parse_frame_header(&h, &i).unwrap().block_size,
                size,
                "{code}"
            );
        }
        // 8-bit and 16-bit "get from end of header" (value plus one).
        let h = seal(vec![0xFF, 0xF9, 0x69, 0x18, 0x00, 99]);
        let p = parse_frame_header(&h, &i).unwrap();
        assert_eq!((p.block_size, p.header_len), (100, 7));
        let h = seal(vec![0xFF, 0xF9, 0x79, 0x18, 0x00, 0x0F, 0xFF]);
        let p = parse_frame_header(&h, &i).unwrap();
        assert_eq!((p.block_size, p.header_len), (4096, 8));
        // Reserved code 0.
        let h = seal(vec![0xFF, 0xF9, 0x09, 0x18, 0x00]);
        assert_eq!(err(parse_frame_header(&h, &i)), "flac block size");
    }

    #[test]
    fn block_size_above_streaminfo_max_is_rejected() {
        // Code 13 = 8192 > max_block 4096.
        let h = seal(vec![0xFF, 0xF8, 0xD9, 0x18, 0x00]);
        assert_eq!(err(parse_frame_header(&h, &info())), "flac block size");
    }

    #[test]
    fn sample_rate_codes() {
        let table = [
            (1u8, 88_200u32),
            (2, 176_400),
            (3, 192_000),
            (4, 8000),
            (5, 16_000),
            (6, 22_050),
            (7, 24_000),
            (8, 32_000),
            (9, 44_100),
            (10, 48_000),
            (11, 96_000),
        ];
        for (code, rate) in table {
            let mut i = info();
            i.sample_rate = rate;
            let h = seal(vec![0xFF, 0xF8, 0xC0 | code, 0x18, 0x00]);
            assert_eq!(parse_frame_header(&h, &i).unwrap().sample_rate, rate);
        }
        // 0 = from STREAMINFO.
        let h = seal(vec![0xFF, 0xF8, 0xC0, 0x18, 0x00]);
        assert_eq!(parse_frame_header(&h, &info()).unwrap().sample_rate, 44_100);
        // 12: 8-bit kHz, 13: 16-bit Hz, 14: 16-bit tens of Hz.
        let mut i = info();
        i.sample_rate = 44_000;
        let h = seal(vec![0xFF, 0xF8, 0xCC, 0x18, 0x00, 44]);
        let p = parse_frame_header(&h, &i).unwrap();
        assert_eq!((p.sample_rate, p.header_len), (44_000, 7));
        i.sample_rate = 44_101;
        let h = seal(vec![0xFF, 0xF8, 0xCD, 0x18, 0x00, 0xAC, 0x45]);
        let p = parse_frame_header(&h, &i).unwrap();
        assert_eq!((p.sample_rate, p.header_len), (44_101, 8));
        i.sample_rate = 44_110;
        let h = seal(vec![0xFF, 0xF8, 0xCE, 0x18, 0x00, 0x11, 0x3B]);
        assert_eq!(parse_frame_header(&h, &i).unwrap().sample_rate, 44_110);
        // 15 is invalid.
        let h = seal(vec![0xFF, 0xF8, 0xCF, 0x18, 0x00]);
        assert_eq!(err(parse_frame_header(&h, &info())), "flac sample rate");
    }

    #[test]
    fn rate_must_match_streaminfo() {
        let h = seal(vec![0xFF, 0xF8, 0xCA, 0x18, 0x00]); // 48000
        assert_eq!(err(parse_frame_header(&h, &info())), "flac header mismatch");
    }

    #[test]
    fn channel_assignments() {
        let mut i = info();
        for n in 0..8u8 {
            i.channels = n + 1;
            let h = seal(vec![0xFF, 0xF8, 0xC9, n << 4 | 0x08, 0x00]);
            let p = parse_frame_header(&h, &i).unwrap();
            assert_eq!(p.channel_assignment, ChannelAssignment::Independent(n + 1));
        }
        let mut i = info();
        i.channels = 2;
        for (code, want) in [
            (8u8, ChannelAssignment::LeftSide),
            (9, ChannelAssignment::RightSide),
            (10, ChannelAssignment::MidSide),
        ] {
            let h = seal(vec![0xFF, 0xF8, 0xC9, code << 4 | 0x08, 0x00]);
            assert_eq!(parse_frame_header(&h, &i).unwrap().channel_assignment, want);
        }
        for code in 11..16u8 {
            let h = seal(vec![0xFF, 0xF8, 0xC9, code << 4 | 0x08, 0x00]);
            assert_eq!(err(parse_frame_header(&h, &i)), "flac channel assignment");
        }
        // Channel count must match STREAMINFO (mono frame in a stereo stream).
        let h = seal(vec![0xFF, 0xF8, 0xC9, 0x08, 0x00]);
        assert_eq!(err(parse_frame_header(&h, &i)), "flac header mismatch");
    }

    #[test]
    fn bit_depth_codes() {
        let table = [(1u8, 8u8), (2, 12), (4, 16), (5, 20), (6, 24), (7, 32)];
        for (code, bps) in table {
            let mut i = info();
            i.bits_per_sample = bps;
            let h = seal(vec![0xFF, 0xF8, 0xC9, 0x10 | code << 1, 0x00]);
            assert_eq!(parse_frame_header(&h, &i).unwrap().bits_per_sample, bps);
        }
        // 0 = from STREAMINFO.
        let h = seal(vec![0xFF, 0xF8, 0xC9, 0x10, 0x00]);
        assert_eq!(parse_frame_header(&h, &info()).unwrap().bits_per_sample, 16);
        // 3 is reserved.
        let h = seal(vec![0xFF, 0xF8, 0xC9, 0x16, 0x00]);
        assert_eq!(err(parse_frame_header(&h, &info())), "flac bit depth");
        // Mismatch.
        let h = seal(vec![0xFF, 0xF8, 0xC9, 0x1C, 0x00]); // 24
        assert_eq!(err(parse_frame_header(&h, &info())), "flac header mismatch");
    }

    #[test]
    fn reserved_and_sync_bits() {
        let good = seal(vec![0xFF, 0xF8, 0xC9, 0x18, 0x00]);
        assert!(parse_frame_header(&good, &info()).is_ok());
        for bad in [
            vec![0xFE, 0xF8, 0xC9, 0x18, 0x00],
            vec![0xFF, 0xF0, 0xC9, 0x18, 0x00],
        ] {
            assert_eq!(err(parse_frame_header(&seal(bad), &info())), "flac sync");
        }
        for bad in [
            vec![0xFF, 0xFA, 0xC9, 0x18, 0x00], // reserved bit after the sync code
            vec![0xFF, 0xF8, 0xC9, 0x19, 0x00], // reserved bit in the fourth byte
        ] {
            assert_eq!(
                err(parse_frame_header(&seal(bad), &info())),
                "flac reserved"
            );
        }
    }

    #[test]
    fn crc8_mismatch_and_truncation() {
        let mut h = seal(vec![0xFF, 0xF8, 0xC9, 0x18, 0x00]);
        *h.last_mut().unwrap() ^= 1;
        assert_eq!(err(parse_frame_header(&h, &info())), "flac header crc");
        let h = seal(vec![0xFF, 0xF8, 0xC9, 0x18, 0x00]);
        for n in 0..h.len() {
            assert!(parse_frame_header(&h[..n], &info()).is_err(), "{n}");
        }
    }

    #[test]
    fn bad_coded_number() {
        // 0xFF lead byte is not a valid utf8-style number.
        let h = seal(vec![0xFF, 0xF8, 0xC9, 0x18, 0xFF]);
        assert!(parse_frame_header(&h, &info()).is_err());
        // Fixed frame number above 31 bits.
        let h = seal(vec![
            0xFF, 0xF8, 0xC9, 0x18, 0xFE, 0x82, 0x80, 0x80, 0x80, 0x80, 0x80,
        ]);
        assert!(parse_frame_header(&h, &info()).is_err());
    }
}
