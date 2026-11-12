//! The three formats of TASK-E01-01, anchored on one tone.
//!
//! AC-1 asks that "FLAC/MP3/WAV decodificam contra referência", and the three files
//! have to belong to the same source for that sentence to mean anything. They do:
//! `tools/fixtures/source-sine.wav` (997 Hz left, 1499 Hz right, 48 kHz stereo s32) is
//! what every fixture in this repository is derived from, and three chains reach it:
//!
//! - WAV: `tests/fixtures/pcm/sine-s16le.wav`, FFmpeg's s32 -> s16 truncation of the source.
//! - FLAC: `tests/fixtures/flac/st16-level0.flac`, whose own reference
//!   `tests/fixtures/flac/ref/st16-48k.wav` is **byte-identical** to that WAV. Both are
//!   lossless, so both must decode to the source exactly.
//! - MP3: `crates/vynamp-decode/tests/fixtures/mpeg/g-cbr128-48k-stereo.mp3`, the
//!   libmp3lame encode of that same WAV looped three times, with a LAME tag carrying the
//!   encoder delay and padding.
//!
//! MP3 is lossy, so "sample a sample com o WAV de origem" cannot be an equality and
//! demanding one would fail by construction. It is measured two other ways: against
//! FFmpeg's own gapless decode under the ISO/IEC 11172-4 accuracy criterion (that is
//! `tests/mpeg.rs`), and here against the source by signal-to-noise ratio, which does
//! not depend on FFmpeg agreeing with us.
//!
//! `triangulation.sha256` is the manifest this TASK added: the gapless corpus was
//! committed without one, so `engineering/05-testes.md` ("fixture manifest precisa
//! hash/licença/formato/oráculo independente") was unmet for those fifteen files.

#![cfg(all(
    feature = "riff",
    feature = "pcm",
    feature = "flac",
    feature = "mpa",
    feature = "mp3"
))]

use std::path::{Path, PathBuf};
use vynamp_decode::{AudioBuffer, GaplessKind, Options, SampleFormat, open_path};
use vynamp_decode_testkit::signal::sine_i32;
use vynamp_decode_testkit::{hex, sha256};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn gapless_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mpeg")
}

/// Decoded integers per channel, whatever the sample format, plus the format.
struct Decoded {
    planes: Vec<Vec<i64>>,
    format: SampleFormat,
    rate: u32,
    frames: usize,
    delay: u32,
    padding: u32,
    gapless: GaplessKind,
}

fn decode(path: &Path) -> Decoded {
    let mut reader =
        open_path(path, Options::default()).unwrap_or_else(|e| panic!("{path:?}: {e}"));
    let track = reader.track().clone();
    let mut planes: Vec<Vec<i64>> = Vec::new();
    let mut format = SampleFormat::F32;
    let mut buf = AudioBuffer::new();
    while reader.read(&mut buf).expect("decode") {
        format = buf.format();
        planes.resize(usize::from(buf.channels()), Vec::new());
        for (c, out) in planes.iter_mut().enumerate() {
            let plane: Vec<i64> = match format {
                SampleFormat::U8 => buf
                    .plane_u8(c)
                    .unwrap()
                    .iter()
                    .map(|&v| i64::from(v))
                    .collect(),
                SampleFormat::I16 => buf
                    .plane_i16(c)
                    .unwrap()
                    .iter()
                    .map(|&v| i64::from(v))
                    .collect(),
                SampleFormat::I24 | SampleFormat::I32 => buf
                    .plane_i32(c)
                    .unwrap()
                    .iter()
                    .map(|&v| i64::from(v))
                    .collect(),
                // MP3 decodes to f32. Scaling to the s16 integer grid keeps every
                // comparison in one unit: 32768.0 is one step of the 16-bit source. The
                // scale is the decoder's contract, not this test's guess — `vynamp-decode-mp3`
                // documents full scale as plus or minus 1.0 — and `assert_full_scale` below
                // proves the files actually use it, so a contract change fails there instead
                // of showing up as a mysterious SNR collapse.
                SampleFormat::F32 => buf
                    .plane_f32(c)
                    .unwrap()
                    .iter()
                    .map(|&v| (f64::from(v) * 32768.0).round() as i64)
                    .collect(),
                SampleFormat::F64 => panic!("{path:?}: unexpected f64 samples"),
            };
            out.extend(plane);
        }
    }
    let frames = planes.first().map_or(0, Vec::len);
    Decoded {
        planes,
        format,
        rate: track.sample_rate,
        frames,
        delay: track.delay,
        padding: track.padding,
        gapless: track.gapless,
    }
}

