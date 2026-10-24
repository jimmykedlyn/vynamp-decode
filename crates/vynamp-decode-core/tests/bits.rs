use vynamp_decode_core::{BitReader, Error, crc8, crc16};

fn invalid(what: &'static str, offset: u64) -> Error {
    Error::Invalid { what, offset }
}

#[test]
fn read_bits_across_byte_boundaries() {
    let data = [0b1010_1100, 0b0101_0011];
    let mut r = BitReader::new(&data);
    assert_eq!(r.read_bits(3).unwrap(), 0b101);
    assert_eq!(r.read_bits(7).unwrap(), 0b0110001);
    assert_eq!(r.byte_pos(), 1);
    assert_eq!(r.bits_left(), 6);
    assert_eq!(r.read_bits(0).unwrap(), 0);
    assert_eq!(r.read_bits(6).unwrap(), 0b010011);
    assert_eq!(r.bits_left(), 0);
}

#[test]
fn read_bits_wide_and_u64() {
    let data = [0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0, 0x11, 0x22];
    let mut r = BitReader::new(&data);
    assert_eq!(r.read_bits(32).unwrap(), 0x1234_5678);
    assert_eq!(r.read_bits_u64(40).unwrap(), 0x9A_BCDE_F011);
    let mut r = BitReader::new(&data);
    assert_eq!(r.read_bits(4).unwrap(), 1);
    assert_eq!(r.read_bits_u64(64).unwrap(), 0x2345_6789_ABCD_EF01);
    assert_eq!(r.bits_left(), 12);
}

#[test]
fn bit_width_errors() {
    let data = [0u8; 16];
    let mut r = BitReader::new(&data);
    assert_eq!(r.read_bits(33), Err(invalid("bit width", 0)));
    assert_eq!(r.read_bits_u64(65), Err(invalid("bit width", 0)));
    assert_eq!(r.read_signed(0), Err(invalid("bit width", 0)));
    assert_eq!(r.read_signed(33), Err(invalid("bit width", 0)));
    assert_eq!(r.read_signed_i64(0), Err(invalid("bit width", 0)));
    assert_eq!(r.read_signed_i64(34), Err(invalid("bit width", 0)));
    assert_eq!(r.read_rice_signed(33), Err(invalid("bit width", 0)));
    assert_eq!(r.bits_left(), 128);
}

#[test]
fn eof_reports_byte_offset_and_does_not_consume() {
    let data = [0xFF, 0xFF];
    let mut r = BitReader::new(&data);
    assert_eq!(r.read_bits(12).unwrap(), 0xFFF);
    assert_eq!(r.read_bits(5), Err(invalid("bitstream eof", 1)));
    assert_eq!(r.bits_left(), 4);
    assert_eq!(r.read_bits(4).unwrap(), 0xF);
    assert_eq!(r.read_bits(1), Err(invalid("bitstream eof", 2)));
    assert_eq!(r.read_bits_u64(40), Err(invalid("bitstream eof", 2)));
    let mut empty = BitReader::new(&[]);
    assert_eq!(empty.read_bits(1), Err(invalid("bitstream eof", 0)));
    assert_eq!(empty.read_unary(), Err(invalid("bitstream eof", 0)));
    assert_eq!(empty.read_utf8_u64(), Err(invalid("bitstream eof", 0)));
    assert_eq!(empty.read_bits(0).unwrap(), 0);
}

#[test]
fn signed_reads() {
    let mut r = BitReader::new(&[0b1111_0111, 0b1000_0000]);
    assert_eq!(r.read_signed(4).unwrap(), -1);
    assert_eq!(r.read_signed(4).unwrap(), 7);
    assert_eq!(r.read_signed(1).unwrap(), -1);
    assert_eq!(r.read_signed(7).unwrap(), 0);
    let mut r = BitReader::new(&[0x80, 0, 0, 0]);
    assert_eq!(r.read_signed(32).unwrap(), i32::MIN);
    let mut r = BitReader::new(&[0xFF; 5]);
    assert_eq!(r.read_signed_i64(33).unwrap(), -1);
    let mut r = BitReader::new(&[0x80, 0, 0, 0, 0]);
    assert_eq!(r.read_signed_i64(33).unwrap(), -(1i64 << 32));
    let mut r = BitReader::new(&[0x7F, 0xFF, 0xFF, 0xFF, 0x80]);
    assert_eq!(r.read_signed_i64(33).unwrap(), (1i64 << 32) - 1);
}

