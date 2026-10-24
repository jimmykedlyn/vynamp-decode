mod synth;

use super::huffman::book;
use super::testenc::*;
use super::*;
use vynamp_decode_mpa::parse_header;

fn rng(seed: &mut u32) -> u32 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 17;
    *seed ^= *seed << 5;
    *seed
}

/// Fills the lines of a granule with random values that its tables can code.
fn fill(g: &mut GrSpec, seed: &mut u32) {
    let bv2 = usize::from(g.info.big_values) * 2;
    for i in (0..bv2).step_by(2) {
        let region = usize::from(i >= g.regions[0]) + usize::from(i >= g.regions[1]);
        let t = g.info.table_select[region];
        if t == 0 {
            continue;
        }
        let b = book(t).unwrap();
        let cap = if b.linbits > 0 {
            15 + (1u32 << b.linbits.min(3)) - 1
        } else {
            b.ysize as u32 - 1
        };
        for k in 0..2 {
            let v = (rng(seed) % (cap + 1)) as i16;
            g.xr[i + k] = if rng(seed) & 1 == 0 { v } else { -v };
        }
    }
    for k in 0..g.quads * 4 {
        g.xr[bv2 + k] = (rng(seed) % 3) as i16 - 1;
    }
}

/// A granule with scalefactors `(value, bits)` and `bv` big value pairs plus `quads` count1
/// quadruples.
fn granule(
    info: GranuleInfo,
    regions: [usize; 2],
    sf: Vec<(u32, u32)>,
    quads: usize,
    seed: &mut u32,
) -> GrSpec {
    let mut g = GrSpec::new(info);
    g.regions = regions;
    g.sf = sf;
    g.quads = quads;
    fill(&mut g, seed);
    g
}

/// Writes the granules of a frame in transmission order and returns the main data and the
/// side information (with `part2_3_length` filled in).
fn assemble(specs: &mut [Vec<GrSpec>], scfsi: [u8; 2], begin: u16) -> (SideInfo, W) {
    let mut w = W::default();
    let mut si = SideInfo {
        main_data_begin: begin,
        scfsi,
        ..SideInfo::default()
    };
    for (gr, chans) in specs.iter_mut().enumerate() {
        for (ch, g) in chans.iter_mut().enumerate() {
            encode_granule(&mut w, g);
            si.granules[gr][ch] = g.info;
        }
    }
    (si, w)
}

fn long_info(compress: u16, bv: u16, tables: [u8; 3], r0: u8, r1: u8) -> GranuleInfo {
    GranuleInfo {
        big_values: bv,
        global_gain: 140,
        scalefac_compress: compress,
        table_select: tables,
        region0_count: r0,
        region1_count: r1,
        ..GranuleInfo::default()
    }
}

fn short_info(compress: u16, bv: u16, tables: [u8; 2], mixed: bool) -> GranuleInfo {
    GranuleInfo {
        big_values: bv,
        global_gain: 120,
        scalefac_compress: compress,
        window_switching: true,
        block_type: 2,
        mixed_block: mixed,
        table_select: [tables[0], tables[1], 0],
        subblock_gain: [1, 2, 3],
        region0_count: if mixed { 7 } else { 8 },
        region1_count: if mixed { 13 } else { 12 },
        ..GranuleInfo::default()
    }
}

fn sf_bits(values: &[u32], bits: &[u32]) -> Vec<(u32, u32)> {
    values.iter().copied().zip(bits.iter().copied()).collect()
}

fn decode(state: &mut Layer3, frame: &[u8], offset: u64) -> Result<Box<FrameData>> {
    let h = parse_header(frame[..4].try_into().unwrap()).unwrap();
    let mut out = FrameData::new();
    decode_granules(state, frame, &h, offset, &mut out)?;
    Ok(out)
}

