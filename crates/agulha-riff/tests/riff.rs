use agulha_core::{
    AudioBuffer, CancelSource, CodecId, Decoder, Demuxer, Endian, Error, LimitKind, Limits, Mode,
    Options, Packet, PcmEncoding, PcmSpec, SampleFormat, VecSource,
};
use agulha_pcm::PcmDecoder;
use agulha_riff::{open_riff, probe_riff};
use agulha_testkit::signal::{sine_f64, sine_i32};
use agulha_testkit::writers::{WavVariant, wav};

const RATE: u32 = 48_000;

const VARIANTS: [WavVariant; 5] = [
    WavVariant::Riff,
    WavVariant::Extensible,
    WavVariant::Rf64,
    WavVariant::Bw64,
    WavVariant::Wave64,
];

fn spec(encoding: PcmEncoding, bits: u8) -> PcmSpec {
    PcmSpec {
        encoding,
        bits,
        valid_bits: bits,
        endian: Endian::Little,
    }
}

fn specs() -> Vec<PcmSpec> {
    vec![
        spec(PcmEncoding::Unsigned, 8),
        spec(PcmEncoding::Signed, 16),
        spec(PcmEncoding::Signed, 24),
        spec(PcmEncoding::Signed, 32),
        spec(PcmEncoding::Float, 32),
        spec(PcmEncoding::Float, 64),
    ]
}

/// Interleaved little-endian bytes, one sine per channel.
fn samples(spec: PcmSpec, channels: u16, frames: usize) -> Vec<u8> {
    let per_ch: Vec<Vec<u8>> = (0..channels)
        .map(|c| {
            let freq = 220.0 * f64::from(c + 1);
            let w = usize::from(spec.bits / 8);
            let mut v = Vec::new();
            match spec.encoding {
                PcmEncoding::Float => {
                    for x in sine_f64(RATE, freq, frames) {
                        if spec.bits == 32 {
                            v.extend((x as f32).to_le_bytes());
                        } else {
                            v.extend(x.to_le_bytes());
                        }
                    }
                }
                enc => {
                    for x in sine_i32(RATE, freq, frames, spec.bits) {
                        let x = if enc == PcmEncoding::Unsigned {
                            x + 128
                        } else {
                            x
                        };
                        v.extend(&x.to_le_bytes()[..w]);
                    }
                }
            }
            v
        })
        .collect();
    let w = usize::from(spec.bits / 8);
    let mut out = Vec::new();
    for f in 0..frames {
        for ch in &per_ch {
            out.extend(&ch[f * w..(f + 1) * w]);
        }
    }
    out
}

fn open(bytes: Vec<u8>, opts: &Options) -> agulha_core::Result<agulha_riff::RiffDemuxer> {
    open_riff(Box::new(VecSource::new(bytes)), opts)
}

fn read_all(d: &mut impl Demuxer) -> agulha_core::Result<Vec<u8>> {
    let mut p = Packet::default();
    let mut out = Vec::new();
    while d.next_packet(&mut p)? {
        assert!(p.frames <= 4096);
        out.extend(&p.data);
    }
    Ok(out)
}

#[test]
fn round_trip_all_variants() {
    for variant in VARIANTS {
        for s in specs() {
            for channels in [1u16, 2, 6] {
                let frames = 5000;
                let data = samples(s, channels, frames);
                let file = wav(s, RATE, channels, &data, variant);
                assert_eq!(probe_riff(&file), 100, "{variant:?}");
                let mut d = open(file, &Options::default()).unwrap();
                let t = &d.tracks()[0];
                assert_eq!((t.id, t.sample_rate, t.channels), (0, RATE, channels));
                assert_eq!(t.frames, Some(frames as u64));
                assert_eq!(t.codec, CodecId::Pcm(s), "{variant:?} {s:?}");
                if variant == WavVariant::Extensible {
                    assert_eq!(t.channel_mask, (1u32 << channels) - 1);
                }
                let got = read_all(&mut d).unwrap();
                assert!(got == data, "{variant:?} {s:?} ch={channels}");
            }
        }
    }
}

