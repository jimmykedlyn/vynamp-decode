use super::super::scalefactors::Layout;
use super::super::sideinfo::GranuleInfo;
use super::super::tables::sfb_row;
use super::super::{Granule, GranuleStatus};
use super::*;
use crate::common::Rng;
use std::f64::consts::{FRAC_1_SQRT_2, PI};
use vynamp_decode_mpa::MpegVersion::{V1, V2, V25};

fn granule(version: MpegVersion, rate: u32, info: GranuleInfo, sf: &[u8], illegal: u64) -> Granule {
    let mut g = Granule::empty();
    let layout = Layout::new(sfb_row(version, rate).unwrap(), version, &info);
    g.clear(GranuleStatus::Decoded, info, layout);
    g.scalefac[..sf.len()].copy_from_slice(sf);
    g.illegal_is_pos = illegal;
    g.used_lines = 576;
    g
}

fn long() -> GranuleInfo {
    GranuleInfo::default()
}

fn short() -> GranuleInfo {
    GranuleInfo {
        window_switching: true,
        block_type: 2,
        ..GranuleInfo::default()
    }
}

/// `(first line, width)` of every entry.
fn bands(g: &Granule) -> Vec<(usize, usize)> {
    let mut at = 0;
    g.layout
        .widths
        .iter()
        .map(|&w| {
            at += usize::from(w);
            (at - usize::from(w), usize::from(w))
        })
        .collect()
}

/// `n` random lines that are never zero (the line after the last one is then the bound),
/// then zeros.
fn random_lines(rng: &mut Rng, n: usize) -> [f32; 576] {
    let mut a = [0.0f32; 576];
    for v in a.iter_mut().take(n) {
        *v = (rng.unit() as f32) * 4.0 + 5.0f32.copysign(rng.unit() as f32);
    }
    a
}

fn close(a: f32, b: f64, what: &str) {
    assert!(
        (f64::from(a) - b).abs() <= 1e-5 * (1.0 + b.abs()),
        "{what}: {a} vs {b}"
    );
}

fn stereo(version: MpegVersion, ms: bool, intensity: bool) -> Stereo {
    Stereo {
        version,
        ms,
        intensity,
    }
}

/// MPEG-1 intensity gains `(left, right)` for position `p` (0 to 6).
fn v1_gains(p: u8) -> (f64, f64) {
    let t = (f64::from(p) * PI / 12.0).tan();
    if p == 6 {
        (1.0, 0.0)
    } else {
        (t / (1.0 + t), 1.0 / (1.0 + t))
    }
}

#[test]
fn middle_side_is_the_sum_and_difference_over_root_two() {
    let mut rng = Rng(21);
    let (l0, r0) = (random_lines(&mut rng, 576), random_lines(&mut rng, 576));
    let (mut l, mut r) = (l0, r0);
    let g = granule(V1, 44100, long(), &[], 0);
    process(&mut l, &mut r, [&g, &g], stereo(V1, true, false));
    for i in 0..576 {
        let (x, y) = (f64::from(l0[i]), f64::from(r0[i]));
        close(l[i], (x + y) * FRAC_1_SQRT_2, "l");
        close(r[i], (x - y) * FRAC_1_SQRT_2, "r");
    }
    // No tool: untouched.
    let (mut l, mut r) = (l0, r0);
    process(&mut l, &mut r, [&g, &g], stereo(V1, false, false));
    assert_eq!((l, r), (l0, r0));
}

/// 44.1 kHz long edges: band 13 is lines 90 to 109, so a right channel that is nonzero up to
/// line 99 starts the intensity region at band 14.
fn mpeg1_long(ms: bool) {
    let mut rng = Rng(22);
    let l0 = random_lines(&mut rng, 576);
    let r0 = random_lines(&mut rng, 100);
    let (mut l, mut r) = (l0, r0);
    // Positions 0 to 7 repeating; entry 21 has none and takes entry 20's.
    let sf: Vec<u8> = (0..21).map(|e| (e % 8) as u8).collect();
    let g0 = granule(V1, 44100, long(), &[], 0);
    let g1 = granule(V1, 44100, long(), &sf, 0);
    process(&mut l, &mut r, [&g0, &g1], stereo(V1, ms, true));
    for (e, (start, w)) in bands(&g1).into_iter().enumerate() {
        let pos = sf[e.min(20)];
        for i in start..start + w {
            let (x, y) = (f64::from(l0[i]), f64::from(r0[i]));
            let what = format!("band {e} line {i}");
            if e < 14 || pos == 7 {
                if ms {
                    close(l[i], (x + y) * FRAC_1_SQRT_2, &what);
                    close(r[i], (x - y) * FRAC_1_SQRT_2, &what);
                } else {
                    close(l[i], x, &what);
                    close(r[i], y, &what);
                }
            } else {
                let (gl, gr) = v1_gains(pos);
                close(l[i], x * gl, &what);
                close(r[i], x * gr, &what);
            }
        }
    }
}

