//! Huffman decoding of the big value and count1 regions (ISO/IEC 11172-3 section 2.4.3.4.6).
//!
//! The code tables are in `codes.rs` as `(length, code)` per symbol. They are turned into
//! lookup tables on first use: a root table indexed by the next `ROOT_BITS` bits, whose entry
//! is either a leaf (symbol and code length) or a pointer to a second table indexed by the
//! following bits (codes longer than `ROOT_BITS`). Almost every code of audio data is
//! resolved by one lookup; none takes a bit-by-bit walk.
//!
//! Entry layout, 32 bits: a leaf is `length << 8 | y << 4 | x` (the length counts the bits
//! after the root bits when the entry sits in a second table); a pointer is
//! `1 << 31 | sub_bits << 20 | offset`. 0 marks a bit pattern that no code uses (none exists
//! in the tables of the standard, which are complete, but the decoder does not rely on it).

use super::bits::Bits;
use super::codes;
use super::scalefactors::Layout;
use super::sideinfo::GranuleInfo;
use std::sync::OnceLock;

/// Bits resolved by the first lookup.
const ROOT_BITS: u32 = 8;
const POINTER: u32 = 1 << 31;

/// Extra bits after a value of 15 by `table_select`.
const LINBITS: [u8; 32] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4, 6, 8, 10, 13, 4, 5, 6, 7, 8, 9, 11,
    13,
];

/// A pair table of the standard as the source of the lookup tables.
pub(crate) struct Book {
    /// `(length, code)` indexed `x * ysize + y`.
    pub codes: &'static [(u8, u32)],
    /// Number of `y` values (and of `x` values) of the table.
    pub ysize: usize,
    /// Extra bits that follow a value of 15.
    pub linbits: u32,
}

/// The pair table of `table_select` value `table`; `None` for 0 (no bits), 4 and 14 (unused)
/// and above 31.
pub(crate) fn book(table: u8) -> Option<Book> {
    let (codes, ysize): (&'static [(u8, u32)], usize) = match table {
        1 => (&codes::T1, 2),
        2 => (&codes::T2, 3),
        3 => (&codes::T3, 3),
        5 => (&codes::T5, 4),
        6 => (&codes::T6, 4),
        7 => (&codes::T7, 6),
        8 => (&codes::T8, 6),
        9 => (&codes::T9, 6),
        10 => (&codes::T10, 8),
        11 => (&codes::T11, 8),
        12 => (&codes::T12, 8),
        13 => (&codes::T13, 16),
        15 => (&codes::T15, 16),
        16..=23 => (&codes::T16, 16),
        24..=31 => (&codes::T24, 16),
        _ => return None,
    };
    Some(Book {
        codes,
        ysize,
        linbits: u32::from(LINBITS[usize::from(table)]),
    })
}

/// Index of the lookup table that serves `table` (tables 16 to 23 and 24 to 31 share one).
fn lookup_id(table: u8) -> Option<usize> {
    Some(match table {
        1..=3 => usize::from(table) - 1,
        5..=13 => usize::from(table) - 2,
        15 => 12,
        16..=23 => 13,
        24..=31 => 14,
        _ => return None,
    })
}

/// A representative `table_select` value for each lookup table, in `lookup_id` order.
const LOOKUP_TABLES: [u8; 15] = [1, 2, 3, 5, 6, 7, 8, 9, 10, 11, 12, 13, 15, 16, 24];

struct Tables {
    pairs: Vec<Vec<u32>>,
    /// Count1 tables A and B: entries of `length << 4 | v << 3 | w << 2 | x << 1 | y`
    /// indexed by the next 6 bits.
    count1: [[u32; 64]; 2],
}