/// Checks a decoded granule against what was encoded.
fn check(got: &Granule, spec: &GrSpec, scalefac: &[u8], what: &str) {
    assert_eq!(got.status, GranuleStatus::Decoded, "{what}");
    assert_eq!(got.info, spec.info, "{what}: side info");
    assert_eq!(&got.xr[..], &spec.xr[..], "{what}: lines");
    assert_eq!(
        got.used_lines,
        usize::from(spec.info.big_values) * 2 + spec.quads * 4,
        "{what}: used lines"
    );
    assert_eq!(
        &got.scalefac[..scalefac.len()],
        scalefac,
        "{what}: scalefactors"
    );
    assert!(
        got.scalefac[scalefac.len()..].iter().all(|&v| v == 0),
        "{what}: extra scalefactors"
    );
    assert_eq!(
        got.bits_used,
        usize::from(spec.info.part2_3_length),
        "{what}: bits"
    );
    assert_eq!(
        got.part2_bits,
        spec.sf.iter().map(|s| s.1 as usize).sum::<usize>()
    );
}

#[test]
fn mpeg1_stereo_frame_with_every_block_kind() {
    let mut seed = 0xC0FFEE;
    let h = v1_stereo();
    // 44.1 kHz: region 0 ends at band 5 (line 20), region 1 at band 9 (line 44).
    let sf00: Vec<u32> = (0..21)
        .map(|i| (i * 3 + 1) % if i < 11 { 4 } else { 2 })
        .collect();
    let b00: Vec<u32> = (0..21).map(|i| if i < 11 { 2 } else { 1 }).collect();
    let g00 = granule(
        GranuleInfo {
            preflag: true,
            scalefac_scale: true,
            ..long_info(8, 50, [7, 15, 24], 4, 3)
        },
        [20, 44],
        sf_bits(&sf00, &b00),
        15,
        &mut seed,
    );
    let sf01: Vec<u32> = (0..36).map(|i| (i * 5 + 2) % 8).collect();
    let g01 = granule(
        short_info(13, 40, [5, 11], false),
        [36, 576],
        sf_bits(&sf01, &[3; 36]),
        10,
        &mut seed,
    );
    let g01 = GrSpec {
        info: GranuleInfo {
            count1_table: true,
            ..g01.info
        },
        ..g01
    };
    // Granule 1, channel 0 shares band groups 1 and 2 with granule 0.
    let sf10: Vec<u32> = [
        1, 3, 0, 2, 2, 1, /* group 0 */ 1, 0, 1, 1, 0, /* group 3 */
    ]
    .to_vec();
    let b10: Vec<u32> = [2, 2, 2, 2, 2, 2, 1, 1, 1, 1, 1].to_vec();
    let g10 = granule(
        long_info(8, 20, [1, 2, 3], 0, 0),
        [4, 8],
        sf_bits(&sf10, &b10),
        6,
        &mut seed,
    );
    let sf11: Vec<u32> = (0..17).map(|i| (i * 3) % 8).collect();
    let g11 = granule(
        short_info(4, 30, [3, 8], true),
        [36, 576],
        sf_bits(&sf11, &[3; 17]),
        5,
        &mut seed,
    );
    let mut specs = vec![vec![g00, g01], vec![g10, g11]];
    let (si, w) = assemble(&mut specs, [0b0110, 0], 0);
    let frame = frame_bytes(&h, &si, &w.bytes);
    let out = decode(&mut Layer3::default(), &frame, 5000).unwrap();
    assert_eq!((out.channels, out.granules), (2, 2));
    assert!(!out.needs_preroll());

    let sf = |s: &[u32]| s.iter().map(|&v| v as u8).collect::<Vec<u8>>();
    check(&out.gr[0][0], &specs[0][0], &sf(&sf00), "gr0 ch0");
    assert_eq!(out.gr[0][0].layout.n_long, 22);
    assert!(out.gr[0][0].preflag);
    check(&out.gr[0][1], &specs[0][1], &sf(&sf01), "gr0 ch1");
    assert_eq!(
        (out.gr[0][1].layout.n_long, out.gr[0][1].layout.n_short),
        (0, 39)
    );
    // Shared groups come from granule 0, the others are new.
    let mut want = sf(&sf00)[..21].to_vec();
    want[0..6].copy_from_slice(&sf(&sf10[..6]));
    want[16..21].copy_from_slice(&sf(&sf10[6..]));
    check(&out.gr[1][0], &specs[1][0], &want, "gr1 ch0");
    check(&out.gr[1][1], &specs[1][1], &sf(&sf11), "gr1 ch1");
    assert_eq!(
        (out.gr[1][1].layout.n_long, out.gr[1][1].layout.n_short),
        (8, 30)
    );
}