/// One row of `triangulation.sha256`.
struct Row {
    name: String,
    sha: String,
}

fn manifest(dir: &Path) -> Vec<Row> {
    let path = dir.join("triangulation.sha256");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    text.lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let (sha, name) = l
                .split_once("  ")
                .unwrap_or_else(|| panic!("bad manifest row: {l}"));
            Row {
                name: name.to_owned(),
                sha: sha.to_owned(),
            }
        })
        .collect()
}

/// Every MP3 gapless fixture is hashed, and the hash is the file's.
///
/// The corpus was committed in `8c771a4` with no manifest, and a fixture without a hash
/// is a fixture nobody can prove was not swapped. This test is the only reader of
/// `triangulation.sha256`, so a regenerated fixture that is not re-hashed fails here.
#[test]
fn every_gapless_fixture_has_its_hash_in_the_manifest() {
    let dir = gapless_dir();
    let rows = manifest(&dir);
    assert_eq!(rows.len(), 15, "5 cases x (.src.wav, .mp3, .ref.f32)");
    for row in &rows {
        let bytes =
            std::fs::read(dir.join(&row.name)).unwrap_or_else(|e| panic!("{}: {e}", row.name));
        assert_eq!(hex(&sha256(&bytes)), row.sha, "{}: sha256", row.name);
    }
    // The manifest is not allowed to be a subset: every fixture on disk is listed.
    let mut listed: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    let mut found: Vec<String> = std::fs::read_dir(&dir)
        .expect("fixture dir")
        .filter_map(|e| {
            let name = e.ok()?.file_name().into_string().ok()?;
            let fixture =
                name.ends_with(".mp3") || name.ends_with(".src.wav") || name.ends_with(".ref.f32");
            fixture.then_some(name)
        })
        .collect();
    found.sort();
    listed.sort_unstable();
    assert_eq!(listed, found, "manifest and directory disagree");
}

/// The hash above is measured, not decorative.
///
/// A manifest that is read but never compared against a changed byte proves nothing.
/// This reproduces the check by hand on a byte of the tone itself: flipping one byte of
/// `.src.wav` moves the sha256 off the pinned value, which is what would happen to a
/// fixture that was regenerated without re-running the manifest step.
#[test]
fn a_flipped_byte_leaves_the_pinned_hash() {
    let dir = gapless_dir();
    let row = manifest(&dir)
        .into_iter()
        .find(|r| r.name == "g-cbr128-48k-stereo.src.wav")
        .expect("g-cbr128-48k-stereo.src.wav in manifest");
    let mut bytes = std::fs::read(dir.join(&row.name)).expect("fixture");
    assert_eq!(hex(&sha256(&bytes)), row.sha, "clean fixture");

    // Byte 100 is inside the first block of PCM samples, not in the RIFF header.
    bytes[100] ^= 0x01;
    let tampered = hex(&sha256(&bytes));
    assert_ne!(tampered, row.sha, "one flipped byte must move the hash");
}

/// The tone every fixture is derived from, as decoded integers.
///
/// Each plane is checked against `sine_i32`, which is an independent generator and not a
/// second call into this repository's decoder. Without that check every comparison below
/// would be one decoder agreeing with itself, which proves nothing.
fn source_planes() -> Vec<Vec<i64>> {
    let path = root().join("tools/fixtures/source-sine.wav");
    let got = decode(&path);
    assert_eq!(got.format, SampleFormat::I32, "source format");
    assert_eq!(got.rate, 48_000, "source rate");
    assert_eq!(got.planes.len(), 2, "source channels");
    for (c, (plane, freq)) in got.planes.iter().zip([997.0, 1499.0]).enumerate() {
        let want: Vec<i64> = sine_i32(48_000, freq, 4_800, 24)
            .into_iter()
            .map(|v| i64::from(v) << 8)
            .collect();
        assert_eq!(plane.len(), want.len(), "source channel {c} length");
        for (i, (&got, &want)) in plane.iter().zip(&want).enumerate() {
            assert_eq!(got, want, "source channel {c} frame {i}: not {freq} Hz");
        }
    }
    got.planes
}

