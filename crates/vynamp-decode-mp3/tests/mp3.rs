//! Layer III decoding through the public `MpegDecoder` API: FFmpeg reference comparison under
//! the ISO/IEC 11172-4 full-accuracy criterion, intensity stereo, errors and garbage.
//!
//! The fixtures (`tests/fixtures/l3-*.mp3`) were encoded by FFmpeg's libmp3lame
//! (`tools/fixtures/generate-mpeg-dev.ps1`) without a Xing/Info/LAME tag; each `.f32` file is
//! FFmpeg's `mp3float` decode of the same file as raw interleaved little-endian f32.
//!
//! **Alignment.** With a LAME tag FFmpeg's demuxer tells the decoder to skip the encoder delay
//! plus 529 + 1 samples at the start and to cut the padding at the end (a tagged 0.3 s encode
//! decodes to exactly 13230 frames). Without the tag it drops nothing: the reference holds
//! `frames * samples per frame` samples per channel and starts with the first sample of the
//! first frame. The decoder under test also trims nothing (gapless trimming is the reader's
//! job), so the two outputs are compared sample for sample at offset 0 over the whole length;
//! a lag of one sample either way must be far worse, which pins the alignment.

mod common;

use common::{Rng, fixture, read_f32, split_frames, track};
use vynamp_decode_core::{AudioBuffer, CodecId, Decoder, Error, Event, Mode, Options, Packet};
use vynamp_decode_mp3::MpegDecoder;
use vynamp_decode_mpa::{MpegVersion, parse_header};

struct Fx {
    name: &'static str,
    rate: u32,
    channels: u16,
    version: MpegVersion,
}

const fn fx(name: &'static str, rate: u32, channels: u16, version: MpegVersion) -> Fx {
    Fx {
        name,
        rate,
        channels,
        version,
    }
}

const V1: MpegVersion = MpegVersion::V1;
const V2: MpegVersion = MpegVersion::V2;
const V25: MpegVersion = MpegVersion::V25;

const FIXTURES: [Fx; 15] = [
    fx("l3-cbr128-44k-joint", 44100, 2, V1),
    fx("l3-cbr320-44k-lr", 44100, 2, V1),
    fx("l3-vbr-v0-44k", 44100, 2, V1),
    fx("l3-vbr-v2-44k", 44100, 2, V1),
    fx("l3-vbr-v5-44k", 44100, 2, V1),
    fx("l3-lr-48k-128k", 48000, 2, V1),
    fx("l3-ms-48k-192k", 48000, 2, V1),
    fx("l3-cbr96-32k", 32000, 2, V1),
    fx("l3-mono-44k-64k", 44100, 1, V1),
    fx("l3-mpeg2-22k-64k", 22050, 2, V2),
    fx("l3-mpeg2-24k-48k", 24000, 2, V2),
    fx("l3-mpeg2-16k-32k", 16000, 2, V2),
    fx("l3-mpeg25-12k-mono", 12000, 1, V25),
    fx("l3-mpeg25-11k-stereo", 11025, 2, V25),
    fx("l3-mpeg25-8k-mono", 8000, 1, V25),
];

fn opts(mode: Mode) -> Options {
    Options {
        mode,
        ..Options::default()
    }
}

fn dec(rate: u32, channels: u16, mode: Mode) -> MpegDecoder {
    MpegDecoder::new(&track(CodecId::Mp3, rate, channels), &opts(mode)).unwrap()
}

/// Decodes a whole fixture into per-channel vectors.
fn decode_fixture(fx: &Fx) -> Vec<Vec<f32>> {
    let data = fixture(&format!("{}.mp3", fx.name));
    let mut d = dec(fx.rate, fx.channels, Mode::Strict);
    let mut planes = vec![Vec::new(); usize::from(fx.channels)];
    let mut out = AudioBuffer::new();
    let mut pts = 0u64;
    for (offset, frame) in split_frames(&data) {
        let h = parse_header(frame[..4].try_into().unwrap()).unwrap();
        let p = Packet {
            track: 0,
            pts,
            frames: h.samples,
            offset,
            data: frame.to_vec(),
        };
        d.decode(&p, &mut out).unwrap();
        assert_eq!(out.frames(), h.samples as usize, "{}", fx.name);
        for (c, plane) in planes.iter_mut().enumerate() {
            plane.extend_from_slice(out.plane_f32(c).unwrap());
        }
        pts += u64::from(h.samples);
    }
    planes
}

/// RMS and maximum absolute error of `got` against `want` shifted by `lag` samples
/// (`got[n]` against `want[n + lag]`).
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

