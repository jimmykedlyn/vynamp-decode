//! ALAC in MP4 and M4B chapters through the facade (`DECODE-AC-53`, `DECODE-AC-54`).
//!
//! Fixtures: `tests/fixtures/modern/alac/`, written by `tools/fixtures/generate-alac.ps1`
//! and listed with sha256, frame count and exact command in its `MANIFEST.txt`. The oracle
//! of each ALAC file is the WAV the encoder was fed, read here by a WAV parser of this file,
//! never by the decoder under test. The oracle of the chapters is `chapters.ffmeta`, the
//! FFMETADATA the M4B was muxed from, parsed here.

#![cfg(all(feature = "mp4", feature = "alac"))]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use vynamp_decode::{AudioBuffer, CodecId, Options, Reader, SampleFormat, open_path};
use vynamp_decode_testkit::{hex, sha256};

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/modern/alac")
}

/// One `MANIFEST.txt` row.
struct Entry {
    sha256: String,
    rate: u32,
    channels: usize,
    bits: u8,
    frames: u64,
    origin: String,
}

/// The manifest, keyed by file name. Rows without audio (`-` columns) are kept with zeros;
/// any other non-numeric column fails the test.
fn manifest() -> HashMap<String, Entry> {
    let text = std::fs::read_to_string(dir().join("MANIFEST.txt")).expect("MANIFEST.txt");
    let mut out = HashMap::new();
    for line in text
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
    {
        let f: Vec<&str> = line.split('|').collect();
        assert!(f.len() >= 9, "short manifest row: {line}");
        // `-` marks a row without audio; anything else must be a number.
        let num = |s: &str| match s {
            "-" => 0,
            _ => s
                .parse::<u64>()
                .unwrap_or_else(|e| panic!("bad number {s:?} in manifest row {line}: {e}")),
        };
        out.insert(
            f[0].to_string(),
            Entry {
                sha256: f[1].to_string(),
                rate: num(f[2]) as u32,
                channels: num(f[3]) as usize,
                bits: num(f[4]) as u8,
                frames: num(f[5]),
                origin: f[6].to_string(),
            },
        );
    }
    out
}

