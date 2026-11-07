//! Black-box cross-check of four Layer III behaviours against the FFmpeg n8.1.3 command line
//! decoder (`mp3float`), on streams hand-built here with the test encoder.
//!
//! The streams exercise what no encoder in the fixture tool chain emits:
//!
//! - `bb-m25-8k-mixed`: MPEG-2.5 8 kHz mixed blocks (72 long lines, short bands from band 3,
//!   long IMDCT and alias reduction over four subbands);
//! - `bb-lsf-mixed-intensity`: MPEG-2 intensity stereo in mixed blocks, with the right
//!   channel's short windows empty (the long bands above the last non-zero one become
//!   intensity) and with a non-zero line in one short window (they do not);
//! - `bb-lsf-layouts`: joint stereo frames whose two channels use different block layouts;
//! - `bb-lsf-slen0`: MPEG-2 intensity positions in a partition whose `slen` is 0.
//!
//! Every frame has `main_data_begin = 0`. The committed `tests/fixtures/bb-*.mp3` are these
//! streams byte for byte, and `bb-*.f32` is FFmpeg's decode of them as raw interleaved
//! little-endian f32, made with
//! `ffmpeg -v error -f mp3 -i bb-X.mp3 -c:a pcm_f32le -f f32le bb-X.f32`
//! after writing the streams with
//! `VYNAMP_BLACKBOX_OUT=<dir> cargo test -p vynamp-decode-mp3 --lib write_blackbox_streams -- --ignored`.
//! A stream without a Xing/Info header decodes in FFmpeg to every sample of every frame, so
//! the reference aligns with our untrimmed output at offset 0.

use super::*;
use crate::MpegDecoder;
use crate::common::track;
use vynamp_decode_core::{AudioBuffer, CodecId, Decoder, Options, Packet};

/// MPEG-2 22.05 kHz 160 kbit/s joint stereo with the given mode extension (bit 0 intensity,
/// bit 1 middle/side); frames of 522 bytes.
fn lsf_joint(mode_ext: u8) -> FrameHeader {
    header(2, 14, 0, 1, mode_ext, false)
}

/// MPEG-2.5 8 kHz 160 kbit/s mono; frames of 1440 bytes.
fn m25_mono() -> FrameHeader {
    header(0, 14, 2, 3, 0, false)
}

/// Random scalefactors for partitions of `(count, bits)`; partitions of 0 bits send nothing.
fn sfs(parts: &[(usize, u32)], seed: &mut u32) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    for &(n, bits) in parts {
        if bits == 0 {
            continue;
        }
        for _ in 0..n {
            out.push((rng(seed) % (1 << bits), bits));
        }
    }
    out
}

/// Scalefactors for an intensity right channel: like [`sfs`], but never the all-ones value
/// of a partition's width. ISO/IEC 11172-3 and 13818-3 reserve that value as the `is_max`
/// marker of an illegal intensity position; FFmpeg n8.1.3 ignores the marker for MPEG-2 and
/// instead treats positions 16 and above as illegal (`sf_max = 16`), so a stream that sends
/// the marker would compare two different, both defensible, readings of the same bits. The
/// frames here stay on values both decoders read the same way; the marker rule itself is
/// covered against the ISO conformance suite and by the unit tests, not black-box.
fn sfs_legal(parts: &[(usize, u32)], seed: &mut u32) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    for &(n, bits) in parts {
        if bits == 0 {
            continue;
        }
        // `2^bits - 1` values, all below the marker.
        let below = (1 << bits) - 1;
        for _ in 0..n {
            out.push((rng(seed) % below, bits));
        }
    }
    out
}

/// Zeroes every line of `g` for which `keep` is false.
fn keep_lines(g: &mut GrSpec, keep: impl Fn(usize) -> bool) {
    for (i, v) in g.xr.iter_mut().enumerate() {
        if !keep(i) {
            *v = 0;
        }
    }
}

/// Block kinds of a granule.
#[derive(Clone, Copy)]
enum Kind {
    Long,
    Short,
    Mixed,
}

