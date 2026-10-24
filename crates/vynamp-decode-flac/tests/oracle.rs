//! End-to-end: demux and decode the FFmpeg-encoded fixtures and compare with the source.
//!
//! The fixtures encode `tools/fixtures/source-sine.wav` (48 kHz stereo s32). FLAC is
//! lossless, so the decoded samples must equal the source narrowed the way FFmpeg
//! narrows: s16 keeps the top 16 bits, s24 the top 24 bits.

use vynamp_decode_core::{AudioBuffer, Decoder, Demuxer, Options, Packet, VecSource};
use vynamp_decode_flac::{FlacDecoder, open_flac};

/// Decodes the fixture into one `Vec<i32>` per channel (samples at their native depth).
fn decode_fixture(name: &str) -> Vec<Vec<i32>> {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    let data = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut demux = open_flac(Box::new(VecSource::new(data)), &Options::default()).unwrap();
    let track = demux.tracks()[0].clone();
    let mut dec = FlacDecoder::new(&track).unwrap();
    let mut chans = vec![Vec::new(); usize::from(track.channels)];
    let (mut p, mut out) = (Packet::default(), AudioBuffer::new());
    while demux.next_packet(&mut p).unwrap() {
        dec.decode(&p, &mut out).unwrap();
        assert_eq!(out.frames(), p.frames as usize);
        for (c, v) in chans.iter_mut().enumerate() {
            if let Some(s) = out.plane_i16(c) {
                v.extend(s.iter().map(|&x| i32::from(x)));
            } else {
                v.extend_from_slice(out.plane_i32(c).unwrap());
            }
        }
    }
    chans
}

/// Source channels as s32 (the WAV is 44 header bytes plus interleaved s32 LE).
fn source() -> Vec<Vec<i32>> {
    let path = format!(
        "{}/../../tools/fixtures/source-sine.wav",
        env!("CARGO_MANIFEST_DIR")
    );
    let wav = std::fs::read(path).unwrap();
    let mut ch = vec![Vec::new(), Vec::new()];
    for (i, b) in wav[44..].as_chunks::<4>().0.iter().enumerate() {
        ch[i % 2].push(i32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    }
    ch
}

#[test]
fn stereo_16_bit_equals_source() {
    let (got, src) = (decode_fixture("sine-16-stereo.flac"), source());
    assert_eq!(got[0].len(), 4800);
    for c in 0..2 {
        let want: Vec<i32> = src[c].iter().map(|&v| v >> 16).collect();
        assert_eq!(got[c], want, "channel {c}");
    }
}

#[test]
fn stereo_24_bit_equals_source() {
    let (got, src) = (decode_fixture("sine-24-stereo.flac"), source());
    assert_eq!(got[0].len(), 4800);
    for c in 0..2 {
        let want: Vec<i32> = src[c].iter().map(|&v| v >> 8).collect();
        assert_eq!(got[c], want, "channel {c}");
    }
}

#[test]
fn mono_fixture_is_the_downmix() {
    // FFmpeg's stereo-to-mono conversion is not part of the codec, so only check that the
    // result is the average of the channels to within one least-significant bit.
    let (got, src) = (decode_fixture("sine-16-mono-tail.flac"), source());
    assert_eq!(got[0].len(), 4800);
    for (i, &g) in got[0].iter().enumerate() {
        let avg = (f64::from(src[0][i]) + f64::from(src[1][i])) / 2.0 / 65536.0;
        assert!(
            (f64::from(g) - avg).abs() <= 1.0,
            "sample {i}: {g} vs {avg}"
        );
    }
}