#[test]
fn mpeg2_mono_long_and_mixed_8khz() {
    let mut seed = 77;
    // 22.05 kHz long, compress 505: slen (1, 2), preflag derived.
    let sf: Vec<u32> = (0..21).map(|i| i % if i < 11 { 2 } else { 4 }).collect();
    let b: Vec<u32> = (0..21).map(|i| if i < 11 { 1 } else { 2 }).collect();
    let g = granule(
        long_info(505, 120, [3, 6, 9], 3, 2),
        [24, 44],
        sf_bits(&sf, &b),
        12,
        &mut seed,
    );
    let mut specs = vec![vec![g]];
    let (si, w) = assemble(&mut specs, [0, 0], 0);
    let out = decode(
        &mut Layer3::default(),
        &frame_bytes(&v2_mono(), &si, &w.bytes),
        0,
    )
    .unwrap();
    assert_eq!((out.channels, out.granules), (1, 1));
    // The derived preflag is not part of the parsed side information.
    let mut want = specs[0][0].clone();
    want.info.preflag = false;
    check(
        &out.gr[0][0],
        &want,
        &sf.iter().map(|&v| v as u8).collect::<Vec<_>>(),
        "22k",
    );
    assert!(out.gr[0][0].preflag);

    // 8 kHz mixed block (MPEG-2.5): compress 300 = slen (3, 3, 3, 0), partitions 6 9 9 9.
    // Region 0 of a mixed block is 72 lines at 8 kHz (see `huffman::region_bounds`).
    let sf: Vec<u32> = (0..24).map(|i| (i * 3 + 1) % 8).collect();
    let g = granule(
        short_info(300, 70, [2, 5], true),
        [72, 576],
        sf_bits(&sf, &[3; 24]),
        8,
        &mut seed,
    );
    let mut specs = vec![vec![g]];
    let (si, w) = assemble(&mut specs, [0, 0], 0);
    let out = decode(
        &mut Layer3::default(),
        &frame_bytes(&v25_mono(), &si, &w.bytes),
        0,
    )
    .unwrap();
    check(
        &out.gr[0][0],
        &specs[0][0],
        &sf.iter().map(|&v| v as u8).collect::<Vec<_>>(),
        "8k mixed",
    );
    assert_eq!(
        (out.gr[0][0].layout.n_long, out.gr[0][0].layout.n_short),
        (3, 36)
    );
}

#[test]
fn mpeg2_intensity_right_channel_is_marked() {
    let mut seed = 5;
    let h = v2_intensity();
    let sf0: Vec<u32> = (0..16).map(|i| i % 8).collect();
    let b0 = [3; 16];
    let g0 = granule(
        long_info(300, 40, [1, 2, 3], 2, 2),
        [18, 36],
        sf_bits(&sf0, &b0),
        3,
        &mut seed,
    );
    // Right channel: compress 100 -> slen (1, 2, 2, 0), partitions 7 7 7 0.
    let sf1: Vec<u32> = (0..21)
        .map(|i| if i < 7 { i % 2 } else { (i * 7) % 4 })
        .collect();
    let b1: Vec<u32> = (0..21).map(|i| if i < 7 { 1 } else { 2 }).collect();
    let g1 = granule(
        long_info(100, 40, [1, 2, 3], 2, 2),
        [18, 36],
        sf_bits(&sf1, &b1),
        3,
        &mut seed,
    );
    let mut specs = vec![vec![g0, g1]];
    let (si, w) = assemble(&mut specs, [0, 0], 0);
    let out = decode(&mut Layer3::default(), &frame_bytes(&h, &si, &w.bytes), 0).unwrap();
    let left = out.gr[0][0].clone();
    assert_eq!(left.illegal_is_pos, 0);
    check(
        &left,
        &specs[0][0],
        &sf0.iter().map(|&v| v as u8).collect::<Vec<_>>(),
        "left",
    );
    let right = &out.gr[0][1];
    check(
        right,
        &specs[0][1],
        &sf1.iter().map(|&v| v as u8).collect::<Vec<_>>(),
        "right",
    );
    let mut illegal = 0u64;
    for (i, &v) in sf1.iter().enumerate() {
        let max = if i < 7 { 1 } else { 3 };
        if v == max {
            illegal |= 1 << i;
        }
    }
    assert_ne!(illegal, 0);
    assert_eq!(right.illegal_is_pos, illegal);
    // Joint stereo without the intensity bit treats the right channel like any other.
    let plain = header(2, 8, 0, 1, 2, false);
    let out = decode(
        &mut Layer3::default(),
        &frame_bytes(&plain, &si, &w.bytes),
        0,
    );
    // The data was written for intensity scalefactors, so only the side effects matter:
    // it must not be flagged.
    if let Ok(o) = out {
        assert_eq!(o.gr[0][1].illegal_is_pos, 0);
    }
}

