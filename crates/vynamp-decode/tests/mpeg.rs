//! MPEG audio through the facade: registration, exact gapless output, seek equivalence,
//! Layer I/II/III delays, tag wrappers and Resilient recovery.
//!
//! Gapless fixtures (`tests/fixtures/mpeg/g-*`) come from `tools/fixtures/generate-mpeg-dev.ps1`:
//! `<name>.src.wav` is the PCM that was encoded, `<name>.mp3` the libmp3lame encode with
//! FFmpeg's Xing/Info and LAME tag, `<name>.ref.f32` FFmpeg's own gapless `mp3float` decode of it.
//! The other fixtures are those of the `vynamp-decode-mpa` and `vynamp-decode-mp3` crates.

#![cfg(all(feature = "mpa", feature = "mp3"))]

use std::path::{Path, PathBuf};
use vynamp_decode::{
    AudioBuffer, CodecId, Error, Event, GaplessKind, Mode, Options, Reader, VecSource,
    default_registry, open, open_path,
};
use vynamp_decode_mpa::parse_header;

fn fixture(krate: &str, name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(krate)
        .join("tests/fixtures")
        .join(name)
}

fn gapless(name: &str) -> PathBuf {
    fixture("vynamp-decode", &format!("mpeg/{name}"))
}

fn options(mode: Mode) -> Options {
    Options {
        mode,
        ..Options::default()
    }
}

fn reader_of(data: Vec<u8>, mode: Mode) -> Reader {
    open(Box::new(VecSource::new(data)), options(mode)).expect("open")
}

/// Drains `r` into one vector per channel, stopping at the first error.
fn drain(r: &mut Reader) -> (Vec<Vec<f32>>, Result<(), Error>) {
    drain_up_to(r, usize::MAX)
}

/// Like [`drain`], but stops once `limit` frames (or more) have been read.
fn drain_up_to(r: &mut Reader, limit: usize) -> (Vec<Vec<f32>>, Result<(), Error>) {
    let mut chans = vec![Vec::new(); usize::from(r.track().channels)];
    let mut buf = AudioBuffer::new();
    while chans[0].len() < limit {
        match r.read(&mut buf) {
            Ok(true) => {}
            Ok(false) => return (chans, Ok(())),
            Err(e) => return (chans, Err(e)),
        }
        for (c, v) in chans.iter_mut().enumerate() {
            v.extend_from_slice(buf.plane_f32(c).expect("f32 planes"));
        }
    }
    (chans, Ok(()))
}

fn decode(path: &Path) -> (Reader, Vec<Vec<f32>>) {
    let mut r = open_path(path, Options::default()).unwrap_or_else(|e| panic!("{path:?}: {e:?}"));
    let (planes, res) = drain(&mut r);
    assert_eq!(res, Ok(()), "{path:?}");
    (r, planes)
}

fn split_planes(interleaved: &[f32], channels: usize) -> Vec<Vec<f32>> {
    (0..channels)
        .map(|c| {
            interleaved
                .iter()
                .skip(c)
                .step_by(channels)
                .copied()
                .collect()
        })
        .collect()
}

fn read_f32(path: &Path, channels: usize) -> Vec<Vec<f32>> {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
    let flat: Vec<f32> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect();
    split_planes(&flat, channels)
}

/// The 16-bit PCM of a WAV with a 44-byte header as f32 in [-1, 1), per channel.
fn read_wav16(path: &Path, channels: usize) -> Vec<Vec<f32>> {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
    assert_eq!(&bytes[..4], b"RIFF");
    assert_eq!(
        &bytes[36..40],
        b"data",
        "{path:?}: expected a 44-byte header"
    );
    let flat: Vec<f32> = bytes[44..]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| f32::from(i16::from_le_bytes(*b)) / 32768.0)
        .collect();
    split_planes(&flat, channels)
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

/// ISO/IEC 11172-4 full accuracy: RMS error below 2^-15 / sqrt(12) and maximum error at most
/// 2^-14, per channel; the alignment is pinned by a lag of one sample either way being far worse.
fn assert_iso(name: &str, got: &[Vec<f32>], want: &[Vec<f32>]) {
    let rms_limit = 2f64.powi(-15) / 12f64.sqrt();
    let max_limit = 2f64.powi(-14);
    assert_eq!(got.len(), want.len(), "{name}");
    for (c, (g, w)) in got.iter().zip(want).enumerate() {
        assert_eq!(g.len(), w.len(), "{name} ch{c} length");
        let (rms, max) = errors(g, w, 0);
        println!("{name} ch{c}: rms {rms:.3e} max {max:.3e}");
        assert!(rms < rms_limit, "{name} ch{c}: rms {rms:e}");
        assert!(max <= max_limit, "{name} ch{c}: max {max:e}");
        for lag in [-1isize, 1] {
            let (r, _) = errors(g, w, lag);
            assert!(r > 100.0 * rms_limit, "{name} ch{c}: lag {lag} rms {r:e}");
        }
    }
}

