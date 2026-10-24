//! Layer II decoding through the public `MpegDecoder` API: FFmpeg reference comparison under
//! the ISO/IEC 11172-4 full-accuracy criterion, errors, CRC, Resilient mode.
//!
//! The fixtures (`tests/fixtures/l2-*.mp2`) were encoded by FFmpeg (`mp2`, and libtwolame for
//! joint stereo with CRC); each `.f32` file is FFmpeg's `mp2float` decode of the same file as
//! raw interleaved little-endian f32. FFmpeg drops no samples: its output starts with the
//! first sample of the first frame, so the comparison is at exact alignment (offset 0).

mod common;

use common::{Rng, fixture, read_f32, split_frames, track};
use vynamp_decode_core::{
    AudioBuffer, CodecId, Decoder, Error, Event, LimitKind, Limits, Mode, Options, Packet,
};
use vynamp_decode_mp3::MpegDecoder;
use vynamp_decode_mpa::{ChannelMode, MpegVersion, parse_header};

struct Fx {
    name: &'static str,
    rate: u32,
    channels: u16,
    mode: ChannelMode,
    version: MpegVersion,
    crc: bool,
    frames: usize,
}

const FIXTURES: [Fx; 4] = [
    Fx {
        name: "l2-stereo-48k-192k",
        rate: 48000,
        channels: 2,
        mode: ChannelMode::Stereo,
        version: MpegVersion::V1,
        crc: false,
        frames: 9,
    },
    Fx {
        name: "l2-joint-44k-128k",
        rate: 44100,
        channels: 2,
        mode: ChannelMode::JointStereo,
        version: MpegVersion::V1,
        crc: true,
        frames: 8,
    },
    Fx {
        name: "l2-mono-48k-64k",
        rate: 48000,
        channels: 1,
        mode: ChannelMode::Mono,
        version: MpegVersion::V1,
        crc: false,
        frames: 9,
    },
    Fx {
        name: "l2-lsf-24k-64k",
        rate: 24000,
        channels: 2,
        mode: ChannelMode::Stereo,
        version: MpegVersion::V2,
        crc: false,
        frames: 5,
    },
];

fn opts(mode: Mode) -> Options {
    Options {
        mode,
        ..Options::default()
    }
}

fn dec(rate: u32, channels: u16, mode: Mode) -> MpegDecoder {
    MpegDecoder::new(&track(CodecId::Mp2, rate, channels), &opts(mode)).unwrap()
}

fn packet(data: &[u8], offset: u64) -> Packet {
    Packet {
        track: 0,
        pts: 2304,
        frames: 1152,
        offset,
        data: data.to_vec(),
    }
}