#[test]
fn decodes_through_pcm_decoder() {
    let s = spec(PcmEncoding::Signed, 16);
    let data = samples(s, 2, 100);
    let file = wav(s, RATE, 2, &data, WavVariant::Riff);
    let mut d = open(file, &Options::default()).unwrap();
    let mut dec = PcmDecoder::new(&d.tracks()[0].clone()).unwrap();
    let mut p = Packet::default();
    assert!(d.next_packet(&mut p).unwrap());
    let mut buf = AudioBuffer::new();
    dec.decode(&p, &mut buf).unwrap();
    assert_eq!(buf.format(), SampleFormat::I16);
    assert_eq!((buf.channels(), buf.frames()), (2, 100));
    let expect = sine_i32(RATE, 220.0, 100, 16);
    let got: Vec<i32> = buf
        .plane_i16(0)
        .unwrap()
        .iter()
        .map(|&x| i32::from(x))
        .collect();
    assert_eq!(got, expect);
}

#[test]
fn probe_rejects_others() {
    assert_eq!(probe_riff(b"RIFF\0\0\0\0AVI LIST"), 0);
    assert_eq!(probe_riff(b"RIFF"), 0);
    assert_eq!(probe_riff(b"ID3\x04\0\0\0\0\0\0\0\0"), 0);
}

fn tweak_odd_chunk(file: &[u8]) -> Vec<u8> {
    // Insert an odd-sized unknown chunk (3 bytes + pad) before "data".
    let pos = file.windows(4).position(|w| w == b"data").unwrap();
    let mut out = file[..pos].to_vec();
    out.extend(b"junk");
    out.extend(3u32.to_le_bytes());
    out.extend([1, 2, 3, 0]);
    out.extend(&file[pos..]);
    let total = (out.len() - 8) as u32;
    out[4..8].copy_from_slice(&total.to_le_bytes());
    out
}

#[test]
fn odd_unknown_chunk_is_skipped() {
    let s = spec(PcmEncoding::Signed, 16);
    let data = samples(s, 2, 300);
    let file = tweak_odd_chunk(&wav(s, RATE, 2, &data, WavVariant::Riff));
    let mut d = open(file, &Options::default()).unwrap();
    assert!(read_all(&mut d).unwrap() == data);
}

#[test]
fn wave64_unknown_chunk_is_skipped() {
    let s = spec(PcmEncoding::Signed, 16);
    let data = samples(s, 2, 300);
    let file = wav(s, RATE, 2, &data, WavVariant::Wave64);
    // Insert a 5-byte-payload unknown chunk (size 29, padded to 32) before data.
    let data_guid = &file[file.len() - data.len() - 24..][..16];
    let pos = file.windows(16).position(|w| w == data_guid).unwrap();
    let mut out = file[..pos].to_vec();
    out.extend([0xAB; 16]);
    out.extend(29u64.to_le_bytes());
    out.extend([1, 2, 3, 4, 5, 0, 0, 0]);
    out.extend(&file[pos..]);
    let mut d = open(out, &Options::default()).unwrap();
    assert!(read_all(&mut d).unwrap() == data);
}

#[test]
fn data_before_fmt() {
    let s = spec(PcmEncoding::Signed, 16);
    let file = wav(s, RATE, 1, &[0; 8], WavVariant::Riff);
    // RIFF header (12) + fmt chunk (24) + data chunk (16): move data first.
    let mut out = file[..12].to_vec();
    out.extend(&file[36..]);
    out.extend(&file[12..36]);
    match open(out, &Options::default()) {
        Err(Error::Invalid {
            what: "data before fmt",
            ..
        }) => {}
        other => panic!("{:?}", other.map(|_| ())),
    }
}

fn patch(file: &mut [u8], at: usize, bytes: &[u8]) {
    file[at..at + bytes.len()].copy_from_slice(bytes);
}

