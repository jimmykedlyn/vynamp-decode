//! Constant tables of the Layer III bitstream layer: scalefactor band widths, the scalefactor
//! partitions, `slen` tables and `pretab`.
//!
//! The scalefactor band widths, the LSF partition sizes and `slen` pairs were transcribed from
//! the public-domain (CC0) `minimp3` source (`minimp3.h`; see `docs/sources/mpeg-audio.md`).
//! The tests assert that every row adds up to 576 lines and spot-check the 44.1 kHz and 8 kHz
//! rows; the committed fixtures decode with exact bit accounting at 44.1, 48, 22.05 and 8 kHz,
//! which exercises those rows through the region boundaries and the scalefactor counts. The
//! rows for 32, 24, 16 and 11.025/12 kHz rest on the transcription alone.

use vynamp_decode_mpa::MpegVersion;

/// Widths in lines of the 22 long scalefactor bands (band 21 covers the rest up to line 576).
/// Rows by sample rate: 0 = 11.025 and 12 kHz, 1 = 8 kHz (all three MPEG-2.5), 2 = 22.05,
/// 3 = 24, 4 = 16 kHz (MPEG-2), 5 = 44.1, 6 = 48, 7 = 32 kHz (MPEG-1).
pub(super) static SFB_LONG: [[u8; 22]; 8] = [
    [
        6, 6, 6, 6, 6, 6, 8, 10, 12, 14, 16, 20, 24, 28, 32, 38, 46, 52, 60, 68, 58, 54,
    ],
    [
        12, 12, 12, 12, 12, 12, 16, 20, 24, 28, 32, 40, 48, 56, 64, 76, 90, 2, 2, 2, 2, 2,
    ],
    [
        6, 6, 6, 6, 6, 6, 8, 10, 12, 14, 16, 20, 24, 28, 32, 38, 46, 52, 60, 68, 58, 54,
    ],
    [
        6, 6, 6, 6, 6, 6, 8, 10, 12, 14, 16, 18, 22, 26, 32, 38, 46, 54, 62, 70, 76, 36,
    ],
    [
        6, 6, 6, 6, 6, 6, 8, 10, 12, 14, 16, 20, 24, 28, 32, 38, 46, 52, 60, 68, 58, 54,
    ],
    [
        4, 4, 4, 4, 4, 4, 6, 6, 8, 8, 10, 12, 16, 20, 24, 28, 34, 42, 50, 54, 76, 158,
    ],
    [
        4, 4, 4, 4, 4, 4, 6, 6, 6, 8, 10, 12, 16, 18, 22, 28, 34, 40, 46, 54, 54, 192,
    ],
    [
        4, 4, 4, 4, 4, 4, 6, 6, 8, 10, 12, 16, 20, 24, 30, 38, 46, 56, 68, 84, 102, 26,
    ],
];

/// Widths of the short scalefactor bands in lines per window, each repeated for the three
/// windows (so `3 * 13` entries, the last band 12 having no scalefactor of its own). Same row
/// order as [`SFB_LONG`]. The entries are in transmission order: the three windows of one band
/// are consecutive.
pub(super) static SFB_SHORT: [[u8; 39]; 8] = [
    [
        4, 4, 4, 4, 4, 4, 4, 4, 4, 6, 6, 6, 8, 8, 8, 10, 10, 10, 12, 12, 12, 14, 14, 14, 18, 18,
        18, 24, 24, 24, 30, 30, 30, 40, 40, 40, 18, 18, 18,
    ],
    [
        8, 8, 8, 8, 8, 8, 8, 8, 8, 12, 12, 12, 16, 16, 16, 20, 20, 20, 24, 24, 24, 28, 28, 28, 36,
        36, 36, 2, 2, 2, 2, 2, 2, 2, 2, 2, 26, 26, 26,
    ],
    [
        4, 4, 4, 4, 4, 4, 4, 4, 4, 6, 6, 6, 6, 6, 6, 8, 8, 8, 10, 10, 10, 14, 14, 14, 18, 18, 18,
        26, 26, 26, 32, 32, 32, 42, 42, 42, 18, 18, 18,
    ],
    [
        4, 4, 4, 4, 4, 4, 4, 4, 4, 6, 6, 6, 8, 8, 8, 10, 10, 10, 12, 12, 12, 14, 14, 14, 18, 18,
        18, 24, 24, 24, 32, 32, 32, 44, 44, 44, 12, 12, 12,
    ],
    [
        4, 4, 4, 4, 4, 4, 4, 4, 4, 6, 6, 6, 8, 8, 8, 10, 10, 10, 12, 12, 12, 14, 14, 14, 18, 18,
        18, 24, 24, 24, 30, 30, 30, 40, 40, 40, 18, 18, 18,
    ],
    [
        4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 6, 6, 6, 8, 8, 8, 10, 10, 10, 12, 12, 12, 14, 14, 14,
        18, 18, 18, 22, 22, 22, 30, 30, 30, 56, 56, 56,
    ],
    [
        4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 6, 6, 6, 6, 6, 6, 10, 10, 10, 12, 12, 12, 14, 14, 14,
        16, 16, 16, 20, 20, 20, 26, 26, 26, 66, 66, 66,
    ],
    [
        4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 6, 6, 6, 8, 8, 8, 12, 12, 12, 16, 16, 16, 20, 20, 20,
        26, 26, 26, 34, 34, 34, 42, 42, 42, 12, 12, 12,
    ],
];