fn snr_db(got: &[Vec<f32>], want: &[Vec<f32>]) -> f64 {
    let (mut sig, mut err) = (0.0f64, 0.0f64);
    for (g, w) in got.iter().zip(want) {
        for (&a, &b) in g.iter().zip(w) {
            sig += f64::from(b) * f64::from(b);
            err += (f64::from(a) - f64::from(b)).powi(2);
        }
    }
    10.0 * (sig / err).log10()
}

// ---------------------------------------------------------------- registration

#[test]
fn the_default_registry_knows_mpeg_audio() {
    let reg = default_registry();
    let mpa = reg.containers().find(|c| c.name == "mpa").expect("mpa");
    assert!(mpa.extensions.contains(&"mp3"));
    for id in [CodecId::Mp1, CodecId::Mp2, CodecId::Mp3] {
        assert!(reg.codec_for(&id).is_some(), "{id:?}");
    }
}

// ---------------------------------------------------------------- gapless

const GAPLESS: [&str; 5] = [
    "g-cbr128-48k-stereo",
    "g-vbr2-48k-stereo",
    "g-mono-48k-64k",
    "g-mpeg2-22k-stereo",
    "g-cbr128-48k-odd",
];

#[test]
fn lame_tagged_files_decode_to_exactly_the_encoded_pcm_length() {
    for name in GAPLESS {
        let (r, planes) = decode(&gapless(&format!("{name}.mp3")));
        let t = r.track().clone();
        let channels = usize::from(t.channels);
        let src = read_wav16(&gapless(&format!("{name}.src.wav")), channels);
        let reference = read_f32(&gapless(&format!("{name}.ref.f32")), channels);
        println!(
            "{name}: delay {} padding {} frames {:?} -> out {} (source {}, FFmpeg {})",
            t.delay,
            t.padding,
            t.frames,
            planes[0].len(),
            src[0].len(),
            reference[0].len()
        );
        assert_eq!(t.gapless, GaplessKind::FromMetadata, "{name}");
        assert_eq!(
            planes[0].len(),
            src[0].len(),
            "{name}: length vs the source"
        );
        assert_eq!(r.frames(), Some(src[0].len() as u64), "{name}");
        assert_eq!(r.position(), src[0].len() as u64, "{name}");
        // Sample for sample as FFmpeg's gapless decode, from sample 0.
        assert_iso(name, &planes, &reference);
        // A sanity bound against the original PCM: MP3 is lossy, a gross misalignment is not.
        let snr = snr_db(&planes, &src);
        println!("{name}: SNR against the source {snr:.1} dB");
        assert!(snr > 20.0, "{name}: SNR {snr}");
    }
}

#[test]
fn gapless_output_is_the_same_in_resilient_mode_and_without_the_id3_tag() {
    let name = "g-cbr128-48k-stereo";
    let data = std::fs::read(gapless(&format!("{name}.mp3"))).unwrap();
    let (_, strict) = decode(&gapless(&format!("{name}.mp3")));
    // FFmpeg's file starts with a 10-byte ID3v2 header and no body.
    assert_eq!(&data[..3], b"ID3");
    let mut r = reader_of(data[10..].to_vec(), Mode::Resilient);
    let (planes, res) = drain(&mut r);
    assert_eq!(res, Ok(()));
    assert!(planes == strict);
    assert!(r.take_events().is_empty());
}

// ---------------------------------------------------------------- untagged streams and Layers I/II

#[test]
fn layer_two_output_is_the_reference_without_the_filterbank_delay() {
    // FFmpeg does not trim Layer II: its decode holds every sample of every frame. Our Reader
    // trims the 481 samples of filterbank delay (Track::delay), so the output is reference[481..].
    for name in [
        "l2-stereo-48k-192k",
        "l2-joint-44k-128k",
        "l2-mono-48k-64k",
        "l2-lsf-24k-64k",
    ] {
        let (r, planes) = decode(&fixture("vynamp-decode-mp3", &format!("{name}.mp2")));
        let t = r.track();
        assert_eq!(
            (t.codec.clone(), t.delay, t.padding),
            (CodecId::Mp2, 481, 0),
            "{name}"
        );
        assert_eq!(
            (t.gapless, t.frames),
            (GaplessKind::Unknown, None),
            "{name}"
        );
        let reference = read_f32(
            &fixture("vynamp-decode-mp3", &format!("{name}.f32")),
            usize::from(t.channels),
        );
        let trimmed: Vec<Vec<f32>> = reference.iter().map(|p| p[481..].to_vec()).collect();
        assert_iso(name, &planes, &trimmed);
    }
}

