//! VBR headers inside the first frame of a Layer III stream: Xing/Info with the optional
//! LAME tag, and Fraunhofer's VBRI.
//!
//! Xing/Info sits after the Layer III side information (offset 4 + side info, so 36 for
//! stereo MPEG-1):
//!
//! ```text
//! "Xing" | "Info"   4 bytes
//! flags             u32 BE: 1 frames, 2 bytes, 4 TOC, 8 quality
//! frames            u32 BE   (flag 1)    audio frames, the header frame excluded
//! bytes             u32 BE   (flag 2)
//! TOC               100 bytes (flag 4)   byte position in 1/256 of `bytes` per 1 % of time
//! quality           u32 BE   (flag 8)
//! ```
//!
//! The LAME tag follows directly (36 bytes, relative to its start `o`):
//!
//! ```text
//! o+0   9 bytes  encoder version string, e.g. "LAME3.100"
//! o+9   revision (4 bits) and VBR method (4 bits)      o+10  low-pass filter
//! o+11  replay gain: peak (4), radio (2), audiophile (2)
//! o+19  encoding flags and ATH type                    o+20  bitrate
//! o+21  encoder delay (12 bits) and padding (12 bits)  o+24  misc   o+25  MP3 gain
//! o+26  preset and surround info (2)                   o+28  music length (4)
//! o+32  music CRC (2)                                  o+34  tag CRC (2)
//! ```
//!
//! The tag CRC is CRC-16/ARC (polynomial 0x8005 reflected, i.e. 0xA001, initial value 0,
//! no final XOR) over every frame byte before the CRC field: the first 190 bytes of a
//! stereo MPEG-1 frame. This was checked on files written by LAME 3.99 and by FFmpeg.
//!
//! VBRI is at offset 36 whatever the channel count:
//!
//! ```text
//! "VBRI" | version u16 | delay u16 | quality u16 | bytes u32 | frames u32 |
//! entries u16 | scale u16 | entry size u16 | frames per entry u16 | table
//! ```
//!
//! All integers are big endian. Each table entry is `entry size` bytes: the byte size of
//! `frames per entry` frames, times `scale`.

use crate::header::parse_header;

/// Which VBR header was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VbrKind {
    /// `Xing`: variable bitrate.
    Xing,
    /// `Info`: the same layout, written for constant bitrate streams.
    Info,
    /// Fraunhofer `VBRI`.
    Vbri,
}

/// The LAME tag following a Xing/Info header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LameInfo {
    /// Samples of encoder delay (not including the decoder's 529).
    pub encoder_delay: u16,
    /// Samples of padding added at the end by the encoder.
    pub padding: u16,
    /// The tag CRC matches; the numbers are trustworthy only if it does.
    pub crc_ok: bool,
}

/// Contents of a VBR header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VbrInfo {
    /// Which header it was.
    pub kind: VbrKind,
    /// Number of audio frames (the header frame itself excluded).
    pub frames: Option<u32>,
    /// Size of the audio stream in bytes (the header frame included).
    pub bytes: Option<u32>,
    /// Xing table of contents.
    pub toc: Option<Box<[u8; 100]>>,
    /// VBRI table: the byte size of each group of `vbri_frames_per_entry` frames.
    pub vbri_table: Vec<u32>,
    /// Frames covered by each VBRI table entry (0 without a VBRI table).
    pub vbri_frames_per_entry: u32,
    /// LAME tag, if present.
    pub lame: Option<LameInfo>,
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    let s = b.get(at..at.checked_add(4)?)?;
    Some(u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}

fn be16(b: &[u8], at: usize) -> Option<u16> {
    let s = b.get(at..at.checked_add(2)?)?;
    Some(u16::from_be_bytes([s[0], s[1]]))
}

/// CRC-16/ARC as used by the LAME tag.
fn lame_crc(data: &[u8]) -> u16 {
    let mut crc = 0u16;
    for &b in data {
        crc ^= u16::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xA001
            } else {
                crc >> 1
            };
        }
    }
    crc
}

