use super::super::testenc::W;
use super::*;
use vynamp_decode_mpa::MpegVersion::{V1, V2, V25};

const ROWS: [(vynamp_decode_mpa::MpegVersion, u32); 8] = [
    (V25, 11025),
    (V25, 8000),
    (V2, 22050),
    (V2, 24000),
    (V2, 16000),
    (V1, 44100),
    (V1, 48000),
    (V1, 32000),
];

fn gi(block_type: u8, mixed: bool, compress: u16) -> GranuleInfo {
    GranuleInfo {
        window_switching: block_type != 0,
        block_type,
        mixed_block: mixed,
        scalefac_compress: compress,
        ..GranuleInfo::default()
    }
}

fn layout(v: vynamp_decode_mpa::MpegVersion, rate: u32, g: &GranuleInfo) -> Layout {
    Layout::new(sfb_row(v, rate).unwrap(), v, g)
}

fn read_with(
    v: vynamp_decode_mpa::MpegVersion,
    rate: u32,
    g: &GranuleInfo,
    scfsi: u8,
    ist: bool,
    prev: &[u8; 40],
    w: &W,
) -> (Scalefactors, usize) {
    let l = layout(v, rate, g);
    let mut bits = Bits::new(&w.bytes, 0);
    let p = Params {
        version: v,
        gi: g,
        layout: &l,
        scfsi,
        intensity_right: ist,
    };
    let s = read(&mut bits, &p, prev);
    (s, bits.pos())
}

fn put_all(w: &mut W, values: &[u32], bits: &[u32]) {
    for (v, n) in values.iter().zip(bits) {
        w.put(*v, *n);
    }
}

#[test]
fn band_tables_cover_exactly_576_lines() {
    for (row, &(v, rate)) in ROWS.iter().enumerate() {
        assert_eq!(sfb_row(v, rate), Some(row));
        let long = layout(v, rate, &gi(0, false, 0));
        assert_eq!((long.widths.len(), long.n_long, long.n_short), (22, 22, 0));
        assert_eq!(
            long.widths.iter().map(|&w| usize::from(w)).sum::<usize>(),
            576
        );
        assert_eq!(long.bound(22), 576);
        assert_eq!(long.bound(99), 576);
        assert_eq!(long.bound(0), 0);
        let short = layout(v, rate, &gi(2, false, 0));
        assert_eq!(
            (short.widths.len(), short.n_long, short.n_short),
            (39, 0, 39)
        );
        assert_eq!(
            short.widths.iter().map(|&w| usize::from(w)).sum::<usize>(),
            576
        );
        let mixed = layout(v, rate, &gi(2, true, 0));
        assert_eq!(
            mixed.widths.iter().map(|&w| usize::from(w)).sum::<usize>(),
            576
        );
        // The 8 kHz row of the source starts its short bands after three long entries.
        assert_eq!(
            mixed.n_long,
            match (v, row) {
                (V1, _) => 8,
                (_, 1) => 6,
                _ => 6,
            }
        );
        assert_eq!(mixed.n_long + mixed.n_short, mixed.widths.len());
        assert_eq!(mixed.n_short % 3, 0);
        let first: usize = mixed.widths[..mixed.n_long]
            .iter()
            .map(|&w| usize::from(w))
            .sum();
        // The two lowest subbands (36 lines) are long at every rate.
        assert_eq!(first, if row == 1 { 72 } else { 36 }, "row {row}");
        assert!(mixed.n_sf <= mixed.widths.len() - 3, "row {row}");
        // Entries that carry a scalefactor: 21 long, 36 short, 35 (MPEG-1) or 33 (MPEG-2/2.5)
        // mixed.
        assert_eq!(long.n_sf, 21);
        assert_eq!(short.n_sf, 36);
        assert_eq!(mixed.n_sf, if v == V1 { 35 } else { 33 });
        // Block types 1 and 3 use the long bands.
        assert_eq!(layout(v, rate, &gi(1, false, 0)).n_long, 22);
        assert_eq!(layout(v, rate, &gi(3, false, 0)).n_long, 22);
    }
    assert_eq!(sfb_row(V1, 22050), None);
    assert_eq!(sfb_row(V2, 44100), None);
    // Spot values of the standard: 44.1 kHz long edges and short band widths.
    let l = layout(V1, 44100, &gi(0, false, 0));
    assert_eq!([l.bound(6), l.bound(8), l.bound(21)], [24, 36, 418]);
    let s = layout(V1, 44100, &gi(2, false, 0));
    assert_eq!(&s.widths[..12], &[4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4]);
    assert_eq!(&s.widths[12..18], &[6, 6, 6, 8, 8, 8]);
    // 8 kHz long edges (0 12 24 36 48 60 72 88 108 ...) as in the MPEG-2.5 tables.
    let l8 = layout(V25, 8000, &gi(0, false, 0));
    assert_eq!([l8.bound(6), l8.bound(7), l8.bound(17)], [72, 88, 566]);
}