#[test]
fn fixtures_have_the_expected_shape() {
    for fx in &FIXTURES {
        let data = fixture(&format!("{}.mp3", fx.name));
        let frames = split_frames(&data);
        assert!(frames.len() >= 5, "{}", fx.name);
        let mut total = 0usize;
        for (_, f) in &frames {
            let h = parse_header(f[..4].try_into().unwrap()).unwrap();
            assert_eq!(h.layer, 3);
            assert_eq!(
                (h.sample_rate, h.channels(), h.version),
                (fx.rate, fx.channels, fx.version),
                "{}",
                fx.name
            );
            assert_eq!(f.len(), h.frame_len, "{}: truncated frame", fx.name);
            total += h.samples as usize;
        }
        // The untagged reference holds every sample of every frame.
        let want = read_f32(&format!("{}.f32", fx.name));
        assert_eq!(want.len(), total * usize::from(fx.channels), "{}", fx.name);
    }
}

/// ISO/IEC 11172-4 full accuracy: RMS error below 2^-15 / sqrt(12) and maximum error at most
/// 2^-14 (full scale being 1.0), per channel, against the reference decoder.
#[test]
fn matches_ffmpeg_under_the_iso_full_accuracy_criterion() {
    let rms_limit = 2f64.powi(-15) / 12f64.sqrt();
    let max_limit = 2f64.powi(-14);
    for fx in &FIXTURES {
        let planes = decode_fixture(fx);
        let want = read_f32(&format!("{}.f32", fx.name));
        let nch = usize::from(fx.channels);
        assert_eq!(want.len(), planes[0].len() * nch, "{}", fx.name);
        for (c, got) in planes.iter().enumerate() {
            let reference: Vec<f32> = want.iter().skip(c).step_by(nch).copied().collect();
            let peak = reference.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            assert!(peak > 0.1, "{}: reference too quiet ({peak})", fx.name);
            let (rms, max) = errors(got, &reference, 0);
            println!(
                "{} ch{c}: rms {rms:.3e} (limit {rms_limit:.3e}), max {max:.3e} (limit {max_limit:.3e}), peak {peak:.3}",
                fx.name
            );
            assert!(rms < rms_limit, "{} ch{c}: rms {rms:e}", fx.name);
            assert!(max <= max_limit, "{} ch{c}: max {max:e}", fx.name);
            // The alignment is exact, not merely close: one sample either way is far off.
            for lag in [-1isize, 1] {
                let (r, _) = errors(got, &reference, lag);
                assert!(
                    r > 100.0 * rms_limit,
                    "{} ch{c}: lag {lag} rms {r:e}",
                    fx.name
                );
            }
        }
    }
}

#[test]
fn garbage_frames_never_panic() {
    // Valid headers over random bytes: the side information and the Huffman data are noise.
    let mut rng = Rng(77);
    for fx in [&FIXTURES[0], &FIXTURES[9], &FIXTURES[14]] {
        let data = fixture(&format!("{}.mp3", fx.name));
        let frames = split_frames(&data);
        for mode in [Mode::Strict, Mode::Resilient] {
            let mut d = dec(fx.rate, fx.channels, mode);
            let mut out = AudioBuffer::new();
            for round in 0..60 {
                let (offset, f) = frames[round % frames.len()];
                let mut bytes = f.to_vec();
                for b in bytes.iter_mut().skip(4) {
                    if rng.below(3) == 0 {
                        *b = rng.next() as u8;
                    }
                }
                if round % 7 == 0 {
                    bytes.truncate(4 + rng.below(40) as usize);
                }
                let p = Packet {
                    track: 0,
                    pts: 0,
                    frames: 1152,
                    offset,
                    data: bytes,
                };
                match d.decode(&p, &mut out) {
                    Ok(()) => assert!(
                        (0..usize::from(fx.channels)).all(|c| out
                            .plane_f32(c)
                            .unwrap()
                            .iter()
                            .all(|v| v.is_finite())),
                        "non-finite output"
                    ),
                    Err(e) => assert!(
                        matches!(e, Error::Invalid { .. }),
                        "{}: unexpected error {e:?}",
                        fx.name
                    ),
                }
            }
        }
    }
}

fn packet_of(data: &[u8], offset: u64, pts: u64) -> Packet {
    Packet {
        track: 0,
        pts,
        frames: 1152,
        offset,
        data: data.to_vec(),
    }
}