#[test]
fn crc_protected_frames_are_verified() {
    let mut seed = 9;
    let h = header(3, 9, 0, 3, 0, true);
    let sf: Vec<u32> = vec![1; 21];
    let g0 = granule(
        long_info(5, 50, [1, 2, 3], 3, 2),
        [16, 30],
        sf_bits(&sf, &[1; 21]),
        2,
        &mut seed,
    );
    let g1 = granule(
        long_info(5, 50, [1, 2, 3], 3, 2),
        [16, 30],
        sf_bits(&sf, &[1; 21]),
        2,
        &mut seed,
    );
    let mut specs = vec![vec![g0], vec![g1]];
    let (si, w) = assemble(&mut specs, [0, 0], 0);
    let frame = frame_bytes(&h, &si, &w.bytes);
    assert!(decode(&mut Layer3::default(), &frame, 0).is_ok());
    let mut bad = frame.clone();
    bad[8] ^= 0x10;
    assert_eq!(
        decode(&mut Layer3::default(), &bad, 40).err(),
        Some(Error::Invalid {
            what: "mpeg crc",
            offset: 40 + 4
        })
    );
    // The CRC field itself.
    let mut bad = frame;
    bad[5] ^= 1;
    assert!(decode(&mut Layer3::default(), &bad, 0).is_err());
}

/// Builds frames whose granule data is placed in the shared byte stream of the main data:
/// frame `i` owns `capacity` bytes and its data starts `begin[i]` bytes before them.
fn chain(h: &FrameHeader, datas: &[(W, u16)], sis: &[SideInfo]) -> Vec<Vec<u8>> {
    let cap = capacity(h);
    let mut stream = vec![0u8; cap * datas.len()];
    for (i, (w, begin)) in datas.iter().enumerate() {
        let start = cap * i - usize::from(*begin);
        assert!(
            w.bytes.len() <= usize::from(*begin) + cap,
            "data of frame {i} outruns its frame"
        );
        for (k, b) in w.bytes.iter().enumerate() {
            assert_eq!(stream[start + k], 0, "overlap");
            stream[start + k] = *b;
        }
    }
    (0..datas.len())
        .map(|i| frame_bytes(h, &sis[i], &stream[cap * i..cap * (i + 1)]))
        .collect()
}

