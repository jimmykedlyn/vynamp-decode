//! Layer III side information (ISO/IEC 11172-3 section 2.4.1.7, ISO/IEC 13818-3).

use vynamp_decode_core::{BitReader, Error, Result};
use vynamp_decode_mpa::{FrameHeader, MpegVersion};

/// Side information of one granule of one channel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct GranuleInfo {
    /// Bits of scalefactors and Huffman data of this granule in the main data.
    pub part2_3_length: u16,
    /// Number of line pairs coded with the big value tables (0 to 288).
    pub big_values: u16,
    /// Quantizer step: the lines are scaled by `2^((global_gain - 210) / 4)`.
    pub global_gain: u8,
    /// Selects the widths of the scalefactors (4 bits in MPEG-1, 9 in MPEG-2/2.5).
    pub scalefac_compress: u16,
    /// The block type is not 0.
    pub window_switching: bool,
    /// 0 normal, 1 start, 2 short (three windows), 3 stop.
    pub block_type: u8,
    /// A short block whose two lowest subbands are long.
    pub mixed_block: bool,
    /// Huffman table of each of the three regions.
    pub table_select: [u8; 3],
    /// Gain of each window of a short block, in steps of 8 (`2^(-2 * subblock_gain)`).
    pub subblock_gain: [u8; 3],
    /// Number of scalefactor bands of region 0 minus one.
    pub region0_count: u8,
    /// Number of scalefactor bands of region 1 minus one.
    pub region1_count: u8,
    /// Add `pretab` to the long band scalefactors. In MPEG-2/2.5 this is derived from
    /// `scalefac_compress` while reading the scalefactors, not parsed.
    pub preflag: bool,
    /// Scalefactor step: `2^-0.5` per unit (false) or `2^-1` (true).
    pub scalefac_scale: bool,
    /// Count1 region uses table B (true) or table A (false).
    pub count1_table: bool,
}

/// The side information of a frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SideInfo {
    /// Bytes back from the first main data byte of this frame where its data begins.
    pub main_data_begin: u16,
    /// MPEG-1 scalefactor sharing flags of each channel: bit 3 is band group 0 (bands 0 to 5),
    /// bit 0 group 3 (bands 16 to 20); a set bit means granule 1 reuses the scalefactors of
    /// granule 0 for that group. Always 0 in MPEG-2/2.5.
    pub scfsi: [u8; 2],
    /// By granule, then channel. MPEG-2/2.5 has one granule.
    pub granules: [[GranuleInfo; 2]; 2],
}

/// Parses the side information bytes of a frame (`header.side_info_len()` of them).
///
/// With window switching on, the standard fixes the regions: region 0 is 8 bands for short
/// blocks and 7 otherwise (so 36 lines for the usual rates), and region 1 runs to the end;
/// `region0_count` and `region1_count` are set accordingly (`region0_count + region1_count`
/// is 20, which reaches past the last band). Window switching also sends no table for
/// region 2 (`table_select[2]` is 0). The mixed flag is kept only for block type 2.
///
/// # Errors
/// `Invalid { what: "mp3 side info", offset }` for too few bytes, `big_values` above 288 and
/// window switching with block type 0 (reserved).
pub(crate) fn parse(data: &[u8], header: &FrameHeader, offset: u64) -> Result<SideInfo> {
    let bad = || Error::Invalid {
        what: "mp3 side info",
        offset,
    };
    let data = data.get(..header.side_info_len()).ok_or_else(bad)?;
    let mut r = BitReader::new(data);
    let mut rd = |n: u32| r.read_bits(n).map_err(|_| bad());
    let v1 = header.version == MpegVersion::V1;
    let channels = usize::from(header.channels());
    let mut si = SideInfo::default();
    if v1 {
        si.main_data_begin = rd(9)? as u16;
        rd(if channels == 1 { 5 } else { 3 })?;
        for ch in 0..channels {
            si.scfsi[ch] = rd(4)? as u8;
        }
    } else {
        si.main_data_begin = rd(8)? as u16;
        rd(if channels == 1 { 1 } else { 2 })?;
    }
    for gr in 0..if v1 { 2 } else { 1 } {
        for ch in 0..channels {
            let mut g = GranuleInfo {
                part2_3_length: rd(12)? as u16,
                big_values: rd(9)? as u16,
                global_gain: rd(8)? as u8,
                scalefac_compress: rd(if v1 { 4 } else { 9 })? as u16,
                ..GranuleInfo::default()
            };
            if g.big_values > 288 {
                return Err(bad());
            }
            g.window_switching = rd(1)? == 1;
            if g.window_switching {
                g.block_type = rd(2)? as u8;
                if g.block_type == 0 {
                    return Err(bad());
                }
                g.mixed_block = rd(1)? == 1 && g.block_type == 2;
                g.table_select[0] = rd(5)? as u8;
                g.table_select[1] = rd(5)? as u8;
                for s in &mut g.subblock_gain {
                    *s = rd(3)? as u8;
                }
                g.region0_count = if g.block_type == 2 && !g.mixed_block {
                    8
                } else {
                    7
                };
                g.region1_count = 20 - g.region0_count;
            } else {
                for t in &mut g.table_select {
                    *t = rd(5)? as u8;
                }
                g.region0_count = rd(4)? as u8;
                g.region1_count = rd(3)? as u8;
            }
            g.preflag = v1 && rd(1)? == 1;
            g.scalefac_scale = rd(1)? == 1;
            g.count1_table = rd(1)? == 1;
            si.granules[gr][ch] = g;
        }
    }
    Ok(si)
}

#[cfg(test)]
mod tests;
