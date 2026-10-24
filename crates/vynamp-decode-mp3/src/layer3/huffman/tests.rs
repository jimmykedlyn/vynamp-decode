use super::super::tables::sfb_row;
use super::super::testenc::{W, put_pair, put_quad};
use super::*;
use vynamp_decode_mpa::MpegVersion;

fn rng(seed: &mut u32) -> u32 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 17;
    *seed ^= *seed << 5;
    *seed
}

fn long_layout(version: MpegVersion, rate: u32) -> Layout {
    let gi = GranuleInfo::default();
    Layout::new(sfb_row(version, rate).unwrap(), version, &gi)
}

type Decoded = Result<(Spectrum, Box<[i16; 576]>), usize>;

fn run(w: &W, gi: &GranuleInfo, layout: &Layout) -> Decoded {
    let mut xr = Box::new([0i16; 576]);
    let mut bits = Bits::new(&w.bytes, 0);
    let s = decode_spectrum(&mut bits, w.bits, gi, layout, &mut xr)?;
    Ok((s, xr))
}

/// Every distinct code of the source tables (32 and 33 are the count1 tables).
const BOOKS: [u8; 17] = [1, 2, 3, 5, 6, 7, 8, 9, 10, 11, 12, 13, 15, 16, 24, 32, 33];

#[test]
fn source_tables_are_complete_prefix_codes() {
    for t in BOOKS {
        let codes: Vec<(u8, u32)> = match t {
            32 => codes::COUNT1_A.to_vec(),
            33 => codes::COUNT1_B.to_vec(),
            _ => book(t).unwrap().codes.to_vec(),
        };
        let max = codes.iter().map(|c| c.0).max().unwrap();
        let kraft: u64 = codes.iter().map(|&(l, _)| 1u64 << (max - l)).sum();
        assert_eq!(kraft, 1u64 << max, "table {t} is not complete");
        for (i, &(la, ca)) in codes.iter().enumerate() {
            assert!(u64::from(ca) < 1 << la, "table {t} code {i} too wide");
            for &(lb, cb) in &codes[i + 1..] {
                let (short, long, ls, ll) = if la <= lb {
                    (ca, cb, la, lb)
                } else {
                    (cb, ca, lb, la)
                };
                assert_ne!(
                    long >> (ll - ls),
                    short,
                    "table {t}: a code prefixes another"
                );
            }
        }
    }
}

#[test]
fn known_codes_and_linbits() {
    // Tables 1 and 2 as printed in the standard (hlen / hcod).
    assert_eq!(codes::T1, [(1, 1), (3, 1), (2, 1), (3, 0)]);
    assert_eq!(
        codes::T2,
        [
            (1, 1),
            (3, 2),
            (6, 1),
            (3, 3),
            (3, 1),
            (5, 1),
            (5, 3),
            (5, 2),
            (6, 0)
        ]
    );
    // Count1 table B is the four bit complement of the quadruple.
    for (i, &(l, c)) in codes::COUNT1_B.iter().enumerate() {
        assert_eq!((l, c), (4, 15 - i as u32));
    }
    // Table A: "0000" is the single bit 1.
    assert_eq!(codes::COUNT1_A[0], (1, 1));
    let low = [
        (16, 1),
        (17, 2),
        (18, 3),
        (19, 4),
        (20, 6),
        (21, 8),
        (22, 10),
        (23, 13),
    ];
    for (t, linbits) in low {
        assert_eq!(book(t).unwrap().linbits, linbits);
        assert_eq!(book(t).unwrap().codes, book(16).unwrap().codes);
    }
    let high = [
        (24, 4),
        (25, 5),
        (26, 6),
        (27, 7),
        (28, 8),
        (29, 9),
        (30, 11),
        (31, 13),
    ];
    for (t, linbits) in high {
        assert_eq!(book(t).unwrap().linbits, linbits);
        assert_eq!(book(t).unwrap().codes, book(24).unwrap().codes);
    }
    for t in [0, 4, 14, 32, 255] {
        assert!(book(t).is_none(), "table {t}");
    }
}