/// Three MPEG-1 mono frames; the third reaches 300 bytes back into the second and its first
/// granule is large.
fn reservoir_frames(seed: &mut u32) -> (Vec<Vec<u8>>, Vec<Vec<GrSpec>>) {
    let h = v1_mono();
    let small = |seed: &mut u32| {
        let info = long_info(0, 10, [1, 0, 0], 15, 7);
        vec![
            granule(info, [576, 576], vec![], 4, seed),
            granule(info, [576, 576], vec![], 4, seed),
        ]
    };
    let mut s0 = small(seed);
    let mut s1 = small(seed);
    let big = granule(
        long_info(0, 200, [16, 16, 16], 15, 7),
        [576, 576],
        vec![],
        0,
        seed,
    );
    let tail = granule(
        long_info(0, 20, [1, 0, 0], 15, 7),
        [576, 576],
        vec![],
        6,
        seed,
    );
    let mut s2: Vec<Vec<GrSpec>> = vec![vec![big], vec![tail]];
    let (si0, w0) = assemble(
        &mut s0.iter_mut().map(|g| vec![g.clone()]).collect::<Vec<_>>(),
        [0, 0],
        0,
    );
    let (si1, w1) = assemble(
        &mut s1.iter_mut().map(|g| vec![g.clone()]).collect::<Vec<_>>(),
        [0, 0],
        0,
    );
    let (si2, w2) = assemble(&mut s2, [0, 0], 300);
    let frames = chain(&h, &[(w0, 0), (w1, 0), (w2, 300)], &[si0, si1, si2]);
    (frames, s2)
}

#[test]
fn the_reservoir_supplies_main_data_from_earlier_frames() {
    let mut seed = 31337;
    let (frames, s2) = reservoir_frames(&mut seed);
    assert!(
        (300 * 8..4096).contains(&usize::from(s2[0][0].info.part2_3_length)),
        "the test needs granule 0 to start before the frame"
    );
    let mut st = Layer3::default();
    for f in &frames[..2] {
        let o = decode(&mut st, f, 0).unwrap();
        assert!(!o.needs_preroll());
    }
    let o = decode(&mut st, &frames[2], 0).unwrap();
    assert!(!o.needs_preroll());
    check(&o.gr[0][0], &s2[0][0], &[0; 21], "big granule");
    check(&o.gr[1][0], &s2[1][0], &[0; 21], "tail granule");
    // Skipping the first frame still leaves enough (396 >= 300 bytes).
    let mut st = Layer3::default();
    decode(&mut st, &frames[1], 0).unwrap();
    assert!(!decode(&mut st, &frames[2], 0).unwrap().needs_preroll());
}

#[test]
fn missing_reservoir_marks_only_the_granules_that_reach_back() {
    let mut seed = 31337;
    let (frames, s2) = reservoir_frames(&mut seed);
    for reset_first in [false, true] {
        let mut st = Layer3::default();
        if reset_first {
            decode(&mut st, &frames[1], 0).unwrap();
            st.reset();
        }
        let o = decode(&mut st, &frames[2], 0).unwrap();
        assert!(o.needs_preroll());
        let g0 = &o.gr[0][0];
        assert_eq!(g0.status, GranuleStatus::NeedsPreroll);
        assert!(g0.xr.iter().all(|&v| v == 0));
        assert_eq!(g0.used_lines, 0);
        // The side information is still reported.
        assert_eq!(g0.info, s2[0][0].info);
        // Granule 1 starts inside the frame's own data and decodes.
        check(&o.gr[1][0], &s2[1][0], &[0; 21], "tail granule");
    }
}

#[test]
fn reaches_is_exact_at_the_boundary() {
    assert!(super::reaches(-8 * 300, 300));
    assert!(!super::reaches(-8 * 300 - 1, 300));
    assert!(super::reaches(0, 0));
    assert!(super::reaches(5, 0));
    assert!(!super::reaches(-1, 0));
}

