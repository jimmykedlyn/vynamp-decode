//! Hand-assembled FLAC frames (RFC 9639 sections 9.2 and 9.3) through `FlacDecoder`.

use vynamp_decode_core::{
    AudioBuffer, CodecId, Decoder, Error, GaplessKind, Packet, SampleFormat, Track, crc8, crc16,
};
use vynamp_decode_flac::FlacDecoder;
use vynamp_decode_testkit::alloc::{CountingAlloc, allocations_on_this_thread};

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

const BASE: u64 = 1000;

/// Channel code, first subframe samples and depth, second subframe samples and depth.
type Case<'a> = (u8, &'a [i64], u32, &'a [i64], u32);

// ---------------------------------------------------------------- helpers

#[derive(Default)]
struct Bits {
    bytes: Vec<u8>,
    nbits: usize,
}

impl Bits {
    fn put(&mut self, value: u64, n: u32) {
        for i in (0..n).rev() {
            if self.nbits.is_multiple_of(8) {
                self.bytes.push(0);
            }
            if (value >> i) & 1 == 1 {
                *self.bytes.last_mut().unwrap() |= 0x80 >> (self.nbits % 8);
            }
            self.nbits += 1;
        }
    }
    fn signed(&mut self, value: i64, n: u32) {
        self.put((value as u64) & ((1u64 << n) - 1), n);
    }
    fn unary(&mut self, q: u32) {
        for _ in 0..q {
            self.put(0, 1);
        }
        self.put(1, 1);
    }
    fn rice(&mut self, v: i32, k: u32) {
        let u = ((i64::from(v) << 1) ^ (i64::from(v) >> 31)) as u32;
        self.unary(u >> k);
        self.put(u64::from(u) & ((1u64 << k) - 1), k);
    }
}

fn info_body(channels: u8, bps: u8) -> Vec<u8> {
    let mut b = Bits::default();
    b.put(16, 16);
    b.put(4096, 16);
    b.put(0, 24);
    b.put(0, 24);
    b.put(44_100, 20);
    b.put(u64::from(channels) - 1, 3);
    b.put(u64::from(bps) - 1, 5);
    b.put(0, 36);
    b.put(0, 64);
    b.put(0, 64);
    b.bytes
}

fn track(channels: u8, bps: u8) -> Track {
    Track {
        id: 0,
        codec: CodecId::Flac,
        sample_rate: 44_100,
        channels: u16::from(channels),
        channel_mask: 0,
        frames: None,
        delay: 0,
        padding: 0,
        gapless: GaplessKind::Exact,
        codec_private: info_body(channels, bps),
    }
}

fn bps_code(bps: u8) -> u8 {
    match bps {
        8 => 1,
        12 => 2,
        16 => 4,
        20 => 5,
        24 => 6,
        32 => 7,
        _ => 0,
    }
}

/// Header (fixed blocking, frame 0, 16-bit block size, rate and depth from STREAMINFO)
/// followed by `body` and the CRC-16.
fn frame(ch_code: u8, bps: u8, bs: usize, body: &Bits) -> Vec<u8> {
    let mut h = vec![
        0xFF,
        0xF8,
        0x70,
        (ch_code << 4) | (bps_code(bps) << 1),
        0x00,
        ((bs - 1) >> 8) as u8,
        ((bs - 1) & 0xFF) as u8,
    ];
    h.push(crc8(&h));
    h.extend_from_slice(&body.bytes);
    h.extend_from_slice(&crc16(&h).to_be_bytes());
    h
}

fn packet(data: Vec<u8>, frames: u32) -> Packet {
    Packet {
        track: 0,
        pts: 0,
        frames,
        offset: BASE,
        data,
    }
}

fn decode(channels: u8, bps: u8, data: Vec<u8>, frames: usize) -> Result<AudioBuffer, Error> {
    let mut d = FlacDecoder::new(&track(channels, bps)).unwrap();
    let mut out = AudioBuffer::new();
    d.decode(&packet(data, frames as u32), &mut out)?;
    Ok(out)
}

