//! Decode errors inside a CRC-valid frame: Strict fails, Resilient substitutes
//! silence and reports the loss.

use vynamp_decode_core::{
    AudioBuffer, Decoder, Demuxer, Error, Event, LimitKind, Limits, Mode, Options, Packet,
    SampleFormat, VecSource, crc16,
};
use vynamp_decode_flac::{FlacDecoder, open_flac};

fn fixture(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn options(mode: Mode) -> Options {
    Options {
        mode,
        ..Options::default()
    }
}

/// Length of the frame header of a fixed-blocking frame.
fn header_len(frame: &[u8]) -> usize {
    let ext = match frame[2] >> 4 {
        6 => 1,
        7 => 2,
        _ => 0,
    };
    let number = 1 + frame[4].leading_ones().saturating_sub(1) as usize;
    4 + number + ext + 1
}

/// Overwrites the body of frame `idx` with 0xFF (an invalid subframe header) and
/// fixes up the CRC-16, so the demuxer still accepts the frame.
fn corrupt_body(file: &[u8], idx: usize) -> Vec<u8> {
    let mut d = open_flac(
        Box::new(VecSource::new(file.to_vec())),
        &options(Mode::Strict),
    )
    .unwrap();
    let mut p = Packet::default();
    let mut n = 0;
    while d.next_packet(&mut p).unwrap() {
        if n == idx {
            let start = p.offset as usize;
            let end = start + p.data.len();
            let body = start + header_len(&p.data);
            let mut out = file.to_vec();
            out[body..end - 2].fill(0xFF);
            let c = crc16(&out[start..end - 2]);
            out[end - 2..end].copy_from_slice(&c.to_be_bytes());
            return out;
        }
        n += 1;
    }
    panic!("no frame {idx}");
}

struct Run {
    frames: Vec<(u64, usize, bool)>,
    err: Option<Error>,
    events: Vec<Event>,
}

/// Demuxes and decodes everything; each entry is (pts, decoded frames, all zero).
fn run(file: Vec<u8>, mode: Mode) -> Run {
    let opts = options(mode);
    let mut d = open_flac(Box::new(VecSource::new(file)), &opts).unwrap();
    let mut dec = FlacDecoder::with_options(&d.tracks()[0], &opts).unwrap();
    let (mut p, mut out) = (Packet::default(), AudioBuffer::new());
    let (mut frames, mut err) = (Vec::new(), None);
    loop {
        match d.next_packet(&mut p) {
            Ok(true) => {}
            Ok(false) => break,
            Err(e) => {
                err = Some(e);
                break;
            }
        }
        match dec.decode(&p, &mut out) {
            Ok(()) => {
                let zero = out.plane_i16(0).unwrap().iter().all(|&s| s == 0);
                frames.push((p.pts, out.frames(), zero));
            }
            Err(e) => {
                err = Some(e);
                break;
            }
        }
    }
    let mut events = Vec::new();
    d.drain_events(&mut events);
    dec.drain_events(&mut events);
    Run {
        frames,
        err,
        events,
    }
}

#[test]
fn strict_fails_on_a_crc_valid_frame_with_a_bad_body() {
    let bad = corrupt_body(&fixture("sine-16-stereo.flac"), 3);
    let r = run(bad, Mode::Strict);
    assert!(matches!(r.err, Some(Error::Invalid { .. })), "{:?}", r.err);
    assert_eq!(r.frames.len(), 3);
}

#[test]
fn resilient_replaces_a_bad_body_by_silence_and_continues() {
    let plain = fixture("sine-16-stereo.flac");
    let want = run(plain.clone(), Mode::Strict);
    assert!(want.err.is_none());
    let r = run(corrupt_body(&plain, 3), Mode::Resilient);
    assert!(r.err.is_none(), "{:?}", r.err);
    assert_eq!(r.frames.len(), want.frames.len());
    for (i, (g, w)) in r.frames.iter().zip(&want.frames).enumerate() {
        assert_eq!((g.0, g.1), (w.0, w.1), "frame {i}");
        assert_eq!(g.2, i == 3, "frame {i} silence");
    }
    let corrupt: Vec<_> = r
        .events
        .iter()
        .filter(|e| matches!(e, Event::CorruptFrame { .. }))
        .collect();
    assert_eq!(corrupt.len(), 1, "{:?}", r.events);
    assert!(matches!(
        corrupt[0],
        Event::CorruptFrame { pts, frames, .. } if *pts == want.frames[3].0 && *frames as usize == want.frames[3].1
    ));
}

fn silence_packet(pts: u64, frames: u32) -> Packet {
    Packet {
        pts,
        frames,
        ..Packet::default()
    }
}

#[test]
fn silence_packets_are_clamped_to_the_maximum_block_size() {
    let file = fixture("sine-16-stereo.flac");
    let opts = Options::default();
    let d = open_flac(Box::new(VecSource::new(file)), &opts).unwrap();
    let max = usize::from(d.stream_info().max_block);
    let mut dec = FlacDecoder::with_options(&d.tracks()[0], &opts).unwrap();
    let mut out = AudioBuffer::new();
    dec.decode(&silence_packet(0, u32::MAX), &mut out).unwrap();
    assert_eq!(out.frames(), max);
    assert_eq!(out.format(), SampleFormat::I16);
}

#[test]
fn silence_packets_respect_the_frame_limit() {
    let file = fixture("sine-16-stereo.flac");
    let opts = Options {
        limits: Limits {
            max_frames: 5000,
            ..Limits::default()
        },
        ..Options::default()
    };
    let d = open_flac(Box::new(VecSource::new(file)), &opts).unwrap();
    let mut dec = FlacDecoder::with_options(&d.tracks()[0], &opts).unwrap();
    let mut out = AudioBuffer::new();
    let r = dec.decode(&silence_packet(4900, 512), &mut out);
    assert_eq!(r, Err(Error::Limit(LimitKind::Frames)));
}