fn build_pair_table(codes: &[(u8, u32)], ysize: usize) -> Vec<u32> {
    let leaf = |len: u32, i: usize| len << 8 | ((i % ysize) as u32) << 4 | (i / ysize) as u32;
    let mut entries = vec![0u32; 1 << ROOT_BITS];
    // Width of the second table behind each root prefix.
    let mut sub_bits = vec![0u32; 1 << ROOT_BITS];
    for &(len, code) in codes {
        let len = u32::from(len);
        if len > ROOT_BITS {
            let prefix = (code >> (len - ROOT_BITS)) as usize;
            sub_bits[prefix] = sub_bits[prefix].max(len - ROOT_BITS);
        }
    }
    for (prefix, &sb) in sub_bits.iter().enumerate() {
        if sb > 0 {
            entries[prefix] = POINTER | sb << 20 | entries.len() as u32;
            entries.resize(entries.len() + (1 << sb), 0);
        }
    }
    for (i, &(len, code)) in codes.iter().enumerate() {
        let len = u32::from(len);
        if len <= ROOT_BITS {
            let first = (code << (ROOT_BITS - len)) as usize;
            for e in &mut entries[first..first + (1 << (ROOT_BITS - len))] {
                *e = leaf(len, i);
            }
        } else {
            let rest = len - ROOT_BITS;
            let prefix = (code >> rest) as usize;
            let sb = sub_bits[prefix];
            let base = (entries[prefix] & 0xF_FFFF) as usize;
            let low = code & ((1 << rest) - 1);
            let first = base + (low << (sb - rest)) as usize;
            for e in &mut entries[first..first + (1 << (sb - rest))] {
                *e = leaf(rest, i);
            }
        }
    }
    entries
}

fn build_count1(codes: &[(u8, u32); 16]) -> [u32; 64] {
    let mut t = [0u32; 64];
    for (quad, &(len, code)) in codes.iter().enumerate() {
        let len = u32::from(len);
        let first = (code << (6 - len)) as usize;
        for e in &mut t[first..first + (1 << (6 - len))] {
            *e = len << 4 | quad as u32;
        }
    }
    t
}

fn tables() -> &'static Tables {
    static TABLES: OnceLock<Tables> = OnceLock::new();
    TABLES.get_or_init(|| Tables {
        pairs: LOOKUP_TABLES
            .iter()
            .map(|&t| {
                let b = book(t).expect("table in the list");
                build_pair_table(b.codes, b.ysize)
            })
            .collect(),
        count1: [
            build_count1(&codes::COUNT1_A),
            build_count1(&codes::COUNT1_B),
        ],
    })
}

/// Outcome of decoding the spectrum of a granule.
pub(crate) struct Spectrum {
    /// Lines up to the zero tail (`2 * big_values + 4 * count1 quadruples`).
    pub used_lines: usize,
}

/// First line of region 1 and of region 2.
///
/// Without window switching region 0 has `region0_count + 1` scalefactor bands and region 1
/// `region1_count + 1`; a count that reaches past the last band ends at line 576. With window
/// switching the side information fixes `region0_count` (8 for short blocks, 7 otherwise) and
/// region 1 runs to the end of the big values. Region 0 is then that many entries of the
/// layout's width list, which for a short block are the three windows of the first three
/// bands: 36 lines at every rate except 8 kHz (72), and for a start or stop block eight long
/// bands. A mixed block's region 0 is the 36 long lines (72 at 8 kHz) in every version
/// ([`Layout::mixed_region0`]): in MPEG-1 that is its eight long bands; in MPEG-2/2.5, where
/// eight entries of the width list would reach 48 (64 at 8 kHz) as `minimp3` counts them, it
/// follows FFmpeg, the oracle of this crate (see `docs/sources/mpeg-audio.md`).
pub(crate) fn region_bounds(gi: &GranuleInfo, layout: &Layout) -> [usize; 2] {
    if gi.window_switching {
        if gi.block_type == 2 && gi.mixed_block {
            return [layout.mixed_region0, 576];
        }
        return [layout.bound(usize::from(gi.region0_count) + 1), 576];
    }
    let r0 = usize::from(gi.region0_count) + 1;
    let r1 = r0 + usize::from(gi.region1_count) + 1;
    [layout.bound(r0), layout.bound(r1)]
}

