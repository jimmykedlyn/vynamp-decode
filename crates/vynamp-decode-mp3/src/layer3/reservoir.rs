//! The bit reservoir: main data of earlier frames that later frames point back into.

/// Capacity kept: `main_data_begin` reaches back 511 bytes at most (MPEG-1; 255 in MPEG-2/2.5),
/// the rest is slack for a frame that is larger than any real one.
pub(super) const CAPACITY: usize = 4096;

/// The tail of the main data of the frames seen so far.
#[derive(Default)]
pub(super) struct Reservoir {
    buf: Vec<u8>,
}

impl Reservoir {
    /// Bytes available to look back into.
    pub(super) fn len(&self) -> usize {
        self.buf.len()
    }

    /// Forgets everything (a seek or a gap in the stream).
    pub(super) fn clear(&mut self) {
        self.buf.clear();
    }

    /// The last `n` bytes (`n <= len()`).
    pub(super) fn tail(&self, n: usize) -> &[u8] {
        &self.buf[self.buf.len() - n.min(self.buf.len())..]
    }

    /// Appends the main data of a frame, keeping at most [`CAPACITY`] bytes.
    pub(super) fn push(&mut self, main: &[u8]) {
        if main.len() >= CAPACITY {
            self.buf.clear();
            self.buf.extend_from_slice(&main[main.len() - CAPACITY..]);
            return;
        }
        let excess = (self.buf.len() + main.len()).saturating_sub(CAPACITY);
        if excess > 0 {
            self.buf.drain(..excess);
        }
        self.buf.extend_from_slice(main);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_most_recent_bytes() {
        let mut r = Reservoir::default();
        assert_eq!(r.len(), 0);
        r.push(&[1, 2, 3]);
        r.push(&[4, 5]);
        assert_eq!(r.tail(3), &[3, 4, 5]);
        assert_eq!(r.tail(99), &[1, 2, 3, 4, 5]);
        r.clear();
        assert_eq!(r.len(), 0);
    }

    #[test]
    fn is_bounded() {
        let mut r = Reservoir::default();
        for i in 0..40u32 {
            r.push(&vec![i as u8; 300]);
        }
        assert_eq!(r.len(), CAPACITY);
        assert_eq!(r.tail(1), &[39]);
        r.push(&vec![7; CAPACITY + 10]);
        assert_eq!(r.len(), CAPACITY);
        assert_eq!(r.tail(CAPACITY), &vec![7; CAPACITY][..]);
    }
}