/// Parses the VBR header of `frame`, the complete first frame of a stream (or at least its
/// first bytes). Returns `None` if `frame` is not a Layer III frame or carries no header.
pub fn parse_vbr(frame: &[u8]) -> Option<VbrInfo> {
    let head: [u8; 4] = frame.get(..4)?.try_into().ok()?;
    let h = parse_header(head)?;
    if h.layer != 3 {
        return None;
    }
    // The CRC, when present, sits between the header and the side information. Some writers
    // ignore it when placing the Xing header, so both positions are tried.
    let plain = 4 + h.side_info_len();
    let offsets: &[usize] = if h.crc { &[plain + 2, plain] } else { &[plain] };
    for &off in offsets {
        match frame.get(off..off + 4) {
            Some(b"Xing") => return parse_xing(frame, off, VbrKind::Xing),
            Some(b"Info") => return parse_xing(frame, off, VbrKind::Info),
            _ => {}
        }
    }
    if frame.get(36..40) == Some(b"VBRI") {
        return parse_vbri(frame);
    }
    None
}

fn parse_xing(f: &[u8], off: usize, kind: VbrKind) -> Option<VbrInfo> {
    let flags = be32(f, off + 4)?;
    let mut pos = off + 8;
    let mut next = |len: usize| {
        let at = pos;
        pos += len;
        at
    };
    let frames = if flags & 1 != 0 {
        Some(be32(f, next(4))?)
    } else {
        None
    };
    let bytes = if flags & 2 != 0 {
        Some(be32(f, next(4))?)
    } else {
        None
    };
    let toc = if flags & 4 != 0 {
        let at = next(100);
        let t: [u8; 100] = f.get(at..at + 100)?.try_into().ok()?;
        Some(Box::new(t))
    } else {
        None
    };
    if flags & 8 != 0 {
        next(4);
    }
    let lame = parse_lame(f, pos);
    Some(VbrInfo {
        kind,
        frames,
        bytes,
        toc,
        vbri_table: Vec::new(),
        vbri_frames_per_entry: 0,
        lame,
    })
}

fn parse_lame(f: &[u8], o: usize) -> Option<LameInfo> {
    let tag = f.get(o..o.checked_add(36)?)?;
    // The version string starts with printable text ("LAME3.100", "Lavf lame", ...).
    if !tag[..4].iter().all(|&c| (0x20..0x7F).contains(&c)) {
        return None;
    }
    let encoder_delay = u16::from(tag[21]) << 4 | u16::from(tag[22] >> 4);
    let padding = u16::from(tag[22] & 0x0F) << 8 | u16::from(tag[23]);
    let stored = u16::from_be_bytes([tag[34], tag[35]]);
    // LAME checksums the frame up to the CRC field. FFmpeg's writer checksums the first 190
    // bytes with the field zeroed, which differs for layouts where the tag does not end at
    // byte 192 (MPEG-2, mono). Both are accepted.
    let upto = lame_crc(&f[..o + 34]) == stored;
    let first_190 = f.get(..190).is_some_and(|head| {
        let mut z = head.to_vec();
        if let Some(field) = z.get_mut(o + 34..o + 36) {
            field.fill(0);
        }
        lame_crc(&z) == stored
    });
    Some(LameInfo {
        encoder_delay,
        padding,
        crc_ok: upto || first_190,
    })
}

fn parse_vbri(f: &[u8]) -> Option<VbrInfo> {
    let bytes = be32(f, 46)?;
    let frames = be32(f, 50)?;
    let entries = usize::from(be16(f, 54)?);
    let scale = u32::from(be16(f, 56)?);
    let size = usize::from(be16(f, 58)?);
    let per_entry = u32::from(be16(f, 60)?);
    if !(1..=4).contains(&size) {
        return None;
    }
    let table = f.get(62..62usize.checked_add(entries.checked_mul(size)?)?)?;
    let vbri_table = table
        .chunks_exact(size)
        .map(|e| {
            let v = e.iter().fold(0u32, |a, &b| a << 8 | u32::from(b));
            v.saturating_mul(scale)
        })
        .collect();
    Some(VbrInfo {
        kind: VbrKind::Vbri,
        frames: Some(frames),
        bytes: Some(bytes),
        toc: None,
        vbri_table,
        vbri_frames_per_entry: per_entry,
        lame: None,
    })
}