#[test]
fn zero_channels_and_bad_block_align() {
    let s = spec(PcmEncoding::Signed, 16);
    let good = wav(s, RATE, 2, &[0; 16], WavVariant::Riff);
    // fmt body starts at 20: tag(20) channels(22) rate(24) byterate(28) align(32) bits(34)
    let mut z = good.clone();
    patch(&mut z, 22, &0u16.to_le_bytes());
    assert!(matches!(
        open(z, &Options::default()),
        Err(Error::Invalid { .. })
    ));
    let mut b = good;
    patch(&mut b, 32, &5u16.to_le_bytes());
    assert!(matches!(
        open(b, &Options::default()),
        Err(Error::Invalid {
            what: "block align",
            ..
        })
    ));
}

#[test]
fn unsupported_format_tag() {
    let s = spec(PcmEncoding::Signed, 16);
    let mut f = wav(s, RATE, 2, &[0; 16], WavVariant::Riff);
    patch(&mut f, 20, &2u16.to_le_bytes());
    assert!(matches!(
        open(f, &Options::default()),
        Err(Error::Unsupported(_))
    ));
}

#[test]
fn streaming_data_size() {
    let s = spec(PcmEncoding::Signed, 16);
    let data = samples(s, 2, 5000);
    let file = wav(s, RATE, 2, &data, WavVariant::RiffStreaming);
    match open(file.clone(), &Options::default()) {
        Err(Error::Invalid {
            what: "unknown data size",
            ..
        }) => {}
        other => panic!("{:?}", other.map(|_| ())),
    }
    let opts = Options {
        mode: Mode::Resilient,
        ..Options::default()
    };
    let mut d = open(file, &opts).unwrap();
    assert_eq!(d.tracks()[0].frames, None);
    assert!(read_all(&mut d).unwrap() == data);
}

#[test]
fn truncated_strict_and_resilient() {
    let s = spec(PcmEncoding::Signed, 16);
    let full = samples(s, 2, 10_000);
    let present = &full[..5000 * 4];
    let file = wav(s, RATE, 2, &full, WavVariant::Riff);
    let file = file[..file.len() - (full.len() - present.len())].to_vec();

    // Strict: one full packet, then the error; never a short packet.
    let mut d = open(file.clone(), &Options::default()).unwrap();
    let mut p = Packet::default();
    assert!(d.next_packet(&mut p).unwrap());
    assert_eq!((p.pts, p.frames), (0, 4096));
    assert_eq!(p.data, &present[..4096 * 4]);
    assert!(matches!(
        d.next_packet(&mut p),
        Err(Error::Invalid {
            what: "unexpected eof",
            ..
        })
    ));

    // Resilient: short final packet with whole frames only, then false.
    let opts = Options {
        mode: Mode::Resilient,
        ..Options::default()
    };
    let mut d = open(file, &opts).unwrap();
    assert!(read_all(&mut d).unwrap() == present);
    assert!(!d.next_packet(&mut p).unwrap());
}

#[test]
fn declared_1000_present_400_strict() {
    let s = spec(PcmEncoding::Signed, 16);
    let full = samples(s, 1, 1000);
    let file = wav(s, RATE, 1, &full, WavVariant::Riff);
    let file = file[..file.len() - 600 * 2].to_vec();
    let mut d = open(file, &Options::default()).unwrap();
    let mut p = Packet::default();
    assert!(matches!(
        d.next_packet(&mut p),
        Err(Error::Invalid {
            what: "unexpected eof",
            ..
        })
    ));
}

#[test]
fn channel_limit() {
    let s = spec(PcmEncoding::Signed, 16);
    let file = wav(s, RATE, 6, &samples(s, 6, 10), WavVariant::Riff);
    let opts = Options {
        limits: Limits {
            max_channels: 2,
            ..Limits::default()
        },
        ..Options::default()
    };
    assert!(matches!(
        open(file, &opts),
        Err(Error::Limit(LimitKind::Channels))
    ));
}

#[test]
fn frame_limit() {
    let s = spec(PcmEncoding::Signed, 16);
    let file = wav(s, RATE, 1, &samples(s, 1, 100), WavVariant::Riff);
    let opts = Options {
        limits: Limits {
            max_frames: 50,
            ..Limits::default()
        },
        ..Options::default()
    };
    assert!(matches!(
        open(file, &opts),
        Err(Error::Limit(LimitKind::Frames))
    ));
}

