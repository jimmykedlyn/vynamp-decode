use vynamp_decode_core::{
    AudioBuffer, CodecId, Decoder, Endian, Error, Packet, PcmEncoding, PcmSpec, SampleFormat, Track,
};
use vynamp_decode_pcm::PcmDecoder;
use vynamp_decode_testkit::alloc::{CountingAlloc, allocations_on_this_thread};

#[global_allocator]
static A: CountingAlloc = CountingAlloc;

fn track(encoding: PcmEncoding, bits: u8, endian: Endian) -> Track {
    Track {
        id: 0,
        codec: CodecId::Pcm(PcmSpec {
            encoding,
            bits,
            valid_bits: bits,
            endian,
        }),
        sample_rate: 44100,
        channels: 2,
        channel_mask: 0,
        frames: None,
        delay: 0,
        padding: 0,
        codec_private: Vec::new(),
    }
}

fn packet(data: Vec<u8>, frames: u32) -> Packet {
    Packet {
        track: 0,
        pts: 0,
        frames,
        data,
    }
}

fn decode(t: &Track, data: Vec<u8>) -> AudioBuffer {
    let mut d = PcmDecoder::new(t).unwrap();
    let mut out = AudioBuffer::new();
    d.decode(&packet(data, 3), &mut out).unwrap();
    assert_eq!((out.channels(), out.frames()), (2, 3));
    out
}

/// Encodes interleaved L0 R0 L1 R1 L2 R2 from per-sample byte arrays (little endian
/// order), reversing each sample for big endian.
fn encode<const N: usize>(l: [[u8; N]; 3], r: [[u8; N]; 3], endian: Endian) -> Vec<u8> {
    let mut data = Vec::new();
    for i in 0..3 {
        for mut s in [l[i], r[i]] {
            if endian == Endian::Big {
                s.reverse();
            }
            data.extend(s);
        }
    }
    data
}

#[test]
fn u8_direct() {
    let t = track(PcmEncoding::Unsigned, 8, Endian::Little);
    let out = decode(&t, vec![0, 255, 128, 128, 255, 0]);
    assert_eq!(out.format(), SampleFormat::U8);
    assert_eq!(out.plane_u8(0).unwrap(), &[0, 128, 255]);
    assert_eq!(out.plane_u8(1).unwrap(), &[255, 128, 0]);
}

#[test]
fn s8_flips_sign_bit() {
    let t = track(PcmEncoding::Signed, 8, Endian::Little);
    let out = decode(&t, vec![0x80, 0x7F, 0x00, 0x00, 0x7F, 0x80]);
    assert_eq!(out.format(), SampleFormat::U8);
    assert_eq!(out.plane_u8(0).unwrap(), &[0, 128, 255]);
    assert_eq!(out.plane_u8(1).unwrap(), &[255, 128, 0]);
}

#[test]
fn s16_both_endians() {
    let l = [i16::MIN, 0, i16::MAX];
    let r = [i16::MAX, 0, i16::MIN];
    for endian in [Endian::Little, Endian::Big] {
        let data = encode(l.map(i16::to_le_bytes), r.map(i16::to_le_bytes), endian);
        let out = decode(&track(PcmEncoding::Signed, 16, endian), data);
        assert_eq!(out.format(), SampleFormat::I16);
        assert_eq!(out.plane_i16(0).unwrap(), &l);
        assert_eq!(out.plane_i16(1).unwrap(), &r);
    }
}

#[test]
fn s24_both_endians_sign_extend() {
    let l = [-8_388_608i32, 0, 8_388_607];
    let r = [8_388_607i32, -1, -8_388_608];
    let three = |v: i32| {
        let b = v.to_le_bytes();
        [b[0], b[1], b[2]]
    };
    for endian in [Endian::Little, Endian::Big] {
        let data = encode(l.map(three), r.map(three), endian);
        let out = decode(&track(PcmEncoding::Signed, 24, endian), data);
        assert_eq!(out.format(), SampleFormat::I24);
        assert_eq!(out.plane_i32(0).unwrap(), &l);
        assert_eq!(out.plane_i32(1).unwrap(), &r);
    }
}

