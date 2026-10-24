//! Frame sync, packetization, Resilient recovery and exact seek.
//!
//! The FFmpeg fixtures in `tests/fixtures` come from
//! `tools/fixtures/generate-flac-dev.ps1`. Synthetic streams (valid headers and
//! CRCs around opaque payloads) cover what FFmpeg cannot produce, such as
//! variable block sizes. The demuxer never looks inside a frame body.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use vynamp_decode_core::{
    CancelSource, Demuxer, Error, Event, LimitKind, Mode, Options, Packet, SeekTo, Source,
    VecSource, crc8, crc16,
};
use vynamp_decode_flac::{FlacDemuxer, open_flac};

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

fn open(data: Vec<u8>, opts: &Options) -> FlacDemuxer {
    open_flac(Box::new(VecSource::new(data)), opts).expect("open")
}

/// Reads packets until the end or the first error. Also returns drained events.
fn read_all(d: &mut FlacDemuxer) -> (Vec<Pkt>, Result<(), Error>, Vec<Event>) {
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
    assert!(ev.is_empty());
    p
}

fn invalid(r: &Result<(), Error>) -> (&'static str, u64) {
    match r {
        Err(Error::Invalid { what, offset }) => (what, *offset),
        other => panic!("expected Invalid, got {other:?}"),
    }
}

// ---- synthetic streams

fn si_body(min_b: u16, max_b: u16, max_frame: u32, total: u64) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend(min_b.to_be_bytes());
    v.extend(max_b.to_be_bytes());
    v.extend([0, 0, 0]);
    v.extend(&max_frame.to_be_bytes()[1..]);
    let packed: u64 = (44_100u64 << 44) | (1u64 << 41) | (15u64 << 36) | total;
    v.extend(packed.to_be_bytes());
    v.extend([7u8; 16]);
    v
}

fn block(kind: u8, last: bool, data: &[u8]) -> Vec<u8> {
    let mut v = vec![kind | if last { 0x80 } else { 0 }];
    v.extend(&(data.len() as u32).to_be_bytes()[1..]);
    v.extend(data);
    v
}

fn utf8(n: u64) -> Vec<u8> {
    if n < 0x80 {
        return vec![n as u8];
    }
    let extra = if n < 0x800 {
        1
    } else if n < 0x1_0000 {
        2
    } else if n < 0x20_0000 {
        3
    } else if n < 0x400_0000 {
        4
    } else if n < 0x8000_0000 {
        5
    } else {
        6
    };
    let lead = (0xFF00u16 >> (extra + 1)) as u8;
    let mut v = vec![lead | (n >> (6 * extra)) as u8];
    for i in (0..extra).rev() {
        v.push(0x80 | ((n >> (6 * i)) & 0x3F) as u8);
    }
    v
}

/// Deterministic payload without 0xFF bytes, so it can never contain a sync code.
fn payload(seed: u64, len: usize) -> Vec<u8> {
    let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            ((x >> 24) as u8).min(0xFE)
        })
        .collect()
}

/// Stereo, 16-bit frame with rate taken from STREAMINFO.
fn syn_frame(variable: bool, number: u64, size: u32, body: &[u8]) -> Vec<u8> {
    let (code, extra): (u8, Vec<u8>) = match size {
        192 => (1, vec![]),
        576 => (2, vec![]),
        2048 => (11, vec![]),
        4096 => (12, vec![]),
        s => (7, ((s - 1) as u16).to_be_bytes().to_vec()),
    };
    let mut v = vec![0xFF, 0xF8 | u8::from(variable), code << 4, 0x18];
    v.extend(utf8(number));
    v.extend(extra);
    v.push(crc8(&v));
    v.extend(body);
    let c = crc16(&v);
    v.extend(c.to_be_bytes());
    v
}

fn syn_stream(min_b: u16, max_b: u16, max_frame: u32, total: u64, frames: &[Vec<u8>]) -> Vec<u8> {
    let mut v = b"fLaC".to_vec();
    v.extend(block(0, true, &si_body(min_b, max_b, max_frame, total)));
    for f in frames {
        v.extend(f);
    }
    v
}

/// `n` fixed 4096-sample frames plus a last frame of `tail` samples.
fn fixed_frames(n: u64, tail: u32) -> Vec<Vec<u8>> {
    let mut v: Vec<Vec<u8>> = (0..n)
        .map(|i| syn_frame(false, i, 4096, &payload(i, 200 + (i as usize % 7) * 13)))
        .collect();
    if tail > 0 {
        v.push(syn_frame(false, n, tail, &payload(n, 90)));
    }
    v
}

fn fixed_stream(n: u64, tail: u32) -> Vec<u8> {
    let total = n * 4096 + u64::from(tail);
    syn_stream(4096, 4096, 0, total, &fixed_frames(n, tail))
}

const VAR_SIZES: [u32; 9] = [4096, 1000, 576, 192, 2048, 300, 100, 4096, 777];

fn variable_stream(rounds: usize) -> (Vec<u8>, Vec<(u64, u32)>) {
    let mut frames = Vec::new();
    let mut spans = Vec::new();
    let mut pos = 0u64;
    for r in 0..rounds {
        for (i, &s) in VAR_SIZES.iter().enumerate() {
            frames.push(syn_frame(
                true,
                pos,
                s,
                &payload((r * 9 + i) as u64, 120 + i * 11),
            ));
            spans.push((pos, s));
            pos += u64::from(s);
        }
    }
    (syn_stream(100, 4096, 0, pos, &frames), spans)
}

/// Inserts a SEEKTABLE after STREAMINFO. `skew` is added to every offset.
fn with_seektable(file: &[u8], stride: usize, skew: u64) -> Vec<u8> {
    assert_eq!(file[4] & 0x7F, 0);
    let si_last = file[4] & 0x80 != 0;
    let d = open(file.to_vec(), &strict());
    let first = d.first_frame_offset();
    let mut body = Vec::new();
    for p in clean(file).iter().step_by(stride) {
        body.extend(p.pts.to_be_bytes());
        body.extend((p.offset - first + skew).to_be_bytes());
        body.extend((p.frames as u16).to_be_bytes());
    }
    body.extend(u64::MAX.to_be_bytes()); // placeholder
    body.extend([0u8; 10]);
    let mut out = file[..42].to_vec();
    out[4] &= 0x7F;
    out.extend(block(3, si_last, &body));
    out.extend(&file[42..]);
    out
}

/// A source that counts reads and can cancel after a number of them.
struct Probe {
    inner: VecSource,
    bytes: Arc<AtomicU64>,
    reads: Arc<AtomicU64>,
    /// Cancels once this many reads happened (`u64::MAX` = never).
    cancel_at: Arc<AtomicU64>,
    cancel: Arc<CancelSource>,
}

impl Probe {
    fn new(data: Vec<u8>) -> (Self, Arc<AtomicU64>, Arc<AtomicU64>) {
        let bytes = Arc::new(AtomicU64::new(0));
        let reads = Arc::new(AtomicU64::new(0));
        let probe = Self {
            inner: VecSource::new(data),
            bytes: bytes.clone(),
            reads: reads.clone(),
            cancel_at: Arc::new(AtomicU64::new(u64::MAX)),
            cancel: Arc::new(CancelSource::new()),
        };
        (probe, bytes, reads)
    }
}

impl Source for Probe {
    fn read(&mut self, buf: &mut [u8]) -> vynamp_decode_core::Result<usize> {
        let n = self.inner.read(buf)?;
        self.bytes.fetch_add(n as u64, Ordering::SeqCst);
        let r = self.reads.fetch_add(1, Ordering::SeqCst) + 1;
        if r >= self.cancel_at.load(Ordering::SeqCst) {
            self.cancel.cancel();
        }
        Ok(n)
    }
    fn seek(&mut self, pos: u64) -> vynamp_decode_core::Result<()> {
        self.inner.seek(pos)
    }
    fn position(&self) -> u64 {
        self.inner.position()
    }
    fn len(&self) -> Option<u64> {
        self.inner.len()
    }
}