#[test]
fn damaged_frames_are_invalid_with_a_position() {
    let mut seed = 4;
    let h = v1_mono();
    let g0 = granule(
        long_info(5, 50, [1, 2, 3], 3, 2),
        [16, 30],
        sf_bits(&[1; 21], &[1; 21]),
        2,
        &mut seed,
    );
    let g1 = g0.clone();
    let mut specs = vec![vec![g0], vec![g1]];
    let (si, w) = assemble(&mut specs, [0, 0], 0);
    let good = frame_bytes(&h, &si, &w.bytes);
    assert!(decode(&mut Layer3::default(), &good, 100).is_ok());
    let invalid = |r: Result<Box<FrameData>>, what: &'static str| match r {
        Err(Error::Invalid { what: w, offset }) => {
            assert_eq!(w, what);
            assert!(offset >= 100, "offset {offset}");
        }
        Err(e) => panic!("unexpected error {e:?}"),
        Ok(_) => panic!("accepted a damaged frame ({what})"),
    };
    // part2_3_length larger than all the data there is.
    let mut big = si;
    big.granules[1][0].part2_3_length = 4095;
    invalid(
        decode(
            &mut Layer3::default(),
            &frame_bytes(&h, &big, &w.bytes),
            100,
        ),
        "mp3 huffman",
    );
    // A table that the standard leaves unused, with lines to decode.
    let mut t14 = si;
    t14.granules[0][0].table_select[0] = 14;
    invalid(
        decode(
            &mut Layer3::default(),
            &frame_bytes(&h, &t14, &w.bytes),
            100,
        ),
        "mp3 huffman",
    );
    // More big values than part2_3_length can hold.
    let mut over = si;
    over.granules[0][0].big_values = 288;
    invalid(
        decode(
            &mut Layer3::default(),
            &frame_bytes(&h, &over, &w.bytes),
            100,
        ),
        "mp3 huffman",
    );
    // Side information that cannot be right.
    let mut bv = si;
    bv.granules[0][0].big_values = 289;
    invalid(
        decode(&mut Layer3::default(), &frame_bytes(&h, &bv, &w.bytes), 100),
        "mp3 side info",
    );
    // Frames cut short.
    invalid(
        decode(&mut Layer3::default(), &good[..20], 100),
        "mp3 side info",
    );
    invalid(
        decode(&mut Layer3::default(), &good[..4], 100),
        "mp3 side info",
    );
    let side_end = 4 + h.side_info_len();
    invalid(
        decode(&mut Layer3::default(), &good[..side_end + 3], 100),
        "mp3 huffman",
    );
}

#[test]
fn a_damaged_frame_still_feeds_the_reservoir() {
    let mut seed = 31337;
    let (frames, s2) = reservoir_frames(&mut seed);
    let mut st = Layer3::default();
    decode(&mut st, &frames[0], 0).unwrap();
    // Frame 1 with a ruined side information: Invalid, yet its main data is kept.
    let mut broken = frames[1].clone();
    broken[4 + 3] ^= 0xFF;
    broken[4 + 4] ^= 0xFF;
    let _ = decode(&mut st, &broken, 0);
    let o = decode(&mut st, &frames[2], 0).unwrap();
    assert!(!o.needs_preroll());
    check(&o.gr[0][0], &s2[0][0], &[0; 21], "big granule");
}

#[test]
fn garbage_never_panics() {
    let mut seed = 0x5EED_u32;
    let headers = [
        v1_stereo(),
        v1_mono(),
        v2_stereo(),
        v2_mono(),
        v2_intensity(),
        v25_mono(),
        header(3, 9, 0, 1, 3, true),
    ];
    let mut st = Layer3::default();
    let mut out = FrameData::new();
    let (mut ok, mut err) = (0, 0);
    for i in 0..3000usize {
        let h = &headers[i % headers.len()];
        let mut frame = header_bytes(h).to_vec();
        let len = match i % 5 {
            0 => h.frame_len,
            1 => 4 + (rng(&mut seed) as usize % h.frame_len),
            _ => h.frame_len,
        };
        while frame.len() < len {
            // Bias toward zeros so that some granules get plausible lengths.
            let r = rng(&mut seed);
            frame.push(if r & 3 == 0 { (r >> 8) as u8 } else { 0 });
        }
        if i % 2 == 0 {
            for b in frame.iter_mut().skip(4 + h.side_info_len()) {
                *b = rng(&mut seed) as u8;
            }
        }
        match decode_granules(&mut st, &frame, h, i as u64 * 1000, &mut out) {
            Ok(()) => ok += 1,
            Err(Error::Invalid { offset, .. }) => {
                assert!(offset >= i as u64 * 1000);
                err += 1;
            }
            Err(e) => panic!("unexpected error {e:?}"),
        }
    }
    assert!(ok > 100, "only {ok} frames decoded");
    assert!(err > 100, "only {err} frames failed");
}