fn i16s(b: &AudioBuffer, c: usize) -> Vec<i32> {
    b.plane_i16(c)
        .unwrap()
        .iter()
        .map(|&v| i32::from(v))
        .collect()
}

fn narrow(s: &[i64]) -> Vec<i32> {
    s.iter().map(|&v| v as i32).collect()
}

fn invalid(r: Result<AudioBuffer, Error>) -> (&'static str, u64) {
    match r {
        Err(Error::Invalid { what, offset }) => (what, offset),
        other => panic!("expected Invalid, got {other:?}"),
    }
}

/// Subframe header: zero bit, 6-bit type, wasted-bits flag (and unary k - 1).
fn sub_header(b: &mut Bits, kind: u32, wasted: u32) {
    b.put(0, 1);
    b.put(u64::from(kind), 6);
    if wasted == 0 {
        b.put(0, 1);
    } else {
        b.put(1, 1);
        b.unary(wasted - 1);
    }
}

/// Residual with a single partition (order 0) and Rice parameter `k`.
fn residual(b: &mut Bits, res: &[i32], method: u32, k: u32) {
    b.put(u64::from(method), 2);
    b.put(0, 4);
    b.put(u64::from(k), if method == 0 { 4 } else { 5 });
    for &r in res {
        b.rice(r, k);
    }
}

fn verbatim(b: &mut Bits, s: &[i64], bps: u32) {
    sub_header(b, 1, 0);
    for &v in s {
        b.signed(v, bps);
    }
}

/// Independent reference for the FIXED predictors (RFC 9639 section 9.2.5).
fn fixed_pred(s: &[i64], i: usize, order: usize) -> i64 {
    let g = |d: usize| s[i - d];
    match order {
        0 => 0,
        1 => g(1),
        2 => 2 * g(1) - g(2),
        3 => 3 * g(1) - 3 * g(2) + g(3),
        _ => 4 * g(1) - 6 * g(2) + 4 * g(3) - g(4),
    }
}

/// FIXED subframe of the given order with one Rice partition.
fn fixed_subframe(b: &mut Bits, s: &[i64], order: usize, bps: u32) {
    sub_header(b, 8 + order as u32, 0);
    for &w in &s[..order] {
        b.signed(w, bps);
    }
    let res: Vec<i32> = (order..s.len())
        .map(|i| (s[i] - fixed_pred(s, i, order)) as i32)
        .collect();
    residual(b, &res, 0, 10);
}

fn signal(n: usize, amp: i64) -> Vec<i64> {
    let mut x: u64 = 12345;
    (0..n)
        .map(|i| {
            x = x
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let noise = ((x >> 33) as i64 % 41) - 20;
            ((i as f64 * 0.07).sin() * amp as f64) as i64 + noise
        })
        .collect()
}

// ---------------------------------------------------------------- tests

#[test]
fn constant_subframe() {
    let mut b = Bits::default();
    sub_header(&mut b, 0, 0);
    b.signed(-1234, 16);
    let out = decode(1, 16, frame(0, 16, 20, &b), 20).unwrap();
    assert_eq!(out.format(), SampleFormat::I16);
    assert_eq!(out.frames(), 20);
    assert_eq!(i16s(&out, 0), vec![-1234; 20]);
}

#[test]
fn verbatim_subframe() {
    let s = signal(33, 30000);
    let mut b = Bits::default();
    verbatim(&mut b, &s, 16);
    let out = decode(1, 16, frame(0, 16, 33, &b), 33).unwrap();
    assert_eq!(i16s(&out, 0), narrow(&s));
}

#[test]
fn fixed_orders_0_to_4() {
    let s = signal(200, 20000);
    for order in 0..=4usize {
        let mut b = Bits::default();
        fixed_subframe(&mut b, &s, order, 16);
        let out = decode(1, 16, frame(0, 16, s.len(), &b), s.len()).unwrap();
        assert_eq!(i16s(&out, 0), narrow(&s), "fixed order {order}");
    }
}

