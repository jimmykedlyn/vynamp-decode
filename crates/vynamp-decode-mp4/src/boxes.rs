//! Bounds-checked parsing of in-memory ISOBMFF boxes.

use vynamp_decode_core::{Error, Result};

/// One box: its type, its payload and the absolute source offset of the payload.
#[derive(Clone, Copy)]
pub(crate) struct Bx<'a> {
    pub kind: [u8; 4],
    pub body: &'a [u8],
    pub offset: u64,
}

impl<'a> Bx<'a> {
    /// A cursor over the payload.
    pub fn cur(&self) -> Cur<'a> {
        Cur {
            d: self.body,
            at: 0,
            base: self.offset,
        }
    }

    /// The child boxes, `skip` bytes into the payload.
    pub fn children(&self, skip: usize) -> Result<Vec<Bx<'a>>> {
        let body = self.body.get(skip..).ok_or(Error::Invalid {
            what: "unexpected eof",
            offset: self.offset,
        })?;
        parse(body, self.offset + skip as u64)
    }

    /// The first child of type `kind`, if any.
    pub fn child(&self, kind: &[u8; 4]) -> Result<Option<Bx<'a>>> {
        Ok(self.children(0)?.into_iter().find(|b| &b.kind == kind))
    }
}

/// Splits `data` (starting at absolute offset `base`) into boxes. A size smaller than its
/// header is `Invalid { what: "mp4 box size" }`; a box that runs past `data` is
/// `Invalid { what: "unexpected eof" }`.
pub(crate) fn parse(data: &[u8], base: u64) -> Result<Vec<Bx<'_>>> {
    let mut out = Vec::new();
    let mut c = Cur {
        d: data,
        at: 0,
        base,
    };
    while c.at < data.len() {
        let start = c.at;
        let size32 = c.u32()?;
        let kind = c.fourcc()?;
        let size = match size32 {
            0 => (data.len() - start) as u64,
            1 => c.u64()?,
            s => u64::from(s),
        };
        let header = (c.at - start) as u64;
        if size < header {
            return Err(Error::Invalid {
                what: "mp4 box size",
                offset: base + start as u64,
            });
        }
        let len = usize::try_from(size - header).map_err(|_| c.eof())?;
        let body = c.take(len)?;
        out.push(Bx {
            kind,
            body,
            offset: base + (start as u64 + header),
        });
    }
    Ok(out)
}

/// Big-endian reader over a byte slice; running out is `Invalid { what: "unexpected eof" }`.
pub(crate) struct Cur<'a> {
    d: &'a [u8],
    at: usize,
    base: u64,
}

impl<'a> Cur<'a> {
    pub fn eof(&self) -> Error {
        Error::Invalid {
            what: "unexpected eof",
            offset: self.base + self.d.len() as u64,
        }
    }

    /// Absolute offset of the next byte.
    pub fn offset(&self) -> u64 {
        self.base + self.at as u64
    }

    pub fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.at.checked_add(n).filter(|&e| e <= self.d.len());
        let end = end.ok_or_else(|| self.eof())?;
        let s = &self.d[self.at..end];
        self.at = end;
        Ok(s)
    }

    pub fn skip(&mut self, n: usize) -> Result<()> {
        self.take(n).map(|_| ())
    }

    /// Bytes left.
    pub fn left(&self) -> usize {
        self.d.len() - self.at
    }

    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    pub fn u16(&mut self) -> Result<u16> {
        let b = self.take(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }

    pub fn u32(&mut self) -> Result<u32> {
        let b = self.take(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub fn u64(&mut self) -> Result<u64> {
        let hi = self.u32()?;
        let lo = self.u32()?;
        Ok((u64::from(hi) << 32) | u64::from(lo))
    }

    pub fn fourcc(&mut self) -> Result<[u8; 4]> {
        let b = self.take(4)?;
        Ok([b[0], b[1], b[2], b[3]])
    }

    /// Reads a full-box header and returns its version.
    pub fn version(&mut self) -> Result<u8> {
        let v = self.u8()?;
        self.skip(3)?;
        Ok(v)
    }

    /// Reads an entry count and checks that `count * entry_size` bytes remain, so a
    /// forged count cannot make the caller allocate more than the box holds.
    pub fn count(&mut self, entry_size: usize) -> Result<usize> {
        let at = self.offset();
        let n = self.u32()? as usize;
        if n.checked_mul(entry_size).is_none_or(|b| b > self.left()) {
            return Err(Error::Invalid {
                what: "mp4 entry count",
                offset: at,
            });
        }
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn largesize_and_to_the_end_sizes() {
        let mut d = 1u32.to_be_bytes().to_vec();
        d.extend_from_slice(b"free");
        d.extend_from_slice(&18u64.to_be_bytes());
        d.extend_from_slice(b"ab");
        d.extend_from_slice(&0u32.to_be_bytes());
        d.extend_from_slice(b"mdat");
        d.extend_from_slice(b"xyz");
        let b = parse(&d, 100).unwrap();
        assert_eq!(b.len(), 2);
        assert_eq!(
            (&b[0].kind, b[0].body, b[0].offset),
            (b"free", &b"ab"[..], 116)
        );
        assert_eq!(
            (&b[1].kind, b[1].body, b[1].offset),
            (b"mdat", &b"xyz"[..], 126)
        );
    }

    #[test]
    fn truncated_header_and_body_are_eof() {
        for d in [&b"\0\0\0"[..], b"\0\0\0\x10free\0\0"] {
            assert!(matches!(
                parse(d, 0),
                Err(Error::Invalid {
                    what: "unexpected eof",
                    ..
                })
            ));
        }
    }

    #[test]
    fn forged_entry_count_is_invalid() {
        let d = [0, 0, 0, 3, 0, 0, 0, 1];
        let mut c = Cur {
            d: &d,
            at: 0,
            base: 0,
        };
        assert!(matches!(
            c.count(4),
            Err(Error::Invalid {
                what: "mp4 entry count",
                ..
            })
        ));
    }
}
