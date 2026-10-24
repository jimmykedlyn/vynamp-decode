//! Third-party oracle corpus for FLAC: files written by FFmpeg (a dev-only tool, never a
//! dependency) from `tools/fixtures/source-sine.wav`. FLAC is lossless, so each file must
//! decode, in `Mode::Strict` (STREAMINFO MD5 verified), to exactly the PCM that FFmpeg fed
//! its encoder. That PCM is committed next to the FLAC as a WAV reference
//! (`tests/fixtures/flac/ref/`) and read through the already-proven PCM path.
//! `tools/fixtures/generate.ps1` also proves, at generation time, that FFmpeg's own FLAC
//! decoder returns the reference.

#![cfg(all(feature = "flac", feature = "riff", feature = "pcm"))]

use std::path::{Path, PathBuf};
use vynamp_decode::{
    AudioBuffer, Demuxer, Mode, Options, Packet, Reader, VecSource, open, open_path,
};
use vynamp_decode_flac::open_flac;
use vynamp_decode_testkit::{hex, sha256};

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/flac")
}

fn strict() -> Options {
    Options {
        mode: Mode::Strict,
        ..Options::default()
    }
}

struct Decoded {
    planes: Vec<Vec<i64>>,
    rate: u32,
}

fn drain(reader: &mut Reader) -> Decoded {
    let rate = reader.track().sample_rate;
    let mut planes: Vec<Vec<i64>> = Vec::new();
    let mut buf = AudioBuffer::new();
    while reader.read(&mut buf).expect("decode") {
        planes.resize(usize::from(buf.channels()), Vec::new());
        for (c, out) in planes.iter_mut().enumerate() {
            if let Some(s) = buf.plane_i16(c) {
                out.extend(s.iter().map(|&v| i64::from(v)));
            } else {
                out.extend(buf.plane_i32(c).unwrap().iter().map(|&v| i64::from(v)));
            }
        }
    }
    Decoded { planes, rate }
}

fn decode(path: &Path) -> Decoded {
    let mut reader = open_path(path, strict()).unwrap_or_else(|e| panic!("{path:?}: {e}"));
    drain(&mut reader)
}

fn manifest() -> Vec<Vec<String>> {
    let text = std::fs::read_to_string(dir().join("MANIFEST.txt")).expect("MANIFEST.txt");
    text.lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let f: Vec<String> = l.split('|').map(str::to_owned).collect();
            assert!(f.len() >= 6, "bad MANIFEST row: {l}");
            f
        })
        .collect()
}

/// Bits per sample from a STREAMINFO block at the start of a FLAC stream without a tag.
fn streaminfo_bps(file: &[u8]) -> u32 {
    ((u32::from(file[20]) & 1) << 4 | u32::from(file[21]) >> 4) + 1
}

fn expected_bps(reference: &str) -> u32 {
    match reference {
        "ref/st16-48k.wav" | "ref/mono16-48k.wav" | "ref/st16-192k.wav" | "ref/st16-44k1.wav" => 16,
        "ref/st32-48k.wav" => 32,
        "ref/st24-48k.wav" | "ref/ch6-24-48k.wav" | "ref/st24-96k.wav" | "ref/st24w-48k.wav" => 24,
        other => panic!("unknown reference {other}"),
    }
}

#[test]
fn every_manifest_file_has_its_hash_and_decodes_to_the_reference() {
    let rows = manifest();
    let (mut refs, mut flacs) = (0, 0);
    for f in &rows {
        let (kind, name, sha, reference) = (&f[0], &f[1], &f[2], &f[3]);
        assert!(!f[5].is_empty(), "{name}: generator missing");
        let path = dir().join(name);
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(&hex(&sha256(&bytes)), sha, "{name}: sha256");
        match kind.as_str() {
            "ref" => refs += 1,
            "flac" => {
                flacs += 1;
                assert_eq!(
                    streaminfo_bps(&bytes),
                    expected_bps(reference),
                    "{name}: bps"
                );
                let want = decode(&dir().join(reference));
                let got = decode(&path);
                assert_eq!(got.rate, want.rate, "{name}: rate");
                assert_eq!(got.planes.len(), want.planes.len(), "{name}: channels");
                assert!(!got.planes[0].is_empty(), "{name}: empty");
                for (c, (g, w)) in got.planes.iter().zip(&want.planes).enumerate() {
                    assert_eq!(g.len(), w.len(), "{name}: frames of channel {c}");
                    if let Some(i) = g.iter().zip(w).position(|(a, b)| a != b) {
                        panic!("{name}: channel {c} frame {i}: {} != {}", g[i], w[i]);
                    }
                }
            }
            other => panic!("unknown kind {other}"),
        }
    }
    assert_eq!((refs, flacs), (9, 22));
}

