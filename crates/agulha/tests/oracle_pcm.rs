//! Third-party oracle corpus: files written by FFmpeg (a dev-only tool, never a
//! dependency) from `tools/fixtures/source-sine.wav` must decode to the values
//! FFmpeg's conversion rules prescribe.
//!
//! The source holds 24-bit-precision s32 samples (low byte zero). FFmpeg
//! narrows integers by truncation (s32 -> s16 keeps the top 16 bits, s32 -> u8
//! keeps the top 8 with a +128 offset) and converts to float as x / 2^31; every
//! conversion in the corpus turned out exact, so all comparisons are strict.

#![cfg(all(feature = "riff", feature = "aiff", feature = "pcm"))]

use agulha::{AudioBuffer, Options, SampleFormat, open_path};
use agulha_testkit::signal::sine_i32;
use agulha_testkit::{hex, sha256};
use std::path::{Path, PathBuf};

/// SHA-256 of `tools/fixtures/source-sine.wav`, the input every corpus file is
/// derived from. Regenerating the source requires updating this and the corpus.
const SOURCE_SHA256: &str = "2c42c1269bd909f4610e136466cb7e22b5956d46c07cb65d5fcf5d20c4cefdc5";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Per-channel decoded samples as `f64` plus the format they came from.
struct Decoded {
    planes: Vec<Vec<f64>>,
    format: SampleFormat,
    rate: u32,
}

fn decode(path: &Path) -> Decoded {
    let mut reader =
        open_path(path, Options::default()).unwrap_or_else(|e| panic!("{path:?}: {e}"));
    let rate = reader.track().sample_rate;
    let mut planes: Vec<Vec<f64>> = Vec::new();
    let mut format = SampleFormat::F32;
    let mut buf = AudioBuffer::new();
    while reader.read(&mut buf).unwrap() {
        format = buf.format();
        planes.resize(usize::from(buf.channels()), Vec::new());
        for (c, out) in planes.iter_mut().enumerate() {
            match format {
                SampleFormat::U8 => {
                    out.extend(buf.plane_u8(c).unwrap().iter().map(|&v| f64::from(v)))
                }
                SampleFormat::I16 => {
                    out.extend(buf.plane_i16(c).unwrap().iter().map(|&v| f64::from(v)))
                }
                SampleFormat::I24 | SampleFormat::I32 => {
                    out.extend(buf.plane_i32(c).unwrap().iter().map(|&v| f64::from(v)))
                }
                SampleFormat::F32 => {
                    out.extend(buf.plane_f32(c).unwrap().iter().map(|&v| f64::from(v)))
                }
                SampleFormat::F64 => out.extend_from_slice(buf.plane_f64(c).unwrap()),
            }
        }
    }
    Decoded {
        planes,
        format,
        rate,
    }
}

/// Expected value of source sample `x` (s32) after conversion to `spec`.
fn convert(x: i32, spec: &str) -> f64 {
    let x = i64::from(x);
    match spec {
        "u8" => (x >> 24) as f64 + 128.0,
        "s16le" | "s16be" => (x >> 16) as f64,
        "s24le" | "s24be" => (x >> 8) as f64,
        "s32le" => x as f64,
        "f32le" | "f32be" | "f64le" => x as f64 / 2147483648.0,
        other => panic!("unknown spec {other}"),
    }
}

fn expected_format(spec: &str) -> SampleFormat {
    match spec {
        "u8" => SampleFormat::U8,
        "s16le" | "s16be" => SampleFormat::I16,
        "s24le" | "s24be" => SampleFormat::I24,
        "s32le" => SampleFormat::I32,
        "f32le" | "f32be" => SampleFormat::F32,
        "f64le" => SampleFormat::F64,
        other => panic!("unknown spec {other}"),
    }
}

#[test]
fn source_is_the_documented_signal() {
    let src = decode(&root().join("tools/fixtures/source-sine.wav"));
    assert_eq!(
        (src.format, src.rate, src.planes.len()),
        (SampleFormat::I32, 48000, 2)
    );
    assert!(src.planes.iter().all(|p| p.len() == 4800));
    assert!(src.planes.iter().flatten().all(|v| (*v as i64) % 256 == 0));
}

/// The source is checked against an independently computed signal (the same
/// parameters as `gen_source_sine`), not just against itself, and its bytes
/// are pinned so the corpus cannot silently drift.
#[test]
fn source_matches_the_independent_signal_and_pinned_hash() {
    let path = root().join("tools/fixtures/source-sine.wav");
    let bytes = std::fs::read(&path).expect("source-sine.wav");
    assert_eq!(hex(&sha256(&bytes)), SOURCE_SHA256, "source sha256");

    let src = decode(&path);
    for (plane, freq) in src.planes.iter().zip([997.0, 1499.0]) {
        let want = sine_i32(48000, freq, 4800, 24);
        assert_eq!(plane.len(), want.len());
        for (i, (&got, &w)) in plane.iter().zip(&want).enumerate() {
            assert_eq!(got, f64::from(w << 8), "{freq} Hz, frame {i}");
        }
    }
}

#[test]
fn manifest_files_decode_to_the_source() {
    let source = decode(&root().join("tools/fixtures/source-sine.wav"));
    let dir = root().join("tests/fixtures/pcm");
    let manifest = std::fs::read_to_string(dir.join("MANIFEST.txt")).expect("MANIFEST.txt");
    let mut rows = 0;
    for line in manifest
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        let f: Vec<&str> = line.split('|').collect();
        assert_eq!(f.len(), 7, "bad MANIFEST row: {line}");
        let (name, sha, rate, channels, spec, frames) = (f[0], f[1], f[2], f[3], f[4], f[5]);
        let (rate, channels, frames): (u32, usize, usize) = (
            rate.parse().unwrap(),
            channels.parse().unwrap(),
            frames.parse().unwrap(),
        );
        assert!(!f[6].is_empty(), "{name}: generator missing");

        let path = dir.join(name);
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(hex(&sha256(&bytes)), sha, "{name}: sha256");

        let got = decode(&path);
        assert_eq!(got.rate, rate, "{name}: rate");
        assert_eq!(got.format, expected_format(spec), "{name}: sample format");
        assert_eq!(got.planes.len(), channels, "{name}: channels");
        for (c, plane) in got.planes.iter().enumerate() {
            assert_eq!(plane.len(), frames, "{name}: frames of channel {c}");
            // Channel c carries source channel c % 2 (see generate.ps1).
            for (i, &v) in plane.iter().enumerate() {
                let x = source.planes[c % 2][i] as i32;
                let want = convert(x, spec);
                assert_eq!(v, want, "{name}: channel {c} frame {i}");
            }
        }
        rows += 1;
    }
    assert!(rows >= 12, "only {rows} manifest rows");
}