/// Decodes a whole fixture into per-channel vectors.
fn decode_fixture(fx: &Fx) -> Vec<Vec<f32>> {
    let data = fixture(&format!("{}.mp2", fx.name));
    let mut d = dec(fx.rate, fx.channels, Mode::Strict);
    let mut planes = vec![Vec::new(); usize::from(fx.channels)];
    let mut out = AudioBuffer::new();
    for (n, (offset, frame)) in split_frames(&data).into_iter().enumerate() {
        let mut p = packet(frame, offset);
        p.pts = (n * 1152) as u64;
        d.decode(&p, &mut out).unwrap();
        assert_eq!(out.frames(), 1152);
        for (c, plane) in planes.iter_mut().enumerate() {
            plane.extend_from_slice(out.plane_f32(c).unwrap());
        }
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
        let data = fixture(&format!("{}.mp2", fx.name));
        let frames = split_frames(&data);
        assert_eq!(frames.len(), fx.frames, "{}", fx.name);
        for (_, f) in frames {
            let h = parse_header(f[..4].try_into().unwrap()).unwrap();
            assert_eq!(h.layer, 2);
            assert_eq!(
                (
                    h.sample_rate,
                    h.channels(),
                    h.channel_mode,
                    h.version,
                    h.crc
                ),
                (fx.rate, fx.channels, fx.mode, fx.version, fx.crc),
                "{}",
                fx.name
            );
        }
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

fn joint() -> (Vec<u8>, Fx) {
    let fx = FIXTURES.into_iter().nth(1).unwrap();
    let data = fixture("l2-joint-44k-128k.mp2");
    (split_frames(&data)[3].1.to_vec(), fx)
}

#[test]
fn crc_covers_header_allocation_and_scfsi_only() {
    let (good, fx) = joint();
    let mut out = AudioBuffer::new();
    dec(fx.rate, 2, Mode::Strict)
        .decode(&packet(&good, 100), &mut out)
        .unwrap();
    // A flipped bit in the last byte is sample data (or padding), outside the CRC span.
    let mut tail = good.clone();
    *tail.last_mut().unwrap() ^= 1;
    dec(fx.rate, 2, Mode::Strict)
        .decode(&packet(&tail, 100), &mut out)
        .unwrap();
    // Stored CRC, allocation bytes, and header bytes 2 and 3 (the private bit and the
    // emphasis bits stay valid values).
    for (idx, bit) in [
        (4usize, 0x01u8),
        (5, 0x80),
        (6, 0x10),
        (14, 0x01),
        (2, 0x01),
        (3, 0x01),
    ] {
        let mut bad = good.clone();
        bad[idx] ^= bit;
        let err = dec(fx.rate, 2, Mode::Strict)
            .decode(&packet(&bad, 100), &mut out)
            .unwrap_err();
        assert_eq!(
            err,
            Error::Invalid {
                what: "mpeg crc",
                offset: 104
            },
            "byte {idx} bit {bit:#x}"
        );
    }
}

#[test]
fn truncated_and_foreign_packets_are_invalid() {
    let (good, fx) = joint();
    let mut out = AudioBuffer::new();
    for cut in [3usize, 4, 5, 20, 100, 300, good.len() - 1] {
        let err = dec(fx.rate, 2, Mode::Strict)
            .decode(&packet(&good[..cut], 5000), &mut out)
            .unwrap_err();
        assert!(
            matches!(err, Error::Invalid { offset, .. } if offset >= 5000),
            "cut {cut}: {err:?}"
        );
    }
    let mut junk = good.clone();
    junk[0] = 0;
    assert_eq!(
        dec(fx.rate, 2, Mode::Strict)
            .decode(&packet(&junk, 5000), &mut out)
            .unwrap_err(),
        Error::Invalid {
            what: "mpeg header",
            offset: 5000
        }
    );
    // Layer I header, a stereo frame for a mono track, another sample rate.
    let mut layer1 = good.clone();
    layer1[1] |= 0x06;
    let mut rate = good.clone();
    rate[2] = (rate[2] & !0x0C) | 0x04;
    for bad in [layer1, rate] {
        assert!(matches!(
            dec(fx.rate, 2, Mode::Strict).decode(&packet(&bad, 5000), &mut out),
            Err(Error::Invalid {
                what: "mpeg header",
                offset: 5000
            })
        ));
    }
    assert!(matches!(
        dec(fx.rate, 1, Mode::Strict).decode(&packet(&good, 5000), &mut out),
        Err(Error::Invalid {
            what: "mpeg header",
            ..
        })
    ));
}

#[test]
fn scalefactor_index_63_is_invalid() {
    // Mono 48 kHz 64 kbit/s: every nonzero allocation is followed by scfsi and scalefactors;
    // set all bits after the CRC-free header + allocation + scfsi region to ones so the first
    // scalefactor read is 63.
    let data = fixture("l2-mono-48k-64k.mp2");
    let mut frame = split_frames(&data)[2].1.to_vec();
    for b in &mut frame[4..] {
        *b = 0xFF;
    }
    let mut out = AudioBuffer::new();
    let err = dec(48000, 1, Mode::Strict)
        .decode(&packet(&frame, 800), &mut out)
        .unwrap_err();
    assert!(
        matches!(err, Error::Invalid { what: "mpeg scalefactor", offset } if offset > 804),
        "{err:?}"
    );
}

#[test]
fn resilient_replaces_a_bad_frame_with_silence_and_resets() {
    let (good, fx) = joint();
    let mut bad = good.clone();
    bad[5] ^= 1;
    let mut d = dec(fx.rate, 2, Mode::Resilient);
    let mut out = AudioBuffer::new();
    d.decode(&packet(&good, 0), &mut out).unwrap();
    d.decode(&packet(&bad, 9000), &mut out).unwrap();
    assert_eq!((out.channels(), out.frames()), (2, 1152));
    for c in 0..2 {
        assert!(out.plane_f32(c).unwrap().iter().all(|&v| v == 0.0));
    }
    let mut ev = Vec::new();
    d.drain_events(&mut ev);
    assert_eq!(
        ev,
        [Event::CorruptFrame {
            pts: 2304,
            offset: 9000,
            frames: 1152
        }]
    );
    // The next frame decodes as it would from a fresh decoder.
    d.decode(&packet(&good, 9100), &mut out).unwrap();
    let mut fresh = AudioBuffer::new();
    dec(fx.rate, 2, Mode::Strict)
        .decode(&packet(&good, 0), &mut fresh)
        .unwrap();
    for c in 0..2 {
        assert_eq!(out.plane_f32(c), fresh.plane_f32(c));
    }
    ev.clear();
    d.drain_events(&mut ev);
    assert!(ev.is_empty(), "no further events");
    // Truncation is silence of the frame length too.
    d.decode(&packet(&good[..60], 70), &mut out).unwrap();
    assert_eq!(out.frames(), 1152);
    d.drain_events(&mut ev);
    assert_eq!(ev.len(), 1);
}

#[test]
fn empty_packet_is_silence_without_event() {
    for mode in [Mode::Strict, Mode::Resilient] {
        let mut d = dec(48000, 2, mode);
        let mut out = AudioBuffer::new();
        let mut p = packet(&[], 0);
        d.decode(&p, &mut out).unwrap();
        assert_eq!((out.channels(), out.frames()), (2, 1152));
        assert!(out.plane_f32(1).unwrap().iter().all(|&v| v == 0.0));
        p.frames = 50_000;
        d.decode(&p, &mut out).unwrap();
        assert_eq!(out.frames(), 1152, "clamped");
        let mut ev = Vec::new();
        d.drain_events(&mut ev);
        assert!(ev.is_empty());
    }
}

#[test]
fn frame_limit_is_enforced() {
    let o = Options {
        limits: Limits {
            max_frames: 2500,
            ..Limits::default()
        },
        ..Options::default()
    };
    let data = fixture("l2-mono-48k-64k.mp2");
    let frame = split_frames(&data)[0].1.to_vec();
    let mut d = MpegDecoder::new(&track(CodecId::Mp2, 48000, 1), &o).unwrap();
    let mut out = AudioBuffer::new();
    assert_eq!(
        d.decode(&packet(&frame, 0), &mut out),
        Err(Error::Limit(LimitKind::Frames))
    );
}

#[test]
fn garbage_never_panics() {
    let mut rng = Rng(4242);
    let mut out = AudioBuffer::new();
    for fx in &FIXTURES {
        let data = fixture(&format!("{}.mp2", fx.name));
        let frames = split_frames(&data);
        for mode in [Mode::Strict, Mode::Resilient] {
            let mut d = dec(fx.rate, fx.channels, mode);
            for _ in 0..300 {
                let mut f = frames[rng.below(frames.len() as u32) as usize].1.to_vec();
                for _ in 0..rng.below(8) {
                    // Leave the header intact most of the time so the payload is parsed.
                    let i = 4 + rng.below(f.len() as u32 - 4) as usize;
                    f[i] = rng.next() as u8;
                }
                if rng.below(4) == 0 {
                    let cut = rng.below(f.len() as u32 + 1) as usize;
                    f.truncate(cut);
                }
                let r = d.decode(&packet(&f, 0), &mut out);
                if mode == Mode::Resilient {
                    // Only a limit error can escape; none is configured.
                    r.unwrap();
                }
            }
        }
    }
}
