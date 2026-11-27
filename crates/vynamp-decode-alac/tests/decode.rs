//! ALAC decoder over hand-built frames: configuration checks, verbatim frames and errors.
//! The compressed path is proven bit-exact against FFmpeg-encoded fixtures in
//! `vynamp-decode/tests/mp4_alac.rs`.

use vynamp_decode_alac::AlacDecoder;
use vynamp_decode_core::{
    AudioBuffer, CodecId, Decoder, Error, Event, GaplessKind, Mode, Options, Packet, SampleFormat,
    Track,
};

fn config(bits: u8, channels: u8) -> Vec<u8> {
    let mut c = 4096u32.to_be_bytes().to_vec();
    c.extend_from_slice(&[0, bits, 40, 10, 14, channels]);
    c.extend_from_slice(&255u16.to_be_bytes());
    for v in [0u32, 0, 48_000] {
        c.extend_from_slice(&v.to_be_bytes());
    }
    c
}

fn track(codec_private: Vec<u8>, channels: u16) -> Track {
    Track {
        id: 1,
        codec: CodecId::Alac,
        sample_rate: 48_000,
        channels,
        channel_mask: 0,
        frames: None,
        delay: 0,
        padding: 0,
        gapless: GaplessKind::Exact,
        codec_private,
    }
}

/// MSB-first bit writer.
#[derive(Default)]
struct W {
    out: Vec<u8>,
    bits: u32,
}

impl W {
    fn put(&mut self, n: u32, v: u32) {
        for i in (0..n).rev() {
            if self.bits.is_multiple_of(8) {
                self.out.push(0);
            }
            let bit = ((v >> i) & 1) as u8;
            let last = self.out.len() - 1;
            self.out[last] |= bit << (7 - self.bits % 8);
            self.bits += 1;
        }
    }
}

/// A verbatim (uncompressed) frame: one element of `planes.len()` channels, `bits` per
/// sample, explicit sample count, then the END tag.
fn verbatim(bits: u32, planes: &[&[i32]]) -> Vec<u8> {
    let mut w = W::default();
    w.put(3, if planes.len() == 2 { 1 } else { 0 });
    w.put(16, 0);
    w.put(1, 1); // has size
    w.put(2, 0); // no extra bits
    w.put(1, 1); // not compressed
    w.put(32, planes[0].len() as u32);
    for i in 0..planes[0].len() {
        for p in planes {
            w.put(bits, p[i] as u32 & ((1u64 << bits) - 1) as u32);
        }
    }
    w.put(3, 7);
    w.out
}

fn packet(data: Vec<u8>, frames: u32) -> Packet {
    Packet {
        track: 1,
        pts: 4096,
        frames,
        offset: 1000,
        data,
    }
}

fn decoder(bits: u8, channels: u16, mode: Mode) -> AlacDecoder {
    let opts = Options {
        mode,
        ..Options::default()
    };
    AlacDecoder::new(&track(config(bits, channels as u8), channels), &opts).expect("decoder")
}

#[test]
fn verbatim_stereo_16_bit_frame_decodes_to_its_samples() {
    let l = [0, 1, -1, 32767, -32768];
    let r = [5, -6, 7, -8, 9];
    let mut d = decoder(16, 2, Mode::Strict);
    let mut buf = AudioBuffer::new();
    d.decode(&packet(verbatim(16, &[&l, &r]), 5), &mut buf)
        .unwrap();
    assert_eq!(buf.format(), SampleFormat::I16);
    assert_eq!(buf.frames(), 5);
    let want_l: Vec<i16> = l.iter().map(|&v| v as i16).collect();
    let want_r: Vec<i16> = r.iter().map(|&v| v as i16).collect();
    assert_eq!(buf.plane_i16(0).unwrap(), want_l.as_slice());
    assert_eq!(buf.plane_i16(1).unwrap(), want_r.as_slice());
}