/// Expected result of seeking to `t` over contiguous packets with `total` frames.
fn expect_seek(pk: &[Pkt], total: u64, t: u64) -> SeekTo {
    if t >= total {
        return SeekTo {
            frame: total,
            preroll: 0,
        };
    }
    let p = pk
        .iter()
        .find(|p| p.pts <= t && t < p.pts + u64::from(p.frames))
        .unwrap();
    SeekTo {
        frame: p.pts,
        preroll: t - p.pts,
    }
}

fn check_seeks(file: &[u8], label: &str) {
    let pk = clean(file);
    let total = pk.iter().map(|p| u64::from(p.frames)).sum::<u64>();
    let mut targets = vec![
        0,
        1,
        total / 2,
        total.saturating_sub(1),
        total,
        total + 1000,
    ];
    targets.push(u64::MAX);
    for p in &pk {
        targets.push(p.pts);
        targets.push(p.pts + 1);
        targets.push((p.pts + u64::from(p.frames)).saturating_sub(1));
    }
    let mut d = open(file.to_vec(), &strict());
    // Visit the targets in a back-and-forth order to exercise repositioning.
    let mut order: Vec<u64> = targets.clone();
    order.reverse();
    order.extend(targets);
    for t in order {
        let got = d
            .seek(0, t)
            .unwrap_or_else(|e| panic!("{label} seek {t}: {e}"));
        let want = expect_seek(&pk, total, t);
        assert_eq!(got, want, "{label} seek {t}");
        let mut p = Packet::default();
        let more = d.next_packet(&mut p).unwrap();
        if t >= total {
            assert!(!more, "{label} seek {t} must be at the end");
        } else {
            assert!(more, "{label} seek {t}");
            assert_eq!(
                (p.pts, p.frames),
                (
                    want.frame,
                    pk.iter().find(|q| q.pts == want.frame).unwrap().frames
                )
            );
        }
    }
}

// ------------------------------------------------------------ fixtures

const FIXTURES: [&str; 3] = [
    "sine-16-stereo.flac",
    "sine-24-stereo.flac",
    "sine-16-mono-tail.flac",
];

#[test]
fn packets_are_contiguous_and_complete() {
    for name in FIXTURES {
        let file = fixture(name);
        let mut d = open(file.clone(), &strict());
        let total = d.stream_info().total_samples;
        let first = d.first_frame_offset();
        let (pk, r, ev) = read_all(&mut d);
        assert_eq!(r, Ok(()), "{name}");
        assert!(ev.is_empty());
        assert!(pk.len() >= 4, "{name}");
        let mut pts = 0;
        let mut off = first;
        for p in &pk {
            assert_eq!(p.pts, pts, "{name}");
            assert_eq!(p.offset, off, "{name}");
            assert_eq!(&p.data[..2], &[0xFF, 0xF8], "{name}");
            pts += u64::from(p.frames);
            off += p.data.len() as u64;
        }
        assert_eq!(pts, total, "{name}");
        assert_eq!(off, file.len() as u64, "{name}");
        // Each frame passes its own CRC-16.
        for p in &pk {
            let n = p.data.len();
            assert_eq!(crc16(&p.data[..n - 2]).to_be_bytes(), p.data[n - 2..]);
        }
    }
}

#[test]
fn last_fixture_frame_is_short() {
    let pk = clean(&fixture("sine-16-mono-tail.flac"));
    assert_eq!(pk.len(), 5);
    assert_eq!(pk[4].frames, 800);
    assert_eq!(pk[4].pts, 4000);
}

#[test]
fn packet_buffer_is_reused() {
    let file = fixture("sine-16-stereo.flac");
    let mut d = open(file, &strict());
    let mut p = Packet::default();
    assert!(d.next_packet(&mut p).unwrap());
    let cap = p.data.capacity();
    assert!(d.next_packet(&mut p).unwrap());
    assert!(p.data.capacity() >= cap);
}

#[test]
fn bit_flip_in_frame_3_strict() {
    let file = fixture("sine-16-stereo.flac");
    let pk = clean(&file);
    let mut bad = file.clone();
    let at = pk[3].offset as usize + 20;
    bad[at] ^= 0x10;
    let mut d = open(bad, &strict());
    let (got, r, _) = read_all(&mut d);
    assert_eq!(got.len(), 3);
    assert_eq!(got[..], pk[..3]);
    assert_eq!(invalid(&r), ("flac frame crc", pk[3].offset));
}

#[test]
fn bit_flip_in_frame_3_resilient() {
    let file = fixture("sine-16-stereo.flac");
    let pk = clean(&file);
    let mut bad = file.clone();
    bad[pk[3].offset as usize + 20] ^= 0x10;
    let mut d = open(bad, &resilient());
    let (got, r, ev) = read_all(&mut d);
    assert_eq!(r, Ok(()));
    assert_eq!(got.len(), pk.len());
    for (i, (g, w)) in got.iter().zip(&pk).enumerate() {
        if i == 3 {
            assert!(g.data.is_empty());
            assert_eq!((g.pts, g.frames, g.offset), (w.pts, w.frames, w.offset));
        } else {
            assert_eq!(g, w, "packet {i}");
        }
    }
    assert_eq!(
        ev,
        vec![Event::CorruptFrame {
            pts: pk[3].pts,
            offset: pk[3].offset,
            frames: pk[3].frames
        }]
    );
}

#[test]
fn seek_without_seektable() {
    for name in FIXTURES {
        check_seeks(&fixture(name), name);
    }
}

#[test]
fn seek_with_seektable() {
    for name in FIXTURES {
        let f = fixture(name);
        for stride in [1, 2, 3] {
            let t = with_seektable(&f, stride, 0);
            let d = open(t.clone(), &strict());
            assert!(!d.seek_points().is_empty());
            // The packets are unchanged by the extra block.
            let (a, b) = (clean(&f), clean(&t));
            assert_eq!(a.len(), b.len());
            check_seeks(&t, &format!("{name} stride {stride}"));
        }
    }
}

#[test]
fn seek_survives_a_wrong_seektable() {
    let f = fixture("sine-16-stereo.flac");
    let t = with_seektable(&f, 1, 1);
    check_seeks(&t, "skewed table");
}

#[test]
fn seek_uses_the_seektable_without_scanning_from_the_start() {
    let file = syn_stream_big();
    let with = with_seektable(&file, 50, 0);
    let (probe, bytes, _) = Probe::new(with);
    let mut d = open_flac(Box::new(probe), &strict()).unwrap();
    let before = bytes.load(Ordering::SeqCst);
    let t = 3000 * 4096 + 5;
    let got = d.seek(0, t).unwrap();
    assert_eq!(got.frame, 3000 * 4096);
    assert_eq!(got.preroll, 5);
    assert!(bytes.load(Ordering::SeqCst) - before < 400_000);
}

fn syn_stream_big() -> Vec<u8> {
    // 4000 frames of about 300 bytes.
    let frames: Vec<Vec<u8>> = (0..4000u64)
        .map(|i| syn_frame(false, i, 4096, &payload(i, 280)))
        .collect();
    syn_stream(4096, 4096, 0, 4000 * 4096, &frames)
}