#[test]
fn unary_counts_zeros() {
    let mut r = BitReader::new(&[0b0001_0000, 0b1000_0000, 0b0000_0000, 0b1000_0000]);
    assert_eq!(r.read_unary().unwrap(), 3);
    assert_eq!(r.read_bits(4).unwrap(), 0);
    assert_eq!(r.read_unary().unwrap(), 0);
    assert_eq!(r.read_unary().unwrap(), 15);
    assert_eq!(r.bits_left(), 7);
}

#[test]
fn unary_across_many_words() {
    let mut data = vec![0u8; 100];
    data.push(0b0010_0000);
    let mut r = BitReader::new(&data);
    assert_eq!(r.read_unary().unwrap(), 802);
    assert_eq!(r.bits_left(), 5);
}

#[test]
fn unary_overflow_and_eof() {
    let mut data = vec![0u8; (1 << 20) / 8 + 8];
    data.push(1);
    let mut r = BitReader::new(&data);
    assert!(matches!(
        r.read_unary(),
        Err(Error::Invalid {
            what: "unary overflow",
            ..
        })
    ));
    // Exactly 2^20 zeros is still accepted.
    let mut data = vec![0u8; (1 << 20) / 8];
    data.push(0x80);
    assert_eq!(BitReader::new(&data).read_unary().unwrap(), 1 << 20);
    // Zeros to the end of the data.
    let mut r = BitReader::new(&[0, 0]);
    assert!(matches!(
        r.read_unary(),
        Err(Error::Invalid {
            what: "bitstream eof",
            ..
        })
    ));
}

#[test]
fn rice_signed() {
    // k=2: values 0, -1, 1, -2, 2 -> zigzag 0..=4 -> 100 101 110 111 0100.
    let mut r = BitReader::new(&[0b1001_0111, 0b0111_0100]);
    for want in [0, -1, 1, -2, 2] {
        assert_eq!(r.read_rice_signed(2).unwrap(), want);
    }
    assert_eq!(r.bits_left(), 0);
    // k=0: zigzag 5 -> unary 5 = 000001 -> -3.
    assert_eq!(
        BitReader::new(&[0b0000_0100]).read_rice_signed(0).unwrap(),
        -3
    );
}

#[test]
fn rice_extremes_and_overflow() {
    // k=31, q=1 (bits "01"), r=0x7FFFFFFF: u = 0xFFFF_FFFF -> i32::MIN.
    let mut r = BitReader::new(&[0b0111_1111, 0xFF, 0xFF, 0xFF, 0xC0]);
    assert_eq!(r.read_rice_signed(31).unwrap(), i32::MIN);
    // k=32, q=0 (bit "1"), r=0xFFFFFFFE -> u = 0xFFFF_FFFE -> i32::MAX.
    let mut r = BitReader::new(&[0b1111_1111, 0xFF, 0xFF, 0xFF, 0x00]);
    assert_eq!(r.read_rice_signed(32).unwrap(), i32::MAX);
    // k=32, q=1 does not fit in 32 bits.
    let mut r = BitReader::new(&[0b0100_0000, 0x00, 0x00, 0x00, 0x00, 0x00]);
    assert!(matches!(
        r.read_rice_signed(32),
        Err(Error::Invalid {
            what: "rice overflow",
            ..
        })
    ));
    // q = 2^20 with k = 12 overflows u32.
    let mut data = vec![0u8; (1 << 20) / 8];
    data.extend([0x80, 0, 0]);
    let mut r = BitReader::new(&data);
    assert!(matches!(
        r.read_rice_signed(12),
        Err(Error::Invalid {
            what: "rice overflow",
            ..
        })
    ));
}

