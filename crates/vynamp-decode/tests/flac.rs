//! FLAC through the facade: probing, ID3v2 prefixes, bit-exact decoding and
//! Resilient recovery. Fixtures are the FFmpeg-encoded files of the FLAC crate.

#![cfg(feature = "flac")]

use std::path::{Path, PathBuf};
use vynamp_decode::{AudioBuffer, Error, Event, Mode, Options, Reader, VecSource, open};

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../vynamp-decode-flac/tests/fixtures")
        .join(name)
}

fn load(name: &str) -> Vec<u8> {
    let p = fixture_path(name);
    std::fs::read(&p).unwrap_or_else(|e| panic!("{p:?}: {e}"))
}

fn options(mode: Mode) -> Options {
    Options {
        mode,
        ..Options::default()
    }
}

fn reader(data: Vec<u8>, mode: Mode) -> Reader {
    open(Box::new(VecSource::new(data)), options(mode)).expect("open")
}

/// Drains `r` into one `Vec<i32>` per channel, stopping at the first error.
fn drain(r: &mut Reader) -> (Vec<Vec<i32>>, Result<(), Error>) {
    let mut chans = vec![Vec::new(); usize::from(r.track().channels)];
    let mut buf = AudioBuffer::new();
    loop {
        match r.read(&mut buf) {
            Ok(true) => {}
            Ok(false) => return (chans, Ok(())),
            Err(e) => return (chans, Err(e)),
        }
        for (c, v) in chans.iter_mut().enumerate() {
            if let Some(s) = buf.plane_i16(c) {
                v.extend(s.iter().map(|&x| i32::from(x)));
            } else {
                v.extend_from_slice(buf.plane_i32(c).unwrap());
            }
        }
    }
}

/// Source channels as s32 (`tools/fixtures/source-sine.wav`: 44 header bytes, s32 LE).
fn source() -> Vec<Vec<i32>> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/fixtures/source-sine.wav");
    let wav = std::fs::read(path).unwrap();
    let mut ch = vec![Vec::new(), Vec::new()];
    for (i, b) in wav[44..].as_chunks::<4>().0.iter().enumerate() {
        ch[i % 2].push(i32::from_le_bytes(*b));
    }
    ch
}

fn id3v2_prefixed(data: &[u8]) -> Vec<u8> {
    // ID3v2.3 header, 10 bytes of flags/size, then 37 bytes of tag body (synchsafe size).
    let mut out = b"ID3\x03\x00\x00\x00\x00\x00\x25".to_vec();
    out.extend((0..37u8).map(|i| i.wrapping_mul(7)));
    out.extend_from_slice(data);
    out
}

/// Offset of the first audio frame of a plain FLAC file (after `fLaC` and the metadata blocks).
fn audio_start(data: &[u8]) -> usize {
    assert_eq!(&data[..4], b"fLaC");
    let mut at = 4;
    loop {
        let last = data[at] & 0x80 != 0;
        let len = usize::from(data[at + 1]) << 16
            | usize::from(data[at + 2]) << 8
            | usize::from(data[at + 3]);
        at += 4 + len;
        if last {
            return at;
        }
    }
}

#[test]
fn registered_in_the_default_registry() {
    let reg = vynamp_decode::default_registry();
    assert!(reg.containers().any(|c| c.name == "flac"));
    assert!(reg.codecs().any(|c| c.name == "flac"));
}

#[test]
fn strict_intact_file_is_bit_exact_for_its_declared_length() {
    let src = source();
    for (name, shift) in [("sine-16-stereo.flac", 16), ("sine-24-stereo.flac", 8)] {
        let mut r = reader(load(name), Mode::Strict);
        let frames = r.frames().expect("known length");
        let (got, end) = drain(&mut r);
        end.unwrap();
        assert!(r.take_events().is_empty());
        for c in 0..2 {
            assert_eq!(got[c].len() as u64, frames, "{name} channel {c}");
            let want: Vec<i32> = src[c].iter().map(|&v| v >> shift).collect();
            assert_eq!(got[c], want, "{name} channel {c}");
        }
    }
}

#[test]
fn open_path_uses_the_extension_and_probe() {
    let mut r =
        vynamp_decode::open_path(&fixture_path("sine-16-stereo.flac"), Options::default()).unwrap();
    let frames = r.frames().unwrap();
    let (got, end) = drain(&mut r);
    end.unwrap();
    assert_eq!(got[0].len() as u64, frames);
}

