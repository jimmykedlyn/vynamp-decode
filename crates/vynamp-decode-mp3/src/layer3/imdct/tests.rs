use super::*;
use crate::common::Rng;
use std::f64::consts::PI;

/// The window of block type `bt` (0, 1, 3) at sample `i` of 36.
fn long_window(bt: u8, i: usize) -> f64 {
    let a = |i: usize| (PI / 36.0 * (i as f64 + 0.5)).sin();
    let b = |i: usize| (PI / 12.0 * (i as f64 + 0.5)).sin();
    match (bt, i) {
        (1, 18..24) => 1.0,
        (1, 24..30) => b(i - 18),
        (1, 30..) => 0.0,
        (3, 0..6) => 0.0,
        (3, 6..12) => b(i - 6),
        (3, 12..18) => 1.0,
        _ => a(i),
    }
}

/// `x_i = sum_k X_k cos(pi / 72 (2 i + 1 + 18) (2 k + 1))`, windowed.
fn direct36(x: &[f64; 18], bt: u8) -> [f64; 36] {
    std::array::from_fn(|i| {
        let s: f64 = (0..18)
            .map(|k| x[k] * (PI / 72.0 * (2 * i + 1 + 18) as f64 * (2 * k + 1) as f64).cos())
            .sum();
        s * long_window(bt, i)
    })
}

/// Three short windows (`x[3 k + w]`), each `y_i = sum_k X_k cos(pi / 24 (2 i + 1 + 6) (2 k + 1))`
/// times `sin(pi / 12 (i + 1 / 2))`, placed at 6, 12 and 18.
fn direct12(x: &[f64; 18]) -> [f64; 36] {
    let mut out = [0.0; 36];
    for w in 0..3 {
        for i in 0..12 {
            let s: f64 = (0..6)
                .map(|k| {
                    x[3 * k + w] * (PI / 24.0 * (2 * i + 1 + 6) as f64 * (2 * k + 1) as f64).cos()
                })
                .sum();
            out[6 + 6 * w + i] += s * (PI / 12.0 * (i as f64 + 0.5)).sin();
        }
    }
    out
}

fn random18(rng: &mut Rng) -> [f32; 18] {
    std::array::from_fn(|_| rng.unit() as f32 * 100.0)
}

fn widen(x: &[f32; 18]) -> [f64; 18] {
    x.map(f64::from)
}

#[test]
fn long_imdct_matches_the_definition_for_every_window() {
    let mut rng = Rng(51);
    for bt in [0u8, 1, 3] {
        for _ in 0..20 {
            let x = random18(&mut rng);
            let mut out = [0.0f32; 36];
            imdct36(&x, bt, &mut out);
            let want = direct36(&widen(&x), bt);
            for i in 0..36 {
                assert!(
                    (f64::from(out[i]) - want[i]).abs() < 1e-4,
                    "type {bt} sample {i}: {} vs {}",
                    out[i],
                    want[i]
                );
            }
        }
    }
}

#[test]
fn long_imdct_of_a_unit_line_is_a_windowed_cosine() {
    for k in [0usize, 1, 8, 17] {
        let mut x = [0.0f32; 18];
        x[k] = 1.0;
        let mut out = [0.0f32; 36];
        imdct36(&x, 0, &mut out);
        for (i, &v) in out.iter().enumerate() {
            let want = (PI / 72.0 * (2 * i + 19) as f64 * (2 * k + 1) as f64).cos()
                * (PI / 36.0 * (i as f64 + 0.5)).sin();
            assert!((f64::from(v) - want).abs() < 1e-6, "k {k} i {i}");
        }
    }
}

#[test]
fn short_imdct_matches_the_definition() {
    let mut rng = Rng(52);
    for _ in 0..30 {
        let x = random18(&mut rng);
        let mut out = [0.0f32; 36];
        imdct12(&x, &mut out);
        let want = direct12(&widen(&x));
        for i in 0..36 {
            assert!(
                (f64::from(out[i]) - want[i]).abs() < 1e-4,
                "sample {i}: {} vs {}",
                out[i],
                want[i]
            );
        }
        // The first and last six samples are outside every window.
        assert!(out[..6].iter().chain(&out[30..]).all(|&v| v == 0.0));
    }
}

/// The whole chain per subband in `f64`: block type per subband, IMDCT, overlap-add and
/// frequency inversion.
pub(in super::super) struct Reference {
    tail: [[f64; 18]; 32],
}

impl Reference {
    pub(in super::super) fn new() -> Self {
        Self {
            tail: [[0.0; 18]; 32],
        }
    }