#[test]
fn bisection_reads_little_on_a_big_file_without_seektable() {
    let file = syn_stream_big();
    let len = file.len() as u64;
    let (probe, bytes, _) = Probe::new(file);
    let mut d = open_flac(Box::new(probe), &strict()).unwrap();
    bytes.store(0, Ordering::SeqCst);
    for (t, exact) in [
        (3999 * 4096 + 17, 3999 * 4096),
        (2000 * 4096, 2000 * 4096),
        (1234 * 4096 + 4095, 1234 * 4096),
        (0, 0),
    ] {
        let got = d.seek(0, t).unwrap();
        assert_eq!(got.frame, exact, "{t}");
        assert_eq!(got.preroll, t - exact);
        let mut p = Packet::default();
        assert!(d.next_packet(&mut p).unwrap());
        assert_eq!(p.pts, exact);
    }
    // Four seeks read a small fraction of the 1.2 MB file in total.
    assert!(
        bytes.load(Ordering::SeqCst) < len / 2,
        "{} of {len}",
        bytes.load(Ordering::SeqCst)
    );
}

#[test]
fn seek_past_the_end_and_to_total() {
    let file = fixture("sine-16-mono-tail.flac");
    let mut d = open(file, &strict());
    let mut p = Packet::default();
    for t in [4800u64, 4801, u64::MAX] {
        assert_eq!(
            d.seek(0, t).unwrap(),
            SeekTo {
                frame: 4800,
                preroll: 0
            }
        );
        assert!(!d.next_packet(&mut p).unwrap());
        assert!(!d.next_packet(&mut p).unwrap());
    }
    assert_eq!(
        d.seek(0, 4799).unwrap(),
        SeekTo {
            frame: 4000,
            preroll: 799
        }
    );
    assert!(d.next_packet(&mut p).unwrap());
    assert_eq!((p.pts, p.frames), (4000, 800));
    assert!(matches!(d.seek(1, 0), Err(Error::Unsupported(_))));
}

#[test]
fn unknown_total_decodes_to_eof_and_seeks() {
    let mut file = fixture("sine-16-stereo.flac");
    // Zero the 36-bit total sample count in STREAMINFO.
    file[8 + 13] &= 0xF0;
    file[8 + 14..8 + 18].fill(0);
    let pk = clean(&fixture("sine-16-stereo.flac"));
    let mut d = open(file.clone(), &strict());
    assert_eq!(d.tracks()[0].frames, None);
    let (got, r, ev) = read_all(&mut d);
    assert_eq!(r, Ok(()));
    assert!(ev.is_empty());
    assert_eq!(got, pk);
    let mut d = open(file, &strict());
    let end = pk.last().map(|p| p.pts + u64::from(p.frames)).unwrap();
    assert_eq!(
        d.seek(0, u64::MAX).unwrap(),
        SeekTo {
            frame: end,
            preroll: 0
        }
    );
    let mut p = Packet::default();
    assert!(!d.next_packet(&mut p).unwrap());
    assert_eq!(
        d.seek(0, pk[5].pts + 3).unwrap(),
        SeekTo {
            frame: pk[5].pts,
            preroll: 3
        }
    );
}

#[test]
fn cancel_before_and_during_seek() {
    let file = fixture("sine-16-stereo.flac");
    let c = CancelSource::new();
    let opts = Options {
        cancel: c.token(),
        ..Options::default()
    };
    let mut d = open(file, &opts);
    c.cancel();
    assert_eq!(d.seek(0, 100), Err(Error::Cancelled));
    let mut p = Packet::default();
    assert_eq!(d.next_packet(&mut p), Err(Error::Cancelled));

    // Cancelled by the source while the seek is reading.
    let big = syn_stream_big();
    let (probe, _, reads) = Probe::new(big);
    let (cancel_at, c) = (probe.cancel_at.clone(), probe.cancel.clone());
    let opts = Options {
        cancel: c.token(),
        ..Options::default()
    };
    let mut d = open_flac(Box::new(probe), &opts).unwrap();
    cancel_at.store(reads.load(Ordering::SeqCst) + 3, Ordering::SeqCst);
    assert_eq!(d.seek(0, 3999 * 4096 + 1), Err(Error::Cancelled));
    assert!(reads.load(Ordering::SeqCst) < 20);
}

// ------------------------------------------------------- robustness

#[test]
fn byte_flip_smoke_never_panics() {
    for (name, step) in [("sine-16-stereo.flac", 3usize), ("sine-24-stereo.flac", 11)] {
        let file = fixture(name);
        let d = open(file.clone(), &strict());
        let first = d.first_frame_offset() as usize;
        let total = d.stream_info().total_samples;
        let end = (first + 4096).min(file.len());
        for i in (first..end).step_by(step) {
            for mask in [0x01u8, 0x80, 0xFF] {
                let mut bad = file.clone();
                bad[i] ^= mask;
                let mut s = open(bad.clone(), &strict());
                let (_, r, _) = read_all(&mut s);
                assert!(matches!(r, Ok(()) | Err(Error::Invalid { .. })), "{r:?}");
                let mut z = open(bad, &resilient());
                let (pk, r, _) = read_all(&mut z);
                assert_eq!(r, Ok(()), "{name} byte {i} mask {mask:#x}");
                let mut pts = 0;
                for p in &pk {
                    assert!(p.pts >= pts);
                    pts = p.pts + u64::from(p.frames);
                }
                assert!(pts <= total + 4096);
            }
        }
    }
}

#[test]
fn garbage_streams_never_panic() {
    let mut x = 0x1234_5678_9ABC_DEF1u64;
    for round in 0..40 {
        let mut f = fixture("sine-16-stereo.flac");
        let first = open(f.clone(), &strict()).first_frame_offset() as usize;
        for b in &mut f[first..] {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            if x.is_multiple_of(5) || round % 2 == 0 {
                *b = (x >> 20) as u8;
            }
        }
        for opts in [strict(), resilient()] {
            let mut d = open(f.clone(), &opts);
            let (_, r, _) = read_all(&mut d);
            if opts.mode == Mode::Resilient {
                assert_eq!(r, Ok(()));
            }
            let _ = d.seek(0, 2000);
            let _ = d.seek(0, u64::MAX);
        }
    }
}

#[test]
fn truncation_at_frame_boundaries() {
    for name in FIXTURES {
        let file = fixture(name);
        let d = open(file.clone(), &strict());
        let total = d.stream_info().total_samples;
        let pk = clean(&file);
        let ends: Vec<u64> = pk.iter().map(|p| p.pts + u64::from(p.frames)).collect();
        for k in 0..pk.len() {
            let cut = pk[k].offset as usize;
            let pre = &pk[..k];
            // Strict: complete packets, then an error instead of a silent short end.
            let mut s = open(file[..cut].to_vec(), &strict());
            let (got, r, _) = read_all(&mut s);
            assert_eq!(&got[..], pre, "{name} cut {k}");
            assert_eq!(
                invalid(&r),
                ("unexpected eof", cut as u64),
                "{name} cut {k}"
            );
            // Resilient: same packets, a clean end and a report of the lost tail.
            let mut z = open(file[..cut].to_vec(), &resilient());
            let (got, r, ev) = read_all(&mut z);
            assert_eq!(r, Ok(()));
            assert_eq!(&got[..], pre);
            let lost_from = if k == 0 { 0 } else { ends[k - 1] };
            assert_eq!(
                ev,
                vec![Event::CorruptFrame {
                    pts: lost_from,
                    offset: cut as u64,
                    frames: (total - lost_from) as u32
                }],
                "{name} cut {k}"
            );
        }
        // Cut in the middle of a frame.
        let k = 2;
        let cut = pk[k].offset as usize + 10;
        let mut s = open(file[..cut].to_vec(), &strict());
        let (got, r, _) = read_all(&mut s);
        assert_eq!(&got[..], &pk[..k]);
        assert_eq!(invalid(&r), ("flac frame crc", pk[k].offset));
        let mut z = open(file[..cut].to_vec(), &resilient());
        let (got, r, ev) = read_all(&mut z);
        assert_eq!(r, Ok(()));
        assert_eq!(&got[..k], &pk[..k]);
        assert!(got[k].data.is_empty());
        assert_eq!((got[k].pts, got[k].frames), (pk[k].pts, pk[k].frames));
        assert!(matches!(ev[0], Event::CorruptFrame { pts, .. } if pts == pk[k].pts));
    }
}

