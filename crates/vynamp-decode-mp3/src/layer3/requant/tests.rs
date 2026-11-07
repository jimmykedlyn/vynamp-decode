use super::super::scalefactors::Layout;
use super::super::sideinfo::GranuleInfo;
use super::super::tables::{PRETAB, sfb_row};
use super::super::{Granule, GranuleStatus};
use super::*;
use crate::common::Rng;
use vynamp_decode_mpa::MpegVersion;

/// The definition, literally, in `f64`: `sign(x) |x|^(4/3) 2^((global_gain - 210) / 4)` times
/// `2^(-8 subblock_gain / 4)` for short windows and `2^(-m (scalefac + preflag pretab))`.
pub(in super::super) fn reference(g: &Granule) -> Vec<f64> {
    let mut out = vec![0.0; 576];
    let mult = if g.info.scalefac_scale { 1.0 } else { 0.5 };
    let mut line = 0;
    for (e, &w) in g.layout.widths.iter().enumerate() {
        let mut exp = (f64::from(g.info.global_gain) - 210.0) / 4.0;
        let mut sf = f64::from(g.scalefac[e]);
        if e < g.layout.n_long {
            if g.preflag {
                sf += f64::from(PRETAB[e]);
            }
        } else {
            exp -= 2.0 * f64::from(g.info.subblock_gain[(e - g.layout.n_long) % 3]);
        }
        exp -= mult * sf;
        for _ in 0..w {
            let x = f64::from(g.xr[line]);
            out[line] = x.signum() * x.abs().powf(4.0 / 3.0) * exp.exp2() * f64::from(x != 0.0);
            line += 1;
        }
    }
    out
}

fn granule(version: MpegVersion, rate: u32, info: GranuleInfo, rng: &mut Rng) -> Granule {
    let mut g = Granule::empty();
    let layout = Layout::new(sfb_row(version, rate).unwrap(), version, &info);
    g.clear(GranuleStatus::Decoded, info, layout);
    for v in g.xr.iter_mut().take(500) {
        let mag = match rng.below(4) {
            0 => 0,
            1 => rng.below(3),
            2 => rng.below(16),
            _ => rng.below(8207),
        } as i16;
        *v = if rng.below(2) == 0 { mag } else { -mag };
    }
    g.used_lines = 500;
    for sf in g.scalefac.iter_mut().take(layout.n_sf) {
        *sf = rng.below(16) as u8;
    }
    g.preflag = info.preflag;
    g
}

fn check(g: &Granule, what: &str) {
    let mut out = [0.0f32; 576];
    requantize(g, &mut out);
    let want = reference(g);
    let mut checked = 0;
    for i in 0..576 {
        let (a, b) = (f64::from(out[i]), want[i]);
        assert!(
            (a - b).abs() <= 2e-6 * b.abs(),
            "{what}: line {i}: {a} vs {b} (x = {})",
            g.xr[i]
        );
        checked += usize::from(b != 0.0);
    }
    assert!(checked > 100, "{what}: only {checked} nonzero lines");
}

fn info(block_type: u8, mixed: bool) -> GranuleInfo {
    GranuleInfo {
        window_switching: block_type != 0,
        block_type,
        mixed_block: mixed,
        ..GranuleInfo::default()
    }
}

fn pow43(x: usize) -> f32 {
    tables().pow43[x]
}

#[test]
fn x_to_the_four_thirds_table() {
    assert_eq!(pow43(0), 0.0);
    assert_eq!(pow43(1), 1.0);
    assert_eq!(pow43(8), 16.0);
    assert_eq!(pow43(27), 81.0);
    for x in [2usize, 3, 15, 16, 100, 1000, 8191, 8205, 8206] {
        let want = (x as f64).powf(4.0 / 3.0);
        assert!((f64::from(pow43(x)) - want).abs() <= 1e-7 * want, "x = {x}");
    }
}

#[test]
fn long_blocks_follow_the_definition() {
    let mut rng = Rng(11);
    for (scale, preflag) in [(false, false), (true, false), (false, true), (true, true)] {
        for gain in [0u8, 77, 140, 210, 255] {
            let gi = GranuleInfo {
                global_gain: gain,
                scalefac_scale: scale,
                preflag,
                ..info(0, false)
            };
            check(
                &granule(MpegVersion::V1, 44100, gi, &mut rng),
                &format!("scale {scale} preflag {preflag} gain {gain}"),
            );
        }
    }
    // Start and stop blocks use the long bands, MPEG-2 the derived preflag.
    for bt in [1, 3] {
        let gi = GranuleInfo {
            global_gain: 150,
            ..info(bt, false)
        };
        check(&granule(MpegVersion::V2, 22050, gi, &mut rng), "ws long");
    }
    let mut g = granule(MpegVersion::V2, 22050, info(0, false), &mut rng);
    g.info.global_gain = 160;
    g.preflag = true;
    check(&g, "derived preflag");
}

