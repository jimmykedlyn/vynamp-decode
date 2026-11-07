//! Detection of tags appended after the audio payload (ID3v1, APEv2, Lyrics3v2,
//! ID3v2 with footer).

use crate::{Result, Source};

/// Trailers recognised after the audio, at most.
const MAX_TRAILERS: usize = 16;
/// Length of an ID3v1 tag at the end of a file.
const ID3V1_LEN: u64 = 128;

/// Kind of a trailer found after the audio payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrailerKind {
    /// ID3v1 tag: 128 bytes starting with `TAG`.
    Id3v1,
    /// APEv2 tag (footer, optional header).
    ApeV2,
    /// Lyrics3v2 tag.
    Lyrics3v2,
    /// ID3v2 tag appended with a `3DI` footer.
    Id3v2Appended,
}

/// A trailer's byte range in the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trailer {
    /// What kind of tag this is.
    pub kind: TrailerKind,
    /// Absolute position of the tag's first byte.
    pub offset: u64,
    /// Length of the tag in bytes.
    pub len: u64,
}

/// Finds the tags after the audio payload by scanning backwards from the source end.
///
/// Recognised, in any order and number (at most 16): ID3v1, APEv2 (footer, optional
/// header), an ID3v2 tag appended with a footer (`3DI`) and Lyrics3v2. No tag is
/// accepted if it would begin before `payload_start`.
///
/// Returns the end of the audio payload (the source length if there is no trailer, or
/// `u64::MAX` if the length is unknown) and the trailers in file order, so the
/// outermost (last in the file) comes last. Leaves the position of `src` unspecified.
///
/// # Errors
/// Propagates source read errors.
pub fn find_trailers(src: &mut dyn Source, payload_start: u64) -> Result<(u64, Vec<Trailer>)> {
    let Some(len) = src.len() else {
        return Ok((u64::MAX, Vec::new()));
    };
    let mut tail = Tail {
        src,
        payload_start,
        len,
    };
    let mut end = len;
    let mut found = Vec::new();
    // Each pass strips one trailer; stop when none is recognised.
    for _ in 0..MAX_TRAILERS {
        let Some((kind, size)) = trailer_at(&mut tail, end)? else {
            break;
        };
        end -= size;
        found.push(Trailer {
            kind,
            offset: end,
            len: size,
        });
    }
    found.reverse();
    Ok((end, found))
}

/// Random access to the end of a source.
struct Tail<'a> {
    src: &'a mut dyn Source,
    payload_start: u64,
    len: u64,
}

impl Tail<'_> {
    /// Reads `n` bytes at `pos`, or `None` outside the audio data.
    fn read_at(&mut self, pos: u64, n: usize) -> Result<Option<Vec<u8>>> {
        if pos < self.payload_start || pos + n as u64 > self.len {
            return Ok(None);
        }
        self.src.seek(pos)?;
        let mut v = vec![0u8; n];
        let mut got = 0;
        while got < n {
            match self.src.read(&mut v[got..])? {
                0 => return Ok(None),
                k => got += k,
            }
        }
        Ok(Some(v))
    }
}

/// Kind and size of the trailer that ends at `end`, if one is recognised.
fn trailer_at(tail: &mut Tail, end: u64) -> Result<Option<(TrailerKind, u64)>> {
    let room = end.saturating_sub(tail.payload_start);
    // ID3v1: 128 bytes starting with "TAG".
    if end >= ID3V1_LEN
        && let Some(tag) = tail.read_at(end - ID3V1_LEN, 3)?
        && &tag[..] == b"TAG"
    {
        return Ok(Some((TrailerKind::Id3v1, ID3V1_LEN)));
    }
    // APEv2: a 32-byte footer, preceded by the items and followed by nothing.
    if end >= 32
        && let Some(f) = tail.read_at(end - 32, 32)?
        && f.starts_with(b"APETAGEX")
    {
        let le = |i: usize| u32::from_le_bytes([f[i], f[i + 1], f[i + 2], f[i + 3]]);
        let (version, size, flags) = (le(8), u64::from(le(12)), le(20));
        // The size covers the items and the footer but not an optional header.
        let total = size + if flags & 0x8000_0000 != 0 { 32 } else { 0 };
        if (version == 1000 || version == 2000) && size >= 32 && total <= room {
            return Ok(Some((TrailerKind::ApeV2, total)));
        }
    }
    // Lyrics3v2: "LYRICSBEGIN" ... a six-digit size ... "LYRICS200". The size counts
    // everything between the two markers' ends, so the tag is the size plus 15 bytes.
    if end >= 15
        && let Some(f) = tail.read_at(end - 15, 15)?
        && &f[6..] == b"LYRICS200"
        && f[..6].iter().all(u8::is_ascii_digit)
    {
        let size = f[..6]
            .iter()
            .fold(0u64, |a, &d| a * 10 + u64::from(d - b'0'));
        let total = size + 15;
        if size >= 11
            && total <= room
            && let Some(begin) = tail.read_at(end - total, 11)?
            && &begin[..] == b"LYRICSBEGIN"
        {
            return Ok(Some((TrailerKind::Lyrics3v2, total)));
        }
    }
    // ID3v2 appended with a footer: "3DI", version, flags, synchsafe size. The size
    // excludes the 10-byte header and the 10-byte footer.
    if end >= 10
        && let Some(f) = tail.read_at(end - 10, 10)?
        && &f[..3] == b"3DI"
        && f[6..].iter().all(|&b| b < 0x80)
    {
        let size = f[6..].iter().fold(0u64, |a, &b| a << 7 | u64::from(b));
        let total = size + 20;
        if total <= room
            && let Some(head) = tail.read_at(end - total, 3)?
            && &head[..] == b"ID3"
        {
            return Ok(Some((TrailerKind::Id3v2Appended, total)));
        }
    }
    Ok(None)
}
