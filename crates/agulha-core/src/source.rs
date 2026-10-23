//! Byte sources: in-memory slices, owned vectors and files.

use crate::Result;
use std::io::{BufReader, Read, Seek, SeekFrom};

/// A seekable byte source.
pub trait Source {
    /// Reads up to `buf.len()` bytes, returning the count (0 at end of data).
    fn read(&mut self, buf: &mut [u8]) -> Result<usize>;
    /// Moves to the absolute position `pos`. Positions past the end are allowed;
    /// subsequent reads return 0 bytes.
    fn seek(&mut self, pos: u64) -> Result<()>;
    /// Current absolute position.
    fn position(&self) -> u64;
    /// Total length, if known.
    fn len(&self) -> Option<u64>;
    /// Whether the source is known to be empty.
    fn is_empty(&self) -> bool {
        self.len() == Some(0)
    }
}

fn read_at(data: &[u8], pos: &mut u64, buf: &mut [u8]) -> usize {
    let start = usize::try_from(*pos).unwrap_or(usize::MAX);
    let Some(rest) = data.get(start..) else {
        return 0;
    };
    let n = rest.len().min(buf.len());
    buf[..n].copy_from_slice(&rest[..n]);
    *pos += n as u64;
    n
}

/// A [`Source`] over a borrowed slice.
#[derive(Debug, Clone)]
pub struct SliceSource<'a> {
    data: &'a [u8],
    pos: u64,
}

impl<'a> SliceSource<'a> {
    /// Creates a source positioned at the start of `data`.
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }
}

impl Source for SliceSource<'_> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        Ok(read_at(self.data, &mut self.pos, buf))
    }
    fn seek(&mut self, pos: u64) -> Result<()> {
        self.pos = pos;
        Ok(())
    }
    fn position(&self) -> u64 {
        self.pos
    }
    fn len(&self) -> Option<u64> {
        Some(self.data.len() as u64)
    }
}

/// A [`Source`] over an owned vector.
#[derive(Debug, Clone)]
pub struct VecSource {
    data: Vec<u8>,
    pos: u64,
}

impl VecSource {
    /// Creates a source positioned at the start of `data`.
    pub fn new(data: Vec<u8>) -> Self {
        Self { data, pos: 0 }
    }
}

impl Source for VecSource {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        Ok(read_at(&self.data, &mut self.pos, buf))
    }
    fn seek(&mut self, pos: u64) -> Result<()> {
        self.pos = pos;
        Ok(())
    }
    fn position(&self) -> u64 {
        self.pos
    }
    fn len(&self) -> Option<u64> {
        Some(self.data.len() as u64)
    }
}

/// A buffered [`Source`] over a file.
#[derive(Debug)]
pub struct FileSource {
    reader: BufReader<std::fs::File>,
    pos: u64,
    len: u64,
}

impl FileSource {
    /// Opens `path` for reading (64 KiB internal buffer).
    pub fn open(path: &std::path::Path) -> Result<Self> {
        let file = std::fs::File::open(path)?;
        let len = file.metadata()?.len();
        Ok(Self {
            reader: BufReader::with_capacity(64 * 1024, file),
            pos: 0,
            len,
        })
    }
}

impl Source for FileSource {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        loop {
            match self.reader.read(buf) {
                Ok(n) => {
                    self.pos += n as u64;
                    return Ok(n);
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e.into()),
            }
        }
    }
    fn seek(&mut self, pos: u64) -> Result<()> {
        match i64::try_from(i128::from(pos) - i128::from(self.pos)) {
            Ok(delta) => self.reader.seek_relative(delta)?,
            Err(_) => {
                self.reader.seek(SeekFrom::Start(pos))?;
            }
        }
        self.pos = pos;
        Ok(())
    }
    fn position(&self) -> u64 {
        self.pos
    }
    fn len(&self) -> Option<u64> {
        Some(self.len)
    }
}
