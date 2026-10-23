//! Bounded byte reader over a [`Source`].

use crate::{Error, Result, Source};

/// Reads fixed-width integers from a [`Source`]; premature end of data is an error.
pub struct ByteReader<'s> {
    src: &'s mut dyn Source,
}

macro_rules! int_reader {
    ($($name:ident, $ty:ty, $from:ident;)*) => {$(
        #[doc = concat!("Reads a `", stringify!($ty), "` (`", stringify!($from), "`).")]
        pub fn $name(&mut self) -> Result<$ty> {
            let mut b = [0u8; core::mem::size_of::<$ty>()];
            self.read_exact(&mut b)?;
            Ok(<$ty>::$from(b))
        }
    )*};
}

impl<'s> ByteReader<'s> {
    /// Wraps `src`.
    pub fn new(src: &'s mut dyn Source) -> Self {
        Self { src }
    }

    fn eof(&self) -> Error {
        Error::Invalid {
            what: "unexpected eof",
            offset: self.src.position(),
        }
    }

    /// Fills `buf` completely or fails with `Invalid { what: "unexpected eof", .. }`.
    pub fn read_exact(&mut self, buf: &mut [u8]) -> Result<()> {
        let mut done = 0;
        while done < buf.len() {
            let n = self.src.read(&mut buf[done..])?;
            if n == 0 {
                return Err(self.eof());
            }
            done += n;
        }
        Ok(())
    }

    /// Skips `n` bytes; fails if that would pass the end of a source of known length.
    pub fn skip(&mut self, n: u64) -> Result<()> {
        let target = self.src.position().checked_add(n).ok_or(Error::Invalid {
            what: "skip overflow",
            offset: self.src.position(),
        })?;
        if let Some(len) = self.src.len()
            && target > len
        {
            return Err(Error::Invalid {
                what: "unexpected eof",
                offset: len,
            });
        }
        self.src.seek(target)
    }

    /// Reads one byte.
    pub fn u8(&mut self) -> Result<u8> {
        let mut b = [0u8; 1];
        self.read_exact(&mut b)?;
        Ok(b[0])
    }

    int_reader! {
        u16_le, u16, from_le_bytes;
        u16_be, u16, from_be_bytes;
        u32_le, u32, from_le_bytes;
        u32_be, u32, from_be_bytes;
        u64_le, u64, from_le_bytes;
        u64_be, u64, from_be_bytes;
    }

    /// Reads a four-character code.
    pub fn fourcc(&mut self) -> Result<[u8; 4]> {
        let mut b = [0u8; 4];
        self.read_exact(&mut b)?;
        Ok(b)
    }

    /// Current absolute position of the underlying source.
    pub fn position(&self) -> u64 {
        self.src.position()
    }
}
