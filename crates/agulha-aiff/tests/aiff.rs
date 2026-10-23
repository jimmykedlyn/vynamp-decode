use agulha_aiff::{extended, open_aiff, probe_aiff};
use agulha_core::{
    AudioBuffer, CancelSource, CodecId, Decoder, Demuxer, Endian, Error, LimitKind, Limits, Mode,
    Options, Packet, PcmEncoding, PcmSpec, SampleFormat, VecSource,
};
use agulha_pcm::PcmDecoder;
use agulha_testkit::signal::{sine_f64, sine_i32};
use agulha_testkit::writers::aiff;

const RATE: u32 = 48_000;

fn spec(encoding: PcmEncoding, bits: u8, endian: Endian) -> PcmSpec {
    PcmSpec {
        encoding,
        bits,
        valid_bits: bits,
        endian,
    }
}

fn s16() -> PcmSpec {
    spec(PcmEncoding::Signed, 16, Endian::Big)
}

/// Interleaved bytes in the byte order of `spec`, one sine per channel.
fn samples(spec: PcmSpec, channels: u16, frames: usize) -> Vec<u8> {
    let w = usize::from(spec.bits / 8);
    let big = spec.endian == Endian::Big;
    let per_ch: Vec<Vec<u8>> = (0..channels)
        .map(|c| {
            let freq = 220.0 * f64::from(c + 1);
            let mut v = Vec::new();
            match spec.encoding {
                PcmEncoding::Float => {
                    for x in sine_f64(RATE, freq, frames) {
                        match (spec.bits, big) {
                            (32, true) => v.extend((x as f32).to_be_bytes()),
                            (32, false) => v.extend((x as f32).to_le_bytes()),
                            (_, true) => v.extend(x.to_be_bytes()),
                            (_, false) => v.extend(x.to_le_bytes()),
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
                        if big {
                            v.extend(&x.to_be_bytes()[4 - w..]);
                        } else {
                            v.extend(&x.to_le_bytes()[..w]);
                        }
                    }
                }
            }
            v
        })
        .collect();
    let mut out = Vec::new();
    for f in 0..frames {
        for ch in &per_ch {
            out.extend(&ch[f * w..(f + 1) * w]);
        }
    }
    out
}

fn open(bytes: Vec<u8>, opts: &Options) -> agulha_core::Result<agulha_aiff::AiffDemuxer> {
    open_aiff(Box::new(VecSource::new(bytes)), opts)
}

fn resilient() -> Options {
    Options {
        mode: Mode::Resilient,
        ..Options::default()
    }
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

fn invalid_what(r: agulha_core::Result<agulha_aiff::AiffDemuxer>) -> &'static str {
    match r {
        Err(Error::Invalid { what, .. }) => what,
        other => panic!("expected Invalid, got {:?}", other.map(|_| ())),
    }
}

fn patch(file: &mut [u8], at: usize, bytes: &[u8]) {
    file[at..at + bytes.len()].copy_from_slice(bytes);
}

/// Position of the first occurrence of a chunk id (headers precede any audio).
fn find(file: &[u8], id: &[u8; 4]) -> usize {
    file.windows(4).position(|w| w == id).unwrap()
}

fn fix_form_size(f: &mut [u8]) {
    let n = (f.len() - 8) as u32;
    f[4..8].copy_from_slice(&n.to_be_bytes());
}

#[test]
fn extended_canonical_rates() {
    let cases: [(u32, [u8; 10]); 6] = [
        (44100, [0x40, 0x0E, 0xAC, 0x44, 0, 0, 0, 0, 0, 0]),
        (48000, [0x40, 0x0E, 0xBB, 0x80, 0, 0, 0, 0, 0, 0]),
        (96000, [0x40, 0x0F, 0xBB, 0x80, 0, 0, 0, 0, 0, 0]),
        (192000, [0x40, 0x10, 0xBB, 0x80, 0, 0, 0, 0, 0, 0]),
        (8000, [0x40, 0x0B, 0xFA, 0x00, 0, 0, 0, 0, 0, 0]),
        (768000, [0x40, 0x12, 0xBB, 0x80, 0, 0, 0, 0, 0, 0]),
    ];
    for (rate, bytes) in cases {
        assert_eq!(extended::to_u32(bytes), Ok(rate));
    }
    // 1.0
    assert_eq!(
        extended::to_u32([0x3F, 0xFF, 0x80, 0, 0, 0, 0, 0, 0, 0]),
        Ok(1)
    );
    // u32::MAX = 0xFFFFFFFF: exponent 16383 + 31, mantissa 0xFFFFFFFF << 32.
    assert_eq!(
        extended::to_u32([0x40, 0x1E, 0xFF, 0xFF, 0xFF, 0xFF, 0, 0, 0, 0]),
        Ok(u32::MAX)
    );
}

#[test]
fn extended_rejects() {
    let bad: [(&str, [u8; 10]); 10] = [
        ("zero", [0; 10]),
        ("negative", [0xC0, 0x0E, 0xAC, 0x44, 0, 0, 0, 0, 0, 0]),
        ("nan", [0x7F, 0xFF, 0xC0, 0, 0, 0, 0, 0, 0, 0]),
        ("inf", [0x7F, 0xFF, 0x80, 0, 0, 0, 0, 0, 0, 0]),
        ("fraction", [0x40, 0x0E, 0xAC, 0x44, 0x80, 0, 0, 0, 0, 0]),
        ("below one", [0x3F, 0xFE, 0x80, 0, 0, 0, 0, 0, 0, 0]),
        ("2^32", [0x40, 0x1F, 0x80, 0, 0, 0, 0, 0, 0, 0]),
        ("no integer bit", [0x40, 0x0E, 0x2C, 0x44, 0, 0, 0, 0, 0, 0]),
        ("denormal", [0x00, 0x00, 0x00, 0, 0, 0, 0, 0, 0, 1]),
        (
            "low bits",
            [0x40, 0x1E, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF],
        ),
    ];
    for (name, b) in bad {
        assert!(
            matches!(
                extended::to_u32(b),
                Err(Error::Invalid {
                    what: "sample rate",
                    ..
                })
            ),
            "{name}"
        );
    }
}

#[test]
fn round_trip_plain_aiff() {
    for bits in [8u8, 16, 24, 32] {
        for channels in [1u16, 2, 6] {
            let s = spec(PcmEncoding::Signed, bits, Endian::Big);
            let data = samples(s, channels, 5000);
            let file = aiff(s, RATE, channels, &data, None);
            assert_eq!(probe_aiff(&file), 100);
            let mut d = open(file, &Options::default()).unwrap();
            let t = &d.tracks()[0];
            assert_eq!((t.id, t.sample_rate, t.channels), (0, RATE, channels));
            assert_eq!(t.frames, Some(5000));
            assert_eq!(t.codec, CodecId::Pcm(s));
            assert!(
                read_all(&mut d).unwrap() == data,
                "bits={bits} ch={channels}"
            );
        }
    }
}

#[test]
fn round_trip_every_compression_type() {
    use Endian::{Big, Little};
    use PcmEncoding::{Float, Signed, Unsigned};
    let cases: Vec<(&[u8; 4], PcmSpec)> = vec![
        (b"NONE", spec(Signed, 16, Big)),
        (b"twos", spec(Signed, 16, Big)),
        (b"twos", spec(Signed, 8, Big)),
        (b"sowt", spec(Signed, 16, Little)),
        (b"fl32", spec(Float, 32, Big)),
        (b"FL32", spec(Float, 32, Big)),
        (b"fl64", spec(Float, 64, Big)),
        (b"FL64", spec(Float, 64, Big)),
        (b"raw ", spec(Unsigned, 8, Big)),
        (b"in24", spec(Signed, 24, Big)),
        (b"in32", spec(Signed, 32, Big)),
        (b"42ni", spec(Signed, 24, Little)),
        (b"23ni", spec(Signed, 32, Little)),
    ];
    for (tag, s) in cases {
        let data = samples(s, 2, 4500);
        let file = aiff(s, RATE, 2, &data, Some(*tag));
        assert_eq!(&file[8..12], b"AIFC");
        assert_eq!(probe_aiff(&file), 100);
        let mut d = open(file, &Options::default()).unwrap();
        assert_eq!(d.tracks()[0].codec, CodecId::Pcm(s), "{tag:?}");
        assert_eq!(d.tracks()[0].frames, Some(4500));
        assert!(read_all(&mut d).unwrap() == data, "{tag:?}");
    }
}

#[test]
fn odd_sample_size_rounds_up_container() {
    let s = PcmSpec {
        encoding: PcmEncoding::Signed,
        bits: 16,
        valid_bits: 12,
        endian: Endian::Big,
    };
    let data = samples(s, 1, 100);
    let mut d = open(aiff(s, RATE, 1, &data, None), &Options::default()).unwrap();
    assert_eq!(d.tracks()[0].codec, CodecId::Pcm(s));
    assert!(read_all(&mut d).unwrap() == data);
    // Declared 20 bits: 24-bit container.
    let s20 = PcmSpec {
        bits: 24,
        valid_bits: 20,
        ..s
    };
    let d = open(
        aiff(s20, RATE, 1, &samples(s20, 1, 10), None),
        &Options::default(),
    )
    .unwrap();
    assert_eq!(d.tracks()[0].codec, CodecId::Pcm(s20));
}

#[test]
fn decodes_through_pcm_decoder() {
    let s = s16();
    let data = samples(s, 2, 100);
    let mut d = open(aiff(s, RATE, 2, &data, None), &Options::default()).unwrap();
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
    assert_eq!(probe_aiff(b"FORM\0\0\0\0AIFF"), 100);
    assert_eq!(probe_aiff(b"FORM\0\0\0\0AIFC"), 100);
    assert_eq!(probe_aiff(b"FORM\0\0\0\08SVX"), 0);
    assert_eq!(probe_aiff(b"FORM"), 0);
    assert_eq!(probe_aiff(b"RIFF\0\0\0\0WAVE"), 0);
}

#[test]
fn bad_signature_and_form_type() {
    let file = aiff(s16(), RATE, 1, &[0; 8], None);
    let mut f = file.clone();
    f[0] = b'X';
    assert_eq!(invalid_what(open(f, &Options::default())), "aiff signature");
    let mut f = file;
    patch(&mut f, 8, b"8SVX");
    assert_eq!(invalid_what(open(f, &Options::default())), "form type");
}

#[test]
fn ssnd_offset_is_respected() {
    let s = s16();
    let data = samples(s, 2, 700);
    let file = aiff(s, RATE, 2, &data, None);
    let at = find(&file, b"SSND");
    let mut out = file[..at].to_vec();
    out.extend(b"SSND");
    out.extend(((8 + 16 + data.len()) as u32).to_be_bytes());
    out.extend(16u32.to_be_bytes());
    out.extend(0u32.to_be_bytes());
    out.extend([0xEE; 16]);
    out.extend(&data);
    fix_form_size(&mut out);
    let mut d = open(out.clone(), &Options::default()).unwrap();
    assert_eq!(d.tracks()[0].frames, Some(700));
    assert!(read_all(&mut d).unwrap() == data);
    // Seek lands relative to the shifted start.
    d.seek(0, 100).unwrap();
    let mut p = Packet::default();
    assert!(d.next_packet(&mut p).unwrap());
    assert!(p.data == data[100 * 4..]);

    // Offset larger than the chunk is malformed.
    patch(&mut out, at + 8, &1_000_000u32.to_be_bytes());
    assert_eq!(invalid_what(open(out, &Options::default())), "ssnd offset");
}

#[test]
fn ssnd_before_comm_parses() {
    let s = s16();
    let data = samples(s, 2, 300);
    let file = aiff(s, RATE, 2, &data, None);
    // FORM header (12), COMM chunk (8 + 18), SSND chunk (rest).
    let mut out = file[..12].to_vec();
    out.extend(&file[38..]);
    out.extend(&file[12..38]);
    assert_eq!(&out[12..16], b"SSND");
    let mut d = open(out.clone(), &Options::default()).unwrap();
    assert_eq!(d.tracks()[0].frames, Some(300));
    assert!(read_all(&mut d).unwrap() == data);
    let mut d = open(out, &resilient()).unwrap();
    assert!(read_all(&mut d).unwrap() == data);
}

#[test]
fn unsupported_compression() {
    for tag in [b"ulaw", b"alaw", b"ima4", b"abcd"] {
        let file = aiff(s16(), RATE, 1, &[0; 8], Some(*tag));
        assert!(
            matches!(open(file, &Options::default()), Err(Error::Unsupported(_))),
            "{tag:?}"
        );
    }
}

#[test]
fn missing_chunks() {
    let file = aiff(s16(), RATE, 1, &[0; 8], None);
    let mut f = file.clone();
    patch(&mut f, 12, b"JUNK");
    assert_eq!(
        invalid_what(open(f, &Options::default())),
        "missing comm chunk"
    );
    let mut f = file;
    let at = find(&f, b"SSND");
    patch(&mut f, at, b"JUNK");
    assert_eq!(
        invalid_what(open(f, &Options::default())),
        "missing ssnd chunk"
    );
}

#[test]
fn rate_channels_and_sample_size() {
    let good = aiff(s16(), RATE, 2, &[0; 16], None);
    // COMM body at 20: channels 20, frames 22, size 26, rate 28.
    let try_rate = |bytes: [u8; 10]| {
        let mut f = good.clone();
        patch(&mut f, 28, &bytes);
        invalid_what(open(f, &Options::default()))
    };
    assert_eq!(try_rate([0; 10]), "sample rate");
    assert_eq!(
        try_rate([0x7F, 0xFF, 0xC0, 0, 0, 0, 0, 0, 0, 0]),
        "sample rate"
    );
    assert_eq!(
        try_rate([0x7F, 0xFF, 0x80, 0, 0, 0, 0, 0, 0, 0]),
        "sample rate"
    );
    assert_eq!(
        try_rate([0x40, 0x0E, 0xAC, 0x44, 0x80, 0, 0, 0, 0, 0]),
        "sample rate"
    );
    // The error offset is the absolute position of the COMM body.
    let mut f = good.clone();
    patch(&mut f, 28, &[0; 10]);
    assert!(matches!(
        open(f, &Options::default()),
        Err(Error::Invalid { offset: 20, .. })
    ));
    let mut f = good.clone();
    patch(&mut f, 20, &0u16.to_be_bytes());
    assert_eq!(invalid_what(open(f, &Options::default())), "channels");
    let mut f = good.clone();
    patch(&mut f, 26, &0u16.to_be_bytes());
    assert_eq!(invalid_what(open(f, &Options::default())), "sample size");
    let mut f = good;
    patch(&mut f, 26, &48u16.to_be_bytes());
    assert!(matches!(
        open(f, &Options::default()),
        Err(Error::Unsupported(_))
    ));
}

#[test]
fn compression_sample_size_mismatch() {
    let f32s = spec(PcmEncoding::Float, 32, Endian::Big);
    let mut f = aiff(f32s, RATE, 1, &[0; 8], Some(*b"fl32"));
    let at = find(&f, b"COMM") + 8;
    patch(&mut f, at + 6, &16u16.to_be_bytes());
    assert_eq!(invalid_what(open(f, &Options::default())), "sample size");
    let i24 = spec(PcmEncoding::Signed, 24, Endian::Big);
    let mut f = aiff(i24, RATE, 1, &[0; 9], Some(*b"in24"));
    let at = find(&f, b"COMM") + 8;
    patch(&mut f, at + 6, &16u16.to_be_bytes());
    assert_eq!(invalid_what(open(f, &Options::default())), "sample size");
}

#[test]
fn aifc_comm_too_short() {
    let mut f = aiff(s16(), RATE, 1, &[0; 8], Some(*b"NONE"));
    let at = find(&f, b"COMM");
    patch(&mut f, at + 4, &18u32.to_be_bytes());
    assert_eq!(invalid_what(open(f, &Options::default())), "comm size");
}

#[test]
fn huge_chunk_size_is_eof() {
    let file = aiff(s16(), RATE, 1, &[0; 8], None);
    let at = find(&file, b"SSND");
    let mut out = file[..at].to_vec();
    out.extend(b"junk");
    out.extend(0xFFFF_FFF0u32.to_be_bytes());
    out.extend([0; 8]);
    assert_eq!(
        invalid_what(open(out, &Options::default())),
        "unexpected eof"
    );
}

#[test]
fn zero_size_and_odd_chunks_are_skipped() {
    let data = samples(s16(), 1, 50);
    let file = aiff(s16(), RATE, 1, &data, None);
    let at = find(&file, b"SSND");
    let mut out = file[..at].to_vec();
    out.extend(b"junk");
    out.extend(0u32.to_be_bytes());
    out.extend(b"odd ");
    out.extend(3u32.to_be_bytes());
    out.extend([1, 2, 3, 0]);
    out.extend(&file[at..]);
    fix_form_size(&mut out);
    let mut d = open(out, &Options::default()).unwrap();
    assert!(read_all(&mut d).unwrap() == data);
}

#[test]
fn truncated_headers() {
    let file = aiff(s16(), RATE, 1, &[0; 8], None);
    for n in [0, 5, 10, 30] {
        assert!(
            matches!(
                open(file[..n].to_vec(), &Options::default()),
                Err(Error::Invalid { .. })
            ),
            "{n}"
        );
    }
}

#[test]
fn partial_trailing_frame_is_dropped() {
    let data = samples(s16(), 2, 11);
    let file = aiff(s16(), RATE, 2, &data[..43], None);
    let mut d = open(file.clone(), &Options::default()).unwrap();
    assert_eq!(d.tracks()[0].frames, Some(10));
    assert!(read_all(&mut d).unwrap() == data[..40]);
}

#[test]
fn comm_frames_beyond_ssnd() {
    let data = samples(s16(), 2, 11);
    let mut f = aiff(s16(), RATE, 2, &data[..40], None);
    patch(&mut f, 22, &11u32.to_be_bytes());
    let ssnd = find(&f, b"SSND") as u64;
    // Strict: inconsistent header is rejected at the SSND chunk header.
    match open(f.clone(), &Options::default()) {
        Err(Error::Invalid {
            what: "comm frames exceed ssnd",
            offset,
        }) => assert_eq!(offset, ssnd),
        other => panic!("{:?}", other.map(|_| ())),
    }
    // Resilient: clamped to what the chunk holds.
    let mut d = open(f, &resilient()).unwrap();
    assert_eq!(d.tracks()[0].frames, Some(10));
    assert!(read_all(&mut d).unwrap() == data[..40]);
}

#[test]
fn truncated_strict_and_resilient() {
    let s = s16();
    let full = samples(s, 2, 10_000);
    let present = &full[..5000 * 4];
    let file = aiff(s, RATE, 2, &full, None);
    let file = file[..file.len() - (full.len() - present.len())].to_vec();

    let mut d = open(file.clone(), &Options::default()).unwrap();
    let mut p = Packet::default();
    assert!(d.next_packet(&mut p).unwrap());
    assert_eq!((p.pts, p.frames), (0, 4096));
    assert_eq!(p.data, &present[..4096 * 4]);
    match d.next_packet(&mut p) {
        Err(Error::Invalid {
            what: "unexpected eof",
            offset,
        }) => assert!(offset <= file.len() as u64),
        other => panic!("{other:?}"),
    }

    let mut d = open(file, &resilient()).unwrap();
    assert!(read_all(&mut d).unwrap() == present);
    assert!(!d.next_packet(&mut p).unwrap());
}

#[test]
fn declared_1000_present_400_strict() {
    let full = samples(s16(), 1, 1000);
    let file = aiff(s16(), RATE, 1, &full, None);
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
fn limits() {
    let s = s16();
    let file = aiff(s, RATE, 6, &samples(s, 6, 10), None);
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
    let file = aiff(s, RATE, 1, &samples(s, 1, 100), None);
    let opts = Options {
        limits: Limits {
            max_frames: 50,
            ..Limits::default()
        },
        ..Options::default()
    };
    assert!(matches!(
        open(file.clone(), &opts),
        Err(Error::Limit(LimitKind::Frames))
    ));
    let opts = Options {
        limits: Limits {
            max_bytes: 10,
            ..Limits::default()
        },
        ..Options::default()
    };
    assert!(matches!(
        open(file, &opts),
        Err(Error::Limit(LimitKind::Bytes))
    ));
}

#[test]
fn cancel_at_open_and_between_packets() {
    let s = s16();
    let file = aiff(s, RATE, 2, &samples(s, 2, 10_000), None);
    let src = CancelSource::new();
    let opts = Options {
        cancel: src.token(),
        ..Options::default()
    };
    let mut d = open(file.clone(), &opts).unwrap();
    let mut p = Packet::default();
    assert!(d.next_packet(&mut p).unwrap());
    src.cancel();
    assert_eq!(d.next_packet(&mut p), Err(Error::Cancelled));
    assert!(matches!(open(file, &opts), Err(Error::Cancelled)));
}

#[test]
fn seek_exact_and_end() {
    let s = spec(PcmEncoding::Signed, 24, Endian::Big);
    let data = samples(s, 2, 4800);
    let mut d = open(aiff(s, RATE, 2, &data, None), &Options::default()).unwrap();
    let r = d.seek(0, 1000).unwrap();
    assert_eq!((r.frame, r.preroll), (1000, 0));
    let mut p = Packet::default();
    assert!(d.next_packet(&mut p).unwrap());
    assert_eq!((p.pts, p.frames), (1000, 3800));
    assert!(p.data == data[1000 * 6..]);
    let r = d.seek(0, 99_999).unwrap();
    assert_eq!((r.frame, r.preroll), (4800, 0));
    assert!(!d.next_packet(&mut p).unwrap());
    d.seek(0, 0).unwrap();
    assert!(d.next_packet(&mut p).unwrap());
    assert_eq!(p.pts, 0);
    assert!(matches!(d.seek(1, 0), Err(Error::Invalid { .. })));
}

#[test]
fn packet_buffer_is_reused() {
    let s = s16();
    let mut d = open(
        aiff(s, RATE, 2, &samples(s, 2, 9000), None),
        &Options::default(),
    )
    .unwrap();
    let mut p = Packet::default();
    assert!(d.next_packet(&mut p).unwrap());
    let ptr = p.data.as_ptr();
    assert!(d.next_packet(&mut p).unwrap());
    assert_eq!(p.data.as_ptr(), ptr);
}

#[test]
fn empty_data_is_valid() {
    let mut d = open(aiff(s16(), RATE, 2, &[], None), &Options::default()).unwrap();
    assert_eq!(d.tracks()[0].frames, Some(0));
    assert!(read_all(&mut d).unwrap().is_empty());
}

#[test]
fn byte_flip_and_truncation_smoke() {
    let data = samples(s16(), 2, 64);
    for comp in [None, Some(*b"NONE"), Some(*b"sowt")] {
        let file = aiff(s16(), RATE, 2, &data, comp);
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
