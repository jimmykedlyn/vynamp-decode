//! Scalefactor band layouts and scalefactor reading (ISO/IEC 11172-3 section 2.4.2.7,
//! ISO/IEC 13818-3).

use super::bits::Bits;
use super::sideinfo::GranuleInfo;
use super::tables::*;
use vynamp_decode_mpa::MpegVersion;

/// The scalefactor bands of a granule, in the order the lines are transmitted.
///
/// Block types 0, 1 and 3 use the 22 long bands. Block type 2 uses the 13 short bands, each
/// sent as three entries (one per window); a mixed block sends the first `n_long` entries as
/// long bands covering lines 0 to 35 (the two lowest subbands) and continues with the short
/// bands from band 3 on.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Layout {
    /// Width in lines of each entry. Long bands first, then (short and mixed blocks) one entry
    /// per band and window: window `w` of the `b`-th short entry group is entry
    /// `n_long + 3 * b + w` (`b` counts from short band 3 in a mixed block).
    pub widths: &'static [u8],
    /// Number of long band entries: all 22 for long blocks; in a mixed block 8 entries in
    /// MPEG-1 and 6 in MPEG-2/2.5, covering lines 0 to 35, except at 8 kHz where the six
    /// 12-line long bands cover 72 lines (as FFmpeg does; the IMDCT and alias reduction still
    /// treat only the two lowest subbands as long).
    pub n_long: usize,
    /// Number of short band entries (a multiple of 3).
    pub n_short: usize,
    /// Entries that carry a scalefactor (21 long, 36 short, 35 or 33 mixed); the rest (the
    /// last band of each kind) have none and read as 0.
    pub n_sf: usize,
    /// Lines of region 0 of a mixed block (0 for other layouts): the 36 long lines, or 72 at
    /// 8 kHz, for every version (see [`super::huffman::region_bounds`]).
    pub mixed_region0: usize,
}

impl Layout {
    /// The layout of a granule. `row` comes from [`sfb_row`]. Block types 0, 1 and 3 use the
    /// long bands; block type 2 the short bands, or the mixed layout (the two lowest
    /// subbands long) when `mixed_block` is set.
    pub(crate) fn new(row: usize, version: MpegVersion, gi: &GranuleInfo) -> Self {
        let row = row.min(7);
        if gi.window_switching && gi.block_type == 2 {
            if gi.mixed_block {
                let widths = SFB_MIXED[row];
                let v1 = version == MpegVersion::V1;
                let n_long = match (v1, row) {
                    (true, _) => 8,
                    (false, _) => 6,
                };
                Self {
                    widths,
                    n_long,
                    n_short: widths.len() - n_long,
                    n_sf: if v1 { 35 } else { 33 },
                    mixed_region0: if row == 1 { 72 } else { 36 },
                }
            } else {
                Self {
                    widths: &SFB_SHORT[row],
                    n_long: 0,
                    n_short: SFB_SHORT[row].len(),
                    n_sf: 36,
                    mixed_region0: 0,
                }
            }
        } else {
            Self {
                widths: &SFB_LONG[row],
                n_long: SFB_LONG[row].len(),
                n_short: 0,
                n_sf: 21,
                mixed_region0: 0,
            }
        }
    }

    /// Subbands (of 18 lines) processed as long blocks by the alias reduction and the IMDCT:
    /// all 32 for long blocks, none for short blocks, and for a mixed block the subbands that
    /// the long bands cover (two; four at 8 kHz).
    pub(crate) fn long_subbands(&self) -> usize {
        if self.n_short == 0 {
            32
        } else {
            self.bound(self.n_long) / 18
        }
    }

    /// Line index where the first `entries` entries end (576 for all of them or more).
    pub(crate) fn bound(&self, entries: usize) -> usize {
        self.widths
            .iter()
            .take(entries)
            .map(|&w| usize::from(w))
            .sum()
    }
}

/// What the scalefactor reader needs to know about a granule.
pub(crate) struct Params<'a> {
    pub version: MpegVersion,
    pub gi: &'a GranuleInfo,
    pub layout: &'a Layout,
    /// `scfsi` of the channel for this granule (0 for granule 0 and for short blocks).
    pub scfsi: u8,
    /// The right channel of an intensity stereo frame.
    pub intensity_right: bool,
}