#[test]
fn rice_method_1_and_partitions() {
    // 64 samples, partition order 2 (16 per partition), 5-bit parameters.
    let s = signal(64, 9000);
    let order = 2usize;
    let mut b = Bits::default();
    sub_header(&mut b, 8 + order as u32, 0);
    for &w in &s[..order] {
        b.signed(w, 16);
    }
    let res: Vec<i32> = (order..s.len())
        .map(|i| (s[i] - fixed_pred(&s, i, order)) as i32)
        .collect();
    b.put(1, 2);
    b.put(2, 4);
    let mut pos = 0;
    for (p, k) in [8u32, 9, 10, 11].into_iter().enumerate() {
        let n = if p == 0 { 16 - order } else { 16 };
        b.put(u64::from(k), 5);
        for &r in &res[pos..pos + n] {
            b.rice(r, k);
        }
        pos += n;
    }
    let out = decode(1, 16, frame(0, 16, 64, &b), 64).unwrap();
    assert_eq!(i16s(&out, 0), narrow(&s));
}

#[test]
fn first_partition_may_be_empty_when_it_holds_only_the_warm_up() {
    // 16 samples, partition order 3 (2 per partition), predictor order 2: the first
    // partition has no residual samples.
    let s = signal(16, 9000);
    let order = 2usize;
    let mut b = Bits::default();
    sub_header(&mut b, 8 + order as u32, 0);
    for &w in &s[..order] {
        b.signed(w, 16);
    }
    let res: Vec<i32> = (order..s.len())
        .map(|i| (s[i] - fixed_pred(&s, i, order)) as i32)
        .collect();
    b.put(0, 2);
    b.put(3, 4);
    b.put(5, 4); // the empty partition still carries a parameter
    let mut pos = 0;
    for _ in 1..8 {
        b.put(9, 4);
        for &r in &res[pos..pos + 2] {
            b.rice(r, 9);
        }
        pos += 2;
    }
    let out = decode(1, 16, frame(0, 16, 16, &b), 16).unwrap();
    assert_eq!(i16s(&out, 0), narrow(&s));
}

#[test]
fn lpc_order_2() {
    let s = signal(120, 15000);
    let (c0, c1, shift) = (3i64, -1i64, 1u32);
    let mut b = Bits::default();
    sub_header(&mut b, 31 + 2, 0);
    for &w in &s[..2] {
        b.signed(w, 16);
    }
    b.put(3, 4); // precision 4 bits
    b.signed(i64::from(shift), 5);
    b.signed(c0, 4);
    b.signed(c1, 4);
    let res: Vec<i32> = (2..s.len())
        .map(|i| (s[i] - ((c0 * s[i - 1] + c1 * s[i - 2]) >> shift)) as i32)
        .collect();
    residual(&mut b, &res, 0, 9);
    let out = decode(1, 16, frame(0, 16, s.len(), &b), s.len()).unwrap();
    assert_eq!(i16s(&out, 0), narrow(&s));
}

#[test]
fn rice_escape_partition() {
    let s = signal(40, 100);
    let mut b = Bits::default();
    sub_header(&mut b, 8, 0); // FIXED order 0: samples are the residual
    b.put(0, 2);
    b.put(0, 4);
    b.put(15, 4); // escape
    b.put(8, 5); // 8 bits per sample
    for &v in &s {
        b.signed(v, 8);
    }
    let out = decode(1, 16, frame(0, 16, 40, &b), 40).unwrap();
    assert_eq!(i16s(&out, 0), narrow(&s));

    // Escape with n = 0: all zeros.
    let mut b = Bits::default();
    sub_header(&mut b, 8, 0);
    b.put(1, 2);
    b.put(0, 4);
    b.put(31, 5);
    b.put(0, 5);
    let out = decode(1, 16, frame(0, 16, 40, &b), 40).unwrap();
    assert_eq!(i16s(&out, 0), vec![0; 40]);
}

fn verbatim_wasted(b: &mut Bits, s: &[i64], wasted: u32, bps: u32) {
    sub_header(b, 1, wasted);
    for &v in s {
        b.signed(v, bps - wasted);
    }
}