#[test]
fn id3v2_prefixed_flac_decodes_identically() {
    let plain = load("sine-16-stereo.flac");
    let (want, end) = drain(&mut reader(plain.clone(), Mode::Strict));
    end.unwrap();
    let mut r = reader(id3v2_prefixed(&plain), Mode::Strict);
    let (got, end) = drain(&mut r);
    end.unwrap();
    assert_eq!(got, want);
}

#[test]
fn resilient_corrupt_middle_frame_keeps_length_and_reports_once() {
    let plain = load("sine-16-stereo.flac");
    let mut clean = reader(plain.clone(), Mode::Resilient);
    let (want, end) = drain(&mut clean);
    end.unwrap();
    let mut bad = plain;
    let start = audio_start(&bad);
    let mid = start + (bad.len() - start) / 2;
    bad[mid] ^= 0xFF;
    let mut r = reader(bad, Mode::Resilient);
    let (got, end) = drain(&mut r);
    end.unwrap();
    assert_eq!(got[0].len(), want[0].len());
    assert_eq!(got[1].len(), want[1].len());
    assert!(got != want, "corruption left the audio unchanged");
    let events = r.take_events();
    let corrupt = events
        .iter()
        .filter(|e| matches!(e, Event::CorruptFrame { .. }))
        .count();
    assert_eq!(corrupt, 1, "{events:?}");
}

#[test]
fn truncated_tail_is_an_error_in_strict_and_reported_in_resilient() {
    let plain = load("sine-16-stereo.flac");
    let cut = plain[..plain.len() - plain.len() / 8].to_vec();

    let (_, end) = match open(Box::new(VecSource::new(cut.clone())), options(Mode::Strict)) {
        Ok(mut r) => drain(&mut r),
        Err(e) => (Vec::new(), Err(e)),
    };
    assert!(end.is_err(), "Strict must reject a truncated stream");

    let mut r = reader(cut, Mode::Resilient);
    let (_, end) = drain(&mut r);
    end.unwrap();
    let events = r.take_events();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::CorruptFrame { .. })),
        "{events:?}"
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, Event::ChecksumMismatch { .. })),
        "{events:?}"
    );
}

#[test]
fn a_stream_without_its_leading_frames_is_rebased_and_decodes_strictly() {
    use vynamp_decode::Demuxer;
    let plain = load("sine-16-stereo.flac");
    let (want, end) = drain(&mut reader(plain.clone(), Mode::Strict));
    end.unwrap();
    let mut d =
        vynamp_decode_flac::open_flac(Box::new(VecSource::new(plain.clone())), &Options::default())
            .unwrap();
    let mut p = vynamp_decode::Packet::default();
    let mut starts = Vec::new();
    while d.next_packet(&mut p).unwrap() {
        starts.push((p.offset as usize, p.pts));
    }
    let (cut_at, origin) = starts[4];
    let mut cut = plain[..audio_start(&plain)].to_vec();
    cut.extend_from_slice(&plain[cut_at..]);
    let mut r = reader(cut, Mode::Strict);
    assert_eq!(r.frames(), Some(want[0].len() as u64 - origin));
    let (got, end) = drain(&mut r);
    end.unwrap();
    assert_eq!(got[0][..], want[0][origin as usize..]);
    assert_eq!(got[1][..], want[1][origin as usize..]);
    assert_eq!(r.take_events(), vec![Event::TimelineRebased { origin }]);
}

#[test]
fn resilient_delivers_samples_beyond_a_too_small_declared_total() {
    let plain = load("sine-16-stereo.flac");
    let (want, end) = drain(&mut reader(plain.clone(), Mode::Strict));
    end.unwrap();
    let mut bad = plain;
    // STREAMINFO body starts at byte 8; total samples are the low nibble of body[13]
    // and body[14..18].
    bad[8 + 13] &= 0xF0;
    bad[8 + 14..8 + 18].copy_from_slice(&2048u32.to_be_bytes());
    let mut r = reader(bad.clone(), Mode::Resilient);
    assert_eq!(r.frames(), Some(2048));
    let (got, end) = drain(&mut r);
    end.unwrap();
    assert!(got == want, "every sample is delivered");
    let events = r.take_events();
    assert!(
        events.iter().any(|e| matches!(e, Event::Clamped { .. })),
        "{events:?}"
    );
    // Seeking is not clamped to the declared length either.
    let mut r = reader(bad, Mode::Resilient);
    r.seek(4000).unwrap();
    assert_eq!(r.position(), 4000);
    let (tail, end) = drain(&mut r);
    end.unwrap();
    assert!(tail[0][..] == want[0][4000..]);
}
