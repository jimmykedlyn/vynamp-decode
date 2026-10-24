//! Layer I decoding through the public `MpegDecoder` API: errors, CRC, Resilient mode.

mod common;

use common::{L1Spec, Rng, random_l1, track};
use vynamp_decode_core::{
    AudioBuffer, CodecId, Decoder, Error, Event, LimitKind, Limits, Mode, Options, Packet,
    SampleFormat,
};
use vynamp_decode_mp3::MpegDecoder;

fn opts(mode: Mode) -> Options {
    Options {
        mode,
        ..Options::default()
    }
}

fn dec(channels: u16, mode: Mode) -> MpegDecoder {
    MpegDecoder::new(&track(CodecId::Mp1, 48000, channels), &opts(mode)).unwrap()
}

fn packet(data: Vec<u8>, offset: u64) -> Packet {
    Packet {
        track: 0,
        pts: 768,
        frames: 384,
        offset,
        data,
    }
}

fn busy(crc: bool) -> L1Spec {
    let mut s = L1Spec::silent(3);
    s.crc = crc;
    for sb in 0..8 {
        s.alloc[0][sb] = 4;
        s.scf[0][sb] = 10;
        for slot in 0..12 {
            s.samples[slot][sb][0] = ((slot * 7 + sb * 3) % 30) as u32;
        }
    }
    s
}

#[test]
fn layer_three_and_foreign_codecs_are_unsupported() {
    let o = Options::default();
    for (codec, what) in [
        (CodecId::Mp3, "mpeg layer III decoding"),
        (CodecId::Flac, "not an mpeg audio track"),
    ] {
        match MpegDecoder::new(&track(codec, 48000, 2), &o) {
            Err(Error::Unsupported(w)) => assert_eq!(w, what),
            other => panic!("{:?}", other.err()),
        }
    }
    for codec in [CodecId::Mp1, CodecId::Mp2] {
        assert!(MpegDecoder::new(&track(codec.clone(), 48000, 2), &o).is_ok());
        assert!(MpegDecoder::new(&track(codec.clone(), 48000, 3), &o).is_err());
        assert!(MpegDecoder::new(&track(codec.clone(), 48000, 0), &o).is_err());
        assert!(MpegDecoder::new(&track(codec, 0, 2), &o).is_err());
    }
}

#[test]
fn silent_frame_is_f32_planar_silence() {
    let mut d = dec(2, Mode::Strict);
    let mut out = AudioBuffer::new();
    d.decode(&packet(L1Spec::silent(0).build(), 0), &mut out)
        .unwrap();
    assert_eq!(
        (out.format(), out.channels(), out.frames()),
        (SampleFormat::F32, 2, 384)
    );
    for c in 0..2 {
        assert!(out.plane_f32(c).unwrap().iter().all(|&v| v == 0.0));
    }
}

#[test]
fn busy_frame_decodes_and_stays_in_range() {
    let mut d = dec(1, Mode::Strict);
    let mut out = AudioBuffer::new();
    d.decode(&packet(busy(false).build(), 0), &mut out).unwrap();
    let p = out.plane_f32(0).unwrap();
    assert!(p.iter().any(|&v| v.abs() > 0.01));
    assert!(p.iter().all(|&v| v.abs() <= 1.0));
}