#[test]
fn mpeg1_intensity_positions_and_the_illegal_value() {
    mpeg1_long(false);
}

#[test]
fn intensity_and_middle_side_share_a_frame() {
    mpeg1_long(true);
}

#[test]
fn an_empty_right_channel_is_intensity_from_the_first_band() {
    let mut rng = Rng(23);
    let l0 = random_lines(&mut rng, 576);
    let (mut l, mut r) = (l0, [0.0f32; 576]);
    let g0 = granule(V1, 48000, long(), &[], 0);
    // Position 6 sends everything to the left channel.
    let g1 = granule(V1, 48000, long(), &[6; 21], 0);
    process(&mut l, &mut r, [&g0, &g1], stereo(V1, false, true));
    for i in 0..576 {
        close(l[i], f64::from(l0[i]), "left");
        assert!(r[i].abs() < 1e-6, "right line {i}");
    }
    // Position 3 splits it evenly: tan(pi / 4) = 1.
    let (mut l, mut r) = (l0, [0.0f32; 576]);
    let g1 = granule(V1, 48000, long(), &[3; 21], 0);
    process(&mut l, &mut r, [&g0, &g1], stereo(V1, false, true));
    for i in 0..576 {
        close(l[i], f64::from(l0[i]) / 2.0, "left");
        close(r[i], f64::from(l0[i]) / 2.0, "right");
    }
}

#[test]
fn lsf_intensity_uses_the_odd_even_ratio_of_the_scale() {
    for scale in [0u16, 1] {
        let mut rng = Rng(24 + u32::from(scale));
        let l0 = random_lines(&mut rng, 576);
        let r0 = random_lines(&mut rng, 100);
        let (mut l, mut r) = (l0, r0);
        let sf: Vec<u8> = (0..21).map(|e| (e % 6) as u8).collect();
        // Entries 17 (position 5) and 19 (position 1) are marked illegal.
        let illegal = (1u64 << 17) | (1 << 19);
        let g0 = granule(V2, 22050, long(), &[], 0);
        let info1 = GranuleInfo {
            scalefac_compress: 100 + scale,
            ..long()
        };
        let g1 = granule(V2, 22050, info1, &sf, illegal);
        process(&mut l, &mut r, [&g0, &g1], stereo(V2, false, true));
        let i0 = if scale == 0 {
            2f64.powf(-0.25)
        } else {
            2f64.powf(-0.5)
        };
        // 22.05 kHz: band 11 is lines 96 to 115, so line 99 is its last nonzero one.
        let bound = 12;
        for (e, (start, w)) in bands(&g1).into_iter().enumerate() {
            let src = e.min(20);
            let pos = u32::from(sf[src]);
            for i in start..start + w {
                let (x, y) = (f64::from(l0[i]), f64::from(r0[i]));
                let what = format!("scale {scale} band {e} line {i}");
                if e < bound || illegal >> src & 1 == 1 {
                    close(l[i], x, &what);
                    close(r[i], y, &what);
                } else if pos % 2 == 1 {
                    close(l[i], x * i0.powi(pos.div_ceil(2) as i32), &what);
                    close(r[i], x, &what);
                } else {
                    close(l[i], x, &what);
                    close(r[i], x * i0.powi((pos / 2) as i32), &what);
                }
            }
        }
    }
}

#[test]
fn lsf_position_zero_copies_the_left_channel() {
    let mut rng = Rng(29);
    let l0 = random_lines(&mut rng, 576);
    let (mut l, mut r) = (l0, [0.0f32; 576]);
    let g0 = granule(V2, 24000, long(), &[], 0);
    let g1 = granule(V2, 24000, long(), &[0; 21], 0);
    process(&mut l, &mut r, [&g0, &g1], stereo(V2, false, true));
    assert_eq!((l, r), (l0, l0));
}

#[test]
fn a_partition_without_positions_is_not_intensity() {
    // Every entry illegal (all partitions of width 0): the frame is plain stereo, with MS if
    // it is on.
    let mut rng = Rng(30);
    let (l0, r0) = (random_lines(&mut rng, 576), random_lines(&mut rng, 30));
    let g0 = granule(V2, 22050, long(), &[], 0);
    let g1 = granule(V2, 22050, long(), &[0; 21], (1 << 21) - 1);
    let (mut l, mut r) = (l0, r0);
    process(&mut l, &mut r, [&g0, &g1], stereo(V2, false, true));
    assert_eq!((l, r), (l0, r0));
    process(&mut l, &mut r, [&g0, &g1], stereo(V2, true, true));
    for i in 0..576 {
        let (x, y) = (f64::from(l0[i]), f64::from(r0[i]));
        close(l[i], (x + y) * FRAC_1_SQRT_2, "l");
        close(r[i], (x - y) * FRAC_1_SQRT_2, "r");
    }
}