/// `scalefac_compress` 300 (slen 3 3 3 0) for an ordinary channel; 244 (`c = 122`: slen
/// 3 2 2 0) or, with `slen0`, 150 (`c = 75`: slen 2 0 3 0) for an intensity right channel.
fn lsf_granule(kind: Kind, right: Option<bool>, bv: u16, quads: usize, seed: &mut u32) -> GrSpec {
    let (compress, slens) = match right {
        None => (300, [3, 3, 3, 0]),
        Some(false) => (244, [3, 2, 2, 0]),
        Some(true) => (150, [2, 0, 3, 0]),
    };
    // nr_of_sfb: ordinary set 0, intensity right set 3 (long, mixed, short).
    let counts: [usize; 4] = match (kind, right.is_some()) {
        (Kind::Long, false) => [6, 5, 5, 5],
        (Kind::Mixed, false) => [6, 9, 9, 9],
        (Kind::Short, false) => [9, 9, 9, 9],
        (Kind::Long, true) => [7, 7, 7, 0],
        (Kind::Mixed, true) => [6, 15, 12, 0],
        (Kind::Short, true) => [12, 12, 12, 0],
    };
    let parts: Vec<(usize, u32)> = counts.into_iter().zip(slens).collect();
    let (mut info, regions) = match kind {
        Kind::Long => (long_info(compress, bv, [13, 7, 24], 3, 2), [24, 44]),
        Kind::Short => (short_info(compress, bv, [9, 15], false), [36, 576]),
        Kind::Mixed => (short_info(compress, bv, [13, 9], true), [36, 576]),
    };
    info.global_gain = 170;
    // An intensity right channel sends positions, which the two decoders do not always agree
    // on: see `sfs_legal`.
    let sf = if right.is_some() {
        sfs_legal(&parts, seed)
    } else {
        sfs(&parts, seed)
    };
    granule(info, regions, sf, quads, seed)
}

/// A frame of one granule per channel.
fn frame(h: &FrameHeader, mut chans: Vec<GrSpec>) -> Vec<u8> {
    let mut specs = vec![std::mem::take(&mut chans)];
    let (si, w) = assemble(&mut specs, [0, 0], 0);
    frame_bytes(h, &si, &w.bytes)
}

fn m25_8k_mixed() -> Vec<u8> {
    let mut seed = 0x8000_0001;
    let h = m25_mono();
    (0..3u8)
        .flat_map(|n| {
            // Region 0 of a mixed block is the 72 long lines at 8 kHz.
            let compress = 300;
            let sf = sfs(&[(6, 3), (9, 3), (9, 3)], &mut seed);
            let mut info = short_info(compress, 160 + 20 * u16::from(n), [13, 9], true);
            info.global_gain = 170;
            info.scalefac_scale = n == 1;
            info.subblock_gain = [n % 3, 1, (n + 1) % 3];
            let g = granule(info, [72, 576], sf, 24, &mut seed);
            frame(&h, vec![g])
        })
        .collect()
}

fn lsf_mixed_intensity() -> Vec<u8> {
    let mut seed = 0x1500_0001;
    let mut out = Vec::new();
    // Mixed rows at 22.05 kHz: six 6-line long bands (lines 0-35), then short band 3 (6 lines
    // per window: 36-41, 42-47, 48-53), band 4 (6), band 5 (8: 72-79, 80-87, 88-95), ...
    for (n, mode_ext) in [1u8, 1, 3, 1].into_iter().enumerate() {
        let left = lsf_granule(Kind::Mixed, None, 120, 10, &mut seed);
        let mut right = lsf_granule(Kind::Mixed, Some(false), 60, 0, &mut seed);
        match n {
            // Every short window empty: long bands 3-5 lie above the last non-zero line (17).
            0 => keep_lines(&mut right, |i| i < 18),
            // One non-zero line in window 1 of band 3: the long bands are not intensity.
            1 | 2 => {
                keep_lines(&mut right, |i| i < 18 || i == 44);
                right.xr[44] = 3;
            }
            // Only window 2 of band 5 is non-zero above the long part.
            _ => {
                keep_lines(&mut right, |i| i < 12 || (88..96).contains(&i));
                right.xr[90] = -2;
            }
        }
        out.extend(frame(&lsf_joint(mode_ext), vec![left, right]));
    }
    out
}