#[test]
fn a_damaged_frame_is_an_error_in_strict_mode_and_silence_in_resilient_mode() {
    let fx = &FIXTURES[0];
    let data = fixture(&format!("{}.mp3", fx.name));
    let frames = split_frames(&data);
    let (offset, frame) = frames[5];
    // Cut in the middle of the main data: the side information promises more bits than the
    // frame holds.
    let cut = &frame[..60];

    let mut out = AudioBuffer::new();
    let err = dec(fx.rate, 2, Mode::Strict)
        .decode(&packet_of(cut, offset, 5 * 1152), &mut out)
        .unwrap_err();
    assert!(
        matches!(err, Error::Invalid { what: "mp3 huffman", offset: o } if o >= offset),
        "{err:?}"
    );

    // Resilient: the frame's worth of silence, an event, and the stream goes on.
    let mut d = dec(fx.rate, 2, Mode::Resilient);
    for (n, (off, f)) in frames.iter().enumerate().take(5) {
        d.decode(&packet_of(f, *off, n as u64 * 1152), &mut out)
            .unwrap();
    }
    d.decode(&packet_of(cut, offset, 5 * 1152), &mut out)
        .unwrap();
    assert_eq!(out.frames(), 1152);
    for c in 0..2 {
        assert!(out.plane_f32(c).unwrap().iter().all(|&v| v == 0.0));
    }
    let mut events = Vec::new();
    d.drain_events(&mut events);
    assert_eq!(
        events,
        vec![Event::CorruptFrame {
            pts: 5 * 1152,
            offset,
            frames: 1152
        }]
    );
    let (off, f) = frames[6];
    d.decode(&packet_of(f, off, 6 * 1152), &mut out).unwrap();
    assert_eq!(out.frames(), 1152);
}

#[test]
fn a_gap_marker_and_a_reset_decode_to_silence_then_resume() {
    let fx = &FIXTURES[1];
    let data = fixture(&format!("{}.mp3", fx.name));
    let frames = split_frames(&data);
    let mut d = dec(fx.rate, 2, Mode::Strict);
    let mut out = AudioBuffer::new();
    for (n, (off, f)) in frames.iter().enumerate().take(4) {
        d.decode(&packet_of(f, *off, n as u64 * 1152), &mut out)
            .unwrap();
    }
    // An empty packet is the demuxer's gap marker.
    d.decode(&packet_of(&[], 0, 4 * 1152), &mut out).unwrap();
    assert_eq!(out.frames(), 1152);
    assert!(out.plane_f32(0).unwrap().iter().all(|&v| v == 0.0));
    // After a reset the reservoir is gone: a frame that points back into it decodes its
    // unreachable granules to silence (no error) inside the preroll the caller announced.
    d.reset();
    d.set_preroll(9 * 1152);
    let (off, f) = frames[8];
    d.decode(&packet_of(f, off, 8 * 1152), &mut out).unwrap();
    assert_eq!(out.frames(), 1152);
}

// ---------------------------------------------------------------- missing reservoir data

/// `main_data_begin` of a Layer III frame.
fn main_data_begin(frame: &[u8]) -> usize {
    let h = parse_header(frame[..4].try_into().unwrap()).unwrap();
    let o = 4 + if h.crc { 2 } else { 0 };
    match h.version {
        MpegVersion::V1 => usize::from(frame[o]) << 1 | usize::from(frame[o + 1] >> 7),
        _ => usize::from(frame[o]),
    }
}

/// The first frame at or after `from` whose main data starts in earlier frames.
fn first_with_reservoir(frames: &[(u64, &[u8])], from: usize) -> usize {
    (from..frames.len())
        .find(|&i| main_data_begin(frames[i].1) > 0)
        .expect("a frame with main_data_begin > 0")
}

fn first_granule_is_silent(out: &AudioBuffer, channels: u16) -> bool {
    (0..usize::from(channels)).all(|c| {
        out.plane_f32(c).unwrap()[..out.frames().min(576)]
            .iter()
            .all(|&v| v == 0.0)
    })
}

#[test]
fn missing_reservoir_data_outside_the_preroll_is_damage() {
    for fx in [&FIXTURES[0], &FIXTURES[9], &FIXTURES[14]] {
        let data = fixture(&format!("{}.mp3", fx.name));
        let frames = split_frames(&data);
        let spf = u64::from(
            parse_header(frames[0].1[..4].try_into().unwrap())
                .unwrap()
                .samples,
        );
        let k = first_with_reservoir(&frames, 3);
        let (offset, f) = frames[k];
        let pts = k as u64 * spf;
        let mut out = AudioBuffer::new();

        // Strict: an error at the frame.
        let err = dec(fx.rate, fx.channels, Mode::Strict)
            .decode(&packet_of(f, offset, pts), &mut out)
            .unwrap_err();
        assert_eq!(
            err,
            Error::Invalid {
                what: "mp3 reservoir",
                offset
            },
            "{}",
            fx.name
        );

        // Resilient: the frame's worth of silence and one event.
        let mut d = dec(fx.rate, fx.channels, Mode::Resilient);
        d.decode(&packet_of(f, offset, pts), &mut out).unwrap();
        assert_eq!(out.frames() as u64, spf);
        for c in 0..usize::from(fx.channels) {
            assert!(out.plane_f32(c).unwrap().iter().all(|&v| v == 0.0));
        }
        let mut ev = Vec::new();
        d.drain_events(&mut ev);
        assert_eq!(
            ev,
            vec![Event::CorruptFrame {
                pts,
                offset,
                frames: spf as u32
            }],
            "{}",
            fx.name
        );
    }
}