/// Signal-to-noise ratio of `got` against `want`, both in [-32768, 32768).
fn snr_db(got: &[Vec<i64>], want: &[Vec<i64>]) -> f64 {
    let (mut signal, mut error) = (0.0f64, 0.0f64);
    for (g, w) in got.iter().zip(want) {
        for (&a, &b) in g.iter().zip(w) {
            signal += (b as f64).powi(2);
            error += ((a - b) as f64).powi(2);
        }
    }
    10.0 * (signal / error).log10()
}

/// The f32 legs really are in [-1.0, 1.0], which is what `* 32768.0` assumes.
///
/// `AudioBuffer` does not carry a scale, so the unit conversion above would silently
/// produce garbage on a decoder that switched to full scale = 32768.0. Asserting the range
/// turns that into a named failure here instead of an SNR that points at the wrong leg.
fn assert_full_scale(path: &Path, planes: &[Vec<i64>]) {
    let peak = planes.iter().flatten().map(|v| v.abs()).max().unwrap_or(0);
    assert!(
        peak <= 32768,
        "{path:?}: peak {peak} on the s16 grid means the decoder is not in [-1.0, 1.0]"
    );
    assert!(
        peak > 32768 / 8,
        "{path:?}: peak {peak} on the s16 grid is too quiet to prove the scale"
    );
}

/// AC-1: WAV, FLAC and MP3 all come from one tone, and each is checked against it.
///
/// The WAV and FLAC legs are exact: s16 samples must equal the source's top 16 bits,
/// and the FLAC's own reference WAV is the very file the PCM leg decodes. The MP3 leg
/// cannot be exact, so it is held to two things that do not depend on our decoder: the
/// LAME tag must report the length the source actually has, and the decoded signal must
/// sit above 20 dB SNR against the source. The measured value on 2026-11-12 is 25.9 dB.
#[test]
fn wav_flac_and_mp3_of_the_same_tone_decode_against_one_reference() {
    let source = source_planes();

    // One reference, not two: the FLAC corpus compares against a copy of the PCM fixture.
    let pcm_wav = root().join("tests/fixtures/pcm/sine-s16le.wav");
    let flac_ref = root().join("tests/fixtures/flac/ref/st16-48k.wav");
    let pcm_sha = hex(&sha256(&std::fs::read(&pcm_wav).expect("pcm wav")));
    let ref_sha = hex(&sha256(&std::fs::read(&flac_ref).expect("flac ref")));
    assert_eq!(
        pcm_sha, ref_sha,
        "the FLAC reference must be the PCM fixture itself"
    );

    // WAV leg: exact, sample for sample, against the source narrowed to s16.
    let wav = decode(&pcm_wav);
    assert_eq!(wav.format, SampleFormat::I16, "wav format");
    assert_eq!(wav.frames, 4_800, "wav frames");
    for (c, plane) in wav.planes.iter().enumerate() {
        assert_eq!(plane.len(), source[c].len(), "wav channel {c} length");
        for (i, (&got, &src)) in plane.iter().zip(&source[c]).enumerate() {
            assert_eq!(got, src >> 16, "wav channel {c} frame {i}");
        }
    }

    // FLAC leg: lossless, so identical to the same source samples.
    let flac_path = root().join("tests/fixtures/flac/st16-level0.flac");
    let flac = decode(&flac_path);
    assert_eq!(flac.format, SampleFormat::I16, "flac format");
    assert_eq!(flac.frames, 4_800, "flac frames");
    for (c, plane) in flac.planes.iter().enumerate() {
        assert_eq!(plane.len(), source[c].len(), "flac channel {c} length");
        for (i, (&got, &src)) in plane.iter().zip(&source[c]).enumerate() {
            assert_eq!(got, src >> 16, "flac channel {c} frame {i}");
        }
    }
    // And the two lossless legs agree with each other without going through the source.
    assert_eq!(wav.planes, flac.planes, "wav and flac decode alike");

    // MP3 leg: the same tone, three times, trimmed to exactly its length.
    let mp3 = decode(&gapless_dir().join("g-cbr128-48k-stereo.mp3"));
    // MP3 reconstructs in floating point; the samples above are on the s16 grid.
    assert_eq!(mp3.format, SampleFormat::F32, "mp3 format");
    assert_eq!(mp3.frames, 3 * 4_800, "mp3 frames: three loops of the tone");
    let loops: Vec<Vec<i64>> = source
        .iter()
        .map(|p| p.iter().map(|&v| v >> 16).cycle().take(3 * 4_800).collect())
        .collect();
    assert_full_scale(&gapless_dir().join("g-cbr128-48k-stereo.mp3"), &mp3.planes);
    let snr = snr_db(&mp3.planes, &loops);
    assert!(snr > 20.0, "mp3 SNR against the source: {snr:.1} dB");
}