#[test]
fn verbatim_mono_24_bit_frame_is_sign_extended_i24() {
    let m = [0, 8_388_607, -8_388_608, -1, 12345];
    let mut d = decoder(24, 1, Mode::Strict);
    let mut buf = AudioBuffer::new();
    d.decode(&packet(verbatim(24, &[&m]), 5), &mut buf).unwrap();
    assert_eq!(buf.format(), SampleFormat::I24);
    assert_eq!(buf.plane_i32(0).unwrap(), &m);
}

#[test]
fn truncated_frame_is_invalid_at_an_absolute_offset() {
    let mut f = verbatim(16, &[&[1, 2, 3, 4]]);
    f.truncate(f.len() - 3);
    let mut d = decoder(16, 1, Mode::Strict);
    let mut buf = AudioBuffer::new();
    match d.decode(&packet(f, 4), &mut buf) {
        Err(Error::Invalid { offset, .. }) => assert!(offset >= 1000, "offset {offset}"),
        other => panic!("expected Invalid, got {other:?}"),
    }
    assert_eq!(buf.frames(), 0);
}

#[test]
fn a_frame_without_end_tag_is_invalid() {
    let mut w = W::default();
    w.put(3, 0);
    w.put(16, 0);
    w.put(1, 1);
    w.put(2, 0);
    w.put(1, 1);
    w.put(32, 1);
    w.put(16, 7);
    let mut d = decoder(16, 1, Mode::Strict);
    let e = d
        .decode(&packet(w.out, 1), &mut AudioBuffer::new())
        .unwrap_err();
    assert!(matches!(e, Error::Invalid { .. }), "{e:?}");
}

#[test]
fn a_mono_element_in_a_stereo_stream_is_invalid() {
    let mut d = decoder(16, 2, Mode::Strict);
    let e = d
        .decode(
            &packet(verbatim(16, &[&[1, 2]]), 2),
            &mut AudioBuffer::new(),
        )
        .unwrap_err();
    assert!(matches!(e, Error::Invalid { .. }), "{e:?}");
}

#[test]
fn resilient_turns_a_bad_frame_into_silence_and_reports_it() {
    let mut d = decoder(16, 2, Mode::Resilient);
    let mut buf = AudioBuffer::new();
    d.decode(&packet(vec![0xFF; 3], 100), &mut buf).unwrap();
    assert_eq!(buf.frames(), 100);
    assert!(buf.plane_i16(0).unwrap().iter().all(|&s| s == 0));
    assert!(buf.plane_i16(1).unwrap().iter().all(|&s| s == 0));
    let mut ev = Vec::new();
    d.drain_events(&mut ev);
    assert_eq!(
        ev,
        vec![Event::CorruptFrame {
            pts: 4096,
            offset: 1000,
            frames: 100
        }]
    );
}

#[test]
fn configuration_errors_are_typed() {
    let opts = Options::default();
    let mk = |cfg: Vec<u8>, ch: u16| AlacDecoder::new(&track(cfg, ch), &opts).err();
    assert!(matches!(
        mk(config(16, 2)[..20].to_vec(), 2),
        Some(Error::Invalid { .. })
    ));
    assert_eq!(
        mk(config(20, 2), 2),
        Some(Error::Unsupported("alac bit depth"))
    );
    assert_eq!(
        mk(config(16, 3), 3),
        Some(Error::Unsupported("alac channels"))
    );
    assert!(matches!(mk(config(16, 2), 1), Some(Error::Invalid { .. })));
    let mut zero_len = config(16, 2);
    zero_len[..4].copy_from_slice(&[0; 4]);
    assert!(matches!(mk(zero_len, 2), Some(Error::Invalid { .. })));
    let mut other = track(config(16, 2), 2);
    other.codec = CodecId::Flac;
    assert!(AlacDecoder::new(&other, &opts).is_err());
}

/// Header of one element with an explicit sample count, as written by `verbatim`.
fn element_header(w: &mut W, tag: u32, samples: u32, compressed: bool) {
    w.put(3, tag);
    w.put(16, 0);
    w.put(1, 1); // has size
    w.put(2, 0); // no extra bits
    w.put(1, u32::from(!compressed));
    w.put(32, samples);
}

