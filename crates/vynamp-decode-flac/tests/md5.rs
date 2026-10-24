//! STREAMINFO MD5 verification, driven through `FlacDemuxer` and `FlacDecoder` on the
//! FFmpeg fixtures (whose STREAMINFO holds the real MD5 of the unencoded audio).

use vynamp_decode_core::{
    AudioBuffer, Decoder, Demuxer, Error, Event, Mode, Options, Packet, VecSource,
};
use vynamp_decode_flac::{FlacDecoder, open_flac};
use vynamp_decode_testkit::alloc::{CountingAlloc, allocations_on_this_thread};

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

const FIXTURES: [&str; 3] = [
    "sine-16-stereo.flac",
    "sine-24-stereo.flac",
    "sine-16-mono-tail.flac",
];
/// Offset of the MD5 in the file: `fLaC`, a 4-byte block header, then 18 bytes of STREAMINFO.
const MD5_AT: usize = 4 + 4 + 18;

fn load(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn opts(mode: Mode) -> Options {
    Options {
        mode,
        ..Options::default()
    }
}

/// How a run over a stream ended.
#[derive(Debug, PartialEq)]
struct Run {
    /// Index of the packet whose decode failed, with the error's `what` and offset.
    failed: Option<(usize, &'static str, u64)>,
    events: Vec<Event>,
    packets: usize,
}

/// Decodes every packet; `tweak` may seek or alter the packet before it is decoded.
fn run(
    data: Vec<u8>,
    mode: Mode,
    mut tweak: impl FnMut(usize, &mut dyn Demuxer, &mut dyn Decoder, &mut Packet),
) -> Run {
    let mut demux = open_flac(Box::new(VecSource::new(data)), &opts(mode)).unwrap();
    let track = demux.tracks()[0].clone();
    let mut dec = FlacDecoder::with_options(&track, &opts(mode)).unwrap();
    let (mut p, mut out) = (Packet::default(), AudioBuffer::new());
    let (mut i, mut failed, mut events) = (0, None, Vec::new());
    while demux.next_packet(&mut p).unwrap() {
        tweak(i, &mut demux, &mut dec, &mut p);
        match dec.decode(&p, &mut out) {
            Ok(()) => {}
            Err(Error::Invalid { what, offset }) => failed = Some((i, what, offset)),
            Err(e) => panic!("unexpected {e:?}"),
        }
        dec.drain_events(&mut events);
        i += 1;
    }
    Run {
        failed,
        events,
        packets: i,
    }
}

fn plain(_: usize, _: &mut dyn Demuxer, _: &mut dyn Decoder, _: &mut Packet) {}

fn flipped(name: &str) -> Vec<u8> {
    let mut d = load(name);
    assert_eq!(&d[..4], b"fLaC");
    assert_ne!(d[MD5_AT..MD5_AT + 16], [0; 16], "{name} has no MD5");
    d[MD5_AT + 5] ^= 0x01;
    d
}

#[test]
fn intact_streams_verify_silently() {
    for name in FIXTURES {
        for mode in [Mode::Strict, Mode::Resilient] {
            let r = run(load(name), mode, plain);
            assert_eq!((r.failed, r.events.len()), (None, 0), "{name} {mode:?}");
            assert!(r.packets > 1);
        }
    }
}

#[test]
fn strict_reports_a_mismatch_on_the_last_packet() {
    for name in FIXTURES {
        let r = run(flipped(name), Mode::Strict, plain);
        let (i, what, offset) = r.failed.unwrap_or_else(|| panic!("{name}: no error"));
        assert_eq!((i, what), (r.packets - 1, "flac md5"), "{name}");
        assert!(offset > 0);
        assert!(r.events.is_empty());
    }
}

#[test]
fn strict_error_offset_is_the_last_packets() {
    let mut last = 0;
    run(load(FIXTURES[0]), Mode::Strict, |_, _, _, p| {
        last = p.offset
    });
    let r = run(flipped(FIXTURES[0]), Mode::Strict, plain);
    assert_eq!(r.failed.map(|f| f.2), Some(last));
}

#[test]
fn resilient_queues_one_event_and_keeps_decoding() {
    for name in FIXTURES {
        let r = run(flipped(name), Mode::Resilient, plain);
        assert_eq!(r.failed, None, "{name}");
        assert_eq!(
            r.events,
            [Event::ChecksumMismatch { what: "flac md5" }],
            "{name}"
        );
    }
}

#[test]
fn a_seek_disables_verification() {
    for mode in [Mode::Strict, Mode::Resilient] {
        let r = run(flipped(FIXTURES[0]), mode, |i, demux, dec, p| {
            if i == 1 {
                let to = demux.seek(0, 2000).unwrap();
                dec.reset();
                assert!(demux.next_packet(p).unwrap());
                assert_eq!(p.pts, to.frame);
            }
        });
        assert_eq!((r.failed, r.events.len()), (None, 0), "{mode:?}");
    }
}

#[test]
fn a_seek_back_to_zero_still_disables_verification() {
    let r = run(flipped(FIXTURES[0]), Mode::Strict, |i, demux, dec, p| {
        if i == 1 {
            demux.seek(0, 0).unwrap();
            dec.reset();
            assert!(demux.next_packet(p).unwrap());
            assert_eq!(p.pts, 0);
        }
    });
    assert_eq!((r.failed, r.events.len()), (None, 0));
}

#[test]
fn a_stream_not_starting_at_sample_zero_is_not_verified() {
    let r = run(flipped(FIXTURES[0]), Mode::Strict, |i, _, _, p| {
        if i == 0 {
            p.pts = 1;
        }
    });
    assert_eq!(r.failed, None);
}

#[test]
fn a_silence_packet_disables_verification() {
    let r = run(flipped(FIXTURES[0]), Mode::Resilient, |i, _, _, p| {
        if i == 2 {
            p.data.clear();
        }
    });
    assert_eq!((r.failed, r.events.len()), (None, 0));
}

#[test]
fn an_all_zero_md5_is_skipped() {
    for name in FIXTURES {
        let mut d = load(name);
        d[MD5_AT..MD5_AT + 16].fill(0);
        let r = run(d, Mode::Strict, plain);
        assert_eq!((r.failed, r.events.len()), (None, 0), "{name}");
    }
}

#[test]
fn drain_events_moves_the_queue_once() {
    let mut demux = open_flac(
        Box::new(VecSource::new(flipped(FIXTURES[0]))),
        &Options::default(),
    )
    .unwrap();
    let track = demux.tracks()[0].clone();
    let mut dec = FlacDecoder::with_options(&track, &opts(Mode::Resilient)).unwrap();
    let (mut p, mut out, mut ev) = (Packet::default(), AudioBuffer::new(), Vec::new());
    while demux.next_packet(&mut p).unwrap() {
        dec.decode(&p, &mut out).unwrap();
    }
    dec.drain_events(&mut ev);
    assert_eq!(ev.len(), 1);
    dec.drain_events(&mut ev);
    assert_eq!(ev.len(), 1, "the queue was already drained");
}

#[test]
fn hashing_does_not_allocate_after_warm_up() {
    let mut demux = open_flac(
        Box::new(VecSource::new(load(FIXTURES[0]))),
        &Options::default(),
    )
    .unwrap();
    let track = demux.tracks()[0].clone();
    let mut dec = FlacDecoder::new(&track).unwrap();
    let mut packets = Vec::new();
    loop {
        let mut p = Packet::default();
        if !demux.next_packet(&mut p).unwrap() {
            break;
        }
        packets.push(p);
    }
    let mut out = AudioBuffer::new();
    let (first, rest) = packets.split_at(2);
    for p in first {
        dec.decode(p, &mut out).unwrap();
    }
    let before = allocations_on_this_thread();
    // The last packet completes the digest and compares it.
    for p in rest {
        dec.decode(p, &mut out).unwrap();
    }
    assert_eq!(allocations_on_this_thread(), before, "decode allocated");
}
