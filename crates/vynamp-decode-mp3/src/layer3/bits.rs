//! Bit cursor over the assembled main data of a granule.

/// An MSB-first cursor with unchecked-length semantics: reading past the end yields zero
/// bits, and the caller checks [`Bits::pos`] against the end of the granule. This keeps the
/// Huffman loop free of per-symbol error handling; every region is bounded by the 576 lines
/// of a granule, so a garbage stream cannot make the loops run away.
pub(super) struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Bits<'a> {
    pub(super) fn new(data: &'a [u8], pos: usize) -> Self {
        Self { data, pos }
    }

    /// Position in bits from the start of `data`.
    #[inline]
    pub(super) fn pos(&self) -> usize {
        self.pos
    }

    pub(super) fn set_pos(&mut self, pos: usize) {
        self.pos = pos;
    }

    /// The next 32 bits without consuming them (zeros past the end).
    #[inline]
    pub(super) fn peek32(&self) -> u32 {
        let byte = self.pos / 8;
        let word = match self.data.get(byte..byte + 8) {
            Some(b) => u64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]),
            None => {
                let mut buf = [0u8; 8];
                if let Some(tail) = self.data.get(byte..) {
                    let n = tail.len().min(8);
                    buf[..n].copy_from_slice(&tail[..n]);
                }
                u64::from_be_bytes(buf)
            }
        };
        ((word << (self.pos % 8)) >> 32) as u32
    }

    #[inline]
    pub(super) fn skip(&mut self, n: usize) {
        self.pos += n;
    }

    /// Reads `n` (0 to 32) bits.
    #[inline]
    pub(super) fn read(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        let v = self.peek32() >> (32 - n);
        self.pos += n as usize;
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_across_bytes_and_past_the_end() {
        let data = [0b1010_0101, 0xFF, 0x00];
        let mut b = Bits::new(&data, 0);
        assert_eq!(b.read(3), 0b101);
        assert_eq!(b.read(7), 0b001_0111);
        assert_eq!(b.pos(), 10);
        assert_eq!(b.peek32() >> 24, 0b1111_1100);
        b.set_pos(100);
        assert_eq!(b.read(32), 0);
        assert_eq!(b.read(0), 0);
        let mut b = Bits::new(&data, 22);
        assert_eq!(b.read(8), 0);
    }

    #[test]
    fn full_word_reads() {
        let data = [0xDE, 0xAD, 0xBE, 0xEF, 0x12, 0x34, 0x56, 0x78, 0x9A];
        let mut b = Bits::new(&data, 4);
        assert_eq!(b.read(32), 0xEADB_EEF1);
    }
}