fn lsf_layouts() -> Vec<u8> {
    let mut seed = 0x1A70_0001;
    let mut out = Vec::new();
    for (mode_ext, kl, kr) in [
        (3u8, Kind::Long, Kind::Short),
        (2, Kind::Short, Kind::Long),
        (1, Kind::Mixed, Kind::Short),
        (2, Kind::Mixed, Kind::Long),
    ] {
        let intensity = mode_ext & 1 != 0;
        let left = lsf_granule(kl, None, 100, 8, &mut seed);
        let mut right = lsf_granule(kr, intensity.then_some(false), 50, 4, &mut seed);
        if intensity {
            keep_lines(&mut right, |i| i < 30);
        }
        out.extend(frame(&lsf_joint(mode_ext), vec![left, right]));
    }
    out
}

fn lsf_slen0() -> Vec<u8> {
    let mut seed = 0x51E0_0001;
    let mut out = Vec::new();
    for (mode_ext, kind, scale) in [
        (1u8, Kind::Long, false),
        (3, Kind::Long, true),
        (1, Kind::Short, false),
    ] {
        let left = lsf_granule(kind, None, 120, 10, &mut seed);
        let mut right = lsf_granule(kind, Some(true), 40, 0, &mut seed);
        // Long: bands 0-3 (lines 0-23) carry data, so bands 4-20 are intensity: 4-6 with
        // 2-bit positions, 7-13 in the slen-0 partition, 14-20 with 3-bit positions.
        // Short: band 0 of each window (lines 0-11) carries data; entries 12-23 (bands 4-7)
        // are the slen-0 partition.
        let limit = match kind {
            Kind::Long => 24,
            _ => 12,
        };
        keep_lines(&mut right, |i| i < limit);
        // Bit 0 of the right channel's scalefac_compress is the intensity scale.
        right.info.scalefac_compress |= u16::from(scale);
        out.extend(frame(&lsf_joint(mode_ext), vec![left, right]));
    }
    out
}

/// The black-box streams: name, sample rate, channels, bytes.
fn streams() -> Vec<(&'static str, u32, u16, Vec<u8>)> {
    vec![
        ("bb-m25-8k-mixed", 8000, 1, m25_8k_mixed()),
        ("bb-lsf-mixed-intensity", 22050, 2, lsf_mixed_intensity()),
        ("bb-lsf-layouts", 22050, 2, lsf_layouts()),
        ("bb-lsf-slen0", 22050, 2, lsf_slen0()),
    ]
}

/// Our decode of a stream of back-to-back frames, interleaved.
fn decode_stream(rate: u32, channels: u16, data: &[u8]) -> Vec<f32> {
    let mut d =
        MpegDecoder::new(&track(CodecId::Mp3, rate, channels), &Options::default()).unwrap();
    let (mut out, mut pcm) = (AudioBuffer::new(), Vec::new());
    let mut at = 0;
    let mut pts = 0;
    while at < data.len() {
        let h = parse_header(data[at..at + 4].try_into().unwrap()).unwrap();
        let p = Packet {
            track: 0,
            pts,
            frames: h.samples,
            offset: at as u64,
            data: data[at..at + h.frame_len].to_vec(),
        };
        d.decode(&p, &mut out).unwrap();
        for i in 0..out.frames() {
            for c in 0..usize::from(channels) {
                pcm.push(out.plane_f32(c).unwrap()[i]);
            }
        }
        at += h.frame_len;
        pts += u64::from(h.samples);
    }
    pcm
}

/// RMS and maximum absolute error per channel.
fn errors(got: &[f32], want: &[f32], channels: usize) -> Vec<(f64, f64)> {
    (0..channels)
        .map(|c| {
            let (mut sum, mut max, mut n) = (0.0f64, 0.0f64, 0usize);
            for (g, w) in got.iter().zip(want).skip(c).step_by(channels) {
                let e = (f64::from(*g) - f64::from(*w)).abs();
                sum += e * e;
                max = max.max(e);
                n += 1;
            }
            ((sum / n as f64).sqrt(), max)
        })
        .collect()
}