#[test]
fn wasted_bits() {
    // 3 wasted bits: stored values have 13 bits and are shifted left by 3.
    let vals: Vec<i64> = vec![-4096, -1, 0, 1, 4095, 17, -300, 5];
    let mut b = Bits::default();
    verbatim_wasted(&mut b, &vals, 3, 16);
    let out = decode(1, 16, frame(0, 16, vals.len(), &b), vals.len()).unwrap();
    let want: Vec<i32> = vals.iter().map(|&v| (v << 3) as i32).collect();
    assert_eq!(i16s(&out, 0), want);

    // Wasted bits equal to the bit depth are invalid.
    let mut b = Bits::default();
    verbatim_wasted(&mut b, &[0], 16, 16);
    assert_eq!(
        invalid(decode(1, 16, frame(0, 16, 1, &b), 1)).0,
        "flac wasted bits"
    );
}

#[test]
fn every_channel_assignment() {
    let l = signal(50, 12000);
    let r: Vec<i64> = l
        .iter()
        .enumerate()
        .map(|(i, &v)| (v / 2) - (i as i64 % 7) * 3)
        .collect();
    let side: Vec<i64> = l.iter().zip(&r).map(|(a, b)| a - b).collect();
    let mid: Vec<i64> = l.iter().zip(&r).map(|(a, b)| (a + b) >> 1).collect();

    let cases: [Case; 4] = [
        (1, &l, 16, &r, 16),
        (8, &l, 16, &side, 17),
        (9, &side, 17, &r, 16),
        (10, &mid, 16, &side, 17),
    ];
    for (code, a, abps, c, cbps) in cases {
        let mut b = Bits::default();
        verbatim(&mut b, a, abps);
        verbatim(&mut b, c, cbps);
        let out = decode(2, 16, frame(code, 16, 50, &b), 50).unwrap();
        assert_eq!(i16s(&out, 0), narrow(&l), "channel code {code} left");
        assert_eq!(i16s(&out, 1), narrow(&r), "channel code {code} right");
    }
}

#[test]
fn side_channel_32_bit_extremes() {
    // L = i32::MIN, R = i32::MAX: side = L - R needs 33 bits.
    let (l, r) = (i64::from(i32::MIN), i64::from(i32::MAX));
    let (side, mid) = (l - r, (l + r) >> 1);
    let cases: [(u8, i64, u32, i64, u32); 3] = [
        (8, l, 32, side, 33),
        (9, side, 33, r, 32),
        (10, mid, 32, side, 33),
    ];
    for (code, a, abps, c, cbps) in cases {
        let mut b = Bits::default();
        verbatim(&mut b, &[a, a], abps);
        verbatim(&mut b, &[c, c], cbps);
        let out = decode(2, 32, frame(code, 32, 2, &b), 2).unwrap();
        assert_eq!(out.format(), SampleFormat::I32);
        assert_eq!(out.plane_i32(0).unwrap(), &[i32::MIN; 2], "code {code}");
        assert_eq!(out.plane_i32(1).unwrap(), &[i32::MAX; 2], "code {code}");
    }
}

#[test]
fn output_formats() {
    let mut b = Bits::default();
    sub_header(&mut b, 0, 0);
    b.signed(-3, 8);
    let out = decode(1, 8, frame(0, 8, 4, &b), 4).unwrap();
    assert_eq!(out.plane_u8(0).unwrap(), &[125; 4]);

    let mut b = Bits::default();
    sub_header(&mut b, 0, 0);
    b.signed(-8_388_608, 24);
    let out = decode(1, 24, frame(0, 24, 4, &b), 4).unwrap();
    assert_eq!(out.format(), SampleFormat::I24);
    assert_eq!(out.plane_i32(0).unwrap(), &[-8_388_608; 4]);

    // 12 bits are carried unshifted in an I16 buffer.
    let mut b = Bits::default();
    sub_header(&mut b, 0, 0);
    b.signed(-2048, 12);
    let out = decode(1, 12, frame(0, 12, 4, &b), 4).unwrap();
    assert_eq!(out.format(), SampleFormat::I16);
    assert_eq!(out.plane_i16(0).unwrap(), &[-2048; 4]);
}