#[test]
fn the_announced_preroll_makes_missing_reservoir_data_silence() {
    let fx = &FIXTURES[0];
    let data = fixture(&format!("{}.mp3", fx.name));
    let frames = split_frames(&data);
    let k = first_with_reservoir(&frames, 3);
    let (offset, f) = frames[k];
    let pts = k as u64 * 1152;
    let mut out = AudioBuffer::new();
    let mut d = dec(fx.rate, 2, Mode::Strict);
    // The frame ends at (k + 1) * 1152: it lies wholly inside the preroll.
    d.set_preroll(pts + 1152);
    d.decode(&packet_of(f, offset, pts), &mut out).unwrap();
    assert_eq!(out.frames(), 1152);
    assert!(first_granule_is_silent(&out, 2));
    let mut ev = Vec::new();
    d.drain_events(&mut ev);
    assert!(ev.is_empty());

    // A frame that reaches past the announced preroll is not covered by it (the seek lands
    // so that the goal frame has all its data) ...
    let mut d = dec(fx.rate, 2, Mode::Strict);
    d.set_preroll(pts + 1151);
    let err = d.decode(&packet_of(f, offset, pts), &mut out).unwrap_err();
    assert!(
        matches!(
            err,
            Error::Invalid {
                what: "mp3 reservoir",
                ..
            }
        ),
        "{err:?}"
    );

    // ... and a reset withdraws the announcement.
    let mut d = dec(fx.rate, 2, Mode::Strict);
    d.set_preroll(pts + 1152);
    d.reset();
    let err = d.decode(&packet_of(f, offset, pts), &mut out).unwrap_err();
    assert!(
        matches!(
            err,
            Error::Invalid {
                what: "mp3 reservoir",
                ..
            }
        ),
        "{err:?}"
    );
}

#[test]
fn a_stream_that_starts_inside_the_reservoir_decodes_silently_until_it_is_filled() {
    // A stream cut out of a longer one: its first frame has main_data_begin > 0, and the
    // timeline starts at 0 for it. Frames starved of data before the reservoir holds the 511
    // bytes that main_data_begin can reach back are silence, with no error and no event.
    let fx = &FIXTURES[0];
    let data = fixture(&format!("{}.mp3", fx.name));
    let frames = split_frames(&data);
    let k = first_with_reservoir(&frames, 3);
    for mode in [Mode::Strict, Mode::Resilient] {
        let mut d = dec(fx.rate, 2, mode);
        let mut out = AudioBuffer::new();
        for (n, (off, f)) in frames[k..].iter().enumerate() {
            d.decode(&packet_of(f, *off, n as u64 * 1152), &mut out)
                .unwrap_or_else(|e| panic!("{mode:?} frame {n}: {e:?}"));
            assert_eq!(out.frames(), 1152);
            if n == 0 {
                assert!(first_granule_is_silent(&out, 2));
            }
        }
        let mut ev = Vec::new();
        d.drain_events(&mut ev);
        assert!(ev.is_empty(), "{ev:?}");
    }
}

#[test]
fn a_gap_empties_the_reservoir_and_the_frames_after_it_are_silent_not_garbage() {
    let fx = &FIXTURES[0];
    let data = fixture(&format!("{}.mp3", fx.name));
    let frames = split_frames(&data);
    let j = first_with_reservoir(&frames, 6);
    let mut d = dec(fx.rate, 2, Mode::Strict);
    let mut out = AudioBuffer::new();
    for (n, (off, f)) in frames.iter().enumerate().take(5) {
        d.decode(&packet_of(f, *off, n as u64 * 1152), &mut out)
            .unwrap();
    }
    // The frames from 5 to j are lost: the demuxer delivers empty packets for them.
    for n in 5..j {
        d.decode(&packet_of(&[], 0, n as u64 * 1152), &mut out)
            .unwrap();
    }
    // What the next frame needs from the lost ones is not there: its first granule is silent,
    // and this is part of the gap that was already reported.
    let (off, f) = frames[j];
    d.decode(&packet_of(f, off, j as u64 * 1152), &mut out)
        .unwrap();
    assert!(first_granule_is_silent(&out, 2));
    let mut ev = Vec::new();
    d.drain_events(&mut ev);
    assert!(ev.is_empty());
}
