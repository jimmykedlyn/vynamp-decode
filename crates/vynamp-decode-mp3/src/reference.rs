//! Test-only reference implementations written literally from the standard's definitions,
//! in `f64`: the matrixing cosine matrix, the window, Layer I requantization and the
//! analysis filterbank.

use crate::common::L1Spec;
use crate::tables::WINDOW_INT;

pub(crate) fn window(i: usize) -> f64 {
    f64::from(WINDOW_INT[i]) / 65536.0
}

/// Synthesis exactly as in the flow chart: shift `V` by 64, matrix with
/// `N[i][k] = cos((16 + i)(2k + 1) pi / 64)`, build `U`, window and sum.
pub(crate) struct RefSynth {
    v: Vec<f64>,
}

impl RefSynth {
    pub(crate) fn new() -> Self {
        Self { v: vec![0.0; 1024] }
    }

    pub(crate) fn slot(&mut self, s: &[f64; 32]) -> [f64; 32] {
        self.v.copy_within(0..960, 64);
        for i in 0..64 {
            self.v[i] = (0..32)
                .map(|k| {
                    s[k] * (f64::from((16 + i) as u32 * (2 * k as u32 + 1)) * std::f64::consts::PI
                        / 64.0)
                        .cos()
                })
                .sum();
        }
        let mut u = [0.0f64; 512];
        for i in 0..8 {
            for j in 0..32 {
                u[64 * i + j] = self.v[128 * i + j];
                u[64 * i + 32 + j] = self.v[128 * i + 96 + j];
            }
        }
        let mut out = [0.0; 32];
        for (j, o) in out.iter_mut().enumerate() {
            *o = (0..16).map(|m| window(j + 32 * m) * u[j + 32 * m]).sum();
        }
        out
    }
}

/// Layer I requantization of an `nb`-bit code with scalefactor index `scf`.
pub(crate) fn dequant(code: u32, nb: u32, scf: u8) -> f64 {
    let v = f64::from(code) - f64::from(1u32 << (nb - 1));
    (v + 1.0) * 2.0 / f64::from((1u32 << nb) - 1) * 2f64.powf(1.0 - f64::from(scf) / 3.0)
}

/// Decodes the subband samples of `spec` into 12 slots per channel.
pub(crate) fn l1_subbands(spec: &L1Spec) -> Vec<Vec<[f64; 32]>> {
    let nch = spec.channels();
    let mut out = vec![vec![[0.0; 32]; 12]; nch];
    for slot in 0..12 {
        for sb in 0..32 {
            for (ch, o) in out.iter_mut().enumerate() {
                let a = spec.alloc_of(ch, sb);
                if a != 0 {
                    let scf = spec.scf[ch][sb];
                    let code = spec.samples[slot][sb][if sb < spec.bound() { ch } else { 0 }];
                    o[slot][sb] = dequant(code, u32::from(a) + 1, scf);
                }
            }
        }
    }
    out
}

/// Analysis filterbank of the standard (figure C.1 of annex C): 512-sample shift register,
/// window `C[i] = D[i] / 32`, partial sums and the matrix `cos((2k+1)(i-16) pi / 64)`.
pub(crate) struct RefAnalysis {
    x: Vec<f64>,
}

impl RefAnalysis {
    pub(crate) fn new() -> Self {
        Self { x: vec![0.0; 512] }
    }

    pub(crate) fn slot(&mut self, input: &[f64; 32]) -> [f64; 32] {
        self.x.copy_within(0..480, 32);
        for (i, &v) in input.iter().enumerate() {
            self.x[31 - i] = v;
        }
        let mut y = [0.0; 64];
        for (i, yi) in y.iter_mut().enumerate() {
            *yi = (0..8)
                .map(|j| window(i + 64 * j) / 32.0 * self.x[i + 64 * j])
                .sum();
        }
        let mut s = [0.0; 32];
        for (k, sk) in s.iter_mut().enumerate() {
            *sk = (0..64)
                .map(|i| {
                    y[i] * (f64::from(2 * k as u32 + 1) * (i as f64 - 16.0) * std::f64::consts::PI
                        / 64.0)
                        .cos()
                })
                .sum();
        }
        s
    }
}