/// Widths of the bands of a mixed block: the first 36 lines are long bands (8 bands in MPEG-1,
/// 6 in MPEG-2 and 2.5), the rest short bands from band 3 on, three windows each. Same row
/// order as [`SFB_LONG`]. The 8 kHz row follows the source and carries three more entries
/// than the scalefactors cover (the last short band has none, like band 12 above).
pub(super) static SFB_MIXED: [&[u8]; 8] = [
    &[
        6, 6, 6, 6, 6, 6, 6, 6, 6, 8, 8, 8, 10, 10, 10, 12, 12, 12, 14, 14, 14, 18, 18, 18, 24, 24,
        24, 30, 30, 30, 40, 40, 40, 18, 18, 18,
    ],
    &[
        12, 12, 12, 4, 4, 4, 8, 8, 8, 12, 12, 12, 16, 16, 16, 20, 20, 20, 24, 24, 24, 28, 28, 28,
        36, 36, 36, 2, 2, 2, 2, 2, 2, 2, 2, 2, 26, 26, 26,
    ],
    &[
        6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 8, 8, 8, 10, 10, 10, 14, 14, 14, 18, 18, 18, 26, 26,
        26, 32, 32, 32, 42, 42, 42, 18, 18, 18,
    ],
    &[
        6, 6, 6, 6, 6, 6, 6, 6, 6, 8, 8, 8, 10, 10, 10, 12, 12, 12, 14, 14, 14, 18, 18, 18, 24, 24,
        24, 32, 32, 32, 44, 44, 44, 12, 12, 12,
    ],
    &[
        6, 6, 6, 6, 6, 6, 6, 6, 6, 8, 8, 8, 10, 10, 10, 12, 12, 12, 14, 14, 14, 18, 18, 18, 24, 24,
        24, 30, 30, 30, 40, 40, 40, 18, 18, 18,
    ],
    &[
        4, 4, 4, 4, 4, 4, 6, 6, 4, 4, 4, 6, 6, 6, 8, 8, 8, 10, 10, 10, 12, 12, 12, 14, 14, 14, 18,
        18, 18, 22, 22, 22, 30, 30, 30, 56, 56, 56,
    ],
    &[
        4, 4, 4, 4, 4, 4, 6, 6, 4, 4, 4, 6, 6, 6, 6, 6, 6, 10, 10, 10, 12, 12, 12, 14, 14, 14, 16,
        16, 16, 20, 20, 20, 26, 26, 26, 66, 66, 66,
    ],
    &[
        4, 4, 4, 4, 4, 4, 6, 6, 4, 4, 4, 6, 6, 6, 8, 8, 8, 12, 12, 12, 16, 16, 16, 20, 20, 20, 26,
        26, 26, 34, 34, 34, 42, 42, 42, 12, 12, 12,
    ],
];

/// Row of the band tables for a sample rate (Hz), `None` for a rate no version defines.
pub(super) fn sfb_row(version: MpegVersion, sample_rate: u32) -> Option<usize> {
    Some(match (version, sample_rate) {
        (MpegVersion::V1, 44100) => 5,
        (MpegVersion::V1, 48000) => 6,
        (MpegVersion::V1, 32000) => 7,
        (MpegVersion::V2, 22050) => 2,
        (MpegVersion::V2, 24000) => 3,
        (MpegVersion::V2, 16000) => 4,
        (MpegVersion::V25, 11025 | 12000) => 0,
        (MpegVersion::V25, 8000) => 1,
        _ => return None,
    })
}

/// `pretab` of the standard: the preemphasis added to the scalefactors of long bands when the
/// `preflag` is set. Applied by the requantizer (the scalefactors stay raw in this module).
pub(crate) const PRETAB: [u8; 22] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 3, 3, 3, 2, 0,
];

/// MPEG-1 `scalefac_compress`: the widths `(slen1, slen2)` in bits of the scalefactors of
/// bands 0 to 10 and 11 to 20 (short blocks: bands 0 to 5 and 6 to 11).
pub(super) const SLEN_V1: [(u8, u8); 16] = [
    (0, 0),
    (0, 1),
    (0, 2),
    (0, 3),
    (3, 0),
    (1, 1),
    (1, 2),
    (1, 3),
    (2, 1),
    (2, 2),
    (2, 3),
    (3, 1),
    (3, 2),
    (3, 3),
    (4, 2),
    (4, 3),
];

/// Number of scalefactors in each of the four partitions (`nr_of_sfb`) of an MPEG-2/2.5
/// granule, by block kind (0 long, 1 mixed, 2 short) and set: sets 0 to 2 for the ordinary
/// ranges of `scalefac_compress` and 3 to 5 for the right channel of an intensity stereo frame.
/// A mixed block counts its long bands first, then three short windows per band from band 3.
pub(super) const LSF_PARTITIONS: [[[u8; 4]; 6]; 3] = [
    [
        [6, 5, 5, 5],
        [6, 5, 7, 3],
        [11, 10, 0, 0],
        [7, 7, 7, 0],
        [6, 6, 6, 3],
        [8, 8, 5, 0],
    ],
    [
        [6, 9, 9, 9],
        [6, 9, 12, 6],
        [15, 18, 0, 0],
        [6, 15, 12, 0],
        [6, 12, 9, 6],
        [6, 18, 9, 0],
    ],
    [
        [9, 9, 9, 9],
        [9, 9, 12, 6],
        [18, 18, 0, 0],
        [12, 12, 12, 0],
        [12, 9, 9, 6],
        [15, 12, 9, 0],
    ],
];

/// MPEG-1 partitions (`nr_of_sfb` of the `slen1`/`slen2` split into the four `scfsi` groups
/// for long blocks): kind 0 long, 1 mixed, 2 short.
pub(super) const V1_PARTITIONS: [[u8; 4]; 3] = [[6, 5, 5, 5], [8, 9, 6, 12], [9, 9, 6, 12]];