#[test]
fn complete_file_has_no_tail_event() {
    let file = fixture("sine-24-stereo.flac");
    let mut z = open(file, &resilient());
    let (_, r, ev) = read_all(&mut z);
    assert_eq!(r, Ok(()));
    assert!(ev.is_empty());
}

// ------------------------------------------------- synthetic streams

#[test]
fn variable_block_size_stream() {
    let (file, spans) = variable_stream(3);
    let mut d = open(file.clone(), &strict());
    let (pk, r, ev) = read_all(&mut d);
    assert_eq!(r, Ok(()));
    assert!(ev.is_empty());
    assert_eq!(pk.len(), spans.len());
    for (p, (pts, size)) in pk.iter().zip(&spans) {
        assert_eq!((p.pts, p.frames), (*pts, *size));
    }
    let total = spans.iter().map(|s| u64::from(s.1)).sum::<u64>();
    check_seeks(&file, "variable");
    let t = with_seektable(&file, 4, 0);
    check_seeks(&t, "variable + table");
    assert_eq!(open(file, &strict()).tracks()[0].frames, Some(total));
}

#[test]
fn fixed_stream_with_short_last_frame() {
    let file = fixed_stream(6, 1000);
    let pk = clean(&file);
    assert_eq!(pk.len(), 7);
    assert_eq!(pk[6].pts, 6 * 4096);
    assert_eq!(pk[6].frames, 1000);
    check_seeks(&file, "fixed");
}

#[test]
fn missing_frame_is_an_error_or_silence() {
    let mut frames = fixed_frames(8, 0);
    frames.remove(3);
    let file = syn_stream(4096, 4096, 0, 8 * 4096, &frames);
    let pk = clean(&fixed_stream(8, 0));

    let mut s = open(file.clone(), &strict());
    let (got, r, _) = read_all(&mut s);
    assert_eq!(got.len(), 3);
    assert_eq!(invalid(&r), ("flac sample number", pk[3].offset));

    let mut z = open(file, &resilient());
    let (got, r, ev) = read_all(&mut z);
    assert_eq!(r, Ok(()));
    assert_eq!(got.len(), 8);
    assert!(got[3].data.is_empty());
    assert_eq!((got[3].pts, got[3].frames), (3 * 4096, 4096));
    assert_eq!(got[4].pts, 4 * 4096);
    assert_eq!(
        ev,
        vec![Event::CorruptFrame {
            pts: 3 * 4096,
            offset: got[3].offset,
            frames: 4096
        }]
    );
    assert_eq!(got[3].offset, got[2].offset + got[2].data.len() as u64);
}

/// `frames` after STREAMINFO, with `edit` applied to the byte image of each
/// frame (index, bytes); returns the file and the offset of every frame.
fn build_with(
    frames: &[Vec<u8>],
    total: u64,
    edit: impl Fn(usize, &[u8]) -> Vec<u8>,
) -> (Vec<u8>, Vec<u64>) {
    let mut file = b"fLaC".to_vec();
    file.extend(block(0, true, &si_body(4096, 4096, 0, total)));
    let mut offs = Vec::new();
    for (i, f) in frames.iter().enumerate() {
        offs.push(file.len() as u64);
        file.extend(edit(i, f));
    }
    (file, offs)
}

#[test]
fn garbage_after_a_frame_is_charged_to_that_frame() {
    // A frame cannot be told apart from the junk that follows it: the span fails
    // its CRC-16, so the frame it follows is the corrupt one.
    let frames = fixed_frames(6, 0);
    let (file, offs) = build_with(&frames, 6 * 4096, |i, f| {
        let mut v = f.to_vec();
        if i == 2 {
            v.extend(vec![0x55u8; 50]);
        }
        v
    });
    let mut s = open(file.clone(), &strict());
    let (got, r, _) = read_all(&mut s);
    assert_eq!(got.len(), 2);
    assert_eq!(invalid(&r), ("flac frame crc", offs[2]));

    // No samples are missing after it: Resilient resyncs at frame 3.
    let mut z = open(file, &resilient());
    let (got, r, ev) = read_all(&mut z);
    assert_eq!(r, Ok(()));
    assert_eq!(got.len(), 6);
    assert!(got[2].data.is_empty());
    assert_eq!(
        (got[2].pts, got[2].frames, got[2].offset),
        (2 * 4096, 4096, offs[2])
    );
    assert_eq!(got[3].offset, offs[3]);
    assert_eq!(got[3].data, frames[3]);
    assert_eq!(
        ev,
        vec![Event::CorruptFrame {
            pts: 2 * 4096,
            offset: offs[2],
            frames: 4096
        }]
    );
}

#[test]
fn garbage_before_the_first_frame_is_a_sync_error() {
    let frames = fixed_frames(4, 0);
    let (mut file, _) = build_with(&frames, 4 * 4096, |_, f| f.to_vec());
    let first = open(file.clone(), &strict()).first_frame_offset() as usize;
    file.splice(first..first, vec![0x55u8; 33]);
    let mut s = open(file.clone(), &strict());
    let (got, r, _) = read_all(&mut s);
    assert!(got.is_empty());
    assert_eq!(invalid(&r), ("flac sync", first as u64));
    let mut z = open(file, &resilient());
    let (got, r, ev) = read_all(&mut z);
    assert_eq!(r, Ok(()));
    assert_eq!(
        ev,
        vec![Event::CorruptFrame {
            pts: 0,
            offset: first as u64,
            frames: 0
        }]
    );
    assert_eq!(got.len(), 4);
    assert_eq!(got[0].offset, first as u64 + 33);
}

#[test]
fn garbage_replacing_a_frame_becomes_silence() {
    let frames = fixed_frames(6, 0);
    let (file, offs) = build_with(&frames, 6 * 4096, |i, f| {
        if i == 2 {
            vec![0x55u8; f.len()]
        } else {
            f.to_vec()
        }
    });
    let mut z = open(file, &resilient());
    let (got, r, ev) = read_all(&mut z);
    assert_eq!(r, Ok(()));
    assert_eq!(got.len(), 6);
    // Frame 1 swallows the garbage that follows it, and frame 2's samples are filled in.
    assert!(got[1].data.is_empty());
    assert!(got[2].data.is_empty());
    assert_eq!(
        (got[2].pts, got[2].frames, got[2].offset),
        (2 * 4096, 4096, offs[3])
    );
    assert_eq!(got[3].data, frames[3]);
    assert_eq!(
        ev,
        vec![
            Event::CorruptFrame {
                pts: 4096,
                offset: offs[1],
                frames: 4096
            },
            Event::CorruptFrame {
                pts: 2 * 4096,
                offset: offs[3],
                frames: 4096
            }
        ]
    );
}

#[test]
fn zero_fill_after_a_frame_is_caught_by_the_sample_numbers() {
    // CRC-16 starts from zero, so zero bytes after a frame keep its CRC valid:
    // the missing frame shows up as a gap instead.
    let frames = fixed_frames(6, 0);
    let (file, offs) = build_with(&frames, 6 * 4096, |i, f| {
        if i == 2 {
            vec![0x00u8; f.len()]
        } else {
            f.to_vec()
        }
    });
    let mut s = open(file.clone(), &strict());
    let (got, r, _) = read_all(&mut s);
    assert_eq!(got.len(), 2);
    assert_eq!(invalid(&r), ("flac sample number", offs[3]));
    let mut z = open(file, &resilient());
    let (got, r, ev) = read_all(&mut z);
    assert_eq!(r, Ok(()));
    assert_eq!(got.len(), 6);
    assert!(got[2].data.is_empty());
    assert_eq!((got[2].pts, got[2].frames), (2 * 4096, 4096));
    assert_eq!(got[3].data, frames[3]);
    assert_eq!(ev.len(), 1);
}