#[test]
fn untagged_layer_three_output_is_the_reference_without_the_decoder_delay() {
    // FFmpeg drops nothing from a file without a LAME tag; the Reader trims 529 samples.
    for name in [
        "l3-cbr128-44k-joint",
        "l3-vbr-v0-44k",
        "l3-mono-44k-64k",
        "l3-mpeg2-22k-64k",
        "l3-mpeg25-8k-mono",
    ] {
        let (r, planes) = decode(&fixture("vynamp-decode-mp3", &format!("{name}.mp3")));
        let t = r.track();
        assert_eq!(
            (t.codec.clone(), t.delay, t.padding),
            (CodecId::Mp3, 529, 0),
            "{name}"
        );
        assert_eq!(
            (t.gapless, t.frames),
            (GaplessKind::Unknown, None),
            "{name}"
        );
        let reference = read_f32(
            &fixture("vynamp-decode-mp3", &format!("{name}.f32")),
            usize::from(t.channels),
        );
        let trimmed: Vec<Vec<f32>> = reference.iter().map(|p| p[529..].to_vec()).collect();
        assert_iso(name, &planes, &trimmed);
    }
}

// ---------------------------------------------------------------- seeking

/// Seeks to a spread of positions (forward, then backward on the same reader) and checks that
/// decoding after the seek gives exactly the samples of the full decode (the decoder is
/// deterministic, so the ISO tolerance is not needed).
fn assert_seek_equivalence(path: &Path) {
    let (mut r, full) = decode(path);
    let len = full[0].len();
    assert!(len > 8000, "{path:?}");
    let mut positions = vec![
        0,
        1,
        576,
        1152,
        len / 7,
        len / 3 + 5,
        len / 2,
        len / 2 + 1151,
        len * 3 / 4 + 17,
        len - 1153,
        len - 1,
        len,
    ];
    let forward = positions.clone();
    positions.extend(forward.iter().rev());
    for &pos in &positions {
        r.seek(pos as u64)
            .unwrap_or_else(|e| panic!("{path:?} seek {pos}: {e:?}"));
        assert_eq!(r.position(), pos as u64, "{path:?} seek {pos}");
        let (got, res) = drain_up_to(&mut r, 10_000);
        assert_eq!(res, Ok(()), "{path:?} seek {pos}");
        for (c, (g, f)) in got.iter().zip(&full).enumerate() {
            // At least 10000 frames (or all that is left) from the seek on, identical.
            assert!(
                g.len() >= (len - pos).min(10_000),
                "{path:?} seek {pos} ch{c} length"
            );
            assert!(g.len() <= len - pos, "{path:?} seek {pos} ch{c} length");
            assert!(
                g[..] == f[pos..pos + g.len()],
                "{path:?} seek {pos} ch{c}: not identical to the full decode"
            );
        }
    }
}

#[test]
fn decode_after_seek_equals_the_full_decode_slice() {
    for (krate, name) in [
        ("vynamp-decode-mpa", "cbr32-48k-stereo.mp3"),
        ("vynamp-decode-mpa", "cbr8-8k-mono.mp3"),
        ("vynamp-decode-mpa", "mpeg25-8k-mono.mp3"),
        ("vynamp-decode-mpa", "mpeg2-22k-stereo.mp3"),
        ("vynamp-decode-mpa", "vbr-q2-48k-stereo.mp3"),
        ("vynamp-decode-mpa", "mp2-128k-44k-stereo.mp2"),
        ("vynamp-decode-mp3", "l3-vbr-v0-44k.mp3"),
        ("vynamp-decode-mp3", "l3-cbr128-44k-joint.mp3"),
    ] {
        assert_seek_equivalence(&fixture(krate, name));
    }
}