/// Result of reading scalefactors.
pub(crate) struct Scalefactors {
    /// Raw values by layout entry (0 where none is transmitted).
    pub values: [u8; 40],
    /// Bit `i` set: entry `i` of an intensity right channel (MPEG-2/2.5) holds the all-ones
    /// value of its width, which marks an illegal intensity position. A partition of width 0
    /// (nothing is sent; `2^0 - 1 = 0` is its only value) is illegal throughout.
    pub illegal: u64,
    /// The `preflag` in effect.
    pub preflag: bool,
}

/// Reads the scalefactors of a granule at the cursor. `prev` holds the values of granule 0 of
/// the same channel (the source of `scfsi` sharing).
///
/// The caller checks the cursor against the end of the granule afterwards. Reading past the
/// data yields zeros.
///
/// MPEG-1 sends the `slen1`-bit scalefactors of the first band group and the `slen2`-bit ones
/// of the second (long blocks: bands 0 to 10 and 11 to 20; short: bands 0 to 5 and 6 to 11),
/// in four partitions that follow the `scfsi` groups. MPEG-2/2.5 derives four widths and four
/// partition sizes from `scalefac_compress`; the right channel of an intensity stereo frame
/// uses its own ranges on half the value, and the `preflag` is on for the third ordinary range
/// (`scalefac_compress >= 500`), never for an intensity right channel.
pub(crate) fn read(bits: &mut Bits, p: &Params, prev: &[u8; 40]) -> Scalefactors {
    let kind = if p.layout.n_short == 0 {
        0
    } else if p.layout.n_long == 0 {
        2
    } else {
        1
    };
    let compress = u32::from(p.gi.scalefac_compress);
    let mut out = Scalefactors {
        values: [0; 40],
        illegal: 0,
        preflag: p.gi.preflag,
    };
    let (slens, parts, scfsi): ([u32; 4], [u8; 4], u8) = if p.version == MpegVersion::V1 {
        let (s1, s2) = SLEN_V1[(compress & 15) as usize];
        let (s1, s2) = (u32::from(s1), u32::from(s2));
        // Sharing is for long blocks of granule 1 only.
        let scfsi = if kind == 0 { p.scfsi } else { 0 };
        ([s1, s1, s2, s2], V1_PARTITIONS[kind], scfsi)
    } else {
        let (slens, set) = if p.intensity_right {
            let c = compress >> 1;
            if c < 180 {
                ([c / 36, (c % 36) / 6, (c % 36) % 6, 0], 3)
            } else if c < 244 {
                let c = c - 180;
                ([(c & 63) >> 4, (c & 15) >> 2, c & 3, 0], 4)
            } else {
                let c = c - 244;
                ([c / 3, c % 3, 0, 0], 5)
            }
        } else if compress < 400 {
            (
                [
                    (compress >> 4) / 5,
                    (compress >> 4) % 5,
                    (compress & 15) >> 2,
                    compress & 3,
                ],
                0,
            )
        } else if compress < 500 {
            let c = compress - 400;
            ([(c >> 2) / 5, (c >> 2) % 5, c & 3, 0], 1)
        } else {
            let c = compress - 500;
            ([c / 3, c % 3, 0, 0], 2)
        };
        out.preflag = !p.intensity_right && compress >= 500;
        (slens, LSF_PARTITIONS[kind][set], 0)
    };
    let mut at = 0usize;
    for (i, (&count, &width)) in parts.iter().zip(&slens).enumerate() {
        let shared = scfsi & (8 >> i) != 0;
        let end = (at + usize::from(count)).min(40);
        if shared {
            out.values[at..end].copy_from_slice(&prev[at..end]);
        } else if width > 0 {
            for (k, slot) in out.values[at..end].iter_mut().enumerate() {
                let v = bits.read(width);
                *slot = v as u8;
                if p.intensity_right && v == (1 << width) - 1 {
                    out.illegal |= 1 << (at + k);
                }
            }
        } else if p.intensity_right {
            // A partition of width 0 sends nothing: every position is 0 = 2^0 - 1 = `is_max`,
            // which the standard (and its reference decoder) treats as illegal.
            for k in at..end {
                out.illegal |= 1 << k;
            }
        }
        at += usize::from(count);
    }
    out
}

#[cfg(test)]
mod tests;
