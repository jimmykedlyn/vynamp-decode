//! Inverse MDCT, windowing, overlap-add and frequency inversion (ISO/IEC 11172-3 section
//! 2.4.3.4.10 and 2.4.3.4.11).
//!
//! Every subband turns its 18 lines into 36 windowed samples (a long block) or into three
//! overlapping 12-sample windowed blocks (a short block); the first 18 samples are added to
//! the second half of the previous granule's result and the second 18 are kept for the next.
//! Odd samples of odd subbands are then negated (frequency inversion) so that the polyphase
//! filterbank sees the spectrum the right way round.
//!
//! # Fast transform
//!
//! The standard's definitions are `x_i = sum_k X_k cos(pi / 2n (2 i + 1 + n / 2) (2 k + 1))`
//! for `n` = 36 (18 lines) or 12 (6 lines per window). With `N = n / 2` and
//! `D(m) = sum_k X_k cos(pi / 4N (2 m + 1) (2 k + 1))`, the DCT-IV of the lines, the outputs
//! are `x_i = D(i + N / 2)`, and `D(2N - 1 - m) = -D(m)`, `D(m + 2N) = -D(m)` fold everything
//! onto `d = D(0..N)`: `x_i = d[i + N / 2]` for `i < N / 2`, `-d[3N / 2 - 1 - i]` up to
//! `i < 3N / 2` and `-d[i - 3N / 2]` after. The DCT-IV of size `N` (18 or 6) is computed with a
//! complex DFT of size `N / 2` (9 or 3) between two twiddle multiplications: with
//! `z_m = x_{2m} + i x_{N-1-2m}`, `Y_p = e^{-i pi (p + 1/8) / N} DFT(z_m e^{-i pi (m + 1/8) / N})_p`,
//! `d_{2p} = Re Y_p` and `d_{N-1-2p} = -Im Y_p`. The DFT of size 9 is three DFTs of size 3, a
//! twiddle and three more; the tests compare the whole chain with the definition.

use super::sideinfo::GranuleInfo;
use std::f64::consts::PI;
use std::sync::OnceLock;

type Complex = (f32, f32);

fn mul(a: Complex, b: Complex) -> Complex {
    (a.0 * b.0 - a.1 * b.1, a.0 * b.1 + a.1 * b.0)
}

struct Tables {
    /// Windows of block types 0, 1, 2 (unused: a copy of type 0) and 3.
    long: [[f32; 36]; 4],
    /// `sin(pi / 12 (i + 1/2))`.
    short: [f32; 12],
    /// `e^{-i pi (n + 1/8) / 18}` for n in 0..9 and `e^{-i pi (n + 1/8) / 6}` for n in 0..3.
    twiddle18: [Complex; 9],
    twiddle6: [Complex; 3],
    /// `e^{-2 pi i k / 9}` for k in 0..5.
    w9: [Complex; 5],
}

fn tables() -> &'static Tables {
    static T: OnceLock<Tables> = OnceLock::new();
    T.get_or_init(|| {
        let sin36 = |i: usize| (PI / 36.0 * (i as f64 + 0.5)).sin();
        let sin12 = |i: usize| (PI / 12.0 * (i as f64 + 0.5)).sin();
        let mut long = [[0.0f32; 36]; 4];
        for (bt, w) in long.iter_mut().enumerate() {
            for (i, v) in w.iter_mut().enumerate() {
                *v = match (bt, i) {
                    (1, 18..24) => 1.0,
                    (1, 24..30) => sin12(i - 18),
                    (1, 30..) => 0.0,
                    (3, 0..6) => 0.0,
                    (3, 6..12) => sin12(i - 6),
                    (3, 12..18) => 1.0,
                    _ => sin36(i),
                } as f32;
            }
        }
        let tw = |n: usize, big: f64| {
            let a = PI * (n as f64 + 0.125) / big;
            (a.cos() as f32, -a.sin() as f32)
        };
        Tables {
            long,
            short: std::array::from_fn(|i| sin12(i) as f32),
            twiddle18: std::array::from_fn(|n| tw(n, 18.0)),
            twiddle6: std::array::from_fn(|n| tw(n, 6.0)),
            w9: std::array::from_fn(|k| {
                let a = 2.0 * PI * k as f64 / 9.0;
                (a.cos() as f32, -a.sin() as f32)
            }),
        }
    })
}

/// Builds the tables now so that decoding never initializes them.
pub(super) fn warm_up() {
    tables();
}

/// The 3-point DFT.
fn dft3(a0: Complex, a1: Complex, a2: Complex) -> [Complex; 3] {
    const H: f32 = 0.866_025_4; // sqrt(3) / 2
    let t = (a1.0 + a2.0, a1.1 + a2.1);
    let m = (a0.0 - 0.5 * t.0, a0.1 - 0.5 * t.1);
    let s = (H * (a1.0 - a2.0), H * (a1.1 - a2.1));
    [
        (a0.0 + t.0, a0.1 + t.1),
        (m.0 + s.1, m.1 - s.0),
        (m.0 - s.1, m.1 + s.0),
    ]
}