// ---------------------------------------------------------------------------- fixtures

fn fixture_frames(name: &str) -> Vec<(u64, Vec<u8>)> {
    let path = format!(
        "{}/../vynamp-decode-mpa/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    let data = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut at = 0usize;
    if data.starts_with(b"ID3") {
        let size = data[6..10]
            .iter()
            .fold(0usize, |a, &b| (a << 7) | usize::from(b & 0x7F));
        at = 10 + size;
    }
    let mut frames = Vec::new();
    while let Some(head) = data.get(at..at + 4) {
        let Some(h) = parse_header(head.try_into().unwrap()) else {
            break;
        };
        let end = (at + h.frame_len).min(data.len());
        frames.push((at as u64, data[at..end].to_vec()));
        at += h.frame_len;
    }
    frames
}

/// Counts of what the fixtures exercised.
#[derive(Default, Debug)]
struct Seen {
    granules: usize,
    short: usize,
    mixed: usize,
    ws_long: usize,
    count1_b: usize,
    scfsi: usize,
    preflag: usize,
    tables: u32,
    linbits_tables: u32,
    exact: usize,
    stuffed: usize,
}

fn run_fixture(name: &str, seen: &mut Seen) {
    let frames = fixture_frames(name);
    assert!(frames.len() > 20, "{name}: {} frames", frames.len());
    let mut st = Layer3::default();
    let mut out = FrameData::new();
    let mut underflow = 0;
    for (n, (offset, frame)) in frames.iter().enumerate() {
        let h = parse_header(frame[..4].try_into().unwrap()).unwrap();
        assert_eq!(h.layer, 3, "{name}");
        decode_granules(&mut st, frame, &h, *offset, &mut out)
            .unwrap_or_else(|e| panic!("{name} frame {n} at {offset}: {e:?}"));
        if out.needs_preroll() {
            underflow += 1;
        }
        for gr in 0..out.granules {
            for ch in 0..out.channels {
                let g = &out.gr[gr][ch];
                if g.status != GranuleStatus::Decoded {
                    continue;
                }
                let info = &g.info;
                seen.granules += 1;
                // The strong self-check: scalefactors plus Huffman data end exactly where the
                // side information says the granule ends.
                if g.bits_used == usize::from(info.part2_3_length) {
                    seen.exact += 1;
                } else {
                    seen.stuffed += 1;
                    assert!(
                        g.bits_used < usize::from(info.part2_3_length),
                        "{name} frame {n} gr{gr} ch{ch}: used {} of {} bits",
                        g.bits_used,
                        info.part2_3_length
                    );
                }
                seen.short += usize::from(info.block_type == 2 && !info.mixed_block);
                seen.mixed += usize::from(info.mixed_block);
                seen.ws_long += usize::from(info.window_switching && info.block_type != 2);
                seen.count1_b += usize::from(
                    info.count1_table && g.used_lines > 2 * usize::from(info.big_values),
                );
                seen.preflag += usize::from(g.preflag);
                if gr == 1 && h.version == MpegVersion::V1 {
                    seen.scfsi += usize::from(st_scfsi_nonzero(frame, &h, ch));
                }
                if info.big_values > 0 {
                    for t in info.table_select {
                        seen.tables |= 1 << t;
                        if t >= 16 {
                            seen.linbits_tables |= 1 << t;
                        }
                    }
                }
            }
        }
    }
    // The stream starts with a frame that has no reservoir to point into.
    assert_eq!(underflow, 0, "{name}: granules without reservoir data");
}

fn st_scfsi_nonzero(frame: &[u8], h: &FrameHeader, ch: usize) -> bool {
    let off = 4 + if h.crc { 2 } else { 0 };
    let si = sideinfo::parse(&frame[off..off + h.side_info_len()], h, 0).unwrap();
    si.scfsi[ch] != 0
}

use vynamp_decode_mpa::{FrameHeader, MpegVersion};

#[test]
fn fixture_streams_decode_with_exactly_consumed_granules() {
    let mut seen = Seen::default();
    for name in [
        "cbr128-44k-stereo.mp3",
        "cbr128-48k-stereo.mp3",
        "cbr32-48k-stereo.mp3",
        "cbr64-44k-mono.mp3",
        "cbr8-8k-mono.mp3",
        "mpeg2-22k-stereo.mp3",
        "mpeg25-8k-mono.mp3",
        "nolame-cbr128-48k.mp3",
        "vbr-q2-48k-stereo.mp3",
    ] {
        run_fixture(name, &mut seen);
    }
    eprintln!("fixture coverage: {seen:?}");
    assert!(seen.granules > 1000, "{seen:?}");
    assert_eq!(seen.stuffed, 0, "granules that ended early: {seen:?}");
    assert!(seen.short > 0, "no short blocks in the fixtures");
    assert!(seen.tables.count_ones() >= 10, "{:#b}", seen.tables);
    assert!(seen.linbits_tables != 0, "no escape table in the fixtures");
}

// ------------------------------------------------------------ synthesis fixtures coverage

/// What the end-to-end fixtures (`tests/fixtures/l3-*.mp3`, compared with FFmpeg in
/// `tests/mp3.rs`) exercise of the synthesis path.
#[derive(Default, Debug)]
struct Tools {
    granules: usize,
    long: usize,
    start: usize,
    short: usize,
    stop: usize,
    mixed: usize,
    ms_frames: usize,
    intensity_frames: usize,
    scale_on: usize,
    preflag: usize,
    subblock_gain: usize,
}

#[test]
fn the_end_to_end_fixtures_cover_every_block_type_and_middle_side() {
    let dir = format!("{}/tests/fixtures", env!("CARGO_MANIFEST_DIR"));
    let mut t = Tools::default();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_str().unwrap().to_owned();
        if !(name.starts_with("l3-") && name.ends_with(".mp3")) {
            continue;
        }
        let data = std::fs::read(&path).unwrap();
        let mut st = Layer3::default();
        let mut out = FrameData::new();
        let mut at = 0usize;
        while let Some(head) = data.get(at..at + 4) {
            let h = parse_header(head.try_into().unwrap()).unwrap();
            let frame = &data[at..(at + h.frame_len).min(data.len())];
            decode_granules(&mut st, frame, &h, at as u64, &mut out).unwrap();
            t.ms_frames += usize::from(
                h.channel_mode == vynamp_decode_mpa::ChannelMode::JointStereo
                    && h.mode_ext & 2 != 0,
            );
            t.intensity_frames += usize::from(
                h.channel_mode == vynamp_decode_mpa::ChannelMode::JointStereo
                    && h.mode_ext & 1 != 0,
            );
            for gr in 0..out.granules {
                for ch in 0..out.channels {
                    let g = &out.gr[gr][ch];
                    if g.status != GranuleStatus::Decoded {
                        continue;
                    }
                    t.granules += 1;
                    match (g.info.block_type, g.info.mixed_block) {
                        (0, _) => t.long += 1,
                        (1, _) => t.start += 1,
                        (2, false) => t.short += 1,
                        (2, true) => t.mixed += 1,
                        _ => t.stop += 1,
                    }
                    t.scale_on += usize::from(g.info.scalefac_scale);
                    t.preflag += usize::from(g.preflag);
                    t.subblock_gain += usize::from(g.info.subblock_gain != [0; 3]);
                }
            }
            at += h.frame_len;
        }
    }
    eprintln!("synthesis fixture coverage: {t:?}");
    assert!(t.granules > 600, "{t:?}");
    assert!(t.long > 200, "{t:?}");
    assert!(t.start >= 5 && t.stop >= 5 && t.short >= 5, "{t:?}");
    assert!(t.ms_frames >= 5, "{t:?}");
    // LAME does not emit mixed blocks or intensity stereo; those are checked by hand-built
    // frames (`tests/mp3.rs`, `stereo`, `imdct`).
    assert_eq!((t.mixed, t.intensity_frames), (0, 0), "{t:?}");
}