#[test]
fn short_and_mixed_blocks_apply_the_subblock_gain_per_window() {
    let mut rng = Rng(12);
    for (v, rate) in [
        (MpegVersion::V1, 48000),
        (MpegVersion::V2, 24000),
        (MpegVersion::V25, 8000),
    ] {
        for mixed in [false, true] {
            let gi = GranuleInfo {
                global_gain: 170,
                subblock_gain: [0, 3, 7],
                scalefac_scale: mixed,
                preflag: true,
                ..info(2, mixed)
            };
            check(
                &granule(v, rate, gi, &mut rng),
                &format!("{v:?} mixed {mixed}"),
            );
        }
    }
}

#[test]
fn extreme_values_stay_finite_and_zero_lines_stay_zero() {
    let mut rng = Rng(13);
    let mut g = granule(
        MpegVersion::V1,
        44100,
        GranuleInfo {
            global_gain: 255,
            scalefac_scale: false,
            ..info(0, false)
        },
        &mut rng,
    );
    g.xr = [8206; 576];
    g.used_lines = 576;
    let mut out = [1.0f32; 576];
    requantize(&g, &mut out);
    assert!(out.iter().all(|v| v.is_finite() && *v > 0.0));
    // Beyond `used_lines` nothing is read: the output is cleared there.
    g.xr = [5; 576];
    g.used_lines = 100;
    g.info.global_gain = 140;
    requantize(&g, &mut out);
    assert!(out[..100].iter().all(|v| *v > 0.0));
    assert!(out[100..].iter().all(|v| *v == 0.0));
    // The smallest gain with the largest scalefactors does not underflow to NaN.
    g.info.global_gain = 0;
    g.info.scalefac_scale = true;
    g.scalefac = [31; 40];
    g.used_lines = 576;
    requantize(&g, &mut out);
    assert!(out.iter().all(|v| v.is_finite() && *v >= 0.0));
}

#[test]
fn short_blocks_are_reordered_into_interleaved_windows() {
    for (v, rate) in [
        (MpegVersion::V1, 44100),
        (MpegVersion::V1, 32000),
        (MpegVersion::V2, 22050),
        (MpegVersion::V25, 8000),
    ] {
        for mixed in [false, true] {
            let layout = Layout::new(sfb_row(v, rate).unwrap(), v, &info(2, mixed));
            let mut xr: [f32; 576] = std::array::from_fn(|i| i as f32);
            reorder(&mut xr, &layout);
            // Expected, band by band: transmitted `first + w * width + k` goes to
            // `first + 3 k + w`.
            let mut want = [0.0f32; 576];
            let long_lines = layout.bound(layout.n_long);
            for (i, w) in want.iter_mut().enumerate().take(long_lines) {
                *w = i as f32;
            }
            let mut first = long_lines;
            for band in layout.widths[layout.n_long..].chunks(3) {
                let width = usize::from(band[0]);
                assert!(band.iter().all(|&b| usize::from(b) == width));
                for w in 0..3 {
                    for k in 0..width {
                        want[first + 3 * k + w] = (first + w * width + k) as f32;
                    }
                }
                first += 3 * width;
            }
            assert_eq!(first, 576);
            assert!(xr == want, "{v:?} {rate} mixed {mixed}");
            // A permutation of the lines.
            let mut seen: Vec<i32> = xr.iter().map(|&v| v as i32).collect();
            seen.sort_unstable();
            assert!(seen.iter().enumerate().all(|(i, &v)| i as i32 == v));
        }
    }
    // Spot check at 44.1 kHz: band 0 is four lines per window.
    let layout = Layout::new(5, MpegVersion::V1, &info(2, false));
    let mut xr: [f32; 576] = std::array::from_fn(|i| i as f32);
    reorder(&mut xr, &layout);
    assert_eq!(
        &xr[..12],
        &[0.0, 4.0, 8.0, 1.0, 5.0, 9.0, 2.0, 6.0, 10.0, 3.0, 7.0, 11.0]
    );
}