#[test]
fn silence_packet() {
    let mut d = FlacDecoder::new(&track(2, 16)).unwrap();
    let mut out = AudioBuffer::new();
    d.decode(&packet(Vec::new(), 77), &mut out).unwrap();
    assert_eq!((out.frames(), out.channels()), (77, 2));
    assert!(out.plane_i16(1).unwrap().iter().all(|&v| v == 0));

    let mut d = FlacDecoder::new(&track(1, 8)).unwrap();
    d.decode(&packet(Vec::new(), 5), &mut out).unwrap();
    assert_eq!(out.plane_u8(0).unwrap(), &[128; 5]);
}

#[test]
fn rejects_malformed_subframes() {
    // Reserved subframe types: 2, 13 (fixed order 5), 16, 31.
    for kind in [2u32, 13, 16, 31] {
        let mut b = Bits::default();
        sub_header(&mut b, kind, 0);
        b.put(0, 64);
        assert_eq!(
            invalid(decode(1, 16, frame(0, 16, 8, &b), 8)).0,
            "flac subframe type"
        );
    }
    // Non-zero padding bit.
    let mut b = Bits::default();
    b.put(1, 1);
    b.put(0, 64);
    assert_eq!(
        invalid(decode(1, 16, frame(0, 16, 8, &b), 8)).0,
        "flac subframe header"
    );

    // LPC precision 0b1111.
    let mut b = Bits::default();
    sub_header(&mut b, 32, 0);
    b.signed(0, 16);
    b.put(15, 4);
    b.put(0, 64);
    assert_eq!(
        invalid(decode(1, 16, frame(0, 16, 8, &b), 8)).0,
        "flac lpc precision"
    );

    // Negative LPC shift.
    let mut b = Bits::default();
    sub_header(&mut b, 32, 0);
    b.signed(0, 16);
    b.put(3, 4);
    b.signed(-1, 5);
    b.put(0, 64);
    assert_eq!(
        invalid(decode(1, 16, frame(0, 16, 8, &b), 8)).0,
        "flac lpc shift"
    );

    // Reserved residual coding method.
    let mut b = Bits::default();
    sub_header(&mut b, 8, 0);
    b.put(2, 2);
    b.put(0, 64);
    assert_eq!(
        invalid(decode(1, 16, frame(0, 16, 8, &b), 8)).0,
        "flac residual method"
    );

    // Block size not divisible by the partition count (order 2, 10 samples).
    let mut b = Bits::default();
    sub_header(&mut b, 8, 0);
    b.put(0, 2);
    b.put(2, 4);
    b.put(0, 64);
    assert_eq!(
        invalid(decode(1, 16, frame(0, 16, 10, &b), 10)).0,
        "flac partition order"
    );

    // (block size >> order) must not be below the predictor order: 8 >> 3 = 1 < 2.
    let mut b = Bits::default();
    sub_header(&mut b, 10, 0);
    b.signed(0, 16);
    b.signed(0, 16);
    b.put(0, 2);
    b.put(3, 4);
    b.put(0, 64);
    assert_eq!(
        invalid(decode(1, 16, frame(0, 16, 8, &b), 8)).0,
        "flac partition order"
    );
}

#[test]
fn rejects_out_of_range_samples() {
    // FIXED-1: 32767 + 1 leaves the 16-bit range.
    let mut b = Bits::default();
    sub_header(&mut b, 9, 0);
    b.signed(32767, 16);
    residual(&mut b, &[1, 0, 0], 0, 0);
    assert_eq!(
        invalid(decode(1, 16, frame(0, 16, 4, &b), 4)).0,
        "flac sample range"
    );

    // Decorrelation: L = 32767 and S = -2 give R = 32769.
    let mut b = Bits::default();
    verbatim(&mut b, &[32767], 16);
    verbatim(&mut b, &[-2], 17);
    assert_eq!(
        invalid(decode(2, 16, frame(8, 16, 1, &b), 1)).0,
        "flac sample range"
    );
}