/// The 9-point DFT as 3 x 3: index `n = 3 a + b` in, `k = c + 3 d` out.
fn dft9(x: &[Complex; 9], w9: &[Complex; 5]) -> [Complex; 9] {
    let mut v = [[(0.0f32, 0.0f32); 3]; 3];
    for (b, row) in v.iter_mut().enumerate() {
        let u = dft3(x[b], x[3 + b], x[6 + b]);
        for (c, e) in row.iter_mut().enumerate() {
            *e = mul(u[c], w9[b * c]);
        }
    }
    let mut out = [(0.0f32, 0.0f32); 9];
    for c in 0..3 {
        let y = dft3(v[0][c], v[1][c], v[2][c]);
        for (d, &e) in y.iter().enumerate() {
            out[c + 3 * d] = e;
        }
    }
    out
}

/// DCT-IV of size 18: `d_m = sum_k x_k cos(pi / 18 (m + 1/2) (k + 1/2))`.
fn dct4_18(x: &[f32; 18], t: &Tables) -> [f32; 18] {
    let z: [Complex; 9] = std::array::from_fn(|m| mul((x[2 * m], x[17 - 2 * m]), t.twiddle18[m]));
    let y = dft9(&z, &t.w9);
    let mut d = [0.0f32; 18];
    for (p, &yp) in y.iter().enumerate() {
        let v = mul(yp, t.twiddle18[p]);
        d[2 * p] = v.0;
        d[17 - 2 * p] = -v.1;
    }
    d
}

/// DCT-IV of size 6.
fn dct4_6(x: &[f32; 6], t: &Tables) -> [f32; 6] {
    let z: [Complex; 3] = std::array::from_fn(|m| mul((x[2 * m], x[5 - 2 * m]), t.twiddle6[m]));
    let y = dft3(z[0], z[1], z[2]);
    let mut d = [0.0f32; 6];
    for (p, &yp) in y.iter().enumerate() {
        let v = mul(yp, t.twiddle6[p]);
        d[2 * p] = v.0;
        d[5 - 2 * p] = -v.1;
    }
    d
}

/// The windowed IMDCT of a long block: 18 lines to 36 samples; `block_type` 0, 1 or 3.
pub(super) fn imdct36(x: &[f32; 18], block_type: u8, out: &mut [f32; 36]) {
    let t = tables();
    let d = dct4_18(x, t);
    let win = &t.long[usize::from(block_type & 3)];
    for (i, o) in out.iter_mut().enumerate() {
        let v = match i {
            0..9 => d[i + 9],
            9..27 => -d[26 - i],
            _ => -d[i - 27],
        };
        *o = v * win[i];
    }
}

/// The windowed IMDCTs of the three short windows of a subband (`x[3 * i + w]` is line `i`
/// of window `w`), overlapped into 36 samples.
pub(super) fn imdct12(x: &[f32; 18], out: &mut [f32; 36]) {
    let t = tables();
    out.fill(0.0);
    for w in 0..3 {
        let d = dct4_6(&std::array::from_fn(|k| x[3 * k + w]), t);
        for i in 0..12 {
            let v = match i {
                0..3 => d[i + 3],
                3..9 => -d[8 - i],
                _ => -d[i - 9],
            };
            out[6 + 6 * w + i] += v * t.short[i];
        }
    }
}

/// The overlap state of one channel: the second half of the previous granule's blocks.
#[derive(Clone)]
pub(super) struct Overlap {
    tail: [[f32; 18]; 32],
}

impl Overlap {
    pub(super) fn new() -> Self {
        Self {
            tail: [[0.0; 18]; 32],
        }
    }

    pub(super) fn clear(&mut self) {
        self.tail = [[0.0; 18]; 32];
    }

    /// Turns the 576 lines of a granule into 18 time slots of 32 subband samples. For short
    /// blocks `xr` must already be reordered (`xr[18 sb + 3 i + w]`: line `i` of window `w` of
    /// subband `sb`); `long_sb` is [`Layout::long_subbands`](super::Layout::long_subbands). A subband whose 18 lines are all zero costs no transform: its output is
    /// just the previous granule's tail.
    pub(super) fn granule(
        &mut self,
        xr: &[f32; 576],
        info: &GranuleInfo,
        long_sb: usize,
        slots: &mut [[f32; 32]; 18],
    ) {
        // Subbands below `long_sb` use the long transform: all of them for a long block,
        // none for a short one, the lowest few for a mixed one (window type 0 there).
        let block_type = if info.block_type == 2 {
            0
        } else {
            info.block_type
        };
        let mut out = [0.0f32; 36];
        for sb in 0..32 {
            let lines: &[f32; 18] = xr[18 * sb..18 * sb + 18]
                .try_into()
                .expect("a subband is 18 lines");
            let tail = &mut self.tail[sb];
            if lines.iter().all(|&v| v == 0.0) {
                for (i, t) in tail.iter_mut().enumerate() {
                    slots[i][sb] = if sb & i & 1 == 1 { -*t } else { *t };
                    *t = 0.0;
                }
                continue;
            }
            if sb < long_sb {
                imdct36(lines, block_type, &mut out);
            } else {
                imdct12(lines, &mut out);
            }
            for i in 0..18 {
                let y = out[i] + tail[i];
                tail[i] = out[18 + i];
                slots[i][sb] = if sb & i & 1 == 1 { -y } else { y };
            }
        }
    }
}

#[cfg(test)]
pub(super) mod tests;