#[test]
fn crc_is_checked_over_header_and_allocation_only() {
    let good = busy(true).build();
    let mut out = AudioBuffer::new();
    dec(1, Mode::Strict)
        .decode(&packet(good.clone(), 100), &mut out)
        .unwrap();
    // A flipped sample bit is outside the CRC span; the frame still decodes.
    let mut sample_flip = good.clone();
    sample_flip[6 + 16 + 8 + 20] ^= 1;
    dec(1, Mode::Strict)
        .decode(&packet(sample_flip, 100), &mut out)
        .unwrap();
    // Bits covered by the CRC: stored CRC, allocation, and header bytes 2 and 3.
    for (idx, bit) in [
        (4usize, 0x01u8),
        (5, 0x80),
        (6, 0x10),
        (21, 0x01),
        (2, 0x01),
        (3, 0x08),
    ] {
        let mut bad = good.clone();
        bad[idx] ^= bit;
        let err = dec(1, Mode::Strict)
            .decode(&packet(bad, 100), &mut out)
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
fn forbidden_allocation_is_invalid_at_its_byte() {
    let mut s = busy(false);
    s.alloc[0][2] = 15; // high nibble of allocation byte 1
    let mut out = AudioBuffer::new();
    let err = dec(1, Mode::Strict)
        .decode(&packet(s.build(), 1000), &mut out)
        .unwrap_err();
    assert_eq!(
        err,
        Error::Invalid {
            what: "mpeg allocation",
            offset: 1005
        }
    );
}

#[test]
fn truncated_and_foreign_packets_are_invalid() {
    let mut out = AudioBuffer::new();
    let frame = busy(false).build();
    for cut in [3usize, 4, 10, 40, 60] {
        let err = dec(1, Mode::Strict)
            .decode(&packet(frame[..cut].to_vec(), 5000), &mut out)
            .unwrap_err();
        assert!(
            matches!(err, Error::Invalid { offset, .. } if offset >= 5000),
            "cut {cut}: {err:?}"
        );
    }
    // Not a header.
    let mut junk = frame.clone();
    junk[0] = 0;
    assert_eq!(
        dec(1, Mode::Strict)
            .decode(&packet(junk, 5000), &mut out)
            .unwrap_err(),
        Error::Invalid {
            what: "mpeg header",
            offset: 5000
        }
    );
    // A Layer II header, a stereo frame for a mono track, another sample rate.
    let mut layer2 = frame.clone();
    layer2[1] = 0xFD;
    let stereo = L1Spec::silent(0).build();
    let mut rate = frame.clone();
    rate[2] &= !0x0C;
    for bad in [layer2, stereo, rate] {
        assert!(matches!(
            dec(1, Mode::Strict).decode(&packet(bad, 5000), &mut out),
            Err(Error::Invalid {
                what: "mpeg header",
                offset: 5000
            })
        ));
    }
}

#[test]
fn resilient_replaces_a_bad_frame_with_silence_and_resets() {
    let mut bad = busy(true).build();
    bad[5] ^= 1;
    let good = busy(true).build();
    let mut d = dec(1, Mode::Resilient);
    let mut out = AudioBuffer::new();
    d.decode(&packet(good.clone(), 0), &mut out).unwrap();
    d.decode(&packet(bad, 9000), &mut out).unwrap();
    assert_eq!((out.channels(), out.frames()), (1, 384));
    assert!(out.plane_f32(0).unwrap().iter().all(|&v| v == 0.0));
    let mut ev = Vec::new();
    d.drain_events(&mut ev);
    assert_eq!(
        ev,
        [Event::CorruptFrame {
            pts: 768,
            offset: 9000,
            frames: 384
        }]
    );
    // The next frame decodes as it would from a fresh decoder.
    d.decode(&packet(good.clone(), 9100), &mut out).unwrap();
    let mut fresh = AudioBuffer::new();
    dec(1, Mode::Strict)
        .decode(&packet(good, 0), &mut fresh)
        .unwrap();
    assert_eq!(out.plane_f32(0), fresh.plane_f32(0));
    ev.clear();
    d.drain_events(&mut ev);
    assert!(ev.is_empty(), "no further events");
}

#[test]
fn resilient_truncation_is_silence_of_the_frame_length() {
    let frame = busy(false).build();
    let mut d = dec(1, Mode::Resilient);
    let mut out = AudioBuffer::new();
    d.decode(&packet(frame[..30].to_vec(), 70), &mut out)
        .unwrap();
    assert_eq!(out.frames(), 384);
    let mut ev = Vec::new();
    d.drain_events(&mut ev);
    assert_eq!(ev.len(), 1);
}

#[test]
fn empty_packet_is_silence_without_event() {
    for mode in [Mode::Strict, Mode::Resilient] {
        let mut d = dec(2, mode);
        let mut out = AudioBuffer::new();
        let mut p = packet(Vec::new(), 0);
        p.frames = 1152;
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
            max_frames: 1000,
            ..Limits::default()
        },
        ..Options::default()
    };
    let mut d = MpegDecoder::new(&track(CodecId::Mp1, 48000, 1), &o).unwrap();
    let mut out = AudioBuffer::new();
    assert_eq!(
        d.decode(&packet(busy(false).build(), 0), &mut out),
        Err(Error::Limit(LimitKind::Frames))
    );
    let mut p = packet(Vec::new(), 0);
    p.pts = 900;
    assert_eq!(d.decode(&p, &mut out), Err(Error::Limit(LimitKind::Frames)));
}

#[test]
fn garbage_never_panics() {
    let mut rng = Rng(99);
    let mut out = AudioBuffer::new();
    for mode in [Mode::Strict, Mode::Resilient] {
        let mut d = MpegDecoder::new(&track(CodecId::Mp1, 32000, 2), &opts(mode)).unwrap();
        for _ in 0..300 {
            let crc = rng.below(2) == 0;
            let mut f = random_l1(&mut rng, 0, 0, crc).build();
            for _ in 0..rng.below(8) {
                let i = rng.below(f.len() as u32) as usize;
                f[i] = rng.next() as u8;
            }
            let cut = rng.below(f.len() as u32 + 1) as usize;
            f.truncate(cut);
            let _ = d.decode(&packet(f, 0), &mut out);
        }
    }
}
