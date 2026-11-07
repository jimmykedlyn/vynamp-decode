//! A tiny Layer III encoder for the tests: it writes side information, scalefactors and
//! Huffman data the way the standard describes them, so that the decoder can be checked
//! against hand-built bitstreams.

use super::huffman::book;
use super::sideinfo::{GranuleInfo, SideInfo};
use vynamp_decode_mpa::{ChannelMode, FrameHeader, MpegVersion, parse_header};

/// MSB-first bit writer that remembers its length in bits.
#[derive(Default, Clone)]
pub(super) struct W {
    pub bytes: Vec<u8>,
    pub bits: usize,
}

impl W {
    pub fn put(&mut self, value: u32, n: u32) {
        for i in (0..n).rev() {
            if self.bits.is_multiple_of(8) {
                self.bytes.push(0);
            }
            let bit = ((value >> i) & 1) as u8;
            let last = self.bytes.len() - 1;
            self.bytes[last] |= bit << (7 - self.bits % 8);
            self.bits += 1;
        }
    }
}

/// Frame header: `version` 3 (MPEG-1), 2 (MPEG-2) or 0 (MPEG-2.5).
pub(super) fn header(
    version: u8,
    bitrate_idx: u8,
    rate_idx: u8,
    mode: u8,
    mode_ext: u8,
    crc: bool,
) -> FrameHeader {
    let b = [
        0xFF,
        0xE0 | (version << 3) | (1 << 1) | u8::from(!crc),
        (bitrate_idx << 4) | (rate_idx << 2),
        (mode << 6) | (mode_ext << 4),
    ];
    let h = parse_header(b).expect("valid test header");
    assert_eq!(h.layer, 3);
    h
}

/// The four header bytes of a parsed test header (re-derived from its fields).
pub(super) fn header_bytes(h: &FrameHeader) -> [u8; 4] {
    let version = match h.version {
        MpegVersion::V1 => 3,
        MpegVersion::V2 => 2,
        MpegVersion::V25 => 0,
    };
    let probe = |b2: u8| parse_header([0xFF, 0xE0 | (version << 3) | 3, b2, 0]);
    let bitrate_idx = (1..=14u8)
        .find(|&i| probe(i << 4).is_some_and(|c| c.bitrate_kbps == h.bitrate_kbps))
        .expect("bitrate");
    let rate_idx = (0..3u8)
        .find(|&i| probe(0x10 | (i << 2)).is_some_and(|c| c.sample_rate == h.sample_rate))
        .expect("rate");
    let mode = match h.channel_mode {
        ChannelMode::Stereo => 0,
        ChannelMode::JointStereo => 1,
        ChannelMode::DualChannel => 2,
        ChannelMode::Mono => 3,
    };
    [
        0xFF,
        0xE0 | (version << 3) | (1 << 1) | u8::from(!h.crc),
        (bitrate_idx << 4) | (rate_idx << 2) | (u8::from(h.padding) << 1),
        (mode << 6) | (h.mode_ext << 4),
    ]
}

/// MPEG-1 44.1 kHz 128 kbit/s (417 byte frames).
pub(super) fn v1_stereo() -> FrameHeader {
    header(3, 9, 0, 0, 0, false)
}

/// MPEG-1 44.1 kHz 128 kbit/s mono.
pub(super) fn v1_mono() -> FrameHeader {
    header(3, 9, 0, 3, 0, false)
}

/// MPEG-2 22.05 kHz 64 kbit/s stereo (208 byte frames).
pub(super) fn v2_stereo() -> FrameHeader {
    header(2, 8, 0, 0, 0, false)
}

/// MPEG-2 22.05 kHz 64 kbit/s mono.
pub(super) fn v2_mono() -> FrameHeader {
    header(2, 8, 0, 3, 0, false)
}

/// MPEG-2 22.05 kHz joint stereo with intensity stereo on.
pub(super) fn v2_intensity() -> FrameHeader {
    header(2, 8, 0, 1, 1, false)
}

/// MPEG-2.5 8 kHz 64 kbit/s mono.
pub(super) fn v25_mono() -> FrameHeader {
    header(0, 8, 2, 3, 0, false)
}

/// Bytes available for main data in a frame.
pub(super) fn capacity(h: &FrameHeader) -> usize {
    h.frame_len - 4 - if h.crc { 2 } else { 0 } - h.side_info_len()
}