#[test]
fn huge_gap_is_capped_by_the_declared_total() {
    // The second frame claims to start at an absurd sample number.
    let a = syn_frame(true, 0, 4096, &payload(1, 100));
    let b = syn_frame(true, 1 << 35, 4096, &payload(2, 100));
    let file = syn_stream(100, 4096, 0, 8192, &[a, b]);
    let mut z = open(file, &resilient());
    let (got, r, _) = read_all(&mut z);
    assert_eq!(r, Ok(()));
    let silent: u64 = got
        .iter()
        .filter(|p| p.data.is_empty())
        .map(|p| u64::from(p.frames))
        .sum();
    assert!(silent <= 8192);
    assert!(got.iter().all(|p| p.frames <= 4096));
    assert_eq!(got.last().unwrap().pts, 1 << 35);
}

#[test]
fn false_sync_inside_a_frame_is_not_a_boundary() {
    // The payload of frame 1 contains a complete valid header (frame 99), but
    // the CRC-16 of the frame decides where it ends.
    let fake = {
        let mut h = vec![0xFF, 0xF8, 12 << 4, 0x18];
        h.extend(utf8(99));
        h.push(crc8(&h));
        h
    };
    let mut body = payload(5, 150);
    body.splice(60..60, fake);
    let frames = vec![
        syn_frame(false, 0, 4096, &payload(4, 100)),
        syn_frame(false, 1, 4096, &body),
        syn_frame(false, 2, 4096, &payload(6, 100)),
    ];
    let file = syn_stream(4096, 4096, 0, 3 * 4096, &frames);
    for opts in [strict(), resilient()] {
        let mut d = open(file.clone(), &opts);
        let (got, r, ev) = read_all(&mut d);
        assert_eq!(r, Ok(()));
        assert!(ev.is_empty());
        assert_eq!(got.len(), 3);
        for (g, f) in got.iter().zip(&frames) {
            assert_eq!(&g.data, f);
        }
    }
}

#[test]
fn headers_inconsistent_with_streaminfo_are_not_syncs() {
    // A well-formed header for 48 kHz inside a 44.1 kHz stream is not a frame.
    let alien = {
        let mut h = vec![0xFF, 0xF8, 12 << 4 | 10, 0x18, 0x00];
        h.push(crc8(&h));
        h
    };
    let mut body = payload(8, 100);
    body.splice(30..30, alien);
    let frames = vec![
        syn_frame(false, 0, 4096, &payload(7, 100)),
        syn_frame(false, 1, 4096, &body),
    ];
    let file = syn_stream(4096, 4096, 0, 2 * 4096, &frames);
    let pk = clean(&file);
    assert_eq!(pk.len(), 2);
    assert_eq!(pk[1].data, frames[1]);
}

#[test]
fn id3v1_trailer_is_ignored() {
    let mut file = fixed_stream(4, 0);
    let mut tag = b"TAG".to_vec();
    tag.resize(128, b' ');
    file.extend(tag);
    for opts in [strict(), resilient()] {
        let mut d = open(file.clone(), &opts);
        let (got, r, ev) = read_all(&mut d);
        assert_eq!(r, Ok(()));
        assert!(ev.is_empty());
        assert_eq!(got.len(), 4);
        assert_eq!(got[3].data.len(), fixed_frames(4, 0)[3].len());
    }
    // Same with an unknown total.
    let mut unknown = syn_stream(4096, 4096, 0, 0, &fixed_frames(4, 0));
    unknown.extend(b"TAG");
    unknown.resize(unknown.len() + 125, 0);
    let mut d = open(unknown, &strict());
    let (got, r, _) = read_all(&mut d);
    assert_eq!(r, Ok(()));
    assert_eq!(got.len(), 4);
}

#[test]
fn frame_larger_than_the_buffer_cap_is_corrupt() {
    let frames = vec![
        syn_frame(false, 0, 4096, &payload(1, 100)),
        syn_frame(false, 1, 4096, &payload(2, 70_000)),
        syn_frame(false, 2, 4096, &payload(3, 100)),
    ];
    let file = syn_stream(4096, 4096, 300, 3 * 4096, &frames);
    let mut s = open(file.clone(), &strict());
    let (got, r, _) = read_all(&mut s);
    assert_eq!(got.len(), 1);
    assert_eq!(invalid(&r).0, "flac frame crc");
    let mut z = open(file, &resilient());
    let (got, r, ev) = read_all(&mut z);
    assert_eq!(r, Ok(()));
    assert_eq!(got.len(), 3);
    assert!(got[1].data.is_empty());
    assert_eq!(got[2].data, frames[2]);
    // The corrupt frame, and the body bytes skipped up to the next header.
    assert_eq!(ev.len(), 2);
}

#[test]
fn unterminated_frame_hits_the_hard_cap() {
    // No STREAMINFO maximum and no next header: reading stops at 16 MiB.
    let mut file = syn_stream(4096, 4096, 0, 4096, &[]);
    let mut hdr = vec![0xFF, 0xF8, 12 << 4, 0x18, 0x00];
    hdr.push(crc8(&hdr));
    file.extend(hdr);
    file.resize(file.len() + (17 << 20), 0);
    let mut d = open(file.clone(), &strict());
    let mut p = Packet::default();
    assert_eq!(d.next_packet(&mut p), Err(Error::Limit(LimitKind::Bytes)));
    // Resilient treats the unterminated frame as a corrupt last frame.
    let mut d = open(file, &resilient());
    let (got, r, ev) = read_all(&mut d);
    assert_eq!(r, Ok(()));
    assert_eq!(got.len(), 1);
    assert!(got[0].data.is_empty());
    assert_eq!((got[0].pts, got[0].frames), (0, 4096));
    assert!(ev.iter().any(|e| matches!(
        e,
        Event::CorruptFrame {
            pts: 0,
            frames: 4096,
            ..
        }
    )));
}

#[test]
fn empty_stream() {
    let file = syn_stream(4096, 4096, 0, 0, &[]);
    let mut d = open(file, &strict());
    let mut p = Packet::default();
    assert!(!d.next_packet(&mut p).unwrap());
    assert_eq!(
        d.seek(0, 10).unwrap(),
        SeekTo {
            frame: 0,
            preroll: 0
        }
    );
}

// ------------------------------------------------------- fix round 1

/// A header (valid CRC-8) for a fixed-size frame numbered `number`, with a junk body and no CRC-16.
fn fake_header(number: u64, tail: usize) -> Vec<u8> {
    let mut h = vec![0xFF, 0xF8, 12 << 4, 0x18];
    h.extend(utf8(number));
    h.push(crc8(&h));
    h.extend(vec![0x55u8; tail]);
    h
}

#[test]
fn gap_fill_is_capped_by_max_frames_when_total_is_unknown() {
    let a = syn_frame(true, 0, 4096, &payload(1, 100));
    let b = syn_frame(true, 1 << 35, 4096, &payload(2, 100));
    let c = syn_frame(true, (1 << 35) + 4096, 4096, &payload(3, 100));
    let file = syn_stream(100, 4096, 0, 0, &[a, b, c]);
    let mut opts = resilient();
    opts.limits.max_frames = 10_000;
    let mut d = open(file.clone(), &opts);
    let (got, r, ev) = read_all(&mut d);
    assert_eq!(r, Ok(()));
    let silent: u64 = got
        .iter()
        .filter(|p| p.data.is_empty())
        .map(|p| u64::from(p.frames))
        .sum();
    assert_eq!(silent, 10_000 - 4096);
    assert_eq!(got.iter().filter(|p| !p.data.is_empty()).count(), 1);
    assert!(!ev.is_empty());
    // Strict keeps the sample-number error.
    let mut opts = strict();
    opts.limits.max_frames = 10_000;
    let mut d = open(file, &opts);
    let (_, r, _) = read_all(&mut d);
    assert!(r.is_err());
}

