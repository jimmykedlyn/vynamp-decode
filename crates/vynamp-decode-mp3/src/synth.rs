//! Polyphase synthesis filterbank (ISO/IEC 11172-3 annex C, figure C.1).
//!
//! One call turns 32 subband samples into 32 PCM samples:
//!
//! 1. Matrixing: `V[i] = sum_k cos((16 + i)(2k + 1) pi / 64) * S[k]` for `i` in 0..64, pushed
//!    in front of the 1024-entry history. With `u[m] = sum_k cos(m (2k + 1) pi / 64)` for
//!    `m` in 0..32 (a 32x32 product), the identities `cos((64 - m) x) = -cos(m x)` and
//!    `cos((64 + m) x) = -cos(m x)` (x being an odd multiple of pi / 64) give
//!    `V[0..16] = u[16..32]`, `V[16] = 0`, `V[17..48] = -u[31..1]` and `V[48..64] = -u[0..16]`.
//! 2. Windowing: `U` takes the first 32 of every 64 entries of `V` at stride 128 (blocks `i`
//!    of 64: `U[64 i + j] = V[128 i + j]`, `U[64 i + 32 + j] = V[128 i + 96 + j]`);
//!    `out[j] = sum over 16 taps of D[j + 32 m] * U[j + 32 m]`.
//!
//! The matrixing is the direct 32x32 product (1024 multiply-adds per slot); a fast DCT
//! would cut that to about 80 and is left to a performance pass.

use crate::tables::WINDOW;
use std::sync::OnceLock;

/// `COS[m][k] = cos(m (2k + 1) pi / 64)`.
fn cos_table() -> &'static [[f32; 32]; 32] {
    static TABLE: OnceLock<[[f32; 32]; 32]> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut t = [[0.0f32; 32]; 32];
        for (m, row) in t.iter_mut().enumerate() {
            for (k, v) in row.iter_mut().enumerate() {
                let angle = (m * (2 * k + 1)) as f64 * std::f64::consts::PI / 64.0;
                *v = angle.cos() as f32;
            }
        }
        t
    })
}

/// Per-channel synthesis state: the 1024-entry matrixing history `V`, as a ring buffer.
pub(crate) struct Synthesis {
    ring: [f32; 1024],
    /// Ring index of `V[0]`.
    pos: usize,
}

impl Synthesis {
    pub(crate) fn new() -> Self {
        // Build the shared table now so that `synth` never initializes it.
        cos_table();
        Self {
            ring: [0.0; 1024],
            pos: 0,
        }
    }

    /// Forgets all history.
    pub(crate) fn reset(&mut self) {
        self.ring.fill(0.0);
        self.pos = 0;
    }

    /// Turns one time slot of 32 subband samples into 32 PCM samples.
    pub(crate) fn synth(&mut self, subbands: &[f32; 32], out: &mut [f32; 32]) {
        let cos = cos_table();
        let mut u = [0.0f32; 32];
        for (um, row) in u.iter_mut().zip(cos) {
            *um = row.iter().zip(subbands).map(|(c, s)| c * s).sum();
        }
        self.pos = (self.pos + 1024 - 64) & 1023;
        let p = self.pos;
        for i in 0..64 {
            self.ring[(p + i) & 1023] = match i {
                0..16 => u[16 + i],
                16 => 0.0,
                17..48 => -u[48 - i],
                _ => -u[i - 48],
            };
        }
        for (j, o) in out.iter_mut().enumerate() {
            let mut acc = 0.0f32;
            for block in 0..8 {
                acc += WINDOW[64 * block + j] * self.ring[(p + 128 * block + j) & 1023];
                acc += WINDOW[64 * block + 32 + j] * self.ring[(p + 128 * block + 96 + j) & 1023];
            }
            *o = acc;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::Rng;
    use crate::reference::RefSynth;

    fn run(inputs: &[[f32; 32]]) -> (Vec<[f32; 32]>, Vec<[f64; 32]>) {
        let mut s = Synthesis::new();
        let mut r = RefSynth::new();
        let (mut got, mut want) = (Vec::new(), Vec::new());
        for inp in inputs {
            let mut out = [0.0f32; 32];
            s.synth(inp, &mut out);
            got.push(out);
            want.push(r.slot(&inp.map(f64::from)));
        }
        (got, want)
    }

    fn max_err(got: &[[f32; 32]], want: &[[f64; 32]]) -> f64 {
        got.iter()
            .zip(want)
            .flat_map(|(g, w)| g.iter().zip(w).map(|(&a, &b)| (f64::from(a) - b).abs()))
            .fold(0.0, f64::max)
    }

    #[test]
    fn subband_impulse_matches_cosine_matrix() {
        for sb in [0usize, 1, 2, 7, 15, 16, 17, 30, 31] {
            let mut inputs = vec![[0.0f32; 32]; 20];
            inputs[0][sb] = 1.0;
            let (got, want) = run(&inputs);
            // The response must be non-trivial, then match to 1e-6.
            assert!(
                want.iter().flatten().any(|v| v.abs() > 0.01),
                "subband {sb}"
            );
            let e = max_err(&got, &want);
            assert!(e < 1e-6, "subband {sb}: max error {e}");
        }
    }

    #[test]
    fn impulse_response_lasts_sixteen_slots() {
        let mut inputs = vec![[0.0f32; 32]; 20];
        inputs[0][3] = 1.0;
        let (got, _) = run(&inputs);
        assert!(got[..16].iter().flatten().any(|&v| v != 0.0));
        assert!(got[16..].iter().flatten().all(|&v| v == 0.0));
    }

    #[test]
    fn random_input_matches_reference() {
        let mut rng = Rng(0x1234_5678);
        let inputs: Vec<[f32; 32]> = (0..60)
            .map(|_| std::array::from_fn(|_| rng.unit() as f32))
            .collect();
        let (got, want) = run(&inputs);
        let e = max_err(&got, &want);
        assert!(e < 1e-5, "max error {e}");
    }

    #[test]
    fn reset_forgets_history() {
        let mut s = Synthesis::new();
        let mut out = [0.0f32; 32];
        s.synth(&[1.0; 32], &mut out);
        s.reset();
        s.synth(&[0.0; 32], &mut out);
        assert!(out.iter().all(|&v| v == 0.0));
        let mut fresh = Synthesis::new();
        let (mut a, mut b) = ([0.0f32; 32], [0.0f32; 32]);
        s.synth(&[0.5; 32], &mut a);
        fresh.synth(&[0.0; 32], &mut b);
        fresh.synth(&[0.5; 32], &mut b);
        assert_eq!(a, b);
    }

    #[test]
    fn window_is_antisymmetric() {
        use crate::tables::WINDOW_INT;
        for i in 1..512 {
            let want = if i % 64 == 0 {
                WINDOW_INT[i]
            } else {
                -WINDOW_INT[i]
            };
            assert_eq!(WINDOW_INT[512 - i], want, "D[{}]", 512 - i);
        }
        assert_eq!((WINDOW_INT[0], WINDOW_INT[256]), (0, 75038));
    }
}