#[test]
fn residual_of_minus_2_pow_31_is_rejected() {
    // RFC 9639 section 9.2.7.3: "residual sample values MUST NOT have an absolute value
    // equal to, or larger than, 2 to the power 31". Rice k = 30, q = 3, r = 2^30 - 1
    // is the zigzag code 0xFFFFFFFF, i.e. -2^31.
    let mut b = Bits::default();
    sub_header(&mut b, 8, 0);
    b.put(1, 2);
    b.put(0, 4);
    b.put(30, 5);
    b.unary(3);
    b.put((1 << 30) - 1, 30);
    b.put(0, 64);
    assert_eq!(
        invalid(decode(1, 16, frame(0, 16, 1, &b), 1)).0,
        "flac residual range"
    );
}

#[test]
fn error_offsets_are_absolute() {
    // The header is 8 bytes; the truncated verbatim body fails inside the packet.
    let mut b = Bits::default();
    sub_header(&mut b, 1, 0);
    let mut data = frame(0, 16, 100, &b);
    data.truncate(10);
    let (what, offset) = invalid(decode(1, 16, data, 100));
    assert_eq!(what, "bitstream eof");
    assert!((BASE + 8..=BASE + 10).contains(&offset), "offset {offset}");

    // Header errors carry the packet offset as well.
    let (_, offset) = invalid(decode(1, 16, vec![0x12, 0x34, 0x56, 0x78], 4));
    assert_eq!(offset, BASE);
}

#[test]
fn no_allocation_after_warm_up() {
    let s = signal(512, 20000);
    let mut mono_body = Bits::default();
    fixed_subframe(&mut mono_body, &s, 2, 16);
    let mut st_body = Bits::default();
    verbatim(&mut st_body, &s, 16);
    fixed_subframe(&mut st_body, &s, 3, 17);
    let pa = packet(frame(0, 16, s.len(), &mono_body), 512);
    let pb = packet(frame(8, 16, s.len(), &st_body), 512);

    let mut mono = FlacDecoder::new(&track(1, 16)).unwrap();
    let mut st = FlacDecoder::new(&track(2, 16)).unwrap();
    let mut out = AudioBuffer::new();
    let mut out2 = AudioBuffer::new();
    mono.decode(&pa, &mut out).unwrap();
    st.decode(&pb, &mut out2).unwrap();
    st.decode(&packet(Vec::new(), 100), &mut out2).unwrap();
    let before = allocations_on_this_thread();
    for _ in 0..10 {
        mono.decode(&pa, &mut out).unwrap();
        st.decode(&pb, &mut out2).unwrap();
        st.decode(&packet(Vec::new(), 100), &mut out2).unwrap();
    }
    // `packet(Vec::new(), ..)` does not allocate: an empty Vec has no buffer.
    assert_eq!(allocations_on_this_thread(), before, "decode allocated");
}

#[test]
fn garbage_never_panics() {
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    // Pure noise: no valid header, always an error.
    let mut d = FlacDecoder::new(&track(2, 16)).unwrap();
    let mut out = AudioBuffer::new();
    for _ in 0..1000 {
        let len = 1 + (next() % 200) as usize;
        let data: Vec<u8> = (0..len).map(|_| next() as u8).collect();
        assert!(d.decode(&packet(data, 64), &mut out).is_err());
    }
    // Valid header (so the subframes are reached) followed by noise.
    let mut errors = 0;
    for &(channels, bps) in &[(2u8, 16u8), (2, 32), (1, 8), (2, 24)] {
        let mut d = FlacDecoder::new(&track(channels, bps)).unwrap();
        for i in 0..1000 {
            let bs = 1 + (next() % 300) as usize;
            let ch_code = if channels == 1 {
                0
            } else {
                [1u8, 8, 9, 10][i % 4]
            };
            let len = (next() % 400) as usize;
            let mut body = Bits {
                bytes: (0..len).map(|_| next() as u8).collect(),
                nbits: 0,
            };
            // Bias the first byte towards valid subframe types.
            if let Some(f) = body.bytes.first_mut() {
                *f = [0x00, 0x02, 0x10, 0x12, 0x40, 0x42, 0x44, 0x7E][next() as usize % 8];
            }
            let data = frame(ch_code, bps, bs, &body);
            if d.decode(&packet(data, bs as u32), &mut out).is_err() {
                errors += 1;
            }
        }
    }
    assert!(errors > 3000, "only {errors} of 4000 garbage frames failed");
}