#[test]
fn pretab_matches_the_standard() {
    assert_eq!(
        PRETAB,
        [
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 3, 3, 3, 2, 0
        ]
    );
}

#[test]
fn mpeg1_long_uses_two_widths() {
    // compress 8 = (slen1, slen2) = (2, 1): 11 values of 2 bits, 10 values of 1 bit.
    let g = gi(0, false, 8);
    let values: Vec<u32> = (0..21)
        .map(|i| (i * 7 + 1) % if i < 11 { 4 } else { 2 })
        .collect();
    let bits: Vec<u32> = (0..21).map(|i| if i < 11 { 2 } else { 1 }).collect();
    let mut w = W::default();
    put_all(&mut w, &values, &bits);
    let (s, pos) = read_with(V1, 44100, &g, 0, false, &[0; 40], &w);
    assert_eq!(pos, 32);
    assert_eq!(s.values[21], 0);
    for (i, v) in values.iter().enumerate() {
        assert_eq!(u32::from(s.values[i]), *v, "band {i}");
    }
    assert!(!s.preflag);
    assert_eq!(s.illegal, 0);
    // The preflag comes from the side information in MPEG-1.
    let g2 = GranuleInfo { preflag: true, ..g };
    assert!(read_with(V1, 44100, &g2, 0, false, &[0; 40], &w).0.preflag);
}

