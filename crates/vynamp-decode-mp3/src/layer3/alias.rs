//! Alias reduction (ISO/IEC 11172-3 section 2.4.3.4.10).
//!
//! The filterbank's subbands overlap, so a long block's lines get eight butterflies across
//! every subband boundary (lines `18 sb - 1 - i` and `18 sb + i`, `i` in 0 to 7):
//! `a' = a cs_i - b ca_i`, `b' = b cs_i + a ca_i` with `cs = 1 / sqrt(1 + c^2)`,
//! `ca = c / sqrt(1 + c^2)` and the eight coefficients `c` of the standard. Short blocks have
//! none; a mixed block only across the boundary between its two long subbands (at every rate).

use std::sync::OnceLock;

/// The coefficients `c_i` of the standard.
const C: [f64; 8] = [
    -0.6, -0.535, -0.33, -0.185, -0.095, -0.041, -0.0142, -0.0037,
];

fn coefficients() -> &'static [(f32, f32); 8] {
    static T: OnceLock<[(f32, f32); 8]> = OnceLock::new();
    T.get_or_init(|| {
        C.map(|c| {
            let n = (1.0 + c * c).sqrt();
            ((1.0 / n) as f32, (c / n) as f32)
        })
    })
}

/// Builds the table now so that decoding never initializes it.
pub(super) fn warm_up() {
    coefficients();
}

/// Applies the butterflies between neighbouring subbands to `xr` (lines in subband order).
/// Lines from `used` on are zero, so boundaries whose butterflies lie wholly beyond them are
/// skipped. Boundaries `1..=boundaries` are processed: 31 for a long block, one fewer than the
/// long subbands for a mixed block (1, or 3 at 8 kHz).
pub(super) fn reduce(xr: &mut [f32; 576], used: usize, boundaries: usize) {
    let coef = coefficients();
    // Boundary `sb` touches lines 18 sb - 8 to 18 sb + 7.
    let last = ((used.min(576) + 7) / 18).min(boundaries.min(31));
    for sb in 1..=last {
        let b = 18 * sb;
        for (i, &(cs, ca)) in coef.iter().enumerate() {
            let (a, c) = (xr[b - 1 - i], xr[b + i]);
            xr[b - 1 - i] = a * cs - c * ca;
            xr[b + i] = c * cs + a * ca;
        }
    }
}

#[cfg(test)]
pub(super) mod tests;