    pub(in super::super) fn granule(
        &mut self,
        xr: &[f32; 576],
        info: &GranuleInfo,
    ) -> [[f64; 32]; 18] {
        let mut slots = [[0.0; 32]; 18];
        for sb in 0..32 {
            let x: [f64; 18] = std::array::from_fn(|i| f64::from(xr[18 * sb + i]));
            let short = info.block_type == 2 && !(info.mixed_block && sb < 2);
            let bt = if info.block_type == 2 {
                0
            } else {
                info.block_type
            };
            let out = if short {
                direct12(&x)
            } else {
                direct36(&x, bt)
            };
            for i in 0..18 {
                let mut v = out[i] + self.tail[sb][i];
                self.tail[sb][i] = out[18 + i];
                if sb % 2 == 1 && i % 2 == 1 {
                    v = -v;
                }
                slots[i][sb] = v;
            }
        }
        slots
    }
}

fn info(block_type: u8, mixed: bool) -> GranuleInfo {
    GranuleInfo {
        window_switching: block_type != 0,
        block_type,
        mixed_block: mixed,
        ..GranuleInfo::default()
    }
}

fn spectrum(rng: &mut Rng, used: usize) -> [f32; 576] {
    let mut a = [0.0f32; 576];
    for v in a.iter_mut().take(used) {
        *v = rng.unit() as f32 * 10.0;
    }
    a
}

fn compare(got: &[[f32; 32]; 18], want: &[[f64; 32]; 18], what: &str) {
    for t in 0..18 {
        for sb in 0..32 {
            assert!(
                (f64::from(got[t][sb]) - want[t][sb]).abs() < 2e-4,
                "{what}: slot {t} subband {sb}: {} vs {}",
                got[t][sb],
                want[t][sb]
            );
        }
    }
}

#[test]
fn overlap_add_and_frequency_inversion_follow_the_definition_across_block_types() {
    let mut rng = Rng(53);
    let mut ov = Overlap::new();
    let mut reference = Reference::new();
    // Long, start, short, short, stop, long, mixed short, stop, long.
    let kinds = [
        (0, false),
        (1, false),
        (2, false),
        (2, false),
        (3, false),
        (0, false),
        (2, true),
        (3, false),
        (0, false),
    ];
    for (n, &(bt, mixed)) in kinds.iter().enumerate() {
        let xr = spectrum(&mut rng, 576);
        let gi = info(bt, mixed);
        let mut slots = [[0.0f32; 32]; 18];
        ov.granule(&xr, &gi, &mut slots);
        compare(
            &slots,
            &reference.granule(&xr, &gi),
            &format!("granule {n}"),
        );
    }
}

#[test]
fn silent_subbands_are_skipped_but_the_overlap_still_drains() {
    let mut rng = Rng(54);
    let mut ov = Overlap::new();
    let mut reference = Reference::new();
    for (n, used) in [576usize, 100, 0, 37, 0, 0, 450].into_iter().enumerate() {
        let xr = spectrum(&mut rng, used);
        let gi = info(0, false);
        let mut slots = [[0.0f32; 32]; 18];
        ov.granule(&xr, &gi, &mut slots);
        compare(
            &slots,
            &reference.granule(&xr, &gi),
            &format!("granule {n}"),
        );
    }
}

#[test]
fn clear_forgets_the_overlap() {
    let mut rng = Rng(55);
    let mut ov = Overlap::new();
    let mut slots = [[0.0f32; 32]; 18];
    ov.granule(&spectrum(&mut rng, 576), &info(0, false), &mut slots);
    ov.clear();
    ov.granule(&[0.0; 576], &info(0, false), &mut slots);
    assert!(slots.iter().flatten().all(|&v| v == 0.0));
}

#[test]
fn a_stationary_sine_reconstructs_through_the_overlap() {
    // TDAC: windows that satisfy the Princen-Bradley condition make consecutive long blocks
    // add up; feeding the forward MDCT of a constant gives a constant (apart from the
    // aliasing terms that cancel). Use the forward transform written from its definition.
    let n = 36usize;
    let signal = |t: usize| (0.31 * t as f64).sin();
    let mut ov = Overlap::new();
    let mut out = Vec::new();
    for g in 0..6 {
        // Subband 0 only: forward MDCT of samples 18 g .. 18 g + 36 with the sine window.
        let mut xr = [0.0f32; 576];
        for (k, v) in xr.iter_mut().take(18).enumerate() {
            let s: f64 = (0..n)
                .map(|i| {
                    signal(18 * g + i)
                        * (PI / 36.0 * (i as f64 + 0.5)).sin()
                        * (PI / 72.0 * (2 * i + 19) as f64 * (2 * k + 1) as f64).cos()
                })
                .sum();
            *v = (s / 9.0) as f32;
        }
        let mut slots = [[0.0f32; 32]; 18];
        ov.granule(&xr, &info(0, false), &mut slots);
        out.extend(slots.iter().map(|s| f64::from(s[0])));
    }
    // Granule g outputs samples 18 g to 18 g + 17 once block g - 1 has contributed.
    for (t, &got) in out.iter().enumerate().skip(18) {
        let want = signal(t);
        assert!((got - want).abs() < 1e-3, "t {t}: {got} vs {want}");
    }
}