#[test]
fn frame_beyond_max_frames_ends_resilient_streams_and_fails_strict() {
    let file = syn_stream(4096, 4096, 0, 0, &fixed_frames(6, 0));
    let mut opts = resilient();
    opts.limits.max_frames = 3 * 4096;
    let mut d = open(file.clone(), &opts);
    let (got, r, ev) = read_all(&mut d);
    assert_eq!(r, Ok(()));
    assert_eq!(got.len(), 3);
    assert_eq!(ev.len(), 1);
    let mut opts = strict();
    opts.limits.max_frames = 3 * 4096;
    let mut d = open(file, &opts);
    let (got, r, _) = read_all(&mut d);
    assert_eq!(got.len(), 3);
    assert_eq!(r, Err(Error::Limit(LimitKind::Frames)));
}

#[test]
fn header_without_a_valid_frame_does_not_justify_a_gap() {
    let frames = fixed_frames(4, 0);
    let (mut file, _) = build_with(&frames, 4 * 4096, |_, f| f.to_vec());
    let first = open(file.clone(), &strict()).first_frame_offset() as usize;
    file.splice(first..first, fake_header(1 << 20, 40));
    let mut z = open(file, &resilient());
    let (got, r, _) = read_all(&mut z);
    assert_eq!(r, Ok(()));
    assert_eq!(got.len(), 4);
    for (i, p) in got.iter().enumerate() {
        assert_eq!(p.pts, i as u64 * 4096);
        assert_eq!(p.data, frames[i]);
    }
}

fn ape_footer(items: usize, with_header: bool) -> Vec<u8> {
    // Items are opaque here; only the sizes matter.
    let size = (32 + items) as u32;
    let mut footer = b"APETAGEX".to_vec();
    footer.extend(2000u32.to_le_bytes());
    footer.extend(size.to_le_bytes());
    footer.extend(0u32.to_le_bytes());
    let flags: u32 = if with_header { 0x8000_0000 } else { 0 };
    footer.extend(flags.to_le_bytes());
    footer.extend([0u8; 8]);
    let mut tag = Vec::new();
    if with_header {
        let mut h = footer.clone();
        h[20..24].copy_from_slice(&(flags | 0x2000_0000).to_le_bytes());
        tag.extend(h);
    }
    tag.extend(vec![0xFFu8; items]);
    tag.extend(footer);
    tag
}

#[test]
fn apev2_trailer_is_not_part_of_the_last_frame() {
    let mut id3 = b"TAG".to_vec();
    id3.resize(128, b' ');
    for name in FIXTURES {
        let base = fixture(name);
        let want = clean(&base);
        for variant in 0..4 {
            let mut file = base.clone();
            match variant {
                0 => file.extend(ape_footer(0, false)),
                1 => file.extend(ape_footer(70, true)),
                2 => {
                    file.extend(ape_footer(31, true));
                    file.extend(&id3);
                }
                _ => {
                    file.extend(ape_footer(5, false));
                    file.extend(&id3);
                }
            }
            for opts in [strict(), resilient()] {
                let mut d = open(file.clone(), &opts);
                let (got, r, ev) = read_all(&mut d);
                assert_eq!(r, Ok(()), "{name} variant {variant}");
                assert!(ev.is_empty());
                assert_eq!(got, want, "{name} variant {variant}");
            }
        }
    }
}

#[test]
fn blocking_strategy_must_not_change() {
    let mut frames = fixed_frames(4, 0);
    // Frame 2 as a variable-size frame with the same first sample.
    frames[2] = syn_frame(true, 2 * 4096, 4096, &payload(9, 100));
    let (file, offs) = build_with(&frames, 4 * 4096, |_, f| f.to_vec());
    let mut s = open(file, &strict());
    let (got, r, _) = read_all(&mut s);
    // The odd header is not a frame boundary. Consecutive valid frames also pass
    // CRC-16 as one span (it starts from zero), so frame 1 absorbs frame 2 and the
    // sample numbers expose the loss.
    assert_eq!(got.len(), 2);
    assert_eq!(got[1].data.len() as u64, offs[3] - offs[1]);
    assert_eq!(invalid(&r), ("flac sample number", offs[3]));
}

#[test]
fn short_block_is_only_valid_as_the_last_frame() {
    let mut frames = fixed_frames(4, 0);
    frames[1] = syn_frame(false, 1, 1000, &payload(9, 100));
    let (file, _) = build_with(&frames, 4 * 4096, |_, f| f.to_vec());
    let mut s = open(file, &strict());
    let (got, r, _) = read_all(&mut s);
    assert_eq!(got.len(), 1);
    assert!(r.is_err());
    // As the last frame it is fine (see fixed_stream_with_short_last_frame).
}

#[test]
fn bisection_ignores_fake_headers_inside_frame_bodies() {
    let frames: Vec<Vec<u8>> = (0..3000u64)
        .map(|i| {
            let mut body = payload(i, 120);
            body.splice(10..10, fake_header(1 << 24, 0));
            syn_frame(false, i, 4096, &body)
        })
        .collect();
    let file = syn_stream(4096, 4096, 0, 3000 * 4096, &frames);
    let len = file.len() as u64;
    let (probe, bytes, _) = Probe::new(file);
    let mut d = open_flac(Box::new(probe), &strict()).unwrap();
    bytes.store(0, Ordering::SeqCst);
    let t = 2900 * 4096 + 7;
    let got = d.seek(0, t).unwrap();
    assert_eq!((got.frame, got.preroll), (2900 * 4096, 7));
    assert!(bytes.load(Ordering::SeqCst) < len / 4);
}

// ------------------------------------ legacy variable block size, totals, minimum size

/// Like `variable_stream`, but in the pre-2007 layout: the blocking-strategy bit says
/// "fixed" while the coded number is a sample number (RFC 9639 appendix B.1: such
/// streams are recognised by `min_block != max_block` in STREAMINFO).
fn legacy_variable_stream(rounds: usize) -> (Vec<u8>, Vec<(u64, u32)>) {
    let mut frames = Vec::new();
    let mut spans = Vec::new();
    let mut pos = 0u64;
    for r in 0..rounds {
        for (i, &s) in VAR_SIZES.iter().enumerate() {
            frames.push(syn_frame(
                false,
                pos,
                s,
                &payload((r * 9 + i) as u64, 120 + i * 11),
            ));
            spans.push((pos, s));
            pos += u64::from(s);
        }
    }
    (syn_stream(100, 4096, 0, pos, &frames), spans)
}

#[test]
fn legacy_variable_block_size_stream() {
    let (file, spans) = legacy_variable_stream(3);
    let mut d = open(file.clone(), &strict());
    let (pk, r, ev) = read_all(&mut d);
    assert_eq!(r, Ok(()));
    assert!(ev.is_empty());
    assert_eq!(pk.len(), spans.len());
    for (p, (pts, size)) in pk.iter().zip(&spans) {
        assert_eq!((p.pts, p.frames), (*pts, *size));
    }
    check_seeks(&file, "legacy variable");
    check_seeks(&with_seektable(&file, 4, 0), "legacy variable + table");
}