#[test]
fn seeking_in_a_gapless_file_lands_on_the_trimmed_timeline() {
    let (mut r, full) = decode(&gapless("g-cbr128-48k-stereo.mp3"));
    let len = full[0].len();
    r.seek(1000).unwrap();
    let (got, res) = drain(&mut r);
    assert_eq!(res, Ok(()));
    assert!(got[0][..] == full[0][1000..]);
    assert_eq!(got[0].len(), len - 1000);
    // Past the end: nothing left, and the position is the length.
    r.seek(u64::MAX).unwrap();
    assert_eq!(r.position(), len as u64);
}

// ---------------------------------------------------------------- tags around the audio

fn synchsafe(n: usize) -> [u8; 4] {
    [
        (n >> 21) as u8 & 0x7F,
        (n >> 14) as u8 & 0x7F,
        (n >> 7) as u8 & 0x7F,
        n as u8 & 0x7F,
    ]
}

fn id3v2_with_fake_frames(body_len: usize) -> Vec<u8> {
    let mut t = b"ID3\x04\x00\x00".to_vec();
    t.extend(synchsafe(body_len));
    for i in 0..body_len {
        t.push([0xFF, 0xFB, 0x94, 0x00][i % 4]);
    }
    t
}

fn apev2_with_fake_frames() -> Vec<u8> {
    let value: Vec<u8> = (0..300).map(|i| [0xFF, 0xFB, 0x94, 0x00][i % 4]).collect();
    let mut items = Vec::new();
    items.extend((value.len() as u32).to_le_bytes());
    items.extend(0u32.to_le_bytes());
    items.extend(b"Title\0");
    items.extend(&value);
    let mut tag = items.clone();
    tag.extend(b"APETAGEX");
    tag.extend(2000u32.to_le_bytes());
    tag.extend(((items.len() + 32) as u32).to_le_bytes());
    tag.extend(1u32.to_le_bytes());
    tag.extend(0u32.to_le_bytes());
    tag.extend([0u8; 8]);
    tag
}

fn id3v1() -> Vec<u8> {
    let mut t = vec![0u8; 128];
    t[..3].copy_from_slice(b"TAG");
    t[3..8].copy_from_slice(b"title");
    t
}

#[test]
fn id3v2_apev2_and_id3v1_wrappers_do_not_change_the_output() {
    for name in ["g-cbr128-48k-stereo.mp3", "g-mono-48k-64k.mp3"] {
        let plain = std::fs::read(gapless(name)).unwrap();
        let (plain_planes, res) = drain(&mut reader_of(plain.clone(), Mode::Strict));
        assert_eq!(res, Ok(()));
        let mut wrapped = id3v2_with_fake_frames(1500);
        wrapped.extend(&plain);
        wrapped.extend(apev2_with_fake_frames());
        wrapped.extend(id3v1());
        for mode in [Mode::Strict, Mode::Resilient] {
            let mut r = reader_of(wrapped.clone(), mode);
            assert_eq!(r.track().gapless, GaplessKind::FromMetadata, "{name}");
            let (planes, res) = drain(&mut r);
            assert_eq!(res, Ok(()), "{name}");
            assert!(planes == plain_planes, "{name}");
            assert!(r.take_events().is_empty(), "{name}");
        }
    }
}

// ---------------------------------------------------------------- damage

/// Absolute offsets of the audio frames of a file that starts with one ID3v2 tag followed by
/// an Info frame (FFmpeg's layout).
fn frame_offsets(data: &[u8]) -> Vec<usize> {
    assert_eq!(&data[..3], b"ID3");
    let tag = 10
        + data[6..10]
            .iter()
            .fold(0usize, |a, &b| a << 7 | usize::from(b));
    let mut at = tag;
    let mut offsets = Vec::new();
    while let Some(head) = data.get(at..at + 4) {
        let h = parse_header(head.try_into().unwrap()).expect("frame header");
        offsets.push(at);
        at += h.frame_len;
    }
    assert_eq!(at, data.len());
    offsets.remove(0); // the Info frame
    offsets
}