/// Reads `name` and checks it against its manifest hash.
fn load_checked(m: &HashMap<String, Entry>, name: &str) -> Vec<u8> {
    let data = std::fs::read(dir().join(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
    let e = m
        .get(name)
        .unwrap_or_else(|| panic!("{name} not in manifest"));
    assert_eq!(hex(&sha256(&data)), e.sha256, "{name}: sha256 differs");
    data
}

/// Source PCM of a WAV (PCM s16 or s24 little endian), one `Vec<i32>` per channel.
/// Independent of the crate under test.
fn read_wav(data: &[u8]) -> (u32, u8, Vec<Vec<i32>>) {
    assert_eq!(&data[..4], b"RIFF");
    assert_eq!(&data[8..12], b"WAVE");
    let (mut rate, mut channels, mut bits) = (0u32, 0usize, 0u16);
    let mut at = 12;
    while at + 8 <= data.len() {
        let id = &data[at..at + 4];
        let len = u32::from_le_bytes(data[at + 4..at + 8].try_into().unwrap()) as usize;
        let body = &data[at + 8..at + 8 + len];
        if id == b"fmt " {
            channels = usize::from(u16::from_le_bytes([body[2], body[3]]));
            rate = u32::from_le_bytes(body[4..8].try_into().unwrap());
            bits = u16::from_le_bytes([body[14], body[15]]);
        } else if id == b"data" {
            let width = usize::from(bits / 8);
            let mut planes = vec![Vec::new(); channels];
            for (i, s) in body.chunks_exact(width).enumerate() {
                let v = match width {
                    2 => i32::from(i16::from_le_bytes([s[0], s[1]])),
                    3 => i32::from_le_bytes([0, s[0], s[1], s[2]]) >> 8,
                    _ => panic!("unexpected WAV width {width}"),
                };
                planes[i % channels].push(v);
            }
            return (rate, bits as u8, planes);
        }
        at += 8 + len + (len & 1);
    }
    panic!("WAV without data chunk")
}

/// Reads `r` until `Ok(false)`, one `Vec<i32>` per channel; checks the buffer format.
fn drain(r: &mut Reader, want: SampleFormat) -> Vec<Vec<i32>> {
    let mut planes = vec![Vec::new(); usize::from(r.track().channels)];
    let mut buf = AudioBuffer::new();
    while r.read(&mut buf).expect("decode") {
        assert_eq!(buf.format(), want);
        for (c, p) in planes.iter_mut().enumerate() {
            match want {
                SampleFormat::I16 => {
                    p.extend(buf.plane_i16(c).unwrap().iter().map(|&v| i32::from(v)))
                }
                _ => p.extend_from_slice(buf.plane_i32(c).unwrap()),
            }
        }
    }
    assert_eq!(buf.frames(), 0, "Ok(false) must leave the buffer empty");
    planes
}

fn format_for(bits: u8) -> SampleFormat {
    match bits {
        16 => SampleFormat::I16,
        24 => SampleFormat::I24,
        b => panic!("no fixture has {b} bits"),
    }
}

/// `DECODE-AC-53` (TASK-E01-11-AC-1).
#[test]
fn alac_in_mp4_decodes_bit_exact_to_its_source() {
    let m = manifest();
    for name in ["alac-s16.m4a", "alac-s24.m4a", "alac-s16-mono.m4a"] {
        load_checked(&m, name);
        let e = &m[name];
        let src = load_checked(&m, &e.origin);
        let (rate, bits, want) = read_wav(&src);
        assert_eq!(
            (rate, bits, want.len()),
            (e.rate, e.bits, e.channels),
            "{name}"
        );
        assert_eq!(want[0].len() as u64, m[&e.origin].frames, "{}", e.origin);
        assert!(e.frames > 0, "{name}: empty fixture proves nothing");

        let mut r = open_path(&dir().join(name), Options::default())
            .unwrap_or_else(|err| panic!("{name}: {err}"));
        assert_eq!(r.track().codec, CodecId::Alac, "{name}");
        assert_eq!(r.track().sample_rate, e.rate, "{name}");
        assert_eq!(usize::from(r.track().channels), e.channels, "{name}");
        assert_eq!(r.frames(), Some(e.frames), "{name}: raw frame count");

        let got = drain(&mut r, format_for(e.bits));
        for c in 0..e.channels {
            assert_eq!(got[c].len(), want[c].len(), "{name} channel {c}: length");
            assert!(got[c] == want[c], "{name} channel {c}: not bit-exact");
        }

        let half = e.frames / 2;
        r.seek(half)
            .unwrap_or_else(|err| panic!("{name}: seek: {err}"));
        assert_eq!(r.position(), half, "{name}");
        let tail = drain(&mut r, format_for(e.bits));
        for c in 0..e.channels {
            assert!(
                tail[c] == got[c][half as usize..],
                "{name} channel {c}: decode after seek is not the slice of the linear decode"
            );
        }
    }
}

/// Chapters of an FFMETADATA file as (title, start frame at `rate`).
fn parse_ffmeta(text: &str, rate: u32) -> Vec<(String, u64)> {
    assert!(text.starts_with(";FFMETADATA1"));
    assert!(
        !text.contains('\\'),
        "escapes are not handled by this parser"
    );
    let mut out = Vec::new();
    let mut cur: Option<(u64, u64, u64, String)> = None;
    let flush = |cur: &mut Option<(u64, u64, u64, String)>, out: &mut Vec<(String, u64)>| {
        if let Some((num, den, start, title)) = cur.take() {
            let scaled = u128::from(start) * u128::from(rate) * u128::from(num);
            assert_eq!(
                scaled % u128::from(den),
                0,
                "chapter start is not on a frame"
            );
            out.push((title, (scaled / u128::from(den)) as u64));
        }
    };
    for line in text.lines() {
        if line == "[CHAPTER]" {
            flush(&mut cur, &mut out);
            cur = Some((1, 1_000_000_000, 0, String::new()));
            continue;
        }
        let (Some(c), Some((k, v))) = (cur.as_mut(), line.split_once('=')) else {
            continue;
        };
        match k {
            "TIMEBASE" => {
                let (n, d) = v.split_once('/').expect("TIMEBASE=num/den");
                c.0 = n.parse().unwrap();
                c.1 = d.parse().unwrap();
            }
            "START" => c.2 = v.parse().unwrap(),
            "title" => c.3 = v.to_string(),
            _ => {}
        }
    }
    flush(&mut cur, &mut out);
    out
}

/// `DECODE-AC-54` (TASK-E01-11-AC-2).
#[test]
fn m4b_chapters_match_their_ffmetadata() {
    let m = manifest();
    let name = "alac-s16-chapters.m4b";
    load_checked(&m, name);
    let e = &m[name];
    let (wav, ffmeta) = e.origin.split_once('+').expect("origin is wav+ffmeta");
    let meta = String::from_utf8(load_checked(&m, ffmeta)).unwrap();
    let want = parse_ffmeta(&meta, e.rate);
    assert_eq!(want.len(), 1, "the fixture declares exactly one chapter");
    assert!(
        want[0].1 > 0,
        "a start of 0 would not tell a dropped timestamp apart"
    );

    let mut r = open_path(&dir().join(name), Options::default())
        .unwrap_or_else(|err| panic!("{name}: {err}"));
    let got: Vec<(String, u64)> = r
        .chapters()
        .iter()
        .map(|c| (c.title.clone(), c.start_frame))
        .collect();
    assert_eq!(got, want);
    assert_eq!(r.frames(), Some(e.frames), "raw frame count");

    let (_, _, src) = read_wav(&load_checked(&m, wav));
    let half = e.frames / 2;
    r.seek(half).expect("seek");
    assert_eq!(r.position(), half);
    let tail = drain(&mut r, format_for(e.bits));
    assert_eq!(tail.len(), src.len());
    for c in 0..src.len() {
        assert!(!tail[c].is_empty());
        assert!(
            tail[c] == src[c][half as usize..],
            "channel {c}: seek did not land on the expected slice"
        );
    }
}