#[test]
fn every_pair_of_every_table_round_trips() {
    let layout = long_layout(MpegVersion::V1, 44100);
    for t in (1..=31u8).filter(|t| *t != 4 && *t != 14) {
        let b = book(t).unwrap();
        let mut values: Vec<i32> = (0..b.ysize.min(16) as i32).collect();
        if b.linbits > 0 {
            let max = 15 + (1i32 << b.linbits) - 1;
            values.extend([16, 17, 30, max - 1, max].into_iter().filter(|v| *v <= max));
        }
        for &x in &values {
            for &y in &values {
                for (sx, sy) in [(1, 1), (-1, 1), (1, -1), (-1, -1)] {
                    let (x, y) = (x * sx, y * sy);
                    let mut w = W::default();
                    put_pair(&mut w, t, x, y);
                    let gi = GranuleInfo {
                        big_values: 1,
                        table_select: [t, 0, 0],
                        ..GranuleInfo::default()
                    };
                    let (s, xr) = run(&w, &gi, &layout)
                        .unwrap_or_else(|p| panic!("table {t} ({x},{y}) failed at bit {p}"));
                    assert_eq!((i32::from(xr[0]), i32::from(xr[1])), (x, y), "table {t}");
                    assert_eq!(s.used_lines, 2);
                }
            }
        }
    }
}

#[test]
fn random_streams_round_trip_through_three_regions() {
    // 48 kHz long bands: 4 4 4 4 4 4 6 6 6 8 10 12 16 ... so the cumulative edges are
    // 4 8 12 16 20 24 30 36 42 50 60 72 88.
    let layout = long_layout(MpegVersion::V1, 48000);
    let mut seed = 0x1234_5678;
    let cases = [
        // (tables, region0_count, region1_count)
        ([1u8, 2, 3], 5u8, 3u8),
        ([5, 13, 24], 10, 0),
        ([16, 31, 7], 0, 7),
        ([9, 12, 15], 15, 7),
    ];
    for (tables, r0, r1) in cases {
        let edge1 = layout.bound(usize::from(r0) + 1);
        let edge2 = layout.bound(usize::from(r0) + usize::from(r1) + 2);
        let gi = GranuleInfo {
            big_values: 288,
            table_select: tables,
            region0_count: r0,
            region1_count: r1,
            ..GranuleInfo::default()
        };
        let mut w = W::default();
        let mut expect = [0i16; 576];
        for i in (0..576).step_by(2) {
            let region = usize::from(i >= edge1) + usize::from(i >= edge2);
            let t = tables[region];
            let b = book(t).unwrap();
            let cap = if b.linbits > 0 {
                15 + (1i32 << b.linbits.min(8)) - 1
            } else {
                b.ysize as i32 - 1
            };
            let mut pick = || {
                let v = (rng(&mut seed) % (cap as u32 + 1)) as i32;
                if rng(&mut seed) & 1 == 0 { v } else { -v }
            };
            let (x, y) = (pick(), pick());
            put_pair(&mut w, t, x, y);
            expect[i] = x as i16;
            expect[i + 1] = y as i16;
        }
        let (s, xr) = run(&w, &gi, &layout).unwrap();
        assert_eq!(s.used_lines, 576);
        assert_eq!(&xr[..], &expect[..], "tables {tables:?} counts {r0} {r1}");
    }
}

#[test]
fn region_bounds_follow_the_band_tables() {
    let gi = |r0, r1| GranuleInfo {
        region0_count: r0,
        region1_count: r1,
        ..GranuleInfo::default()
    };
    // 44.1 kHz edges: 4 8 12 16 20 24 30 36 44 52 62 74 90 110 134 162 196 238 288 342 418 576.
    let l = long_layout(MpegVersion::V1, 44100);
    assert_eq!(region_bounds(&gi(3, 2), &l), [16, 30]);
    assert_eq!(region_bounds(&gi(0, 0), &l), [4, 8]);
    assert_eq!(region_bounds(&gi(15, 7), &l), [162, 576]);
    // Start and stop blocks: region 0 is 8 bands, region 1 the rest.
    let ws = GranuleInfo {
        window_switching: true,
        block_type: 1,
        region0_count: 7,
        region1_count: 20,
        ..GranuleInfo::default()
    };
    assert_eq!(region_bounds(&ws, &l), [36, 576]);
    assert_eq!(
        region_bounds(&ws, &long_layout(MpegVersion::V2, 22050)),
        [54, 576]
    );
    assert_eq!(
        region_bounds(&ws, &long_layout(MpegVersion::V25, 8000)),
        [108, 576]
    );
    // Short blocks: nine entries, the first three bands of three windows.
    let short = GranuleInfo {
        window_switching: true,
        block_type: 2,
        region0_count: 8,
        region1_count: 12,
        ..GranuleInfo::default()
    };
    for (v, r, want) in [
        (MpegVersion::V1, 44100, 36),
        (MpegVersion::V1, 32000, 36),
        (MpegVersion::V2, 24000, 36),
        (MpegVersion::V25, 11025, 36),
        (MpegVersion::V25, 8000, 72),
    ] {
        let layout = Layout::new(sfb_row(v, r).unwrap(), v, &short);
        assert_eq!(region_bounds(&short, &layout), [want, 576], "{v:?} {r}");
    }
    // Mixed blocks: eight entries, which are the eight long bands of MPEG-1.
    let mixed = GranuleInfo {
        mixed_block: true,
        region0_count: 7,
        region1_count: 13,
        ..short
    };
    let layout = Layout::new(5, MpegVersion::V1, &mixed);
    assert_eq!(region_bounds(&mixed, &layout), [36, 576]);
}

