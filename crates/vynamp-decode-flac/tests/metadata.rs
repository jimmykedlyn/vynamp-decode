use vynamp_decode_core::{
    CodecId, Demuxer, Error, GaplessKind, LimitKind, Limits, Mode, Options, Packet, VecSource,
};
use vynamp_decode_flac::{BlockKind, FlacDemuxer, open_flac, probe_flac};

const MD5: [u8; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];

/// Hand-built 34-byte STREAMINFO body.
fn si_body(min_b: u16, max_b: u16, rate: u32, ch: u8, bps: u8, total: u64) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend(min_b.to_be_bytes());
    v.extend(max_b.to_be_bytes());
    v.extend([0, 0x10, 0x00]); // min frame 4096
    v.extend([0, 0x20, 0x00]); // max frame 8192
    let packed: u64 =
        (u64::from(rate) << 44) | (u64::from(ch - 1) << 41) | (u64::from(bps - 1) << 36) | total;
    v.extend(packed.to_be_bytes());
    v.extend(MD5);
    assert_eq!(v.len(), 34);
    v
}

fn std_body() -> Vec<u8> {
    si_body(4096, 4096, 44_100, 2, 16, 4417)
}

/// One metadata block: header plus body.
fn block(kind: u8, last: bool, data: &[u8]) -> Vec<u8> {
    let mut v = vec![kind | if last { 0x80 } else { 0 }];
    v.extend(&(data.len() as u32).to_be_bytes()[1..]);
    v.extend(data);
    v
}

fn stream(blocks: &[Vec<u8>]) -> Vec<u8> {
    let mut v = b"fLaC".to_vec();
    for b in blocks {
        v.extend(b);
    }
    v
}

fn minimal() -> Vec<u8> {
    stream(&[block(0, true, &std_body())])
}

fn open(data: Vec<u8>, opts: &Options) -> Result<FlacDemuxer, Error> {
    open_flac(Box::new(VecSource::new(data)), opts)
}

fn resilient() -> Options {
    Options {
        mode: Mode::Resilient,
        ..Options::default()
    }
}

fn invalid(r: Result<FlacDemuxer, Error>) -> (&'static str, u64) {
    match r {
        Err(Error::Invalid { what, offset }) => (what, offset),
        other => panic!("expected Invalid, got {:?}", other.map(|_| ())),
    }
}

#[test]
fn probe() {
    assert_eq!(probe_flac(b"fLaC\0\0"), 100);
    assert_eq!(probe_flac(b"fLa"), 0);
    assert_eq!(probe_flac(b"RIFF"), 0);
}

#[test]
fn streaminfo_fields_and_track() {
    let d = open(minimal(), &Options::default()).unwrap();
    let si = d.stream_info();
    assert_eq!(si.min_block, 4096);
    assert_eq!(si.max_block, 4096);
    assert_eq!(si.min_frame, 4096);
    assert_eq!(si.max_frame, 8192);
    assert_eq!(si.sample_rate, 44_100);
    assert_eq!(si.channels, 2);
    assert_eq!(si.bits_per_sample, 16);
    assert_eq!(si.total_samples, 4417);
    assert_eq!(si.md5, MD5);
    assert_eq!(d.first_frame_offset(), 4 + 4 + 34);
    let t = &d.tracks()[0];
    assert_eq!(t.id, 0);
    assert_eq!(t.codec, CodecId::Flac);
    assert_eq!(t.sample_rate, 44_100);
    assert_eq!(t.channels, 2);
    assert_eq!(t.frames, Some(4417));
    assert_eq!((t.delay, t.padding), (0, 0));
    assert_eq!(t.gapless, GaplessKind::Exact);
    assert_eq!(t.codec_private, std_body());
    assert!(d.seek_points().is_empty());
}

