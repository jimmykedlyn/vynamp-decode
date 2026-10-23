//! Deterministic test signals.

use std::f64::consts::TAU;

/// Sine of `frames` samples at `freq` Hz, amplitude 0.5 full scale (as `f64`, +-0.5).
pub fn sine_f64(rate: u32, freq: f64, frames: usize) -> Vec<f64> {
    (0..frames)
        .map(|n| 0.5 * (TAU * freq * n as f64 / f64::from(rate)).sin())
        .collect()
}

/// Sine as signed integers of `bits` width (1..=32), amplitude 0.5 full scale,
/// rounded to the nearest integer (2^(bits-2) peak).
pub fn sine_i32(rate: u32, freq: f64, frames: usize, bits: u8) -> Vec<i32> {
    assert!((1..=32).contains(&bits), "bits must be in 1..=32");
    let full_scale = 2f64.powi(i32::from(bits) - 1);
    sine_f64(rate, freq, frames)
        .into_iter()
        .map(|x| (x * full_scale).round() as i32)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sine_i32_16bit_half_scale() {
        let s = sine_i32(48000, 1000.0, 48, 16);
        assert_eq!(s.len(), 48);
        assert_eq!(s[0], 0);
        let max = *s.iter().max().unwrap();
        assert!((max - 16384).abs() <= 1, "max = {max}");
    }
}