#[test]
fn cancel_between_packets() {
    let s = spec(PcmEncoding::Signed, 16);
    let file = wav(s, RATE, 2, &samples(s, 2, 10_000), WavVariant::Riff);
    let src = CancelSource::new();
    let opts = Options {
        cancel: src.token(),
        ..Options::default()
    };
    let mut d = open(file, &opts).unwrap();
    let mut p = Packet::default();
    assert!(d.next_packet(&mut p).unwrap());
    src.cancel();
    assert_eq!(d.next_packet(&mut p), Err(Error::Cancelled));
}

#[test]
fn seek_exact_and_end() {
    for variant in VARIANTS {
        let s = spec(PcmEncoding::Signed, 24);
        let data = samples(s, 2, 4800);
        let file = wav(s, RATE, 2, &data, variant);
        let mut d = open(file, &Options::default()).unwrap();
        let r = d.seek(0, 1000).unwrap();
        assert_eq!((r.frame, r.preroll), (1000, 0));
        let mut p = Packet::default();
        assert!(d.next_packet(&mut p).unwrap());
        assert_eq!(p.pts, 1000);
        assert_eq!(p.frames, 3800);
        assert!(p.data == data[1000 * 6..], "{variant:?}");
        let r = d.seek(0, 99_999).unwrap();
        assert_eq!((r.frame, r.preroll), (4800, 0));
        assert!(!d.next_packet(&mut p).unwrap());
        // Seek back works after reaching the end.
        d.seek(0, 0).unwrap();
        assert!(d.next_packet(&mut p).unwrap());
        assert_eq!(p.pts, 0);
    }
}

#[test]
fn packet_buffer_is_reused() {
    let s = spec(PcmEncoding::Signed, 16);
    let file = wav(s, RATE, 2, &samples(s, 2, 9000), WavVariant::Riff);
    let mut d = open(file, &Options::default()).unwrap();
    let mut p = Packet::default();
    assert!(d.next_packet(&mut p).unwrap());
    let ptr = p.data.as_ptr();
    assert!(d.next_packet(&mut p).unwrap());
    assert_eq!(p.data.as_ptr(), ptr);
}

fn resilient() -> Options {
    Options {
        mode: Mode::Resilient,
        ..Options::default()
    }
}

fn s16() -> PcmSpec {
    spec(PcmEncoding::Signed, 16)
}

fn invalid_what(r: agulha_core::Result<agulha_riff::RiffDemuxer>) -> &'static str {
    match r {
        Err(Error::Invalid { what, .. }) => what,
        other => panic!("expected Invalid, got {:?}", other.map(|_| ())),
    }
}

#[test]
fn riff_zero_data_size() {
    let s = s16();
    // Genuinely empty: the file ends right after the data header.
    let empty = wav(s, RATE, 2, &[], WavVariant::Riff);
    let mut d = open(empty, &Options::default()).unwrap();
    assert_eq!(d.tracks()[0].frames, Some(0));
    assert!(read_all(&mut d).unwrap().is_empty());

    // Zero size with data following: unknown size.
    let data = samples(s, 2, 300);
    let mut file = wav(s, RATE, 2, &data, WavVariant::Riff);
    patch(&mut file, 40, &0u32.to_le_bytes());
    assert_eq!(
        invalid_what(open(file.clone(), &Options::default())),
        "unknown data size"
    );
    let mut d = open(file.clone(), &resilient()).unwrap();
    assert_eq!(d.tracks()[0].frames, None);
    assert!(read_all(&mut d).unwrap() == data);

    // Trailing chunks beyond the RIFF size are not audio.
    let mut with_list = file;
    with_list.extend(b"LIST");
    with_list.extend(8u32.to_le_bytes());
    with_list.extend([7u8; 8]);
    let mut d = open(with_list, &resilient()).unwrap();
    assert!(read_all(&mut d).unwrap() == data);
}