/// Writes the side information of a frame.
pub(super) fn side_info_bytes(si: &SideInfo, h: &FrameHeader) -> Vec<u8> {
    let v1 = h.version == MpegVersion::V1;
    let channels = usize::from(h.channels());
    let mut w = W::default();
    if v1 {
        w.put(u32::from(si.main_data_begin), 9);
        w.put(0, if channels == 1 { 5 } else { 3 });
        for ch in 0..channels {
            w.put(u32::from(si.scfsi[ch]), 4);
        }
    } else {
        w.put(u32::from(si.main_data_begin), 8);
        w.put(0, if channels == 1 { 1 } else { 2 });
    }
    for gr in 0..if v1 { 2 } else { 1 } {
        for ch in 0..channels {
            let g = &si.granules[gr][ch];
            w.put(u32::from(g.part2_3_length), 12);
            w.put(u32::from(g.big_values), 9);
            w.put(u32::from(g.global_gain), 8);
            w.put(u32::from(g.scalefac_compress), if v1 { 4 } else { 9 });
            w.put(u32::from(g.window_switching), 1);
            if g.window_switching {
                w.put(u32::from(g.block_type), 2);
                w.put(u32::from(g.mixed_block), 1);
                w.put(u32::from(g.table_select[0]), 5);
                w.put(u32::from(g.table_select[1]), 5);
                for s in g.subblock_gain {
                    w.put(u32::from(s), 3);
                }
            } else {
                for t in g.table_select {
                    w.put(u32::from(t), 5);
                }
                w.put(u32::from(g.region0_count), 4);
                w.put(u32::from(g.region1_count), 3);
            }
            if v1 {
                w.put(u32::from(g.preflag), 1);
            }
            w.put(u32::from(g.scalefac_scale), 1);
            w.put(u32::from(g.count1_table), 1);
        }
    }
    assert_eq!(w.bits.div_ceil(8), h.side_info_len());
    w.bytes
}

/// Assembles a whole frame: header, CRC if the header has one, side information and `main`
/// (zero padded to the frame length).
pub(super) fn frame_bytes(h: &FrameHeader, si: &SideInfo, main: &[u8]) -> Vec<u8> {
    let side = side_info_bytes(si, h);
    let mut f = header_bytes(h).to_vec();
    if h.crc {
        let mut c = vynamp_decode_core::crc16_mpeg(&f[2..4]);
        c = vynamp_decode_core::crc16_update(c, &side);
        f.extend_from_slice(&c.to_be_bytes());
    }
    f.extend_from_slice(&side);
    assert!(
        main.len() <= capacity(h),
        "{} > {}",
        main.len(),
        capacity(h)
    );
    f.extend_from_slice(main);
    f.resize(h.frame_len, 0);
    f
}

/// Writes one pair of signed values with table `table`.
pub(super) fn put_pair(w: &mut W, table: u8, x: i32, y: i32) {
    let b = book(table).expect("usable table");
    let (ax, ay) = (x.unsigned_abs(), y.unsigned_abs());
    let esc = |v: u32| if b.linbits > 0 { v.min(15) } else { v };
    let (cx, cy) = (esc(ax), esc(ay));
    assert!(
        (cx as usize) < b.ysize && (cy as usize) < b.ysize,
        "value out of table {table}"
    );
    let (len, code) = b.codes[cx as usize * b.ysize + cy as usize];
    w.put(code, u32::from(len));
    for (v, c, s) in [(ax, cx, x), (ay, cy, y)] {
        if b.linbits > 0 && c == 15 {
            assert!(v - 15 < (1 << b.linbits), "escape too large");
            w.put(v - 15, b.linbits);
        }
        if v != 0 {
            w.put(u32::from(s < 0), 1);
        }
    }
}

/// Writes a count1 quadruple of values in -1..=1.
pub(super) fn put_quad(w: &mut W, table_b: bool, q: [i32; 4]) {
    let idx = q.iter().fold(0usize, |a, &v| a * 2 + usize::from(v != 0));
    let (len, code) = if table_b {
        super::codes::COUNT1_B[idx]
    } else {
        super::codes::COUNT1_A[idx]
    };
    w.put(code, u32::from(len));
    for v in q {
        if v != 0 {
            w.put(u32::from(v < 0), 1);
        }
    }
}

/// A granule to encode.
#[derive(Clone)]
pub(super) struct GrSpec {
    pub info: GranuleInfo,
    /// Scalefactors as `(value, bits)` in transmission order.
    pub sf: Vec<(u32, u32)>,
    pub xr: [i16; 576],
    /// Number of count1 quadruples.
    pub quads: usize,
    /// First line of region 1 and of region 2.
    pub regions: [usize; 2],
}

impl GrSpec {
    pub fn new(info: GranuleInfo) -> Self {
        Self {
            info,
            sf: Vec::new(),
            xr: [0; 576],
            quads: 0,
            regions: [576, 576],
        }
    }
}

/// Writes the scalefactors and Huffman data of a granule and sets its `part2_3_length`.
/// Returns the number of scalefactor bits.
pub(super) fn encode_granule(w: &mut W, g: &mut GrSpec) -> usize {
    let start = w.bits;
    for &(v, n) in &g.sf {
        w.put(v, n);
    }
    let part2 = w.bits - start;
    let bv2 = usize::from(g.info.big_values) * 2;
    for i in (0..bv2).step_by(2) {
        let region = if i < g.regions[0] {
            0
        } else if i < g.regions[1] {
            1
        } else {
            2
        };
        let t = g.info.table_select[region];
        if t == 0 {
            assert_eq!((g.xr[i], g.xr[i + 1]), (0, 0));
            continue;
        }
        put_pair(w, t, i32::from(g.xr[i]), i32::from(g.xr[i + 1]));
    }
    for q in 0..g.quads {
        let b = bv2 + 4 * q;
        let vals = [0, 1, 2, 3].map(|k| i32::from(g.xr[b + k]));
        put_quad(w, g.info.count1_table, vals);
    }
    g.info.part2_3_length = (w.bits - start) as u16;
    part2
}
