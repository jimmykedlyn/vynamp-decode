//! Writes `tools/fixtures/source-sine.wav`: 48 kHz stereo s32, 4800 frames,
//! 997 Hz (left) and 1499 Hz (right) sines at 0.5 full scale with 24-bit precision (low byte zero) so that
//! conversions to s24/f32/f64 are exact.
//!
//! Usage: `cargo run -p agulha-testkit --example gen_source_sine -- <output.wav>`

use agulha_core::{Endian, PcmEncoding, PcmSpec};
use agulha_testkit::signal::sine_i32;
use agulha_testkit::writers::{WavVariant, wav};

fn main() {
    let Some(out) = std::env::args().nth(1) else {
        eprintln!("usage: gen_source_sine <output.wav>");
        std::process::exit(1);
    };
    let left = sine_i32(48000, 997.0, 4800, 24);
    let right = sine_i32(48000, 1499.0, 4800, 24);
    let mut bytes = Vec::with_capacity(left.len() * 8);
    for (l, r) in left.into_iter().zip(right) {
        bytes.extend_from_slice(&(l << 8).to_le_bytes());
        bytes.extend_from_slice(&(r << 8).to_le_bytes());
    }
    let spec = PcmSpec {
        encoding: PcmEncoding::Signed,
        bits: 32,
        valid_bits: 32,
        endian: Endian::Little,
    };
    let file = wav(spec, 48000, 2, &bytes, WavVariant::Riff);
    std::fs::write(&out, file).unwrap_or_else(|e| {
        eprintln!("{out}: {e}");
        std::process::exit(1);
    });
}
