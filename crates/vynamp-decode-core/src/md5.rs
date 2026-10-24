//! Streaming MD5 (RFC 1321), used to verify FLAC decoded-audio checksums.

/// Per-step additive constants: `floor(2^32 * abs(sin(i + 1)))`.
const K: [u32; 64] = [
    0xd76a_a478,
    0xe8c7_b756,
    0x2420_70db,
    0xc1bd_ceee,
    0xf57c_0faf,
    0x4787_c62a,
    0xa830_4613,
    0xfd46_9501,
    0x6980_98d8,
    0x8b44_f7af,
    0xffff_5bb1,
    0x895c_d7be,
    0x6b90_1122,
    0xfd98_7193,
    0xa679_438e,
    0x49b4_0821,
    0xf61e_2562,
    0xc040_b340,
    0x265e_5a51,
    0xe9b6_c7aa,
    0xd62f_105d,
    0x0244_1453,
    0xd8a1_e681,
    0xe7d3_fbc8,
    0x21e1_cde6,
    0xc337_07d6,
    0xf4d5_0d87,
    0x455a_14ed,
    0xa9e3_e905,
    0xfcef_a3f8,
    0x676f_02d9,
    0x8d2a_4c8a,
    0xfffa_3942,
    0x8771_f681,
    0x6d9d_6122,
    0xfde5_380c,
    0xa4be_ea44,
    0x4bde_cfa9,
    0xf6bb_4b60,
    0xbebf_bc70,
    0x289b_7ec6,
    0xeaa1_27fa,
    0xd4ef_3085,
    0x0488_1d05,
    0xd9d4_d039,
    0xe6db_99e5,
    0x1fa2_7cf8,
    0xc4ac_5665,
    0xf429_2244,
    0x432a_ff97,
    0xab94_23a7,
    0xfc93_a039,
    0x655b_59c3,
    0x8f0c_cc92,
    0xffef_f47d,
    0x8584_5dd1,
    0x6fa8_7e4f,
    0xfe2c_e6e0,
    0xa301_4314,
    0x4e08_11a1,
    0xf753_7e82,
    0xbd3a_f235,
    0x2ad7_d2bb,
    0xeb86_d391,
];

/// Per-step left-rotation amounts.
const S: [u32; 64] = [
    7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9,
    14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15,
    21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
];

/// A streaming MD5 hasher (RFC 1321).
///
/// MD5 is used here only as the integrity checksum defined by the FLAC
/// format, not for security.
///
/// ```
/// let mut m = vynamp_decode_core::Md5::new();
/// m.update(b"ab");
/// m.update(b"c");
/// assert_eq!(m.finish()[..4], [0x90, 0x01, 0x50, 0x98]);
/// ```
#[derive(Debug, Clone)]
pub struct Md5 {
    state: [u32; 4],
    buf: [u8; 64],
    /// Bytes currently held in `buf` (0..64).
    buf_len: usize,
    /// Total bytes hashed so far.
    total: u64,
}

impl Default for Md5 {
    fn default() -> Self {
        Self::new()
    }
}

impl Md5 {
    /// Creates a hasher for the empty message.
    pub fn new() -> Self {
        Self {
            state: [0x6745_2301, 0xefcd_ab89, 0x98ba_dcfe, 0x1032_5476],
            buf: [0; 64],
            buf_len: 0,
            total: 0,
        }
    }

    /// Feeds `data` into the hash.
    pub fn update(&mut self, mut data: &[u8]) {
        self.total = self.total.wrapping_add(data.len() as u64);
        if self.buf_len > 0 {
            let take = (64 - self.buf_len).min(data.len());
            self.buf[self.buf_len..self.buf_len + take].copy_from_slice(&data[..take]);
            self.buf_len += take;
            data = &data[take..];
            if self.buf_len < 64 {
                return;
            }
            let block = self.buf;
            compress(&mut self.state, &block);
            self.buf_len = 0;
        }
        let (blocks, rest) = data.as_chunks::<64>();
        for block in blocks {
            compress(&mut self.state, block);
        }
        self.buf[..rest.len()].copy_from_slice(rest);
        self.buf_len = rest.len();
    }

    /// Completes the hash and returns the 16-byte digest.
    pub fn finish(mut self) -> [u8; 16] {
        let bit_len = self.total.wrapping_mul(8);
        let mut pad = [0u8; 72];
        pad[0] = 0x80;
        // Pad to 56 mod 64, then append the 64-bit little-endian bit length.
        let pad_len = if self.buf_len < 56 {
            56 - self.buf_len
        } else {
            120 - self.buf_len
        };
        pad[pad_len..pad_len + 8].copy_from_slice(&bit_len.to_le_bytes());
        let total = self.total;
        self.update(&pad[..pad_len + 8]);
        debug_assert_eq!(self.buf_len, 0);
        self.total = total;
        let mut out = [0u8; 16];
        for (chunk, word) in out.as_chunks_mut::<4>().0.iter_mut().zip(self.state) {
            *chunk = word.to_le_bytes();
        }
        out
    }
}

fn compress(state: &mut [u32; 4], block: &[u8; 64]) {
    let mut m = [0u32; 16];
    for (w, c) in m.iter_mut().zip(block.as_chunks::<4>().0) {
        *w = u32::from_le_bytes(*c);
    }
    let [mut a, mut b, mut c, mut d] = *state;
    for i in 0..64 {
        let (f, g) = match i / 16 {
            0 => ((b & c) | (!b & d), i),
            1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
            2 => (b ^ c ^ d, (3 * i + 5) % 16),
            _ => (c ^ (b | !d), (7 * i) % 16),
        };
        let sum = a.wrapping_add(f).wrapping_add(K[i]).wrapping_add(m[g]);
        a = d;
        d = c;
        c = b;
        b = b.wrapping_add(sum.rotate_left(S[i]));
    }
    state[0] = state[0].wrapping_add(a);
    state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c);
    state[3] = state[3].wrapping_add(d);
}

#[cfg(test)]
mod tests {
    #[test]
    fn k_table_matches_sine_definition() {
        for (i, &k) in super::K.iter().enumerate() {
            let want = (((i + 1) as f64).sin().abs() * 4_294_967_296.0).floor() as u32;
            assert_eq!(k, want, "K[{i}]");
        }
    }
}