#[test]
fn fixed_stream_whose_streaminfo_minimum_counts_the_short_tail_is_still_fixed() {
    let mut frames = fixed_frames(6, 1000);
    frames.truncate(7);
    let file = syn_stream(1000, 4096, 0, 6 * 4096 + 1000, &frames);
    let pk = clean(&file);
    assert_eq!(pk.len(), 7);
    assert_eq!((pk[6].pts, pk[6].frames), (6 * 4096, 1000));
    check_seeks(&file, "fixed, min != max");
}

#[test]
fn more_samples_than_declared_is_invalid_in_strict() {
    let file = syn_stream(4096, 4096, 0, 2 * 4096, &fixed_frames(3, 0));
    let mut d = open(file, &strict());
    let (pk, r, _) = read_all(&mut d);
    assert_eq!(pk.len(), 2);
    assert_eq!(invalid(&r).0, "flac total samples");
}

#[test]
fn more_samples_than_declared_is_clamped_in_resilient() {
    let file = syn_stream(4096, 4096, 0, 2 * 4096, &fixed_frames(4, 0));
    let mut d = open(file, &resilient());
    let (pk, r, ev) = read_all(&mut d);
    assert_eq!(r, Ok(()));
    assert_eq!(pk.len(), 4, "no audio is dropped");
    assert_eq!(
        ev,
        [Event::Clamped {
            what: "flac total samples",
            declared: 2 * 4096,
            actual: 3 * 4096,
        }]
    );
}

#[test]
fn undersized_non_final_blocks_are_invalid_in_strict_only() {
    let frames: Vec<Vec<u8>> = (0..5)
        .map(|i| syn_frame(false, i, 8, &payload(i, 40)))
        .collect();
    let file = syn_stream(8, 8, 0, 40, &frames);
    let mut d = open(file.clone(), &strict());
    let (_, r, _) = read_all(&mut d);
    assert_eq!(invalid(&r).0, "flac block size");
    let mut d = open(file, &resilient());
    let (pk, r, ev) = read_all(&mut d);
    assert_eq!((r, pk.len(), ev.len()), (Ok(()), 5, 0));
    // A short final frame stays legal.
    let short = syn_stream(4096, 4096, 0, 4096 + 5, &fixed_frames(1, 5));
    assert_eq!(clean(&short).len(), 2);
}

// ------------------------------------------------------- final fix wave

#[test]
fn a_huge_gap_is_filled_in_bounded_work_and_reported_once() {
    let a = syn_frame(true, 0, 4096, &payload(1, 100));
    let b = syn_frame(true, 1 << 34, 4096, &payload(2, 100));
    let file = syn_stream(4096, 4096, 0, 0, &[a, b]);
    let (probe, _, reads) = Probe::new(file);
    let mut d = open_flac(Box::new(probe), &resilient()).unwrap();
    let (got, r, ev) = read_all(&mut d);
    assert_eq!(r, Ok(()));
    // Frame, at most 8 chunks of silence (8 x max_block frames), frame.
    assert!(got.len() <= 10, "{} packets", got.len());
    let silent: u64 = got
        .iter()
        .filter(|p| p.data.is_empty())
        .map(|p| u64::from(p.frames))
        .sum();
    assert_eq!(silent, 8 * 4096);
    assert_eq!(got.last().unwrap().pts, 1 << 34);
    assert_eq!(
        ev,
        vec![Event::CorruptFrame {
            pts: 4096,
            offset: got[1].offset,
            frames: u32::MAX,
        }]
    );
    // The far frame is located once, not once per silence chunk.
    assert!(reads.load(Ordering::SeqCst) <= 16);
}

#[test]
fn a_gap_within_the_cap_is_one_event_with_the_full_length() {
    let frames = fixed_frames(6, 0);
    let mut kept = frames.clone();
    kept.remove(2);
    kept.remove(2);
    let mut d = open(syn_stream(4096, 4096, 0, 6 * 4096, &kept), &resilient());
    let (got, r, ev) = read_all(&mut d);
    assert_eq!(r, Ok(()));
    assert_eq!(got.len(), 6);
    assert_eq!(
        ev,
        vec![Event::CorruptFrame {
            pts: 2 * 4096,
            offset: got[2].offset,
            frames: 2 * 4096,
        }]
    );
}

/// `file` without its first `skip` frames (metadata kept).
fn without_leading_frames(file: &[u8], skip: usize) -> (Vec<u8>, Vec<Pkt>) {
    let pk = clean(file);
    let first = open(file.to_vec(), &strict()).first_frame_offset() as usize;
    let mut out = file[..first].to_vec();
    out.extend(&file[pk[skip].offset as usize..]);
    (out, pk)
}

#[test]
fn a_stream_cut_at_a_frame_boundary_is_rebased_to_sample_zero() {
    let (file, pk) = without_leading_frames(&fixture("sine-16-stereo.flac"), 3);
    let origin = pk[3].pts;
    let total = pk.iter().map(|p| u64::from(p.frames)).sum::<u64>();
    for opts in [strict(), resilient()] {
        let mut d = open(file.clone(), &opts);
        assert_eq!(d.tracks()[0].frames, Some(total - origin));
        assert_eq!(d.tracks()[0].codec_private[18..34], [0u8; 16]);
        let (got, r, ev) = read_all(&mut d);
        assert_eq!(r, Ok(()));
        assert_eq!(ev, vec![Event::TimelineRebased { origin }]);
        assert_eq!(got.len(), pk.len() - 3);
        for (g, w) in got.iter().zip(&pk[3..]) {
            assert_eq!(
                (g.pts, g.frames, &g.data),
                (w.pts - origin, w.frames, &w.data)
            );
        }
    }
}

#[test]
fn seeking_works_in_the_rebased_timeline() {
    let (file, pk) = without_leading_frames(&fixture("sine-16-stereo.flac"), 3);
    let origin = pk[3].pts;
    let total = pk.iter().map(|p| u64::from(p.frames)).sum::<u64>() - origin;
    let mut d = open(file, &strict());
    for t in [0, 1, total / 2, total - 1, 5, total / 3] {
        let got = d.seek(0, t).unwrap();
        let p = pk[3..]
            .iter()
            .find(|p| p.pts - origin <= t && t < p.pts - origin + u64::from(p.frames))
            .unwrap();
        assert_eq!(
            got,
            SeekTo {
                frame: p.pts - origin,
                preroll: t - (p.pts - origin)
            }
        );
        let mut out = Packet::default();
        assert!(d.next_packet(&mut out).unwrap());
        assert_eq!(out.pts, got.frame);
    }
}

// ------------------------------------------------------------ trailers

fn synchsafe(n: u32) -> [u8; 4] {
    [
        ((n >> 21) & 0x7F) as u8,
        ((n >> 14) & 0x7F) as u8,
        ((n >> 7) & 0x7F) as u8,
        (n & 0x7F) as u8,
    ]
}

/// An ID3v2.4 tag with a footer, as appended after the audio.
fn id3v2_with_footer(body_len: usize) -> Vec<u8> {
    let mut t = b"ID3\x04\x00\x10".to_vec();
    t.extend(synchsafe(body_len as u32));
    t.extend(vec![0xFFu8; body_len]);
    t.extend(b"3DI\x04\x00\x10");
    t.extend(synchsafe(body_len as u32));
    t
}

fn lyrics3v2(body_len: usize) -> Vec<u8> {
    let mut t = b"LYRICSBEGIN".to_vec();
    t.extend(vec![0xFFu8; body_len]);
    t.extend(format!("{:06}", 11 + body_len).bytes());
    t.extend(b"LYRICS200");
    t
}

fn id3v1() -> Vec<u8> {
    let mut tag = b"TAG".to_vec();
    tag.resize(128, b' ');
    tag
}

