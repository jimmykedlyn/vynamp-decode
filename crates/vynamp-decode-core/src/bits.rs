//! MSB-first bit reader.

use crate::{Error, Result};

/// Maximum number of zero bits accepted by [`BitReader::read_unary`].
const MAX_UNARY: u32 = 1 << 20;

/// An MSB-first bit reader over a byte slice.
///
/// All reads are bounds-checked: reading past the end returns
/// `Error::Invalid { what: "bitstream eof", offset }` where `offset` is the
/// byte index inside the slice (callers add the slice's own offset). A failed
/// read consumes nothing, except for [`read_unary`](Self::read_unary) and
/// [`read_rice_signed`](Self::read_rice_signed), which may consume bits before
/// reporting the error.
#[derive(Debug, Clone)]
pub struct BitReader<'a> {
    data: &'a [u8],
    /// Index of the next byte to load into the cache.
    next: usize,
    /// Unconsumed bits, left-aligned. Bits below `cache_bits` are zero.
    cache: u64,
    /// Number of valid bits in `cache` (0..=64).
    cache_bits: u32,
}

impl<'a> BitReader<'a> {
    /// Creates a reader positioned at the first bit of `data`.
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            next: 0,
            cache: 0,
            cache_bits: 0,
        }
    }

    /// Tops the cache up bytewise until it holds more than 56 bits or the
    /// data is exhausted.
    #[inline]
    fn refill(&mut self) {
        while self.cache_bits <= 56 {
            let Some(&b) = self.data.get(self.next) else {
                break;
            };
            self.cache |= u64::from(b) << (56 - self.cache_bits);
            self.cache_bits += 8;
            self.next += 1;
        }
    }

    #[inline]
    fn consume(&mut self, n: u32) {
        debug_assert!(n <= self.cache_bits);
        // `n == 64` would overflow the shift; the cache is then fully empty.
        self.cache = self.cache.checked_shl(n).unwrap_or(0);
        self.cache_bits -= n;
    }

    fn err(&self, what: &'static str) -> Error {
        Error::Invalid {
            what,
            offset: self.byte_pos() as u64,
        }
    }

    /// Reads `n` (0..=32) bits, MSB first.
    ///
    /// # Errors
    /// `bit width` if `n > 32`; `bitstream eof` if fewer than `n` bits remain.
    #[inline]
    pub fn read_bits(&mut self, n: u32) -> Result<u32> {
        if n > 32 {
            return Err(self.err("bit width"));
        }
        if n == 0 {
            return Ok(0);
        }
        if self.cache_bits < n {
            self.refill();
            if self.cache_bits < n {
                return Err(self.err("bitstream eof"));
            }
        }
        let v = (self.cache >> (64 - n)) as u32;
        self.consume(n);
        Ok(v)
    }

    /// Reads `n` (0..=64) bits, MSB first.
    ///
    /// # Errors
    /// `bit width` if `n > 64`; `bitstream eof` if fewer than `n` bits remain
    /// (nothing is consumed).
    pub fn read_bits_u64(&mut self, n: u32) -> Result<u64> {
        if n > 64 {
            return Err(self.err("bit width"));
        }
        if n <= 32 {
            return self.read_bits(n).map(u64::from);
        }
        if self.bits_left() < u64::from(n) {
            return Err(self.err("bitstream eof"));
        }
        let hi = self.read_bits(n - 32)?;
        let lo = self.read_bits(32)?;
        Ok((u64::from(hi) << 32) | u64::from(lo))
    }

    /// Reads an `n`-bit (1..=32) two's-complement signed value.
    ///
    /// # Errors
    /// `bit width` if `n` is 0 or above 32; `bitstream eof` on truncation.
    #[inline]
    pub fn read_signed(&mut self, n: u32) -> Result<i32> {
        if n == 0 || n > 32 {
            return Err(self.err("bit width"));
        }
        let v = self.read_bits(n)?;
        let shift = 32 - n;
        Ok(((v << shift) as i32) >> shift)
    }

    /// Reads an `n`-bit (1..=33) two's-complement signed value.
    ///
    /// # Errors
    /// `bit width` if `n` is 0 or above 33; `bitstream eof` on truncation.
    pub fn read_signed_i64(&mut self, n: u32) -> Result<i64> {
        if n == 0 || n > 33 {
            return Err(self.err("bit width"));
        }
        let v = self.read_bits_u64(n)?;
        let shift = 64 - n;
        Ok(((v << shift) as i64) >> shift)
    }

    /// Reads a unary-coded value: the number of `0` bits before the next `1`
    /// bit (which is consumed).
    ///
    /// # Errors
    /// `unary overflow` if more than 2^20 zeros precede the `1`;
    /// `bitstream eof` if the data ends first.
    #[inline]
    pub fn read_unary(&mut self) -> Result<u32> {
        let mut zeros = 0u32;
        loop {
            if self.cache_bits < 64 {
                self.refill();
            }
            if self.cache_bits == 0 {
                return Err(self.err("bitstream eof"));
            }
            let lz = self.cache.leading_zeros();
            if lz < self.cache_bits {
                zeros += lz;
                if zeros > MAX_UNARY {
                    return Err(self.err("unary overflow"));
                }
                self.consume(lz + 1);
                return Ok(zeros);
            }
            // The whole cache is zeros: drop it and keep counting.
            zeros += self.cache_bits;
            if zeros > MAX_UNARY {
                return Err(self.err("unary overflow"));
            }
            self.cache = 0;
            self.cache_bits = 0;
        }
    }

    /// Reads a Rice-coded signed value with parameter `k` (0..=32): a unary
    /// quotient, `k` remainder bits, and a zigzag-decoded sign.
    ///
    /// # Errors
    /// `bit width` if `k > 32`; `rice overflow` if the unsigned value does not
    /// fit in 32 bits; plus the errors of [`read_unary`](Self::read_unary).
    #[inline]
    pub fn read_rice_signed(&mut self, k: u32) -> Result<i32> {
        if k > 32 {
            return Err(self.err("bit width"));
        }
        let q = self.read_unary()?;
        let r = self.read_bits(k)?;
        // q <= 2^20 and k <= 32, so the shift cannot overflow a u64.
        let u = (u64::from(q) << k) | u64::from(r);
        let Ok(u) = u32::try_from(u) else {
            return Err(self.err("rice overflow"));
        };
        Ok(((u >> 1) as i32) ^ -((u & 1) as i32))
    }

    /// Reads a FLAC "UTF-8"-coded number (up to 7 bytes, 36 bits).
    ///
    /// Overlong encodings are accepted, like the reference decoder.
    ///
    /// # Errors
    /// `utf8 number` for an invalid lead or continuation byte;
    /// `bitstream eof` on truncation.
    pub fn read_utf8_u64(&mut self) -> Result<u64> {
        let start = self.byte_pos() as u64;
        let bad = Error::Invalid {
            what: "utf8 number",
            offset: start,
        };
        let lead = self.read_bits(8)?;
        let extra = match lead {
            0x00..=0x7F => return Ok(u64::from(lead)),
            0xC0..=0xDF => 1,
            0xE0..=0xEF => 2,
            0xF0..=0xF7 => 3,
            0xF8..=0xFB => 4,
            0xFC..=0xFD => 5,
            0xFE => 6,
            _ => return Err(bad),
        };
        let mut v = u64::from(lead & (0x7F >> (extra + 1)));
        for _ in 0..extra {
            let b = self.read_bits(8)?;
            if b & 0xC0 != 0x80 {
                return Err(bad);
            }
            v = (v << 6) | u64::from(b & 0x3F);
        }
        Ok(v)
    }

    /// Discards bits up to the next byte boundary (no-op if aligned).
    pub fn align_byte(&mut self) {
        self.consume(self.cache_bits % 8);
    }

    /// Index of the byte containing the next unread bit (the data length when
    /// everything is consumed).
    pub fn byte_pos(&self) -> usize {
        self.next - self.cache_bits.div_ceil(8) as usize
    }

    /// Number of unread bits.
    pub fn bits_left(&self) -> u64 {
        ((self.data.len() - self.next) as u64) * 8 + u64::from(self.cache_bits)
    }
}