#[test]
fn short_blocks_find_the_bound_per_window() {
    let mut rng = Rng(26);
    let l0 = random_lines(&mut rng, 576);
    let mut r0 = [0.0f32; 576];
    let sf: Vec<u8> = (0..36).map(|e| ((e * 3) % 7) as u8).collect();
    let g0 = granule(V1, 44100, short(), &[], 0);
    let g1 = granule(V1, 44100, short(), &sf, 0);
    let bands = bands(&g1);
    // Last nonzero band of windows 0 and 1; window 2 is silent on the right.
    let last = [5usize, 9];
    for (w, &lb) in last.iter().enumerate() {
        for b in 0..=lb {
            let (start, width) = bands[3 * b + w];
            for v in &mut r0[start..start + width] {
                *v = 3.0;
            }
        }
    }
    let (mut l, mut r) = (l0, r0);
    process(&mut l, &mut r, [&g0, &g1], stereo(V1, false, true));
    let mut intensity_bands = 0;
    for (e, &(start, w)) in bands.iter().enumerate() {
        let (band, win) = (e / 3, e % 3);
        let intensity = win == 2 || band > last[win];
        // Band 12 has no scalefactor and takes band 11's position of the same window.
        let pos = sf[if e >= 36 { e - 3 } else { e }];
        for i in start..start + w {
            let what = format!("band {band} window {win} line {i}");
            let (x, y) = (f64::from(l0[i]), f64::from(r0[i]));
            if intensity && pos != 7 {
                let (gl, gr) = v1_gains(pos);
                close(l[i], x * gl, &what);
                close(r[i], x * gr, &what);
                intensity_bands += 1;
            } else {
                close(l[i], x, &what);
                close(r[i], y, &what);
            }
        }
    }
    assert!(intensity_bands > 200);
}

#[test]
fn mixed_blocks_treat_the_long_part_as_one_more_band_group() {
    // The right channel is nonzero in the long part (lines 0 to 35) and in window 0 of the
    // short bands up to band 4; the long part then has no intensity bands, the windows do.
    let mut rng = Rng(31);
    let info = GranuleInfo {
        mixed_block: true,
        ..short()
    };
    let l0 = random_lines(&mut rng, 576);
    let mut r0 = random_lines(&mut rng, 36);
    let sf: Vec<u8> = (0..35).map(|e| (e % 6) as u8).collect();
    let g0 = granule(V1, 44100, info, &[], 0);
    let g1 = granule(V1, 44100, info, &sf, 0);
    assert_eq!(g1.layout.n_long, 8);
    let bands = bands(&g1);
    for b in 0..=1 {
        let (start, width) = bands[8 + 3 * b];
        for v in &mut r0[start..start + width] {
            *v = 2.0;
        }
    }
    let (mut l, mut r) = (l0, r0);
    process(&mut l, &mut r, [&g0, &g1], stereo(V1, false, true));
    assert_eq!(&l[..36], &l0[..36]);
    assert_eq!(&r[..36], &r0[..36]);
    for (e, &(start, _)) in bands.iter().enumerate().skip(8) {
        let (band, win) = ((e - 8) / 3, (e - 8) % 3);
        let intensity = win != 0 || band > 1;
        let pos = sf[if e >= 35 { e - 3 } else { e }];
        let (x, y) = (f64::from(l0[start]), f64::from(r0[start]));
        if intensity {
            let (gl, gr) = v1_gains(pos);
            close(l[start], x * gl, &format!("entry {e}"));
            close(r[start], x * gr, &format!("entry {e}"));
        } else {
            close(l[start], x, &format!("entry {e}"));
            close(r[start], y, &format!("entry {e}"));
        }
    }
}

#[test]
fn channels_with_different_block_layouts_skip_intensity() {
    let mut rng = Rng(27);
    let l0 = random_lines(&mut rng, 576);
    let r0 = random_lines(&mut rng, 50);
    let (mut l, mut r) = (l0, r0);
    let g0 = granule(V1, 44100, long(), &[], 0);
    let g1 = granule(V1, 44100, short(), &[2; 36], 0);
    process(&mut l, &mut r, [&g0, &g1], stereo(V1, false, true));
    assert_eq!((l, r), (l0, r0));
}

#[test]
fn garbage_scalefactors_never_panic() {
    let mut rng = Rng(28);
    for (v, rate) in [(V1, 44100), (V2, 24000), (V25, 8000)] {
        for info in [
            long(),
            short(),
            GranuleInfo {
                mixed_block: true,
                ..short()
            },
        ] {
            let sf: Vec<u8> = (0..40).map(|_| rng.below(256) as u8).collect();
            let g0 = granule(v, rate, info, &[], 0);
            let g1 = granule(
                v,
                rate,
                info,
                &sf,
                u64::from(rng.next()) * u64::from(rng.next()),
            );
            let (mut l, mut r) = (random_lines(&mut rng, 576), random_lines(&mut rng, 200));
            process(&mut l, &mut r, [&g0, &g1], stereo(v, true, true));
            assert!(l.iter().chain(&r).all(|v| v.is_finite()));
        }
    }
}