#[test]
fn every_slen_pair_has_the_documented_widths() {
    let expect = [
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
    for (c, &(a, b)) in expect.iter().enumerate() {
        let g = gi(0, false, c as u16);
        let w = W {
            bytes: vec![0; 16],
            bits: 128,
        };
        let (_, pos) = read_with(V1, 48000, &g, 0, false, &[0; 40], &w);
        assert_eq!(pos, 11 * a + 10 * b, "scalefac_compress {c}");
    }
}

#[test]
fn scfsi_reuses_the_first_granule_group_by_group() {
    // compress 8: slen (2, 1). scfsi 0b1010: groups 0 (bands 0-5) and 2 (bands 11-15) are shared.
    let g = gi(0, false, 8);
    let mut prev = [0u8; 40];
    for (i, p) in prev.iter_mut().enumerate().take(21) {
        *p = (i % 4) as u8 + 10;
    }
    let mut w = W::default();
    // Group 1: bands 6-10, 5 values of 2 bits. Group 3: bands 16-20, 5 values of 1 bit.
    put_all(&mut w, &[3, 2, 1, 0, 3], &[2; 5]);
    put_all(&mut w, &[1, 0, 1, 1, 0], &[1; 5]);
    let (s, pos) = read_with(V1, 44100, &g, 0b1010, false, &prev, &w);
    assert_eq!(pos, 15);
    assert_eq!(&s.values[0..6], &prev[0..6]);
    assert_eq!(&s.values[6..11], &[3, 2, 1, 0, 3]);
    assert_eq!(&s.values[11..16], &prev[11..16]);
    assert_eq!(&s.values[16..21], &[1, 0, 1, 1, 0]);
    // All four groups shared: nothing is read.
    let (s, pos) = read_with(V1, 44100, &g, 0b1111, false, &prev, &W::default());
    assert_eq!(pos, 0);
    assert_eq!(&s.values[..21], &prev[..21]);
}

#[test]
fn mpeg1_short_and_mixed_blocks() {
    // compress 13 = (3, 3): 36 short values of 3 bits, windows interleaved per band.
    let g = gi(2, false, 13);
    let values: Vec<u32> = (0..36).map(|i| (i * 5 + 2) % 8).collect();
    let mut w = W::default();
    put_all(&mut w, &values, &[3; 36]);
    let (s, pos) = read_with(V1, 44100, &g, 0, false, &[0; 40], &w);
    assert_eq!(pos, 108);
    for (i, v) in values.iter().enumerate() {
        assert_eq!(u32::from(s.values[i]), *v, "short entry {i}");
    }
    assert_eq!(&s.values[36..40], &[0, 0, 0, 0]);
    // Mixed, compress 4 = (3, 0): 8 long + 9 short entries of 3 bits, then 18 without bits.
    let g = gi(2, true, 4);
    let mut w = W::default();
    put_all(&mut w, &values[..17], &[3; 17]);
    let (s, pos) = read_with(V1, 44100, &g, 0, false, &[0; 40], &w);
    assert_eq!(pos, 51);
    for (i, v) in values.iter().take(17).enumerate() {
        assert_eq!(u32::from(s.values[i]), *v, "mixed entry {i}");
    }
    assert!(s.values[17..].iter().all(|&v| v == 0));
    // compress 14 = (4, 2): both parts present.
    let g = gi(2, true, 14);
    let w = W {
        bytes: vec![0xFF; 32],
        bits: 256,
    };
    let (_, pos) = read_with(V1, 44100, &g, 0, false, &[0; 40], &w);
    assert_eq!(pos, 17 * 4 + 18 * 2);
}

#[test]
fn lsf_ranges_of_scalefac_compress() {
    // Range 1 (< 400): sfc 300 -> slen (3, 3, 3, 0), bands 6 5 5 5.
    let g = gi(0, false, 300);
    let w = W {
        bytes: vec![0xFF; 32],
        bits: 256,
    };
    let (s, pos) = read_with(V2, 22050, &g, 0, false, &[0; 40], &w);
    assert_eq!(pos, 6 * 3 + 5 * 3 + 5 * 3);
    assert!(s.values[..16].iter().all(|&v| v == 7));
    assert!(s.values[16..21].iter().all(|&v| v == 0));
    assert!(!s.preflag);
    // Range 2 (400..500): sfc 50 -> slen (2, 2, 2, 0), bands 6 5 7 3.
    let (_, pos) = read_with(V2, 22050, &gi(0, false, 450), 0, false, &[0; 40], &w);
    assert_eq!(pos, 6 * 2 + 5 * 2 + 7 * 2);
    // Range 3 (500..512): sfc 5 -> slen (1, 2, 0, 0), bands 11 10; the preflag is on.
    let (s, pos) = read_with(V2, 22050, &gi(0, false, 505), 0, false, &[0; 40], &w);
    assert_eq!(pos, 11 + 10 * 2);
    assert!(s.preflag);
    assert_eq!(&s.values[..11], &[1; 11]);
    assert_eq!(&s.values[11..21], &[3; 10]);
    // The band beyond the transmitted ones stays 0.
    assert_eq!(s.values[21], 0);
    // The preflag threshold.
    assert!(
        !read_with(V2, 22050, &gi(0, false, 499), 0, false, &[0; 40], &w)
            .0
            .preflag
    );
    assert!(
        read_with(V2, 22050, &gi(0, false, 500), 0, false, &[0; 40], &w)
            .0
            .preflag
    );
}

#[test]
fn lsf_intensity_right_channel_variant() {
    let w = W {
        bytes: vec![0xFF; 32],
        bits: 256,
    };
    // compress 100 -> sfc 50: slen (1, 2, 2, 0), bands 7 7 7 0; all-ones values are illegal.
    let (s, pos) = read_with(V2, 22050, &gi(0, false, 100), 0, true, &[0; 40], &w);
    assert_eq!(pos, 7 + 14 + 14);
    assert_eq!(&s.values[..7], &[1; 7]);
    assert_eq!(&s.values[7..21], &[3; 14]);
    assert_eq!(s.illegal, (1u64 << 21) - 1);
    assert!(!s.preflag);
    // Zero bits: a partition of width 0 sends nothing, so no value can carry the all-ones
    // marker; every entry is position 0 and none is illegal.
    let zeros = W {
        bytes: vec![0; 32],
        bits: 256,
    };
    let (s, _) = read_with(V2, 22050, &gi(0, false, 381), 0, true, &[0; 40], &zeros);
    assert_eq!(s.illegal, 0, "width 0 partitions are not transmitted");
    assert_eq!(&s.values[..6], &[0; 6]);
    // The same value on the left channel is an ordinary scalefactor.
    let (s, _) = read_with(V2, 22050, &gi(0, false, 100), 0, false, &[0; 40], &w);
    assert_eq!(s.illegal, 0);
    // compress 381 -> sfc 190 - 180 = 10: slen (0, 2, 2, 0), bands 6 6 6 3.
    let (s, pos) = read_with(V2, 22050, &gi(0, false, 381), 0, true, &[0; 40], &w);
    assert_eq!(pos, 6 * 2 + 6 * 2);
    assert_eq!(&s.values[..6], &[0; 6]);
    assert_eq!(&s.values[6..18], &[3; 12]);
    // Partitions 0 (6 values) and 3 (3 values) have width 0: nothing is sent, so they are
    // position 0, not the all-ones marker. Everything else is transmitted as 255 (all ones
    // for width 2 or 3).
    assert_eq!(s.illegal, !0b111_111 & !(0b111 << 18) & ((1u64 << 21) - 1));
    // compress 500 -> sfc 250 - 244 = 6: slen (2, 0, 0, 0), bands 8 8 5 0; no preflag even
    // though the field is >= 500.
    let (s, pos) = read_with(V2, 22050, &gi(0, false, 500), 0, true, &[0; 40], &w);
    assert_eq!(pos, 16);
    assert!(!s.preflag);
    assert_eq!(&s.values[..8], &[3; 8]);
    // Partitions 1 and 2 (13 values) have width 0; only the 8 transmitted values are marked.
    assert_eq!(s.illegal, 0xFF);
}

#[test]
fn lsf_short_and_mixed_partitions() {
    let w = W {
        bytes: vec![0xFF; 64],
        bits: 512,
    };
    // Short: compress 0 sends nothing (slen all 0), 4 x 9 values.
    let (s, pos) = read_with(V2, 22050, &gi(2, false, 0), 0, false, &[0; 40], &w);
    assert_eq!(pos, 0);
    assert!(s.values.iter().all(|&v| v == 0));
    // sfc 300 -> slen (3, 3, 3, 0): 27 values of 3 bits.
    let (s, pos) = read_with(V2, 22050, &gi(2, false, 300), 0, false, &[0; 40], &w);
    assert_eq!(pos, 81);
    assert!(s.values[..27].iter().all(|&v| v == 7));
    assert!(s.values[27..].iter().all(|&v| v == 0));
    // Mixed: partitions 6 9 9 9 (33 values) with the same widths.
    let (s, pos) = read_with(V2, 22050, &gi(2, true, 300), 0, false, &[0; 40], &w);
    assert_eq!(pos, (6 + 9 + 9) * 3);
    assert!(s.values[..24].iter().all(|&v| v == 7));
    // Intensity right, sfc 50 -> slen (1, 2, 2, 0) with partitions 6 15 12 0 (mixed).
    let (_, pos) = read_with(V2, 22050, &gi(2, true, 100), 0, true, &[0; 40], &w);
    assert_eq!(pos, 6 + 15 * 2 + 12 * 2);
}

#[test]
fn reading_past_the_end_yields_zeros_without_panicking() {
    let (s, pos) = read_with(
        V1,
        44100,
        &gi(0, false, 15),
        0,
        false,
        &[0; 40],
        &W::default(),
    );
    assert_eq!(pos, 11 * 4 + 10 * 3);
    assert!(s.values.iter().all(|&v| v == 0));
}

#[test]
fn the_8khz_mixed_layout_is_six_long_bands_then_the_short_bands_from_band_3() {
    let mixed = layout(V25, 8000, &gi(2, true, 0));
    let short = layout(V25, 8000, &gi(2, false, 0));
    assert_eq!(&mixed.widths[..6], &[12; 6]);
    // Short bands 3 to 12 follow unchanged (three entries per band).
    assert_eq!(&mixed.widths[6..], &short.widths[9..]);
    assert_eq!((mixed.n_long, mixed.n_short, mixed.n_sf), (6, 30, 33));
    assert_eq!(mixed.mixed_region0, 72);
}