/// Length of the frame header at the start of `d` (RFC 9639 section 9.1).
fn header_len(d: &[u8]) -> usize {
    let (bs, sr) = (d[2] >> 4, d[2] & 15);
    let lead = d[4];
    let mut pos = 4 + if lead < 0x80 {
        1
    } else {
        lead.leading_ones() as usize
    };
    pos += match bs {
        6 => 1,
        7 => 2,
        _ => 0,
    };
    pos += match sr {
        12 => 1,
        13 | 14 => 2,
        _ => 0,
    };
    pos + 1
}

/// `(frames, side-coded frames, frames whose first subframe has wasted bits)`.
fn frame_stats(name: &str) -> (usize, usize, usize, Vec<u8>) {
    let data = std::fs::read(dir().join(name)).unwrap();
    let mut demux = open_flac(Box::new(VecSource::new(data)), &strict()).unwrap();
    let (mut frames, mut side, mut wasted) = (0, 0, 0);
    let mut kinds = Vec::new();
    let mut p = Packet::default();
    while demux.next_packet(&mut p).unwrap() {
        frames += 1;
        let ch = p.data[3] >> 4;
        if (8..=10).contains(&ch) {
            side += 1;
            if !kinds.contains(&ch) {
                kinds.push(ch);
            }
        }
        if p.data[header_len(&p.data)] & 1 != 0 {
            wasted += 1;
        }
    }
    (frames, side, wasted, kinds)
}

#[test]
fn side_channel_frames_decode_bit_exactly() {
    // The two sines are uncorrelated, so the encoder's automatic mode stays independent;
    // `-ch_mode` forces each side-channel assignment instead.
    for (name, code) in [
        ("st16-leftside.flac", 8u8),
        ("st16-rightside.flac", 9),
        ("st16-midside.flac", 10),
    ] {
        let (frames, side, _, kinds) = frame_stats(name);
        assert!(frames > 0 && side > 0, "{name}: no side-channel frame");
        assert_eq!(kinds, [code], "{name}: channel assignments");
    }
}

#[test]
fn wasted_bits_are_signalled_and_decoded() {
    for name in ["st24-wasted.flac", "st32.flac"] {
        let (frames, _, wasted, _) = frame_stats(name);
        assert!(
            wasted > 0,
            "{name}: no wasted-bits subframe among {frames} frames"
        );
    }
    // 16-bit content in a 24-bit stream: the 8 low bits of every sample are zero.
    let got = decode(&dir().join("st24-wasted.flac"));
    assert!(got.planes.iter().flatten().all(|v| v % 256 == 0));
    assert!(got.planes.iter().flatten().any(|&v| v != 0));
}

#[test]
fn id3v2_prefix_does_not_change_the_samples() {
    let plain = std::fs::read(dir().join("st16-level5.flac")).unwrap();
    // ID3v2.3 header with a 1000-byte body (synchsafe size 0x07 0x68).
    let mut tagged = b"ID3\x03\x00\x00\x00\x00\x07\x68".to_vec();
    tagged.extend(vec![0u8; 1000]);
    tagged.extend(&plain);
    let mut reader = open(Box::new(VecSource::new(tagged)), strict()).unwrap();
    let got = drain(&mut reader);
    let want = decode(&dir().join("ref/st16-48k.wav"));
    assert_eq!(got.planes, want.planes);
}