#[test]
fn unknown_total_gives_no_frames() {
    let f = stream(&[block(0, true, &si_body(4096, 4096, 44_100, 2, 16, 0))]);
    let d = open(f, &Options::default()).unwrap();
    assert_eq!(d.tracks()[0].frames, None);
}

#[test]
fn wrong_signature() {
    let mut f = minimal();
    f[0] = b'X';
    assert_eq!(invalid(open(f, &Options::default())), ("flac signature", 0));
}

#[test]
fn streaminfo_must_be_first() {
    let f = stream(&[block(1, false, &[0; 8]), block(0, true, &std_body())]);
    assert_eq!(
        invalid(open(f, &Options::default())),
        ("streaminfo first", 4)
    );
}

#[test]
fn streaminfo_wrong_length() {
    let mut body = std_body();
    body.push(0);
    let f = stream(&[block(0, true, &body)]);
    assert_eq!(
        invalid(open(f, &Options::default())),
        ("streaminfo length", 4)
    );
    let f = stream(&[block(0, true, &std_body()[..33])]);
    assert_eq!(invalid(open(f, &Options::default())).0, "streaminfo length");
}

#[test]
fn duplicate_streaminfo() {
    let f = stream(&[block(0, false, &std_body()), block(0, true, &std_body())]);
    assert_eq!(
        invalid(open(f, &Options::default())).0,
        "duplicate streaminfo"
    );
}

#[test]
fn forbidden_block_type() {
    let f = stream(&[block(0, false, &std_body()), block(127, true, &[])]);
    assert_eq!(invalid(open(f, &Options::default())).0, "block type");
}

#[test]
fn bad_streaminfo_fields() {
    let cases: [(Vec<u8>, &str); 5] = [
        (si_body(4096, 4096, 0, 2, 16, 10), "sample rate"),
        (si_body(4096, 1024, 44_100, 2, 16, 10), "block sizes"),
        (si_body(8, 4096, 44_100, 2, 16, 10), "block sizes"),
        (si_body(4096, 4096, 44_100, 2, 3, 10), "bits per sample"),
        (si_body(0, 0, 44_100, 2, 16, 10), "block sizes"),
    ];
    for (body, what) in cases {
        let r = open(stream(&[block(0, true, &body)]), &Options::default());
        assert_eq!(invalid(r).0, what);
    }
}

#[test]
fn single_short_frame_stream_is_valid() {
    let f = stream(&[block(0, true, &si_body(8, 8, 44_100, 1, 16, 8))]);
    assert!(open(f, &Options::default()).is_ok());
}

#[test]
fn rate_beyond_limit() {
    let f = stream(&[block(0, true, &si_body(4096, 4096, 1_000_000, 2, 16, 10))]);
    assert!(matches!(
        open(f, &Options::default()),
        Err(Error::Limit(LimitKind::SampleRate))
    ));
}

#[test]
fn metadata_over_limit() {
    let f = stream(&[block(0, false, &std_body()), block(2, true, &[7; 10])]);
    let opts = Options {
        limits: Limits {
            max_metadata_bytes: 40,
            ..Limits::default()
        },
        ..Options::default()
    };
    assert!(matches!(
        open(f.clone(), &opts),
        Err(Error::Limit(LimitKind::Metadata))
    ));
    let opts = Options {
        limits: Limits {
            max_metadata_bytes: 52,
            ..Limits::default()
        },
        ..Options::default()
    };
    assert!(open(f, &opts).is_ok());
}

#[test]
fn block_headers_count_toward_the_metadata_limit() {
    // Each block costs its 4-byte header plus its body: 38 + 4 + 6 * 4 = 66.
    let mut blocks = vec![block(0, false, &std_body())];
    blocks.extend((0..6).map(|_| block(2, false, &[])));
    blocks.push(block(2, true, &[]));
    let f = stream(&blocks);
    let over = Options {
        limits: Limits {
            max_metadata_bytes: 65,
            ..Limits::default()
        },
        ..Options::default()
    };
    assert!(matches!(
        open(f.clone(), &over),
        Err(Error::Limit(LimitKind::Metadata))
    ));
    let at = Options {
        limits: Limits {
            max_metadata_bytes: 66,
            ..Limits::default()
        },
        ..Options::default()
    };
    assert!(open(f, &at).is_ok());
}