#[test]
fn utf8_numbers() {
    let cases: &[(&[u8], u64)] = &[
        (&[0x00], 0),
        (&[0x7F], 0x7F),
        (&[0xC2, 0x80], 0x80),
        (&[0xDF, 0xBF], 0x7FF),
        (&[0xE0, 0xA0, 0x80], 0x800),
        (&[0xEF, 0xBF, 0xBF], 0xFFFF),
        (&[0xF0, 0x90, 0x80, 0x80], 0x1_0000),
        (&[0xFE, 0x83, 0xBF, 0xBF, 0xBF, 0xBF, 0xBF], 0xFFFF_FFFF),
        (&[0xFE, 0xBF, 0xBF, 0xBF, 0xBF, 0xBF, 0xBF], 0xF_FFFF_FFFF),
        (&[0xFE, 0x82, 0x80, 0x80, 0x80, 0x80, 0x80], 0x8000_0000),
    ];
    for (bytes, want) in cases {
        let mut r = BitReader::new(bytes);
        assert_eq!(r.read_utf8_u64().unwrap(), *want, "{bytes:02X?}");
        assert_eq!(r.bits_left(), 0);
    }
}

#[test]
fn utf8_number_malformed() {
    let bad: &[&[u8]] = &[
        &[0x80],
        &[0xBF],
        &[0xFF],
        &[0xC2, 0x00],
        &[0xC2, 0xC0],
        &[0xE0, 0xA0, 0x00],
        &[0xFE, 0xBF, 0xBF, 0xBF, 0xBF, 0xBF, 0x3F],
    ];
    for bytes in bad {
        let mut r = BitReader::new(bytes);
        assert_eq!(
            r.read_utf8_u64(),
            Err(invalid("utf8 number", 0)),
            "{bytes:02X?}"
        );
    }
    // Truncated multi-byte sequence is an error, not a panic.
    let mut r = BitReader::new(&[0xE0, 0xA0]);
    assert!(r.read_utf8_u64().is_err());
}

#[test]
fn align_and_positions() {
    let mut r = BitReader::new(&[0xFF, 0xAA, 0x55]);
    r.align_byte();
    assert_eq!(r.byte_pos(), 0);
    assert_eq!(r.read_bits(3).unwrap(), 7);
    r.align_byte();
    assert_eq!(r.byte_pos(), 1);
    assert_eq!(r.bits_left(), 16);
    assert_eq!(r.read_bits(8).unwrap(), 0xAA);
    r.align_byte();
    assert_eq!(r.byte_pos(), 2);
    assert_eq!(r.read_bits(8).unwrap(), 0x55);
    assert_eq!(r.byte_pos(), 3);
}

#[test]
fn never_panics_on_garbage() {
    let mut x = 0x1234_5678u32;
    let mut data = vec![0u8; 257];
    for b in &mut data {
        x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        *b = (x >> 24) as u8;
    }
    for step in 0..2000u32 {
        let mut r = BitReader::new(&data[..(step as usize) % 258]);
        for i in 0..200u32 {
            let n = (step.wrapping_mul(31) ^ i.wrapping_mul(7)) % 70;
            let _ = match i % 6 {
                0 => r.read_bits(n).map(u64::from),
                1 => r.read_bits_u64(n),
                2 => r.read_signed(n).map(|v| v as u64),
                3 => r.read_signed_i64(n).map(|v| v as u64),
                4 => r
                    .read_unary()
                    .map(u64::from)
                    .and_then(|_| r.read_utf8_u64()),
                _ => r.read_rice_signed(n % 34).map(|v| v as u64),
            };
            if i % 11 == 0 {
                r.align_byte();
            }
            assert!(r.byte_pos() <= data.len());
        }
    }
}

#[test]
fn crc8_check_value() {
    assert_eq!(crc8(b"123456789"), 0xF4);
    assert_eq!(crc8(b""), 0);
}

#[test]
fn crc16_check_value() {
    assert_eq!(crc16(b"123456789"), 0xFEE8);
    assert_eq!(crc16(b""), 0);
}

#[test]
fn crc_of_message_with_appended_crc_is_zero() {
    let msg = b"\xFF\xF8\x69\x18\x00\x00";
    let mut with8 = msg.to_vec();
    with8.push(crc8(msg));
    assert_eq!(crc8(&with8), 0);
    let mut with16 = msg.to_vec();
    with16.extend(crc16(msg).to_be_bytes());
    assert_eq!(crc16(&with16), 0);
}