/// AC-2, gapless half: the trim is real, and it does not come from FLAC.
///
/// The `Then` asks for "um FLAC com encoder delay e padding declarados no streaminfo".
/// STREAMINFO (RFC 9639 section 8.2) has no such fields — it carries block and frame
/// sizes, rate, channels, bits per sample, total samples and MD5, nothing else — so the
/// FLAC leg can only show that delay and padding are absent and that the output is the
/// declared length. The codec that actually declares them is MPEG through its LAME tag,
/// and that is where the subtraction is exercised: 16 128 declared frames minus 1 105
/// delay minus 623 padding is the 14 400 frames the source has.
#[test]
fn the_gapless_trim_is_declared_by_mpeg_and_absent_in_flac() {
    let flac = decode(&root().join("tests/fixtures/flac/st16-level0.flac"));
    assert_eq!(flac.delay, 0, "flac has no encoder delay");
    assert_eq!(flac.padding, 0, "flac has no encoder padding");
    assert_eq!(
        flac.gapless,
        GaplessKind::Exact,
        "flac is exact by construction, not by metadata"
    );
    assert_eq!(flac.frames, 4_800, "flac output is total_samples");

    let mp3 = decode(&gapless_dir().join("g-cbr128-48k-stereo.mp3"));
    assert_eq!(mp3.delay, 1_105, "LAME tag delay");
    assert_eq!(mp3.padding, 623, "LAME tag padding");
    assert_eq!(mp3.gapless, GaplessKind::FromMetadata, "trim is declared");
    assert_eq!(
        mp3.frames + mp3.delay as usize + mp3.padding as usize,
        16_128,
        "trimmed frames + delay + padding is the declared frame count"
    );
    assert_eq!(mp3.frames, 14_400, "which is what the source actually has");
}

/// AC-2, width half: PCM24 and PCM32 keep every valid bit.
///
/// `sine-s24le.wav` and `sine-s32le.wav` are the source narrowed to 24 and kept at 32 bits,
/// so each sample must be exactly the source shifted. The check that matters is that the
/// s24 fixture is not a widened s16: `st24w-48k.wav` in the FLAC corpus comes from an
/// s16 -> s32 chain, so its low byte is always zero and it would pass a "24 bits survive"
/// test while proving nothing about the low 8 bits.
#[test]
fn pcm24_and_pcm32_keep_every_valid_bit() {
    let source = source_planes();
    let dir = root().join("tests/fixtures/pcm");

    let s32 = decode(&dir.join("sine-s32le.wav"));
    assert_eq!(s32.format, SampleFormat::I32, "s32 format");
    for (c, plane) in s32.planes.iter().enumerate() {
        assert_eq!(plane.len(), source[c].len(), "s32 channel {c} length");
        for (i, (&got, &src)) in plane.iter().zip(&source[c]).enumerate() {
            assert_eq!(got, src, "s32 channel {c} frame {i}");
        }
    }

    let s24 = decode(&dir.join("sine-s24le.wav"));
    assert_eq!(s24.format, SampleFormat::I24, "s24 format");
    for (c, plane) in s24.planes.iter().enumerate() {
        assert_eq!(plane.len(), source[c].len(), "s24 channel {c} length");
        for (i, (&got, &src)) in plane.iter().zip(&source[c]).enumerate() {
            assert_eq!(got, src >> 8, "s24 channel {c} frame {i}");
        }
    }

    // The low 8 bits of the s24 samples are the source's own, not padding zeros.
    let source_has_low_bits = source.iter().flatten().any(|&v| (v >> 8) & 0xff != 0);
    assert!(
        source_has_low_bits,
        "the source must carry bits below the s16 cut for this fixture to prove anything"
    );
}