#[test]
fn the_number_of_metadata_blocks_is_capped() {
    for kind in [2u8, 1] {
        let mut v = stream(&[block(0, false, &std_body())]);
        for _ in 0..65_536 {
            v.extend(block(kind, false, &[]));
        }
        v.extend(block(2, true, &[]));
        assert!(matches!(
            open(v, &Options::default()),
            Err(Error::Limit(LimitKind::Metadata))
        ));
    }
    // 65 536 blocks in total are fine.
    let mut v = stream(&[block(0, false, &std_body())]);
    for _ in 0..65_534 {
        v.extend(block(2, false, &[]));
    }
    v.extend(block(2, true, &[]));
    assert!(open(v, &Options::default()).is_ok());
}

#[test]
fn padding_is_skipped_unread_and_not_counted() {
    let f = stream(&[
        block(0, false, &std_body()),
        block(1, false, &vec![0; 100_000]),
        block(2, true, b"APPLabc"),
    ]);
    let opts = Options {
        limits: Limits {
            max_metadata_bytes: 100,
            ..Limits::default()
        },
        ..Options::default()
    };
    let d = open(f.clone(), &opts).unwrap();
    assert_eq!(d.first_frame_offset(), f.len() as u64);
    let kinds: Vec<_> = d.metadata_blocks().iter().map(|b| b.kind).collect();
    assert_eq!(kinds, [BlockKind::StreamInfo, BlockKind::Application]);
    assert_eq!(d.metadata_blocks()[1].data, b"APPLabc");
}

#[test]
fn huge_padding_past_eof_is_eof() {
    let mut f = stream(&[block(0, false, &std_body())]);
    f.extend([0x81, 0xFF, 0xFF, 0xFF]);
    let end = f.len() as u64;
    assert_eq!(
        invalid(open(f, &Options::default())),
        ("unexpected eof", end)
    );
}

#[test]
fn missing_last_block_is_eof() {
    let f = stream(&[block(0, false, &std_body())]);
    assert_eq!(invalid(open(f, &Options::default())).0, "unexpected eof");
}

#[test]
fn seek_table_and_placeholders() {
    let mut t = Vec::new();
    for (s, o, n) in [(0u64, 0u64, 4096u16), (4096, 9000, 4096), (u64::MAX, 0, 0)] {
        t.extend(s.to_be_bytes());
        t.extend(o.to_be_bytes());
        t.extend(n.to_be_bytes());
    }
    let f = stream(&[block(0, false, &std_body()), block(3, true, &t)]);
    let d = open(f, &Options::default()).unwrap();
    let sp = d.seek_points();
    assert_eq!(sp.len(), 2);
    assert_eq!(
        (sp[1].sample, sp[1].offset, sp[1].frames),
        (4096, 9000, 4096)
    );
}

#[test]
fn seek_table_bad_length() {
    let f = stream(&[block(0, false, &std_body()), block(3, true, &[0; 19])]);
    assert_eq!(invalid(open(f, &Options::default())).0, "seektable length");
}

#[test]
fn reserved_and_known_kinds_are_kept() {
    let f = stream(&[
        block(0, false, &std_body()),
        block(4, false, b"vc"),
        block(5, false, b"cue"),
        block(6, false, b"pic"),
        block(77, true, b"x"),
    ]);
    let d = open(f, &Options::default()).unwrap();
    let kinds: Vec<_> = d.metadata_blocks().iter().map(|b| b.kind).collect();
    assert_eq!(
        kinds,
        [
            BlockKind::StreamInfo,
            BlockKind::VorbisComment,
            BlockKind::CueSheet,
            BlockKind::Picture,
            BlockKind::Reserved(77)
        ]
    );
}

