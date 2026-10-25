//! Third-party oracle corpus for MPEG audio Layers II and III: files written by FFmpeg (a
//! dev-only tool, never a dependency) from synthetic signals (`tools/fixtures/generate.ps1`,
//! section "MPEG audio oracle corpus"), each with FFmpeg's own floating point decode
//! (`mp2float`, `mp3float`) as raw f32. The Reader's output must meet the ISO/IEC 11172-4
//! full accuracy criterion against that decode: RMS error below 2^-15 / sqrt(12) and maximum
//! error at most 2^-14, per channel, at exact alignment.
//!
//! The files carry no Xing/Info/LAME tag, so FFmpeg drops nothing while the Reader trims the
//! decoder delay (481 samples for Layer II, 529 for Layer III): the output is `reference[delay..]`.
//! Layer I has no encoder in FFmpeg, so it has no fixtures here and stays experimental.

#![cfg(all(feature = "mpa", feature = "mp3"))]

use std::path::{Path, PathBuf};
use vynamp_decode::{AudioBuffer, CodecId, GaplessKind, Mode, Options, Reader, open_path};
use vynamp_decode_mpa::{ChannelMode, MpegVersion, parse_header};
use vynamp_decode_testkit::{hex, sha256};

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/mpeg")
}

struct Row {
    file: String,
    sha: String,
    reference: String,
    ref_sha: String,
    rate: u32,
    channels: u16,
    layer: u8,
    note: String,
}

fn manifest() -> Vec<Row> {
    let text = std::fs::read_to_string(dir().join("MANIFEST.txt")).expect("MANIFEST.txt");
    text.lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let f: Vec<&str> = l.split('|').collect();
            assert!(f.len() >= 11, "bad MANIFEST row: {l}");
            Row {
                file: f[0].to_owned(),
                sha: f[1].to_owned(),
                reference: f[2].to_owned(),
                ref_sha: f[3].to_owned(),
                rate: f[4].parse().unwrap(),
                channels: f[5].parse().unwrap(),
                layer: f[7].parse().unwrap(),
                note: f[8].to_owned(),
            }
        })
        .collect()
}

fn drain(r: &mut Reader) -> Vec<Vec<f32>> {
    let mut chans = vec![Vec::new(); usize::from(r.track().channels)];
    let mut buf = AudioBuffer::new();
    while r.read(&mut buf).expect("decode") {
        for (c, v) in chans.iter_mut().enumerate() {
            v.extend_from_slice(buf.plane_f32(c).expect("f32 planes"));
        }
    }
    chans
}

fn read_f32(path: &Path, channels: usize) -> Vec<Vec<f32>> {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
    let flat: Vec<f32> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect();
    (0..channels)
        .map(|c| flat.iter().skip(c).step_by(channels).copied().collect())
        .collect()
}

/// RMS and maximum absolute error of `got[i]` against `want[i + lag]`.
fn errors(got: &[f32], want: &[f32], lag: isize) -> (f64, f64) {
    let (mut sum, mut max, mut n) = (0.0f64, 0.0f64, 0usize);
    for (i, &g) in got.iter().enumerate() {
        let Some(j) = i.checked_add_signed(lag).filter(|&j| j < want.len()) else {
            continue;
        };
        let e = (f64::from(g) - f64::from(want[j])).abs();
        sum += e * e;
        max = max.max(e);
        n += 1;
    }
    ((sum / n as f64).sqrt(), max)
}

/// The Layer II allocation table the standard selects (ISO/IEC 11172-3 Annex B, 13818-3):
/// `A`/`B` 27/30 subbands, `C`/`D` the low rate tables (8 and 12 subbands), `LSF` for MPEG-2.
fn l2_table(file: &[u8]) -> &'static str {
    let h = parse_header(file[..4].try_into().unwrap()).expect("header");
    if h.version != MpegVersion::V1 {
        return "LSF";
    }
    let per_channel = if h.channel_mode == ChannelMode::Mono {
        h.bitrate_kbps
    } else {
        h.bitrate_kbps / 2
    };
    match (per_channel, h.sample_rate) {
        (..56, 32000) => "D",
        (..56, _) => "C",
        (96.., r) if r != 48000 => "B",
        _ => "A",
    }
}