fn fixture_path(name: &str) -> String {
    format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))
}

/// Writes the streams and our decode of them (`.ours.f32`) to `$VYNAMP_BLACKBOX_OUT`, and, if
/// FFmpeg's decode (`.f32`) is already there, prints the comparison.
#[test]
#[ignore = "writes scratch files for the black-box comparison"]
fn write_blackbox_streams() {
    let dir = std::env::var("VYNAMP_BLACKBOX_OUT").expect("VYNAMP_BLACKBOX_OUT");
    for (name, rate, channels, data) in streams().into_iter().chain(controls()) {
        std::fs::write(format!("{dir}/{name}.mp3"), &data).unwrap();
        let ours = decode_stream(rate, channels, &data);
        let bytes: Vec<u8> = ours.iter().flat_map(|v| v.to_le_bytes()).collect();
        std::fs::write(format!("{dir}/{name}.ours.f32"), bytes).unwrap();
        if let Ok(theirs) = std::fs::read(format!("{dir}/{name}.f32")) {
            let theirs: Vec<f32> = theirs
                .as_chunks::<4>()
                .0
                .iter()
                .map(|b| f32::from_le_bytes(*b))
                .collect();
            let ch = usize::from(channels);
            println!(
                "{name}: {} bytes, {} samples ours, {} theirs",
                data.len(),
                ours.len(),
                theirs.len()
            );
            for (k, (o, t)) in ours
                .chunks(576 * ch)
                .zip(theirs.chunks(576 * ch))
                .enumerate()
            {
                println!("  frame {k}: (rms, max) per channel {:?}", errors(o, t, ch));
            }
        }
    }
}