#[test]
fn count1_tables_round_trip_every_quadruple() {
    let layout = long_layout(MpegVersion::V1, 44100);
    for table_b in [false, true] {
        let mut expect = Vec::new();
        for q in 0..16u32 {
            for signs in 0..16u32 {
                let quad = [3, 2, 1, 0].map(|s| {
                    if q >> s & 1 == 0 {
                        0
                    } else if signs >> s & 1 == 0 {
                        1
                    } else {
                        -1
                    }
                });
                expect.extend(quad);
            }
        }
        expect.truncate(576);
        let mut w = W::default();
        for c in expect.chunks(4) {
            put_quad(&mut w, table_b, [c[0], c[1], c[2], c[3]]);
        }
        let gi = GranuleInfo {
            big_values: 0,
            count1_table: table_b,
            ..GranuleInfo::default()
        };
        let (s, xr) = run(&w, &gi, &layout).unwrap();
        assert_eq!(s.quads, 144);
        assert_eq!(s.used_lines, 576);
        for (i, e) in expect.iter().enumerate() {
            assert_eq!(i32::from(xr[i]), *e, "line {i} table_b={table_b}");
        }
    }
}

#[test]
fn count1_follows_big_values_and_a_crossing_quadruple_is_stuffing() {
    let layout = long_layout(MpegVersion::V1, 44100);
    let mut w = W::default();
    put_pair(&mut w, 1, 1, -1);
    put_pair(&mut w, 1, 0, 1);
    put_quad(&mut w, false, [1, 0, -1, 1]);
    put_quad(&mut w, false, [0, 0, 0, 0]);
    let gi = GranuleInfo {
        big_values: 2,
        table_select: [1, 0, 0],
        ..GranuleInfo::default()
    };
    let (s, xr) = run(&w, &gi, &layout).unwrap();
    assert_eq!(&xr[..12], &[1, -1, 0, 1, 1, 0, -1, 1, 0, 0, 0, 0]);
    assert_eq!((s.used_lines, s.quads), (12, 2));
    // Cut one bit off the last quadruple's code (a single 1 bit): it crosses the end.
    let mut cut = w.clone();
    cut.bits -= 1;
    let (s, _) = run(&cut, &gi, &layout).unwrap();
    assert_eq!(s.quads, 1);
}

#[test]
fn errors_report_a_bit_position_and_garbage_never_panics() {
    let layout = long_layout(MpegVersion::V1, 44100);
    for t in [4u8, 14] {
        let gi = GranuleInfo {
            big_values: 1,
            table_select: [t, 0, 0],
            ..GranuleInfo::default()
        };
        let mut w = W::default();
        w.put(0, 16);
        assert!(run(&w, &gi, &layout).is_err(), "table {t}");
        // Nothing to decode: the table is irrelevant.
        let gi = GranuleInfo {
            big_values: 0,
            table_select: [t, t, t],
            ..GranuleInfo::default()
        };
        assert!(run(&W::default(), &gi, &layout).is_ok());
    }
    // Big values that run past the end of the granule.
    let mut w = W::default();
    for _ in 0..4 {
        put_pair(&mut w, 1, 1, 1);
    }
    let gi = GranuleInfo {
        big_values: 4,
        table_select: [1, 0, 0],
        region0_count: 15,
        ..GranuleInfo::default()
    };
    assert!(run(&w, &gi, &layout).is_ok());
    w.bits -= 3;
    assert!(run(&w, &gi, &layout).is_err());
    // Garbage with a fixed seed, including short buffers.
    let mut seed = 99;
    for n in 0..600usize {
        let len = if n % 3 == 0 { 3 } else { 300 };
        let bytes: Vec<u8> = (0..len).map(|_| rng(&mut seed) as u8).collect();
        let w = W {
            bits: bytes.len() * 8,
            bytes,
        };
        let gi = GranuleInfo {
            big_values: (rng(&mut seed) % 289) as u16,
            table_select: [
                rng(&mut seed) as u8 & 31,
                rng(&mut seed) as u8 & 31,
                rng(&mut seed) as u8 & 31,
            ],
            region0_count: (rng(&mut seed) & 15) as u8,
            region1_count: (rng(&mut seed) & 7) as u8,
            count1_table: rng(&mut seed) & 1 == 1,
            ..GranuleInfo::default()
        };
        let _ = run(&w, &gi, &layout);
    }
}