#[test]
fn wave64_zero_data_size() {
    let s = s16();
    let empty = wav(s, RATE, 1, &[], WavVariant::Wave64);
    let mut d = open(empty, &Options::default()).unwrap();
    assert_eq!(d.tracks()[0].frames, Some(0));
    assert!(read_all(&mut d).unwrap().is_empty());

    let data = samples(s, 1, 400);
    let mut file = wav(s, RATE, 1, &data, WavVariant::Wave64);
    patch(&mut file, 96, &24u64.to_le_bytes());
    assert_eq!(
        invalid_what(open(file.clone(), &Options::default())),
        "unknown data size"
    );
    let mut d = open(file.clone(), &resilient()).unwrap();
    assert_eq!(d.tracks()[0].frames, None);
    assert!(read_all(&mut d).unwrap() == data);

    let mut with_tail = file;
    with_tail.extend([9u8; 32]);
    let mut d = open(with_tail, &resilient()).unwrap();
    assert!(read_all(&mut d).unwrap() == data);
}

#[test]
fn rate_and_channels_zero() {
    let good = wav(s16(), RATE, 2, &[0; 16], WavVariant::Riff);
    let mut f = good.clone();
    patch(&mut f, 24, &0u32.to_le_bytes());
    assert_eq!(invalid_what(open(f, &Options::default())), "sample rate");
    let mut f = good;
    patch(&mut f, 22, &0u16.to_le_bytes());
    assert_eq!(invalid_what(open(f, &Options::default())), "channels");
}

#[test]
fn huge_chunk_size_is_eof() {
    let file = wav(s16(), RATE, 1, &[0; 8], WavVariant::Riff);
    let mut out = file[..36].to_vec();
    out.extend(b"junk");
    out.extend(0xFFFF_FFF0u32.to_le_bytes());
    out.extend([0; 8]);
    assert_eq!(
        invalid_what(open(out, &Options::default())),
        "unexpected eof"
    );
}

#[test]
fn zero_size_unknown_chunk() {
    let data = samples(s16(), 1, 50);
    let file = wav(s16(), RATE, 1, &data, WavVariant::Riff);
    let mut out = file[..36].to_vec();
    out.extend(b"junk");
    out.extend(0u32.to_le_bytes());
    out.extend(&file[36..]);
    let mut d = open(out, &Options::default()).unwrap();
    assert!(read_all(&mut d).unwrap() == data);
}

#[test]
fn truncated_headers() {
    let file = wav(s16(), RATE, 1, &[0; 8], WavVariant::Riff);
    assert!(matches!(
        open(file[..10].to_vec(), &Options::default()),
        Err(Error::Invalid { .. })
    ));
    assert!(matches!(
        open(file[..30].to_vec(), &Options::default()),
        Err(Error::Invalid { .. })
    ));
}

#[test]
fn rf64_without_ds64() {
    let file = wav(s16(), RATE, 1, &[0; 8], WavVariant::Rf64);
    // 12-byte header, then ds64 (8 + 28 bytes).
    let mut out = file[..12].to_vec();
    out.extend(&file[48..]);
    assert_eq!(invalid_what(open(out, &Options::default())), "missing ds64");
}

#[test]
fn partial_trailing_frame_is_dropped() {
    let data = samples(s16(), 2, 11);
    let file = wav(s16(), RATE, 2, &data[..43], WavVariant::Riff);
    let mut d = open(file, &Options::default()).unwrap();
    assert_eq!(d.tracks()[0].frames, Some(10));
    assert!(read_all(&mut d).unwrap() == data[..40]);
}

#[test]
fn wave64_data_before_fmt() {
    let file = wav(s16(), RATE, 1, &[0; 16], WavVariant::Wave64);
    let mut out = file[..40].to_vec();
    out.extend(&file[80..]);
    out.extend(&file[40..80]);
    assert_eq!(
        invalid_what(open(out, &Options::default())),
        "data before fmt"
    );
}

#[test]
fn byte_flip_and_truncation_smoke() {
    let s = s16();
    let data = samples(s, 2, 64);
    for variant in [
        WavVariant::Riff,
        WavVariant::Extensible,
        WavVariant::Rf64,
        WavVariant::Wave64,
        WavVariant::RiffStreaming,
    ] {
        let file = wav(s, RATE, 2, &data, variant);
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
                    while let Ok(true) = d.next_packet(&mut p) {}
                }
            }
        }
    }
}