#[test]
fn the_corpus_covers_every_layer_two_table_and_the_layer_three_modes() {
    let rows = manifest();
    let l2: Vec<&str> = rows
        .iter()
        .filter(|r| r.layer == 2)
        .map(|r| r.note.as_str())
        .collect();
    for t in ["table A", "table B", "table C", "table D", "table LSF"] {
        assert!(l2.contains(&t), "no Layer II fixture for {t}");
    }
    let l3 = |s: &str| rows.iter().any(|r| r.layer == 3 && r.note.contains(s));
    for s in [
        "CBR", "VBR", "ABR", "mono", "joint", "LR", "MPEG-2 ", "MPEG-2.5",
    ] {
        assert!(l3(s), "no Layer III fixture for {s}");
    }
    assert!(rows.iter().any(|r| r.layer == 3 && r.rate == 8000));
}

/// The manifest lists exactly 12 Layer II and 21 Layer III encodes, and the directory holds
/// nothing but `MANIFEST.txt`, `.gitattributes`, those files and the references they name
/// (no orphans).
fn assert_corpus_is_exactly_the_manifest(rows: &[Row]) {
    let count = |layer| rows.iter().filter(|r| r.layer == layer).count();
    assert_eq!((count(2), count(3), rows.len()), (12, 21, 33));
    let mut listed: Vec<&str> = rows
        .iter()
        .flat_map(|r| [r.file.as_str(), r.reference.as_str()])
        .chain(["MANIFEST.txt", ".gitattributes"])
        .collect();
    listed.sort_unstable();
    let before = listed.len();
    listed.dedup();
    assert_eq!(listed.len(), before, "a file is listed twice");
    let mut present: Vec<String> = std::fs::read_dir(dir())
        .expect("corpus directory")
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    present.sort_unstable();
    assert_eq!(
        present, listed,
        "the corpus directory and MANIFEST.txt disagree"
    );
}

#[test]
fn every_fixture_matches_its_checksum_and_the_reference_within_the_iso_criterion() {
    let rms_limit = 2f64.powi(-15) / 12f64.sqrt();
    let max_limit = 2f64.powi(-14);
    let rows = manifest();
    assert_corpus_is_exactly_the_manifest(&rows);
    for row in &rows {
        let path = dir().join(&row.file);
        let data = std::fs::read(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
        assert_eq!(hex(&sha256(&data)), row.sha, "{}", row.file);
        let ref_path = dir().join(&row.reference);
        let ref_bytes = std::fs::read(&ref_path).unwrap_or_else(|e| panic!("{ref_path:?}: {e}"));
        assert_eq!(hex(&sha256(&ref_bytes)), row.ref_sha, "{}", row.reference);

        if row.layer == 2 {
            assert_eq!(
                format!("table {}", l2_table(&data)),
                row.note,
                "{}: allocation table",
                row.file
            );
        }

        let mut r = open_path(
            &path,
            Options {
                mode: Mode::Strict,
                ..Options::default()
            },
        )
        .unwrap_or_else(|e| panic!("{}: {e:?}", row.file));
        let t = r.track().clone();
        let (codec, delay) = if row.layer == 2 {
            (CodecId::Mp2, 481)
        } else {
            (CodecId::Mp3, 529)
        };
        assert_eq!(
            (t.codec.clone(), t.sample_rate, t.channels, t.delay),
            (codec, row.rate, row.channels, delay),
            "{}",
            row.file
        );
        assert_eq!(t.gapless, GaplessKind::Unknown, "{}", row.file);

        let got = drain(&mut r);
        let want: Vec<Vec<f32>> = read_f32(&ref_path, usize::from(row.channels))
            .iter()
            .map(|p| p[delay as usize..].to_vec())
            .collect();
        assert_eq!(got.len(), want.len(), "{}", row.file);
        for (c, (g, w)) in got.iter().zip(&want).enumerate() {
            assert_eq!(g.len(), w.len(), "{} ch{c} length", row.file);
            assert!(g.len() >= 4000, "{} ch{c}: too short", row.file);
            let (rms, max) = errors(g, w, 0);
            println!("{} ch{c}: rms {rms:.3e} max {max:.3e}", row.file);
            assert!(rms < rms_limit, "{} ch{c}: rms {rms:e}", row.file);
            assert!(max <= max_limit, "{} ch{c}: max {max:e}", row.file);
            // The alignment is pinned: one sample either way is far worse.
            for lag in [-1isize, 1] {
                let (r, _) = errors(g, w, lag);
                assert!(
                    r > 100.0 * rms_limit,
                    "{} ch{c}: lag {lag} rms {r:e}",
                    row.file
                );
            }
        }
    }
}
