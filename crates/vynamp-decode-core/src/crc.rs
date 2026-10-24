//! FLAC CRC-8 and CRC-16 (RFC 9639 section 9.1.8 and 9.3).

const fn make_table8(poly: u8) -> [u8; 256] {
    let mut table = [0u8; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u8;
        let mut bit = 0;
        while bit < 8 {
            c = if c & 0x80 != 0 {
                (c << 1) ^ poly
            } else {
                c << 1
            };
            bit += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
}

const fn make_table16(poly: u16) -> [u16; 256] {
    let mut table = [0u16; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = (i as u16) << 8;
        let mut bit = 0;
        while bit < 8 {
            c = if c & 0x8000 != 0 {
                (c << 1) ^ poly
            } else {
                c << 1
            };
            bit += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
}

static TABLE8: [u8; 256] = make_table8(0x07);
static TABLE16: [u16; 256] = make_table16(0x8005);

/// CRC-8 as used by FLAC frame headers: polynomial `0x07`, initial value 0,
/// MSB-first, no reflection, no final XOR.
///
/// ```
/// assert_eq!(vynamp_decode_core::crc8(b"123456789"), 0xF4);
/// ```
pub fn crc8(data: &[u8]) -> u8 {
    data.iter()
        .fold(0u8, |crc, &b| TABLE8[usize::from(crc ^ b)])
}

/// CRC-16 as used by FLAC frame footers: polynomial `0x8005`, initial value 0,
/// MSB-first, no reflection, no final XOR.
///
/// ```
/// assert_eq!(vynamp_decode_core::crc16(b"123456789"), 0xFEE8);
/// ```
pub fn crc16(data: &[u8]) -> u16 {
    crc16_update(0, data)
}

/// Continues a CRC-16 over more data: `crc16_update(crc16(a), b) == crc16(a ++ b)`.
///
/// ```
/// use vynamp_decode_core::{crc16, crc16_update};
/// assert_eq!(crc16_update(crc16(b"1234"), b"56789"), crc16(b"123456789"));
/// ```
pub fn crc16_update(crc: u16, data: &[u8]) -> u16 {
    data.iter().fold(crc, |crc, &b| {
        (crc << 8) ^ TABLE16[usize::from((crc >> 8) as u8 ^ b)]
    })
}