#[test]
fn hand_built_streams_match_the_black_box_reference() {
    let rms_limit = 2f64.powi(-15) / 12f64.sqrt();
    let max_limit = 2f64.powi(-14);
    let mut failures = Vec::new();
    for (name, rate, channels, data) in streams() {
        let committed = std::fs::read(fixture_path(&format!("{name}.mp3"))).unwrap();
        assert!(
            committed == data,
            "{name}: the committed stream is not the generated one"
        );
        let reference: Vec<f32> = std::fs::read(fixture_path(&format!("{name}.f32")))
            .unwrap()
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b))
            .collect();
        let ours = decode_stream(rate, channels, &data);
        assert_eq!(ours.len(), reference.len(), "{name}: length");
        assert!(ours.iter().any(|&v| v.abs() > 0.01), "{name}: silent");
        for (c, (rms, max)) in errors(&ours, &reference, usize::from(channels))
            .into_iter()
            .enumerate()
        {
            println!("{name} ch{c}: rms {rms:.3e} max {max:.3e}");
            if rms >= rms_limit {
                failures.push(format!("{name} ch{c}: rms {rms:e}"));
            }
            if max > max_limit {
                failures.push(format!("{name} ch{c}: max {max:e}"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Control streams for the scratch comparison: the same tool chain without the behaviour
/// under test.
fn controls() -> Vec<(&'static str, u32, u16, Vec<u8>)> {
    let mut seed = 0xC0_0001;
    let lsf = |kind: Kind, mode: u8, seed: &mut u32| {
        let h = header(2, 14, 0, mode, 0, false);
        let chans = (0..2)
            .map(|_| lsf_granule(kind, None, 100, 8, seed))
            .collect();
        frame(&h, chans)
    };
    let long: Vec<u8> = (0..3).flat_map(|_| lsf(Kind::Long, 0, &mut seed)).collect();
    let short: Vec<u8> = (0..3)
        .flat_map(|_| lsf(Kind::Short, 0, &mut seed))
        .collect();
    let mixed: Vec<u8> = (0..3)
        .flat_map(|_| lsf(Kind::Mixed, 0, &mut seed))
        .collect();
    let ms: Vec<u8> = (0..3).flat_map(|_| lsf(Kind::Long, 1, &mut seed)).collect();
    let m25 = |kind: Kind, seed: &mut u32| {
        let h = m25_mono();
        let compress = 300;
        let (mut info, regions, parts) = match kind {
            Kind::Long => (
                long_info(compress, 160, [13, 7, 24], 3, 2),
                // 8 kHz long bands: 12 12 12 12 | 12 12 16 | ...
                [48, 88],
                vec![(6, 3), (5, 3), (5, 3)],
            ),
            _ => (
                short_info(compress, 160, [13, 9], false),
                [72, 576],
                vec![(9, 3), (9, 3), (9, 3)],
            ),
        };
        info.global_gain = 170;
        let sf = sfs(&parts, seed);
        frame(&h, vec![granule(info, regions, sf, 24, seed)])
    };
    let m25_long: Vec<u8> = (0..3).flat_map(|_| m25(Kind::Long, &mut seed)).collect();
    let m25_short: Vec<u8> = (0..3).flat_map(|_| m25(Kind::Short, &mut seed)).collect();
    let is = |kind: Kind, ext: u8, scale: bool, limit: usize, seed: &mut u32| {
        let left = lsf_granule(kind, None, 120, 10, seed);
        let mut right = lsf_granule(kind, Some(false), 40, 0, seed);
        keep_lines(&mut right, |i| i < limit);
        right.info.scalefac_compress |= u16::from(scale);
        let f = frame(&lsf_joint(ext), vec![left, right]);
        [f.clone(), f].concat()
    };
    let i1 = is(Kind::Long, 1, false, 24, &mut seed);
    let i2 = is(Kind::Long, 1, true, 24, &mut seed);
    let i3 = is(Kind::Long, 3, false, 24, &mut seed);
    let i4 = is(Kind::Short, 1, false, 12, &mut seed);
    let i5 = is(Kind::Short, 3, false, 12, &mut seed);
    let i6 = is(Kind::Mixed, 1, false, 18, &mut seed);
    let i7 = is(Kind::Mixed, 1, false, 0, &mut seed);
    let i8 = is(Kind::Long, 1, false, 0, &mut seed);
    let i9 = {
        let left = lsf_granule(Kind::Short, None, 120, 10, &mut seed);
        let mut right = lsf_granule(Kind::Short, Some(true), 40, 0, &mut seed);
        keep_lines(&mut right, |i| i < 12);
        let f = frame(&lsf_joint(1), vec![left, right]);
        [f.clone(), f].concat()
    };
    let i10 = {
        let mut s = Vec::new();
        for kind in [Kind::Long, Kind::Short] {
            let chans = (0..2)
                .map(|_| lsf_granule(kind, None, 100, 8, &mut seed))
                .collect();
            s.extend(frame(&header(2, 14, 0, 0, 0, false), chans));
        }
        s
    };
    vec![
        ("ctl-is-long", 22050, 2, i1),
        ("ctl-is-long-scale1", 22050, 2, i2),
        ("ctl-is-long-ms", 22050, 2, i3),
        ("ctl-is-short", 22050, 2, i4),
        ("ctl-is-short-ms", 22050, 2, i5),
        ("ctl-is-mixed-long-only", 22050, 2, i6),
        ("ctl-is-mixed-empty", 22050, 2, i7),
        ("ctl-is-long-empty", 22050, 2, i8),
        ("ctl-is-short-slen0", 22050, 2, i9),
        ("ctl-long-then-short", 22050, 2, i10),
        ("ctl-lsf-long", 22050, 2, long),
        ("ctl-lsf-short", 22050, 2, short),
        ("ctl-lsf-mixed", 22050, 2, mixed),
        ("ctl-lsf-ms", 22050, 2, ms),
        ("ctl-m25-long", 8000, 1, m25_long),
        ("ctl-m25-short", 8000, 1, m25_short),
    ]
}

/// Intensity probes for the scratch comparison: the left channel has one non-zero line (in
/// long band `band`), the right channel no lines and every intensity position `pos`. With
/// one line, the outputs of the two channels are the same waveform scaled by `kl` and `kr`.
fn intensity_probes() -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    // Long bands at 22.05 kHz start at 0 6 12 18 24 30 36 44 54 66 80 96 116 140 168 200 238
    // 284 336 396 464 522.
    let probe = |ext: u8, scale: bool, line: usize, pos: u32| {
        let lsf_long = |compress: u16, sf: Vec<(u32, u32)>, x: Option<(usize, i16)>| {
            let mut info = long_info(compress, 288, [13, 13, 13], 3, 2);
            info.global_gain = 190;
            let mut g = GrSpec::new(info);
            g.regions = [24, 44];
            g.sf = sf;
            if let Some((i, v)) = x {
                g.xr[i] = v;
            }
            g
        };
        // Left: compress 0 (all slen 0, no scalefactors). Right: c = 129, slen 3 3 3 0, so
        // positions 0-6 are legal and 7 is not; bit 0 is the intensity scale.
        let left = lsf_long(0, Vec::new(), Some((line, 9)));
        let mut right = lsf_long(258 | u16::from(scale), vec![(pos, 3); 21], None);
        right.info.big_values = 0;
        let f = frame(&lsf_joint(ext), vec![left, right]);
        [f.clone(), f].concat()
    };
    for line in [3usize, 100, 530] {
        out.push((format!("probe-plain-{line}"), probe(0, false, line, 0)));
        for scale in [false, true] {
            for pos in 0..8 {
                let name = format!("probe-is-{line}-s{}-p{pos}", u8::from(scale));
                out.push((name, probe(1, scale, line, pos)));
            }
        }
    }
    out
}

#[test]
#[ignore = "writes and analyses scratch files for the black-box comparison"]
fn intensity_probe_gains() {
    let dir = std::env::var("VYNAMP_BLACKBOX_OUT").expect("VYNAMP_BLACKBOX_OUT");
    let read = |name: &str| -> Option<Vec<f32>> {
        let b = std::fs::read(format!("{dir}/{name}")).ok()?;
        Some(
            b.as_chunks::<4>()
                .0
                .iter()
                .map(|b| f32::from_le_bytes(*b))
                .collect(),
        )
    };
    // (left·plain / plain², right·plain / plain²) over the second frame.
    let gains = |pcm: &[f32], plain: &[f32]| {
        let (mut l, mut r, mut p2) = (0.0f64, 0.0f64, 0.0f64);
        for i in 576..1152 {
            let p = f64::from(plain[2 * i]);
            l += f64::from(pcm[2 * i]) * p;
            r += f64::from(pcm[2 * i + 1]) * p;
            p2 += p * p;
        }
        (l / p2, r / p2)
    };
    for (name, data) in intensity_probes() {
        std::fs::write(format!("{dir}/{name}.mp3"), &data).unwrap();
        let ours = decode_stream(22050, 2, &data);
        let bytes: Vec<u8> = ours.iter().flat_map(|v| v.to_le_bytes()).collect();
        std::fs::write(format!("{dir}/{name}.ours.f32"), bytes).unwrap();
    }
    for line in [3usize, 100, 530] {
        let (Some(plain), Some(plain_ours)) = (
            read(&format!("probe-plain-{line}.f32")),
            read(&format!("probe-plain-{line}.ours.f32")),
        ) else {
            continue;
        };
        for scale in 0..2 {
            for pos in 0..8 {
                let name = format!("probe-is-{line}-s{scale}-p{pos}");
                let (Some(t), Some(o)) = (
                    read(&format!("{name}.f32")),
                    read(&format!("{name}.ours.f32")),
                ) else {
                    continue;
                };
                let (tl, tr) = gains(&t, &plain);
                let (ol, or) = gains(&o, &plain_ours);
                println!(
                    "line {line} scale {scale} pos {pos}: ffmpeg kl {tl:.4} kr {tr:.4} | ours kl {ol:.4} kr {or:.4}"
                );
            }
        }
    }
}
