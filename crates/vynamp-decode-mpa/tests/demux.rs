//! MPEG audio demuxer: open, VBR/LAME headers, packetization, tags, free format, exact seek
//! and Strict/Resilient behaviour.
//!
//! The FFmpeg fixtures in `tests/fixtures` come from `tools/fixtures/generate-mpeg-dev.ps1`
//! (3 s of the repository's 48 kHz sine, looped). Expected packet counts are
//! `ffprobe -count_packets` (nb_read_packets) on those files. Streams FFmpeg cannot produce (Layer I, free
//! format, damaged files) are built here from the fixtures or from synthetic frames.

use vynamp_decode_core::{
    CancelSource, CodecId, Demuxer, Error, Event, GaplessKind, LimitKind, Limits, Mode, Options,
    Packet, SeekTo, VecSource,
};
use vynamp_decode_mpa::{MpaDemuxer, MpegVersion, VbrKind, open_mpa, parse_header};

// ---------------------------------------------------------------- helpers

#[derive(Debug, Clone, PartialEq, Eq)]
struct Pkt {
    pts: u64,
    frames: u32,
    offset: u64,
    data: Vec<u8>,
}

fn fixture(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn strict() -> Options {
    Options::default()
}

fn resilient() -> Options {
    Options {
        mode: Mode::Resilient,
        ..Options::default()
    }
}

fn open(data: Vec<u8>, opts: &Options) -> MpaDemuxer {
    open_mpa(Box::new(VecSource::new(data)), opts).expect("open")
}

fn read_all(d: &mut MpaDemuxer) -> (Vec<Pkt>, Result<(), Error>, Vec<Event>) {
    let mut out = Vec::new();
    let mut p = Packet::default();
    let mut result = Ok(());
    for _ in 0..1_000_000 {
        match d.next_packet(&mut p) {
            Ok(true) => out.push(Pkt {
                pts: p.pts,
                frames: p.frames,
                offset: p.offset,
                data: p.data.clone(),
            }),
            Ok(false) => break,
            Err(e) => {
                result = Err(e);
                break;
            }
        }
    }
    let mut ev = Vec::new();
    d.drain_events(&mut ev);
    (out, result, ev)
}

fn clean(data: &[u8]) -> Vec<Pkt> {
    let mut d = open(data.to_vec(), &strict());
    let (p, r, ev) = read_all(&mut d);
    assert_eq!(r, Ok(()));
    assert!(ev.is_empty(), "{ev:?}");
    p
}

fn invalid(r: &Result<(), Error>) -> (&'static str, u64) {
    match r {
        Err(Error::Invalid { what, offset }) => (what, *offset),
        other => panic!("expected Invalid, got {other:?}"),
    }
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

struct Fx {
    name: &'static str,
    codec: CodecId,
    rate: u32,
    channels: u16,
    /// `ffprobe -count_packets` (nb_read_packets).
    packets: usize,
    /// Length of the PCM that was encoded, when the file carries a LAME tag.
    pcm: Option<u64>,
}

fn fixtures() -> Vec<Fx> {
    let f = |name, codec, rate, channels, packets, pcm| Fx {
        name,
        codec,
        rate,
        channels,
        packets,
        pcm,
    };
    vec![
        f(
            "cbr128-48k-stereo.mp3",
            CodecId::Mp3,
            48000,
            2,
            126,
            Some(144_000),
        ),
        f(
            "cbr128-44k-stereo.mp3",
            CodecId::Mp3,
            44100,
            2,
            116,
            Some(132_300),
        ),
        f(
            "vbr-q2-48k-stereo.mp3",
            CodecId::Mp3,
            48000,
            2,
            126,
            Some(144_000),
        ),
        f(
            "mpeg2-22k-stereo.mp3",
            CodecId::Mp3,
            22050,
            2,
            117,
            Some(66_150),
        ),
        f(
            "mpeg25-8k-mono.mp3",
            CodecId::Mp3,
            8000,
            1,
            44,
            Some(24_000),
        ),
        f(
            "cbr64-44k-mono.mp3",
            CodecId::Mp3,
            44100,
            1,
            116,
            Some(132_300),
        ),
        f("nolame-cbr128-48k.mp3", CodecId::Mp3, 48000, 2, 126, None),
        f(
            "cbr32-48k-stereo.mp3",
            CodecId::Mp3,
            48000,
            2,
            126,
            Some(144_000),
        ),
        f("cbr8-8k-mono.mp3", CodecId::Mp3, 8000, 1, 44, Some(24_000)),
        f("mp2-192k-48k-stereo.mp2", CodecId::Mp2, 48000, 2, 125, None),
        f("mp2-128k-44k-stereo.mp2", CodecId::Mp2, 44100, 2, 115, None),
    ]
}

fn spf_of(codec: &CodecId, rate: u32) -> u64 {
    match codec {
        CodecId::Mp1 => 384,
        CodecId::Mp3 if rate < 32000 => 576,
        _ => 1152,
    }
}

// ---------------------------------------------------------------- fixtures

#[test]
fn fixtures_have_ffprobes_packet_counts_and_a_contiguous_timeline() {
    for fx in fixtures() {
        let data = fixture(fx.name);
        let mut d = open(data.clone(), &strict());
        let t = d.tracks()[0].clone();
        assert_eq!(t.codec, fx.codec, "{}", fx.name);
        assert_eq!(t.sample_rate, fx.rate, "{}", fx.name);
        assert_eq!(t.channels, fx.channels, "{}", fx.name);
        let (pkts, r, ev) = read_all(&mut d);
        assert_eq!(r, Ok(()), "{}", fx.name);
        assert!(ev.is_empty(), "{}", fx.name);
        assert_eq!(pkts.len(), fx.packets, "{}", fx.name);
        let spf = spf_of(&fx.codec, fx.rate);
        let mut next_off = pkts[0].offset;
        for (i, p) in pkts.iter().enumerate() {
            assert_eq!(p.pts, i as u64 * spf, "{} pts {i}", fx.name);
            assert_eq!(u64::from(p.frames), spf);
            assert_eq!(p.offset, next_off, "{} offset {i}", fx.name);
            let h = parse_header(p.data[..4].try_into().unwrap()).unwrap();
            assert_eq!(h.frame_len, p.data.len(), "{} len {i}", fx.name);
            assert_eq!(h.sample_rate, fx.rate);
            next_off += p.data.len() as u64;
        }
        // Only an ID3v1/APEv2-free file ends exactly at the last frame.
        assert_eq!(next_off, data.len() as u64, "{}", fx.name);
        assert_eq!(d.first_frame_offset(), pkts[0].offset);
    }
}

#[test]
fn lame_tag_gives_exact_gapless_numbers() {
    for fx in fixtures() {
        let d = open(fixture(fx.name), &strict());
        let t = &d.tracks()[0];
        let n = fx.packets as u64;
        let spf = spf_of(&fx.codec, fx.rate);
        match fx.pcm {
            Some(pcm) => {
                let lame = d
                    .vbr_info()
                    .and_then(|v| v.lame.clone())
                    .unwrap_or_else(|| panic!("{} has no LAME tag", fx.name));
                assert!(lame.crc_ok, "{}", fx.name);
                assert_eq!(t.gapless, GaplessKind::FromMetadata, "{}", fx.name);
                assert_eq!(t.frames, Some(n * spf), "{}", fx.name);
                assert_eq!(t.delay, u32::from(lame.encoder_delay) + 529, "{}", fx.name);
                assert_eq!(
                    t.padding,
                    u32::from(lame.padding).saturating_sub(529),
                    "{}",
                    fx.name
                );
                // frames * samples - delay - padding is the length of the encoded PCM.
                assert_eq!(
                    n * spf - u64::from(t.delay) - u64::from(t.padding),
                    pcm,
                    "{}",
                    fx.name
                );
            }
            None => {
                assert_eq!(t.frames, None, "{}", fx.name);
                assert_eq!(t.gapless, GaplessKind::Unknown, "{}", fx.name);
                assert!(t.codec_private.is_empty());
                // Layer III without a LAME tag: the decoder delay only. Layer I/II: the
                // decoder task fixes the value.
                let delay = if fx.codec == CodecId::Mp3 { 529 } else { 0 };
                assert_eq!((t.delay, t.padding), (delay, 0), "{}", fx.name);
            }
        }
    }
}

#[test]
fn xing_and_info_headers_are_consumed_not_emitted() {
    // CBR files carry an Info frame, the VBR file a Xing frame.
    let d = open(fixture("cbr128-48k-stereo.mp3"), &strict());
    let v = d.vbr_info().unwrap();
    assert_eq!(v.kind, VbrKind::Info);
    assert_eq!(v.frames, Some(126));
    let d = open(fixture("vbr-q2-48k-stereo.mp3"), &strict());
    let v = d.vbr_info().unwrap();
    assert!(
        matches!(v.kind, VbrKind::Xing | VbrKind::Info),
        "{:?}",
        v.kind
    );
    assert_eq!(v.frames, Some(126));
    assert!(v.toc.is_some());
    // The Info frame is not audio: the first packet comes right after it.
    let first = open(fixture("cbr128-48k-stereo.mp3"), &strict()).first_frame_offset();
    let plain = fixture("cbr128-48k-stereo.mp3");
    let at = plain.windows(4).position(|w| w == b"Info").unwrap();
    assert_eq!(first, (at - 36 + 384) as u64);
}

#[test]
fn ffmpeg_lame_tag_crc_validates_as_crc16_arc() {
    let d = open(fixture("cbr128-48k-stereo.mp3"), &strict());
    let lame = d.vbr_info().unwrap().lame.clone().unwrap();
    assert!(lame.crc_ok);
    assert_eq!(lame.encoder_delay, 576);
}

// ---------------------------------------------------------------- tags

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
    // The body is full of things that look like frame headers.
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
fn id3v2_apev2_and_id3v1_yield_identical_packets() {
    for name in [
        "cbr128-44k-stereo.mp3",
        "vbr-q2-48k-stereo.mp3",
        "mp2-128k-44k-stereo.mp2",
    ] {
        let plain = fixture(name);
        let base = clean(&plain);
        let prefix = id3v2_with_fake_frames(1500);
        let mut wrapped = prefix.clone();
        wrapped.extend(&plain);
        wrapped.extend(apev2_with_fake_frames());
        wrapped.extend(id3v1());
        let mut d = open(wrapped.clone(), &strict());
        assert_eq!(d.tracks(), open(plain, &strict()).tracks(), "{name}");
        let (pkts, r, ev) = read_all(&mut d);
        assert_eq!(r, Ok(()), "{name}");
        assert!(ev.is_empty());
        assert_eq!(pkts.len(), base.len(), "{name}");
        for (a, b) in pkts.iter().zip(&base) {
            assert_eq!(a.pts, b.pts);
            assert_eq!(a.data, b.data);
            assert_eq!(a.offset, b.offset + prefix.len() as u64);
        }
        // Absolute error offsets: damage the third frame of the wrapped file.
        let mut damaged = wrapped;
        let at = pkts[2].offset as usize;
        damaged[at..at + 4].fill(0);
        let mut d = open(damaged, &strict());
        let (_, r, _) = read_all(&mut d);
        assert_eq!(invalid(&r), ("mpeg sync", pkts[2].offset), "{name}");
    }
}

// ---------------------------------------------------------------- sync

#[test]
fn false_sync_guard_skips_junk_that_looks_like_a_header() {
    let plain = fixture("cbr128-48k-stereo.mp3");
    let base = clean(&plain);
    // A valid-looking header with no consistent successor, then a free-format look-alike
    // pair 20 bytes apart (too short for Layer III side information).
    let mut junk = vec![0xFF, 0xFB, 0x90, 0x00];
    junk.extend([0u8; 100]);
    for _ in 0..2 {
        junk.extend([0xFF, 0xFB, 0x04, 0x00]);
        junk.extend([0u8; 16]);
    }
    junk.extend([0xFF, 0xFF, 0xFF]);
    let mut data = junk.clone();
    data.extend(&plain);
    for opts in [strict(), resilient()] {
        let mut d = open(data.clone(), &opts);
        assert_eq!(d.first_frame_offset(), base[0].offset + junk.len() as u64);
        let (pkts, r, ev) = read_all(&mut d);
        assert_eq!(r, Ok(()));
        assert!(ev.is_empty());
        assert_eq!(pkts.len(), base.len());
        assert_eq!(pkts[0].data, base[0].data);
    }
}

#[test]
fn garbage_does_not_open() {
    let mut rng = Rng(0x1234_5678_9ABC_DEF1);
    let data: Vec<u8> = (0..64 * 1024).map(|_| rng.next() as u8).collect();
    for opts in [strict(), resilient()] {
        let r = open_mpa(Box::new(VecSource::new(data.clone())), &opts);
        assert!(
            matches!(
                r,
                Err(Error::Invalid {
                    what: "mpeg sync",
                    ..
                })
            ),
            "{:?}",
            r.err()
        );
    }
    let r = open_mpa(Box::new(VecSource::new(Vec::new())), &strict());
    assert!(matches!(r, Err(Error::Invalid { .. })));
}

// ---------------------------------------------------------------- layer I, free format

/// `n` empty Layer I frames, MPEG-1, 44.1 kHz, 256 kbps, stereo; padding alternates.
fn layer1_stream(n: usize) -> Vec<u8> {
    let mut out = Vec::new();
    for i in 0..n {
        let pad = i % 2;
        let len = (12 * 256_000 / 44_100 + pad) * 4;
        let mut f = vec![0u8; len];
        f[..4].copy_from_slice(&[0xFF, 0xFF, 0x80 | (pad as u8) << 1, 0x00]);
        out.extend(f);
    }
    out
}

#[test]
fn layer_one_synthetic_stream() {
    let data = layer1_stream(10);
    let mut d = open(data.clone(), &strict());
    let t = d.tracks()[0].clone();
    assert_eq!(t.codec, CodecId::Mp1);
    assert_eq!((t.sample_rate, t.channels), (44100, 2));
    assert_eq!((t.frames, t.delay, t.padding), (None, 0, 0));
    assert_eq!(t.gapless, GaplessKind::Unknown);
    let (pkts, r, _) = read_all(&mut d);
    assert_eq!(r, Ok(()));
    assert_eq!(pkts.len(), 10);
    assert!(
        pkts.iter()
            .enumerate()
            .all(|(i, p)| p.pts == i as u64 * 384 && p.frames == 384)
    );
    // One frame of preroll for Layer I.
    assert_eq!(
        d.seek(0, 5 * 384 + 7).unwrap(),
        SeekTo {
            frame: 4 * 384,
            preroll: 384 + 7
        }
    );
    let mut p = Packet::default();
    assert!(d.next_packet(&mut p).unwrap());
    assert_eq!((p.pts, p.offset), (4 * 384, pkts[4].offset));
}

/// Rewrites the bitrate index of every frame (the Info frame included) to 0 = free format.
fn to_free_format(data: &[u8]) -> Vec<u8> {
    let pkts = clean(data);
    let mut out = data.to_vec();
    for p in &pkts {
        out[p.offset as usize + 2] &= 0x0F;
    }
    if let Some(i) = data.windows(4).position(|w| w == b"Info" || w == b"Xing") {
        out[i - 36 + 2] &= 0x0F;
    }
    out
}

#[test]
fn free_format_streams_are_measured() {
    for (name, base_len) in [
        ("cbr128-48k-stereo.mp3", 384),
        ("cbr128-44k-stereo.mp3", 417),
        ("mp2-192k-48k-stereo.mp2", 576),
    ] {
        let plain = fixture(name);
        let reference = clean(&plain);
        let free = to_free_format(&plain);
        assert_ne!(free, plain);
        for opts in [strict(), resilient()] {
            let mut d = open(free.clone(), &opts);
            assert_eq!(d.free_format_len(), Some(base_len), "{name}");
            let (pkts, r, ev) = read_all(&mut d);
            assert_eq!(r, Ok(()), "{name}");
            assert!(ev.is_empty());
            assert_eq!(pkts.len(), reference.len(), "{name}");
            for (a, b) in pkts.iter().zip(&reference) {
                assert_eq!(
                    (a.pts, a.offset, a.data.len()),
                    (b.pts, b.offset, b.data.len())
                );
                assert_eq!(a.data[3], b.data[3]);
                assert_eq!(a.data[2] & 0x0F, b.data[2] & 0x0F);
                assert_eq!(a.data[4..], b.data[4..]);
            }
        }
        // Normal streams report no free-format length.
        assert_eq!(open(plain, &strict()).free_format_len(), None);
    }
}

// ---------------------------------------------------------------- seek

fn check_seeks(name: &str, data: &[u8], opts: &Options) {
    let pkts = clean(data);
    let n = pkts.len() as u64;
    let spf = u64::from(pkts[0].frames);
    let layer3 = parse_header(pkts[0].data[..4].try_into().unwrap())
        .unwrap()
        .layer
        == 3;
    let mut targets: Vec<u64> = (0..n).step_by(10).map(|i| i * spf + i % 7).collect();
    targets.push((n - 1) * spf);
    targets.push(n * spf - 1);
    let mut d = open(data.to_vec(), opts);
    let mut order = targets.clone();
    order.extend(targets.iter().rev());
    let mut rng = Rng(77);
    for _ in 0..10 {
        order.push(targets[rng.below(targets.len())]);
    }
    for t in order {
        let idx = t / spf;
        let start = idx
            - if layer3 {
                reservoir_preroll(&pkts, idx)
            } else {
                1.min(idx)
            };
        let to = d.seek(0, t).unwrap();
        assert_eq!(
            to,
            SeekTo {
                frame: start * spf,
                preroll: t - start * spf
            },
            "{name} target {t}"
        );
        let mut p = Packet::default();
        for k in start..(start + 3).min(n) {
            assert!(d.next_packet(&mut p).unwrap(), "{name} {t}");
            let want = &pkts[k as usize];
            assert_eq!(
                (p.pts, p.offset, p.frames, &p.data),
                (want.pts, want.offset, want.frames, &want.data),
                "{name} target {t} packet {k}"
            );
        }
    }
    // At and past the end: the landing point is the end, nothing is left to read.
    for t in [n * spf, n * spf + 5000, u64::MAX] {
        let to = d.seek(0, t).unwrap();
        assert_eq!(
            to,
            SeekTo {
                frame: n * spf,
                preroll: 0
            },
            "{name} target {t}"
        );
        let mut p = Packet::default();
        assert!(!d.next_packet(&mut p).unwrap());
    }
    // And back to the start: everything is read again.
    assert_eq!(
        d.seek(0, 0).unwrap(),
        SeekTo {
            frame: 0,
            preroll: 0
        }
    );
    let (again, r, _) = read_all(&mut d);
    assert_eq!(r, Ok(()));
    assert_eq!(again, pkts, "{name}");
    assert_eq!(d.seek(1, 0), Err(Error::Unsupported("track")));
}

#[test]
fn seeks_land_on_the_expected_frame_with_the_expected_preroll() {
    for fx in fixtures() {
        let data = fixture(fx.name);
        check_seeks(fx.name, &data, &strict());
        check_seeks(fx.name, &data, &resilient());
    }
    check_seeks("layer1", &layer1_stream(40), &strict());
}

#[test]
fn seeks_in_wrapped_and_free_format_files() {
    let plain = fixture("cbr128-44k-stereo.mp3");
    let mut wrapped = id3v2_with_fake_frames(777);
    wrapped.extend(&plain);
    wrapped.extend(id3v1());
    check_seeks("wrapped", &wrapped, &strict());
    check_seeks("free", &to_free_format(&plain), &strict());
}

#[test]
fn seeks_stay_exact_when_the_info_header_lies() {
    // Wrong frame and byte counts must not mislead the CBR shortcut.
    let plain = fixture("cbr128-44k-stereo.mp3");
    let at = plain.windows(4).position(|w| w == b"Info").unwrap();
    let mut lying = plain.clone();
    lying[at + 8..at + 12].copy_from_slice(&90u32.to_be_bytes());
    lying[at + 12..at + 16].copy_from_slice(&12_345u32.to_be_bytes());
    check_seeks("lying info", &lying, &resilient());
    // Frame sizes that vary under an Info tag: not CBR at all.
    let vbr = fixture("vbr-q2-48k-stereo.mp3");
    let at = vbr
        .windows(4)
        .position(|w| w == b"Xing" || w == b"Info")
        .unwrap();
    let mut relabelled = vbr;
    relabelled[at..at + 4].copy_from_slice(b"Info");
    check_seeks("relabelled vbr", &relabelled, &strict());
}

// ---------------------------------------------------------------- Strict and Resilient

#[test]
fn strict_lost_sync_is_an_error_at_the_damaged_frame() {
    let plain = fixture("cbr128-48k-stereo.mp3");
    let pkts = clean(&plain);
    let mut damaged = plain;
    let at = pkts[40].offset as usize;
    damaged[at..at + 4].fill(0);
    let mut d = open(damaged, &strict());
    let (got, r, _) = read_all(&mut d);
    assert_eq!(got.len(), 40);
    assert_eq!(invalid(&r), ("mpeg sync", pkts[40].offset));
}

#[test]
fn strict_truncated_last_frame_is_unexpected_eof() {
    let plain = fixture("cbr128-48k-stereo.mp3");
    let pkts = clean(&plain);
    let last = pkts.last().unwrap();
    let cut = plain[..last.offset as usize + 100].to_vec();
    let mut d = open(cut, &strict());
    let (got, r, _) = read_all(&mut d);
    assert_eq!(got.len(), pkts.len() - 1);
    assert_eq!(invalid(&r), ("unexpected eof", last.offset));
}

#[test]
fn resilient_replaces_one_damaged_frame_by_silence() {
    let plain = fixture("cbr128-48k-stereo.mp3");
    let pkts = clean(&plain);
    let mut damaged = plain;
    let at = pkts[40].offset as usize;
    damaged[at..at + 4].fill(0);
    let mut d = open(damaged, &resilient());
    let (got, r, ev) = read_all(&mut d);
    assert_eq!(r, Ok(()));
    assert_eq!(got.len(), pkts.len());
    for (i, (a, b)) in got.iter().zip(&pkts).enumerate() {
        if i == 40 {
            assert_eq!((a.pts, a.frames, a.offset), (b.pts, b.frames, b.offset));
            assert!(a.data.is_empty());
        } else {
            assert_eq!(a, b, "packet {i}");
        }
    }
    assert_eq!(
        ev,
        vec![Event::CorruptFrame {
            pts: 40 * 1152,
            offset: pkts[40].offset,
            frames: 1152
        }]
    );
}

#[test]
fn resilient_bounds_the_silence_of_a_long_gap_and_keeps_the_timeline() {
    let plain = fixture("cbr128-48k-stereo.mp3");
    let pkts = clean(&plain);
    let mut damaged = plain;
    let from = pkts[30].offset as usize;
    let to = pkts[50].offset as usize;
    damaged[from..to].fill(0);
    let mut d = open(damaged, &resilient());
    let (got, r, ev) = read_all(&mut d);
    assert_eq!(r, Ok(()));
    // Twenty frames lost, eight of silence delivered, the timeline jumps over the rest.
    assert_eq!(got.len(), pkts.len() - 20 + 8);
    for i in 0..30 {
        assert_eq!(got[i], pkts[i]);
    }
    for i in 0..8 {
        let p = &got[30 + i];
        assert_eq!((p.pts, p.frames), ((30 + i as u64) * 1152, 1152));
        assert!(p.data.is_empty());
    }
    assert_eq!(got[38], pkts[50]);
    assert_eq!(got.last(), pkts.last());
    assert_eq!(
        ev,
        vec![Event::CorruptFrame {
            pts: 30 * 1152,
            offset: pkts[30].offset,
            frames: 20 * 1152
        }]
    );
}

#[test]
fn resilient_truncated_last_frame_becomes_silence() {
    let plain = fixture("cbr128-44k-stereo.mp3");
    let pkts = clean(&plain);
    let last = pkts.last().unwrap();
    let cut = plain[..last.offset as usize + 100].to_vec();
    let mut d = open(cut, &resilient());
    let (got, r, ev) = read_all(&mut d);
    assert_eq!(r, Ok(()));
    assert_eq!(got.len(), pkts.len());
    assert!(got.last().unwrap().data.is_empty());
    assert_eq!(got.last().unwrap().pts, last.pts);
    assert_eq!(
        ev,
        vec![Event::CorruptFrame {
            pts: last.pts,
            offset: last.offset,
            frames: 1152
        }]
    );
}

#[test]
fn resilient_trailing_garbage_is_reported_once() {
    let plain = fixture("cbr128-48k-stereo.mp3");
    let n = clean(&plain).len();
    let mut data = plain.clone();
    data.extend([0xAA; 1000]);
    let mut d = open(data, &resilient());
    let (got, r, ev) = read_all(&mut d);
    assert_eq!((r, got.len()), (Ok(()), n));
    assert_eq!(
        ev,
        vec![Event::CorruptFrame {
            pts: n as u64 * 1152,
            offset: plain.len() as u64,
            frames: 0
        }]
    );
}

#[test]
fn damaged_files_never_panic_and_resilient_never_fails() {
    let mut rng = Rng(0xDEAD_BEEF_CAFE_F00D);
    for fx in fixtures() {
        let plain = fixture(fx.name);
        for round in 0..30 {
            let mut data = plain.clone();
            match round % 3 {
                0 => {
                    for _ in 0..1 + rng.below(8) {
                        let i = rng.below(data.len());
                        data[i] = rng.next() as u8;
                    }
                }
                1 => data.truncate(rng.below(data.len())),
                _ => {
                    let at = rng.below(data.len());
                    let len = rng.below(600).min(data.len() - at);
                    data[at..at + len].fill(rng.next() as u8);
                }
            }
            for opts in [strict(), resilient()] {
                let Ok(mut d) = open_mpa(Box::new(VecSource::new(data.clone())), &opts) else {
                    continue;
                };
                let (pkts, r, _) = read_all(&mut d);
                if opts.mode == Mode::Resilient {
                    assert_eq!(r, Ok(()), "{} round {round}", fx.name);
                }
                assert!(pkts.windows(2).all(|w| w[0].pts < w[1].pts));
                let _ = d.seek(0, rng.next() % 200_000);
                let _ = read_all(&mut d);
            }
        }
    }
}

// ---------------------------------------------------------------- limits and cancellation

#[test]
fn limits() {
    let plain = fixture("nolame-cbr128-48k.mp3");
    let opts = Options {
        limits: Limits {
            max_sample_rate: 40_000,
            ..Limits::default()
        },
        ..strict()
    };
    let r = open_mpa(Box::new(VecSource::new(plain.clone())), &opts);
    assert!(matches!(r, Err(Error::Limit(LimitKind::SampleRate))));
    let opts = Options {
        limits: Limits {
            max_bytes: 1000,
            ..Limits::default()
        },
        ..strict()
    };
    let r = open_mpa(Box::new(VecSource::new(plain.clone())), &opts);
    assert!(matches!(r, Err(Error::Limit(LimitKind::Bytes))));
    let limits = Limits {
        max_frames: 5000,
        ..Limits::default()
    };
    let mut d = open(plain.clone(), &Options { limits, ..strict() });
    let (got, r, _) = read_all(&mut d);
    assert_eq!(got.len(), 4);
    assert_eq!(r, Err(Error::Limit(LimitKind::Frames)));
    let mut d = open(
        plain,
        &Options {
            limits,
            ..resilient()
        },
    );
    let (got, r, ev) = read_all(&mut d);
    assert_eq!((got.len(), r), (4, Ok(())));
    assert_eq!(ev.len(), 1);
    // A declared length over the limit is refused at open.
    let r = open_mpa(
        Box::new(VecSource::new(fixture("cbr128-48k-stereo.mp3"))),
        &Options { limits, ..strict() },
    );
    assert!(matches!(r, Err(Error::Limit(LimitKind::Frames))));
}

#[test]
fn cancellation() {
    let src = CancelSource::new();
    let opts = Options {
        cancel: src.token(),
        ..strict()
    };
    let mut d = open(fixture("cbr128-48k-stereo.mp3"), &opts);
    let mut p = Packet::default();
    assert!(d.next_packet(&mut p).unwrap());
    src.cancel();
    assert_eq!(d.next_packet(&mut p), Err(Error::Cancelled));
    assert_eq!(d.seek(0, 5000), Err(Error::Cancelled));
}

// ---------------------------------------------------------------- VBR header corner cases

#[test]
fn a_lame_tag_with_a_bad_crc_is_not_trusted() {
    let plain = fixture("cbr128-48k-stereo.mp3");
    let at = plain.windows(4).position(|w| w == b"Info").unwrap() - 36;
    let mut bad = plain.clone();
    bad[at + 156 + 21] ^= 0x10; // the encoder delay
    let d = open(bad, &strict());
    let t = &d.tracks()[0];
    assert!(!d.vbr_info().unwrap().lame.as_ref().unwrap().crc_ok);
    // The frame count is still taken from the Info header; the gapless numbers are not.
    assert_eq!(t.frames, Some(126 * 1152));
    assert_eq!(
        (t.delay, t.padding, t.gapless),
        (529, 0, GaplessKind::Unknown)
    );
}

#[test]
fn a_vbri_frame_gives_the_frame_count_and_is_not_delivered() {
    let mut data = Vec::new();
    for i in 0..6 {
        let mut f = vec![0u8; 384];
        f[..4].copy_from_slice(&[0xFF, 0xFB, 0x94, 0x00]);
        if i == 0 {
            f[36..40].copy_from_slice(b"VBRI");
            f[46..50].copy_from_slice(&(6 * 384u32).to_be_bytes());
            f[50..54].copy_from_slice(&5u32.to_be_bytes());
            f[58..60].copy_from_slice(&1u16.to_be_bytes());
            f[56..58].copy_from_slice(&1u16.to_be_bytes());
        }
        data.extend(f);
    }
    let mut d = open(data, &strict());
    let t = d.tracks()[0].clone();
    assert_eq!(t.frames, Some(5 * 1152));
    assert_eq!(
        (t.delay, t.padding, t.gapless),
        (529, 0, GaplessKind::Unknown)
    );
    assert_eq!(d.vbr_info().unwrap().kind, VbrKind::Vbri);
    let (pkts, r, _) = read_all(&mut d);
    assert_eq!((r, pkts.len(), pkts[0].offset), (Ok(()), 5, 384));
}

#[test]
fn the_cbr_index_is_used_only_when_the_info_header_agrees_with_the_audio() {
    let used = |name: &str| open(fixture(name), &strict()).uses_cbr_index();
    assert!(used("cbr128-44k-stereo.mp3"));
    assert!(used("cbr128-48k-stereo.mp3"));
    assert!(used("cbr64-44k-mono.mp3"));
    assert!(!used("nolame-cbr128-48k.mp3"));
    assert!(!used("vbr-q2-48k-stereo.mp3"));
    assert!(!used("mp2-192k-48k-stereo.mp2"));
    let plain = fixture("cbr128-44k-stereo.mp3");
    let at = plain.windows(4).position(|w| w == b"Info").unwrap();
    let mut lying = plain;
    lying[at + 8..at + 12].copy_from_slice(&90u32.to_be_bytes());
    assert!(!open(lying, &strict()).uses_cbr_index());
}

// ---------------------------------------------------------------- reservoir-aware preroll

fn main_data_begin(p: &Pkt) -> usize {
    let h = parse_header(p.data[..4].try_into().unwrap()).unwrap();
    let o = 4 + if h.crc { 2 } else { 0 };
    match h.version {
        MpegVersion::V1 => usize::from(p.data[o]) << 1 | usize::from(p.data[o + 1] >> 7),
        _ => usize::from(p.data[o]),
    }
}

fn main_data_len(p: &Pkt) -> usize {
    let h = parse_header(p.data[..4].try_into().unwrap()).unwrap();
    p.data.len() - 4 - if h.crc { 2 } else { 0 } - h.side_info_len()
}

/// Frames before `goal` a Layer III decode must start with: enough earlier main data to cover
/// `main_data_begin`, plus one frame for the IMDCT overlap, at most 32.
fn reservoir_preroll(pkts: &[Pkt], goal: u64) -> u64 {
    let goal = goal as usize;
    let need = main_data_begin(&pkts[goal]);
    let (mut k, mut have) = (0usize, 0usize);
    while have < need && k < 31 && k < goal {
        k += 1;
        have += main_data_len(&pkts[goal - k]);
    }
    (k + 1).min(goal) as u64
}

#[test]
fn low_bitrate_seeks_start_early_enough_to_cover_the_reservoir() {
    let mut deeper_than_two = 0;
    for name in ["cbr32-48k-stereo.mp3", "cbr8-8k-mono.mp3"] {
        let data = fixture(name);
        let pkts = clean(&data);
        let spf = u64::from(pkts[0].frames);
        let mut d = open(data, &strict());
        for goal in 0..pkts.len() as u64 {
            let t = goal * spf + 3;
            let to = d.seek(0, t).unwrap();
            let start = to.frame / spf;
            assert_eq!(to.frame + to.preroll, t, "{name} goal {goal}");
            assert_eq!(
                goal - start,
                reservoir_preroll(&pkts, goal),
                "{name} {goal}"
            );
            // The main data of the frames from `start` up to the goal covers what the goal
            // frame needs (or the stream starts there).
            let have: usize = pkts[start as usize..goal as usize]
                .iter()
                .map(main_data_len)
                .sum();
            assert!(
                start == 0 || have >= main_data_begin(&pkts[goal as usize]),
                "{name} goal {goal}: have {have}, need {}",
                main_data_begin(&pkts[goal as usize])
            );
            if goal - start > 2 {
                deeper_than_two += 1;
            }
            let mut p = Packet::default();
            assert!(d.next_packet(&mut p).unwrap());
            assert_eq!(p.pts, to.frame);
        }
    }
    assert!(
        deeper_than_two > 0,
        "no low-bitrate target needed more than two frames"
    );
}

// ---------------------------------------------------------------- frame chaining

fn set_bitrate_index(data: &mut [u8], off: u64, idx: u8) {
    let b = &mut data[off as usize + 2];
    *b = (*b & 0x0F) | idx << 4;
}

#[test]
fn a_header_that_changes_the_frame_length_is_caught_at_that_frame() {
    // Bitrate index 9 (128 kbps) -> 10 (160 kbps) or 4 (56 kbps); padding bit flipped on a
    // 44.1 kHz file.
    type Damage = fn(&mut [u8], u64);
    let cases: [(&str, Damage); 3] = [
        ("cbr128-48k-stereo.mp3", |d, o| set_bitrate_index(d, o, 10)),
        ("cbr128-48k-stereo.mp3", |d, o| set_bitrate_index(d, o, 4)),
        ("cbr128-44k-stereo.mp3", |d, o| d[o as usize + 2] ^= 0x02),
    ];
    for (name, damage) in cases {
        let plain = fixture(name);
        let pkts = clean(&plain);
        let k = 40;
        let mut damaged = plain;
        damage(&mut damaged, pkts[k].offset);
        let mut d = open(damaged.clone(), &strict());
        let (got, r, _) = read_all(&mut d);
        assert_eq!(got.len(), k, "{name}: the damaged frame is not delivered");
        assert_eq!(invalid(&r), ("mpeg sync", pkts[k].offset), "{name}");

        let mut d = open(damaged, &resilient());
        let (got, r, ev) = read_all(&mut d);
        assert_eq!(r, Ok(()));
        assert_eq!(got.len(), pkts.len(), "{name}");
        for (i, (a, b)) in got.iter().zip(&pkts).enumerate() {
            if i == k {
                assert!(a.data.is_empty());
                assert_eq!((a.pts, a.frames, a.offset), (b.pts, b.frames, b.offset));
            } else {
                assert_eq!(a, b, "{name} packet {i}");
            }
        }
        assert_eq!(
            ev,
            vec![Event::CorruptFrame {
                pts: k as u64 * 1152,
                offset: pkts[k].offset,
                frames: 1152
            }],
            "{name}"
        );
    }
}

// ---------------------------------------------------------------- trailing junk and counts

#[test]
fn strict_ignores_junk_after_the_last_frame() {
    let plain = fixture("cbr128-48k-stereo.mp3");
    let n = clean(&plain).len();
    let mut rng = Rng(99);
    let random: Vec<u8> = (0..3000).map(|_| rng.next() as u8).collect();
    let junk: Vec<Vec<u8>> = vec![
        vec![0x00],
        vec![0xFF, 0xFB],
        vec![0xFF, 0xFB, 0x94],
        vec![0u8; 4096],
        vec![0xAA; 1000],
        random,
    ];
    for j in junk {
        let mut data = plain.clone();
        data.extend(&j);
        let mut d = open(data.clone(), &strict());
        let (got, r, ev) = read_all(&mut d);
        assert_eq!((r, got.len()), (Ok(()), n), "junk of {} bytes", j.len());
        assert!(ev.is_empty());
        // Seeks to and past the end do not fail either.
        check_seeks("junk", &data, &strict());
    }
}

#[test]
fn resilient_reports_junk_after_the_last_frame_once() {
    let plain = fixture("cbr128-48k-stereo.mp3");
    let n = clean(&plain).len();
    for len in [1usize, 3, 4096] {
        let mut data = plain.clone();
        data.extend(vec![0u8; len]);
        let mut d = open(data, &resilient());
        let (got, r, ev) = read_all(&mut d);
        assert_eq!((r, got.len()), (Ok(()), n));
        assert_eq!(
            ev,
            vec![Event::CorruptFrame {
                pts: n as u64 * 1152,
                offset: plain.len() as u64,
                frames: 0
            }]
        );
    }
}

#[test]
fn a_stream_shorter_than_its_declared_frame_count() {
    let plain = fixture("cbr128-48k-stereo.mp3");
    let pkts = clean(&plain);
    // Cut at a frame boundary: the Info header still says 126 frames.
    let cut = plain[..pkts[100].offset as usize].to_vec();
    let mut d = open(cut.clone(), &strict());
    let (got, r, _) = read_all(&mut d);
    assert_eq!(got.len(), 100);
    assert_eq!(invalid(&r), ("mpeg frame count", cut.len() as u64));
    let mut d = open(cut.clone(), &resilient());
    let (got, r, ev) = read_all(&mut d);
    assert_eq!((r, got.len()), (Ok(()), 100));
    assert_eq!(
        ev,
        vec![Event::Clamped {
            what: "mpeg frame count",
            declared: 126,
            actual: 100
        }]
    );
    // A cut inside the next header leaves stray bytes: the count catches it.
    let last = pkts.last().unwrap();
    let stray = plain[..last.offset as usize + 2].to_vec();
    let mut d = open(stray, &strict());
    let (got, r, _) = read_all(&mut d);
    assert_eq!(got.len(), pkts.len() - 1);
    assert_eq!(invalid(&r).0, "mpeg frame count");
    // Seeking in such a file does not fail.
    let mut d = open(cut, &strict());
    assert_eq!(d.seek(0, u64::MAX).unwrap().preroll, 0);
}

// ---------------------------------------------------------------- checkpoints

#[test]
fn resilient_seek_counts_a_gap_like_reading_does() {
    // The gap starts exactly at a checkpoint (frame 64) of a VBR file, whose frames differ in
    // length: the lost frames are estimated from the frame before the gap, not from whatever
    // was read last.
    let plain = fixture("vbr-q2-48k-stereo.mp3");
    let pkts = clean(&plain);
    let mut damaged = plain;
    let (from, to) = (pkts[64].offset as usize, pkts[74].offset as usize);
    damaged[from..to].fill(0);
    let mut d = open(damaged, &resilient());
    let (seq, r, _) = read_all(&mut d);
    assert_eq!(r, Ok(()));
    for goal in [100u64, 110, 120] {
        let to = d.seek(0, goal * 1152).unwrap();
        let mut p = Packet::default();
        assert!(d.next_packet(&mut p).unwrap());
        let want = seq
            .iter()
            .find(|s| s.pts == to.frame)
            .expect("landing frame");
        assert_eq!(
            (p.pts, p.offset, &p.data),
            (want.pts, want.offset, &want.data)
        );
        assert_eq!(to.frame + to.preroll, goal * 1152);
    }
}