fn assert_all_frames(file: Vec<u8>, n: usize) {
    for opts in [strict(), resilient()] {
        let mut d = open(file.clone(), &opts);
        let (got, r, ev) = read_all(&mut d);
        assert_eq!(r, Ok(()));
        assert!(ev.is_empty(), "{ev:?}");
        assert_eq!(got.len(), n);
    }
}

#[test]
fn id3v2_footer_trailer_is_ignored() {
    let mut file = syn_stream(4096, 4096, 0, 0, &fixed_frames(4, 0));
    file.extend(id3v2_with_footer(300));
    assert_all_frames(file, 4);
}

#[test]
fn lyrics3v2_with_id3v1_is_ignored() {
    let mut file = syn_stream(4096, 4096, 0, 0, &fixed_frames(4, 0));
    file.extend(lyrics3v2(200));
    file.extend(id3v1());
    assert_all_frames(file, 4);
}

#[test]
fn trailers_combine_in_any_order() {
    let base = syn_stream(4096, 4096, 0, 0, &fixed_frames(3, 0));
    let ape = ape_footer(2, true);
    let orders: [Vec<Vec<u8>>; 4] = [
        vec![ape.clone(), id3v2_with_footer(40), id3v1()],
        vec![id3v2_with_footer(40), ape.clone(), id3v1()],
        vec![lyrics3v2(30), ape, id3v1()],
        vec![id3v1(), id3v2_with_footer(5)],
    ];
    for parts in orders {
        let mut file = base.clone();
        for p in parts {
            file.extend(p);
        }
        assert_all_frames(file, 3);
    }
}

#[test]
fn unrecognised_junk_after_the_last_frame_is_ignored() {
    let mut x = 0x1234_5678_9ABC_DEF1u64;
    let junk: Vec<u8> = (0..1024)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            (x >> 32) as u8
        })
        .collect();
    let mut file = syn_stream(4096, 4096, 0, 0, &fixed_frames(4, 0));
    file.extend(&junk);
    let mut d = open(file.clone(), &strict());
    let (got, r, ev) = read_all(&mut d);
    assert_eq!(r, Ok(()));
    assert!(ev.is_empty(), "{ev:?}");
    assert_eq!(got.len(), 4);
    assert_eq!(got[3].data, fixed_frames(4, 0)[3]);
    // Resilient reports the ignored bytes, once, without losing samples.
    let mut d = open(file, &resilient());
    let (got, r, ev) = read_all(&mut d);
    assert_eq!(r, Ok(()));
    assert_eq!(got.len(), 4);
    assert_eq!(ev.len(), 1);
    assert!(matches!(ev[0], Event::CorruptFrame { frames: 0, .. }));
}

#[test]
fn a_short_final_frame_may_be_followed_by_trailers_and_junk() {
    for total in [0, 3 * 4096 + 5] {
        let frames = fixed_frames(3, 5);
        let base = syn_stream(4096, 4096, 0, total, &frames);
        let mut tagged = base.clone();
        tagged.extend(id3v1());
        assert_all_frames(tagged, 4);
        let mut junk = base;
        junk.extend(vec![0x11u8; 700]);
        let mut d = open(junk, &strict());
        let (got, r, _) = read_all(&mut d);
        assert_eq!(r, Ok(()));
        assert_eq!(got.len(), 4);
    }
}

#[test]
fn a_seek_discards_pending_skipped_byte_reports() {
    // Frame 1 is far larger than the STREAMINFO hint, so its body is skipped as junk
    // while the seek scans past it; that must not surface as an event afterwards.
    let frames = [
        syn_frame(false, 0, 4096, &payload(1, 100)),
        syn_frame(false, 1, 4096, &payload(2, 70_000)),
        syn_frame(false, 2, 4096, &payload(3, 100)),
        syn_frame(false, 3, 4096, &payload(4, 100)),
    ];
    // A seek table with one entry at sample 0 makes the seek scan from the start.
    let mut table = Vec::new();
    table.extend(0u64.to_be_bytes());
    table.extend(0u64.to_be_bytes());
    table.extend(4096u16.to_be_bytes());
    let mut file = b"fLaC".to_vec();
    file.extend(block(0, false, &si_body(4096, 4096, 300, 4 * 4096)));
    file.extend(block(3, true, &table));
    file.extend(frames.concat());
    let mut d = open(file, &resilient());
    let to = d.seek(0, 2 * 4096 + 5).unwrap();
    assert_eq!(
        to,
        SeekTo {
            frame: 2 * 4096,
            preroll: 5
        }
    );
    let mut p = Packet::default();
    assert!(d.next_packet(&mut p).unwrap());
    assert_eq!((p.pts, p.data.as_slice()), (2 * 4096, frames[2].as_slice()));
    let mut ev = Vec::new();
    d.drain_events(&mut ev);
    assert!(ev.is_empty(), "{ev:?}");
}

#[test]
fn a_sparse_seektable_only_bounds_the_bisection() {
    let file = syn_stream_big();
    let len = file.len() as u64;
    // Only the entry for sample 0 survives: seeks near the end must still bisect.
    let sparse = with_seektable(&file, 100_000, 0);
    let (probe, bytes, _) = Probe::new(sparse);
    let mut d = open_flac(Box::new(probe), &strict()).unwrap();
    let before = bytes.load(Ordering::SeqCst);
    let t = 3999 * 4096 + 17;
    let got = d.seek(0, t).unwrap();
    assert_eq!((got.frame, got.preroll), (3999 * 4096, 17));
    assert!(
        bytes.load(Ordering::SeqCst) - before < len / 4,
        "{} of {len}",
        bytes.load(Ordering::SeqCst) - before
    );
    // The next entry bounds the search from above: a seek between two entries is exact.
    let table = with_seektable(&file, 1000, 0);
    let (probe, bytes, _) = Probe::new(table);
    let mut d = open_flac(Box::new(probe), &strict()).unwrap();
    let before = bytes.load(Ordering::SeqCst);
    let t = 2500 * 4096 + 3;
    let got = d.seek(0, t).unwrap();
    assert_eq!((got.frame, got.preroll), (2500 * 4096, 3));
    assert!(bytes.load(Ordering::SeqCst) - before < len / 8);
}

#[test]
fn demuxer_events_beyond_the_queue_capacity_are_counted() {
    // 4200 frames, each failing its CRC-16: one CorruptFrame event per frame.
    let frames: Vec<Vec<u8>> = (0..4200u64)
        .map(|i| {
            let mut f = syn_frame(false, i, 4096, &payload(i, 3));
            let n = f.len();
            f[n - 1] ^= 0xFF;
            f
        })
        .collect();
    let file = syn_stream(4096, 4096, 0, 4200 * 4096, &frames);
    let mut d = open(file, &resilient());
    let (got, r, ev) = read_all(&mut d);
    assert_eq!(r, Ok(()));
    assert_eq!(got.len(), 4200);
    assert_eq!(ev.len(), 4096);
    assert_eq!(d.take_events_dropped(), 4200 - 4096);
    assert_eq!(d.take_events_dropped(), 0);
}

#[test]
fn the_streaminfo_maximum_frame_size_is_only_a_hint() {
    // Frame 1 is three times the declared maximum but valid: Strict accepts it.
    let frames = vec![
        syn_frame(false, 0, 4096, &payload(1, 100)),
        syn_frame(false, 1, 4096, &payload(2, 900)),
        syn_frame(false, 2, 4096, &payload(3, 100)),
    ];
    let file = syn_stream(4096, 4096, 300, 3 * 4096, &frames);
    for opts in [strict(), resilient()] {
        let mut d = open(file.clone(), &opts);
        let (got, r, ev) = read_all(&mut d);
        assert_eq!(r, Ok(()));
        assert!(ev.is_empty(), "{ev:?}");
        assert_eq!(got.len(), 3);
        assert_eq!(got[1].data, frames[1]);
    }
}