#[test]
fn id3v2_prefix_opens_with_same_track() {
    let plain = open(minimal(), &Options::default()).unwrap();
    let mut f = b"ID3\x04\x00\x00\x00\x00\x00\x05".to_vec();
    f.extend([0xAA; 5]);
    let tag = f.len() as u64;
    f.extend(minimal());
    let d = open(f, &Options::default()).unwrap();
    assert_eq!(d.tracks(), plain.tracks());
    assert_eq!(d.first_frame_offset(), plain.first_frame_offset() + tag);
}

#[test]
fn truncated_metadata_is_eof() {
    let f = stream(&[block(0, false, &std_body()), block(2, true, &[1; 20])]);
    for n in [5, 8, 20, 41, 45, f.len() - 1] {
        let what = invalid(open(f[..n].to_vec(), &Options::default())).0;
        assert_eq!(what, "unexpected eof", "{n}");
    }
}

/// `DECODE-AC-32` (a): a cut inside `STREAMINFO` is a typed error naming the cut offset.
///
/// The header claims 34 bytes of `STREAMINFO`; the file has 40 bytes, so the cut leaves
/// 6 body bytes — not enough for the 34-byte body, and not a container the probe can
/// rescue. `offset` must be the cut, not the start of the block: the error is about
/// where the data ran out.
#[test]
fn truncated_streaminfo_is_a_typed_error() {
    let f = stream(&[block(0, true, &std_body())]);
    assert!(f.len() > 40, "fixture must have more than 40 bytes");
    let cut = 40;
    assert_eq!(
        invalid(open(f[..cut].to_vec(), &Options::default())),
        ("unexpected eof", cut as u64)
    );
    // The same cut in Resilient mode is still an open-time error: there is no stream to
    // recover into, so resilience cannot invent a length.
    assert_eq!(
        invalid(open(f[..cut].to_vec(), &resilient())).0,
        "unexpected eof"
    );
}

/// `DECODE-AC-32` (b): a declared length beyond `max_frames` fails at open, as a limit.
///
/// `STREAMINFO` carries 36 bits of `total_samples`, so 2³¹−1 is a legal value the format
/// can express and an absurd one to accept: the check must come from `Limits`, not from
/// the bit width.
#[test]
fn declared_length_beyond_max_frames_is_a_typed_error() {
    let declared: u64 = (1u64 << 31) - 1;
    let f = stream(&[block(
        0,
        true,
        &si_body(4096, 4096, 44_100, 2, 16, declared),
    )]);
    let opts = Options {
        limits: Limits {
            max_frames: 1_000_000,
            ..Limits::default()
        },
        ..Options::default()
    };
    assert_eq!(
        open(f.clone(), &opts).err(),
        Some(Error::Limit(LimitKind::Frames))
    );
    // Just under the limit opens: the boundary is the configured number, not the format's.
    let ok = stream(&[block(
        0,
        true,
        &si_body(4096, 4096, 44_100, 2, 16, 1_000_000),
    )]);
    assert!(open(ok, &opts).is_ok());
}

#[test]
fn byte_flip_and_truncation_smoke() {
    let file = stream(&[
        block(0, false, &std_body()),
        block(1, false, &[0; 16]),
        block(3, false, &[0; 36]),
        block(4, true, b"abc"),
    ]);
    let mut cases: Vec<Vec<u8>> = Vec::new();
    for i in 0..128.min(file.len()) {
        let mut f = file.clone();
        f[i] ^= 0xFF;
        cases.push(f);
    }
    for n in 0..=128.min(file.len()) {
        cases.push(file[..n].to_vec());
    }
    for case in cases {
        for opts in [Options::default(), resilient()] {
            if let Ok(mut d) = open(case.clone(), &opts) {
                let mut p = Packet::default();
                let _ = d.next_packet(&mut p);
            }
        }
    }
}