/// Decodes one code of a pair table and its escapes and signs. `None` for a bit pattern that
/// no code uses.
#[inline(always)]
fn decode_pair(entries: &[u32], linbits: u32, bits: &mut Bits) -> Option<(i16, i16)> {
    let word = bits.peek32();
    let mut e = *entries.get((word >> (32 - ROOT_BITS)) as usize)?;
    let mut used = 0;
    if e & POINTER != 0 {
        let sub = (e >> 20) & 15;
        let at = (e & 0xF_FFFF) as usize + ((word << ROOT_BITS) >> (32 - sub)) as usize;
        used = ROOT_BITS;
        e = *entries.get(at)?;
    }
    if e == 0 {
        return None;
    }
    bits.skip((used + ((e >> 8) & 0xFF)) as usize);
    let mut x = (e & 15) as i32;
    let mut y = ((e >> 4) & 15) as i32;
    if linbits > 0 && x == 15 {
        x += bits.read(linbits) as i32;
    }
    if x != 0 && bits.read(1) == 1 {
        x = -x;
    }
    if linbits > 0 && y == 15 {
        y += bits.read(linbits) as i32;
    }
    if y != 0 && bits.read(1) == 1 {
        y = -y;
    }
    Some((x as i16, y as i16))
}

/// Decodes big values and count1 into `xr` (zeroed by the caller) from the cursor to `end` (a
/// bit position). Returns the bit position where the failure was noticed on error: an
/// unusable table or code, or big values that run past `end`.
///
/// The count1 region has no length of its own: quadruples are decoded while bits remain
/// before `end` and lines remain. A quadruple that would end past `end` is padding that the
/// encoder counted into `part2_3_length`; it is dropped.
pub(crate) fn decode_spectrum(
    bits: &mut Bits,
    end: usize,
    gi: &GranuleInfo,
    layout: &Layout,
    xr: &mut [i16; 576],
) -> Result<Spectrum, usize> {
    let t = tables();
    let bv2 = (usize::from(gi.big_values) * 2).min(576);
    let [r1, r2] = region_bounds(gi, layout);
    let stops = [r1.min(bv2), r2.min(bv2), bv2];
    let mut line = 0;
    for (region, &stop) in stops.iter().enumerate() {
        if stop <= line {
            continue;
        }
        let table = gi.table_select[region];
        if table == 0 {
            // All zeros, no bits.
            line = stop;
            continue;
        }
        let Some(id) = lookup_id(table) else {
            return Err(bits.pos());
        };
        let entries = &t.pairs[id][..];
        let linbits = book(table).map_or(0, |b| b.linbits);
        while line < stop {
            let Some((x, y)) = decode_pair(entries, linbits, bits) else {
                return Err(bits.pos());
            };
            xr[line] = x;
            xr[line + 1] = y;
            line += 2;
        }
    }
    if bits.pos() > end {
        return Err(bits.pos());
    }
    let count1 = &t.count1[usize::from(gi.count1_table)];
    while line + 4 <= 576 && bits.pos() < end {
        let start = bits.pos();
        let e = count1[(bits.peek32() >> 26) as usize];
        if e == 0 {
            return Err(start);
        }
        bits.skip((e >> 4) as usize);
        let mut quad = [0i16; 4];
        for (k, q) in quad.iter_mut().enumerate() {
            if e >> (3 - k) & 1 != 0 {
                *q = if bits.read(1) == 1 { -1 } else { 1 };
            }
        }
        if bits.pos() > end {
            bits.set_pos(start);
            break;
        }
        xr[line..line + 4].copy_from_slice(&quad);
        line += 4;
    }
    Ok(Spectrum { used_lines: line })
}

#[cfg(test)]
mod tests;