#[test]
fn s32_both_endians() {
    let l = [i32::MIN, 0, i32::MAX];
    let r = [i32::MAX, 1, i32::MIN];
    for endian in [Endian::Little, Endian::Big] {
        let data = encode(l.map(i32::to_le_bytes), r.map(i32::to_le_bytes), endian);
        let out = decode(&track(PcmEncoding::Signed, 32, endian), data);
        assert_eq!(out.format(), SampleFormat::I32);
        assert_eq!(out.plane_i32(0).unwrap(), &l);
        assert_eq!(out.plane_i32(1).unwrap(), &r);
    }
}

#[test]
fn f32_both_endians() {
    let l = [-1.0f32, 0.0, 0.5];
    let r = [0.5f32, 0.0, -1.0];
    for endian in [Endian::Little, Endian::Big] {
        let data = encode(l.map(f32::to_le_bytes), r.map(f32::to_le_bytes), endian);
        let out = decode(&track(PcmEncoding::Float, 32, endian), data);
        assert_eq!(out.format(), SampleFormat::F32);
        assert_eq!(out.plane_f32(0).unwrap(), &l);
        assert_eq!(out.plane_f32(1).unwrap(), &r);
    }
}

#[test]
fn f64_both_endians() {
    let l = [-1.0f64, 0.0, 0.5];
    let r = [0.5f64, 0.0, -1.0];
    for endian in [Endian::Little, Endian::Big] {
        let data = encode(l.map(f64::to_le_bytes), r.map(f64::to_le_bytes), endian);
        let out = decode(&track(PcmEncoding::Float, 64, endian), data);
        assert_eq!(out.format(), SampleFormat::F64);
        assert_eq!(out.plane_f64(0).unwrap(), &l);
        assert_eq!(out.plane_f64(1).unwrap(), &r);
    }
}

#[test]
fn valid_bits_does_not_change_samples() {
    let mut t = track(PcmEncoding::Signed, 16, Endian::Little);
    if let CodecId::Pcm(s) = &mut t.codec {
        s.valid_bits = 12;
    }
    let out = decode(&t, vec![0xFF, 0x7F, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(out.plane_i16(0).unwrap(), &[i16::MAX, 0, 0]);
}

#[test]
fn wrong_packet_size_is_invalid() {
    let t = track(PcmEncoding::Signed, 16, Endian::Little);
    let mut d = PcmDecoder::new(&t).unwrap();
    let mut out = AudioBuffer::new();
    let invalid = Error::Invalid {
        what: "pcm packet size",
        offset: 0,
    };
    for len in [0usize, 11, 13] {
        assert_eq!(
            d.decode(&packet(vec![0; len], 3), &mut out),
            Err(invalid.clone())
        );
    }
    // A huge frame count must not panic or allocate.
    assert_eq!(
        d.decode(&packet(vec![0; 4], u32::MAX), &mut out),
        Err(invalid)
    );
}

#[test]
fn unsupported_specs() {
    let unsupported = Error::Unsupported("pcm spec");
    for (enc, bits) in [
        (PcmEncoding::Signed, 12),
        (PcmEncoding::Unsigned, 16),
        (PcmEncoding::Signed, 64),
        (PcmEncoding::Float, 16),
        (PcmEncoding::Float, 24),
    ] {
        let t = track(enc, bits, Endian::Little);
        assert_eq!(PcmDecoder::new(&t).err(), Some(unsupported.clone()));
    }
    let mut t = track(PcmEncoding::Signed, 16, Endian::Little);
    t.codec = CodecId::Flac;
    assert_eq!(PcmDecoder::new(&t).err(), Some(unsupported));
}

#[test]
fn no_allocations_after_warmup() {
    let t = track(PcmEncoding::Signed, 24, Endian::Little);
    let mut d = PcmDecoder::new(&t).unwrap();
    let mut out = AudioBuffer::new();
    let p = packet(vec![1; 2 * 3 * 1152], 1152);
    d.decode(&p, &mut out).unwrap();
    let before = allocations_on_this_thread();
    for _ in 0..100 {
        d.decode(&p, &mut out).unwrap();
    }
    assert_eq!(allocations_on_this_thread(), before);
}