/// A compressed mono frame declaring `samples`, whose predictor uses `mode`, followed by
/// a single residual bit `0` and the END tag. With one sample that bit is a zero residual
/// (rice parameter 1, no zero run for the last sample), so the frame decodes to `[0]`.
fn compressed_mono(samples: u32, mode: u32) -> Vec<u8> {
    let mut w = W::default();
    element_header(&mut w, 0, samples, true);
    w.put(8, 0); // shift
    w.put(8, 0); // weight
    w.put(4, mode);
    w.put(4, 9); // quant
    w.put(3, 4); // mult
    w.put(5, 0); // order
    w.put(1, 0); // residual 0
    w.put(3, 7);
    w.out
}

fn expect_invalid(r: Result<(), Error>, what: &str) {
    match r {
        Err(Error::Invalid { what: got, .. }) => assert_eq!(got, what),
        other => panic!("expected Invalid({what}), got {other:?}"),
    }
}

#[test]
fn a_sample_count_the_packet_cannot_hold_is_rejected_before_decoding() {
    // The largest frame length the configuration allows: a forged count must not size the
    // per-channel buffers (16M samples each) before the packet is checked against it.
    let mut cfg = config(16, 1);
    cfg[..4].copy_from_slice(&(4096u32 * 4096).to_be_bytes());
    let mut d = AlacDecoder::new(&track(cfg, 1), &Options::default()).expect("decoder");
    let mut buf = AudioBuffer::new();
    let mut w = W::default();
    element_header(&mut w, 0, 4096 * 4096, false);
    w.put(16, 1);
    w.put(3, 7);
    expect_invalid(d.decode(&packet(w.out, 1), &mut buf), "alac frame size");
    expect_invalid(
        d.decode(&packet(compressed_mono(4096 * 4096, 0), 1), &mut buf),
        "alac frame size",
    );
}

#[test]
fn a_compressed_frame_with_zero_residuals_decodes_to_silence() {
    // The positive control of the two tests below: the same frame with prediction mode 0.
    let mut d = decoder(16, 1, Mode::Strict);
    let mut buf = AudioBuffer::new();
    d.decode(&packet(compressed_mono(1, 0), 1), &mut buf)
        .unwrap();
    assert_eq!(buf.plane_i16(0).unwrap(), &[0]);
}

#[test]
fn an_unknown_prediction_type_is_invalid_and_resilient_silence() {
    let mut d = decoder(16, 1, Mode::Strict);
    expect_invalid(
        d.decode(&packet(compressed_mono(1, 5), 1), &mut AudioBuffer::new()),
        "alac prediction type",
    );
    let mut d = decoder(16, 1, Mode::Resilient);
    let mut buf = AudioBuffer::new();
    d.decode(&packet(compressed_mono(1, 5), 1), &mut buf)
        .unwrap();
    assert_eq!(buf.plane_i16(0).unwrap(), &[0]);
    let mut ev = Vec::new();
    d.drain_events(&mut ev);
    assert_eq!(ev.len(), 1, "{ev:?}");
}

#[test]
fn an_unknown_element_tag_is_invalid_and_resilient_silence() {
    // Tag 2 (CCE) is not an ALAC audio element.
    let mut f = verbatim(16, &[&[1, 2]]);
    f[0] = (f[0] & 0x1F) | (2 << 5);
    let mut d = decoder(16, 1, Mode::Strict);
    expect_invalid(
        d.decode(&packet(f.clone(), 2), &mut AudioBuffer::new()),
        "alac element",
    );
    let mut d = decoder(16, 1, Mode::Resilient);
    let mut buf = AudioBuffer::new();
    d.decode(&packet(f, 2), &mut buf).unwrap();
    assert_eq!(buf.plane_i16(0).unwrap(), &[0; 2]);
    let mut ev = Vec::new();
    d.drain_events(&mut ev);
    assert_eq!(ev.len(), 1, "{ev:?}");
}