#[test]
fn a_frame_with_impossible_side_information_is_an_error_or_silence() {
    let plain = std::fs::read(gapless("g-cbr128-48k-stereo.mp3")).unwrap();
    let offsets = frame_offsets(&plain);
    let n = 8;
    let off = offsets[n];
    let len = parse_header(plain[off..off + 4].try_into().unwrap())
        .unwrap()
        .frame_len;
    let mut bad = plain.clone();
    // big_values of granule 0, channel 0 (bits 32..41 of the side information) = 511; at most
    // 288 is possible.
    bad[off + 8] = 0xFF;
    bad[off + 9] |= 0x80;

    // Strict: an error at that frame.
    let mut r = reader_of(bad.clone(), Mode::Strict);
    let (_, res) = drain(&mut r);
    match res {
        Err(Error::Invalid {
            what: "mp3 side info",
            offset,
        }) => {
            assert!(
                (off as u64..(off + len) as u64).contains(&offset),
                "{offset} vs {off}"
            );
        }
        other => panic!("expected an mp3 side info error, got {other:?}"),
    }
    let delay = r.track().delay as usize;

    // Resilient: same length, silence for the frame, one event, and the clean decode again
    // from the third frame after it (the overlap and filterbank history refill).
    let (_, clean) = decode(&gapless("g-cbr128-48k-stereo.mp3"));
    let mut r = reader_of(bad, Mode::Resilient);
    let (planes, res) = drain(&mut r);
    assert_eq!(res, Ok(()));
    let first = n * 1152 - delay;
    for c in 0..2 {
        assert_eq!(planes[c].len(), clean[c].len());
        assert!(planes[c][..first] == clean[c][..first], "before the damage");
        assert!(
            planes[c][first..first + 1152].iter().all(|&v| v == 0.0),
            "the damaged frame"
        );
        let resume = first + 3 * 1152;
        assert!(
            planes[c][resume..] == clean[c][resume..],
            "after the damage"
        );
    }
    assert_eq!(
        r.take_events(),
        vec![Event::CorruptFrame {
            pts: n as u64 * 1152,
            offset: off as u64,
            frames: 1152
        }]
    );
}

#[test]
fn the_reader_reports_conflicting_gapless_information() {
    // An Info header declaring 3 frames cannot hold the delay and padding of its LAME tag.
    fn arc(data: &[u8]) -> u16 {
        let mut crc = 0u16;
        for &b in data {
            crc ^= u16::from(b);
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ 0xA001
                } else {
                    crc >> 1
                };
            }
        }
        crc
    }
    let mut data = std::fs::read(gapless("g-cbr128-48k-stereo.mp3")).unwrap();
    let info = data.windows(4).position(|w| w == b"Info").unwrap();
    let o = info + 120;
    data[info + 8..info + 12].copy_from_slice(&3u32.to_be_bytes());
    data[o + 22] = (data[o + 22] & 0xF0) | 0x0F;
    data[o + 23] = 0xFF;
    let crc = arc(&data[info - 36..o + 34]);
    data[o + 34..o + 36].copy_from_slice(&crc.to_be_bytes());
    let mut r = reader_of(data, Mode::Resilient);
    assert_eq!(r.track().gapless, GaplessKind::Unknown);
    assert_eq!(
        r.take_events(),
        vec![Event::GaplessConflict {
            what: "lame tag exceeds frame count"
        }]
    );
}

#[test]
fn a_stream_cut_out_of_a_longer_one_starts_silently_and_seeks_in_strict_mode() {
    // The first frame of this stream points back into main data that was cut off.
    let plain = std::fs::read(fixture("vynamp-decode-mp3", "l3-cbr128-44k-joint.mp3")).unwrap();
    let mut frames = Vec::new();
    let mut at = 0;
    while let Some(head) = plain.get(at..at + 4) {
        let h = parse_header(head.try_into().unwrap()).unwrap();
        frames.push(at);
        at += h.frame_len;
    }
    let k = (3..frames.len())
        .find(|&k| plain[frames[k] + 4] != 0 || plain[frames[k] + 5] & 0x80 != 0)
        .expect("a frame with main_data_begin > 0");
    let cut = plain[frames[k]..].to_vec();
    let (_, whole) = decode(&fixture("vynamp-decode-mp3", "l3-cbr128-44k-joint.mp3"));
    for mode in [Mode::Strict, Mode::Resilient] {
        let mut r = reader_of(cut.clone(), mode);
        let (planes, res) = drain(&mut r);
        assert_eq!(res, Ok(()), "{mode:?}");
        assert!(r.take_events().is_empty(), "{mode:?}");
        let n = planes[0].len();
        assert_eq!(n, (frames.len() - k) * 1152 - 529);
        // Once the reservoir is filled and the filterbank has run through, it is the same audio.
        let from = 4 * 1152;
        let shift = k * 1152;
        assert!(planes[0][from..] == whole[0][shift + from..], "{mode:?}");
        // Seeking back to the start of such a stream works in Strict mode as well.
        r.seek(0).unwrap();
        let (again, res) = drain(&mut r);
        assert_eq!(res, Ok(()));
        assert!(again == planes);
    }
}
