//! Container and codec registry with format probing.

use crate::{CodecId, Decoder, Demuxer, Error, LimitKind, Limits, Options, Result, Source, Track};

/// How thoroughly a codec has been verified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verification {
    /// Bit-exact against a conformance suite.
    Conformance,
    /// Matches a reference implementation's output.
    Oracle,
    /// Not yet verified.
    Experimental,
}

/// Opens a demuxer over a source.
pub type OpenFn = fn(Box<dyn Source>, &Options) -> Result<Box<dyn Demuxer>>;

/// Describes a container format.
pub struct ContainerDescriptor {
    /// Format name.
    pub name: &'static str,
    /// Usual file extensions (without the dot).
    pub extensions: &'static [&'static str],
    /// Scores how likely the header bytes are this format (0 = not at all).
    pub probe: fn(&[u8]) -> u8,
    /// Opens a demuxer over `src`.
    pub open: OpenFn,
}

/// Describes a codec.
pub struct CodecDescriptor {
    /// Codec name.
    pub name: &'static str,
    /// Verification level.
    pub verification: Verification,
    /// Whether this codec can decode `id`.
    pub supports: fn(&CodecId) -> bool,
    /// Creates a decoder for `track`.
    pub make: fn(&Track, &Options) -> Result<Box<dyn Decoder>>,
}

/// Number of bytes read from the start of a source when probing.
const PROBE_BYTES: usize = 4096;

/// Fills `buf` from `src`, stopping early only at the end of data.
fn read_full(src: &mut dyn Source, buf: &mut [u8]) -> Result<usize> {
    let mut got = 0;
    while got < buf.len() {
        let n = src.read(&mut buf[got..])?;
        if n == 0 {
            break;
        }
        got += n;
    }
    Ok(got)
}

/// Returns the offset just past the ID3v2 tag(s) that start the stream, or 0.
///
/// Each tag is `"ID3"`, version, revision, flags, a 4-byte synchsafe size and
/// an optional 10-byte footer (flag 0x10). Skipping stops at the first header
/// that is truncated or malformed (version byte 0xFF or a size byte >= 0x80);
/// the offset then points at that header.
fn skip_id3v2(src: &mut dyn Source, limits: &Limits) -> Result<u64> {
    let mut offset = 0u64;
    loop {
        src.seek(offset)?;
        let mut h = [0u8; 10];
        if read_full(src, &mut h)? < h.len() || &h[..3] != b"ID3" {
            return Ok(offset);
        }
        if h[3] == 0xFF || h[4] == 0xFF || h[6..].iter().any(|&b| b >= 0x80) {
            return Ok(offset);
        }
        let size = h[6..]
            .iter()
            .fold(0u64, |acc, &b| (acc << 7) | u64::from(b));
        let footer = if h[5] & 0x10 != 0 { 10 } else { 0 };
        let end = offset
            .checked_add(10 + size + footer)
            .ok_or(Error::Limit(LimitKind::Metadata))?;
        if end > u64::from(limits.max_metadata_bytes) {
            return Err(Error::Limit(LimitKind::Metadata));
        }
        offset = end;
    }
}

/// A set of known containers and codecs.
#[derive(Default)]
pub struct Registry {
    containers: Vec<&'static ContainerDescriptor>,
    codecs: Vec<&'static CodecDescriptor>,
}

impl Registry {
    /// Creates an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a container.
    pub fn add_container(&mut self, d: &'static ContainerDescriptor) {
        self.containers.push(d);
    }

    /// Registers a codec.
    pub fn add_codec(&mut self, d: &'static CodecDescriptor) {
        self.codecs.push(d);
    }

    /// Identifies the container of `src`.
    ///
    /// Skips any ID3v2 tags at the start of the stream (their combined size is
    /// bounded by `limits.max_metadata_bytes`, else `Limit(Metadata)`; a
    /// malformed tag header is not skipped), reads up to 4096 bytes after
    /// them and picks the highest non-zero
    /// score; ties go to the descriptor whose extensions contain `hint_ext`
    /// (case-insensitive), else to the first registered. The source is left
    /// at position 0; demuxers handle the tags themselves. Returns `Unsupported("container")` if nothing matches.
    pub fn probe(
        &self,
        src: &mut dyn Source,
        hint_ext: Option<&str>,
        limits: &Limits,
    ) -> Result<&'static ContainerDescriptor> {
        let offset = match skip_id3v2(src, limits) {
            Ok(o) => o,
            Err(e) => {
                // Best effort: the error being reported matters more.
                let _ = src.seek(0);
                return Err(e);
            }
        };
        src.seek(offset)?;
        let mut head = vec![0u8; PROBE_BYTES];
        let got = read_full(src, &mut head)?;
        head.truncate(got);
        src.seek(0)?;

        let mut best: Option<(u8, &'static ContainerDescriptor)> = None;
        for &d in &self.containers {
            let score = (d.probe)(&head);
            if score == 0 {
                continue;
            }
            let hinted = |d: &ContainerDescriptor| {
                hint_ext.is_some_and(|h| d.extensions.iter().any(|e| e.eq_ignore_ascii_case(h)))
            };
            best = match best {
                None => Some((score, d)),
                Some((s, _)) if score > s => Some((score, d)),
                Some((s, b)) if score == s && !hinted(b) && hinted(d) => Some((score, d)),
                keep => keep,
            };
        }
        best.map(|(_, d)| d).ok_or(Error::Unsupported("container"))
    }

    /// Finds the first registered codec that supports `id`.
    pub fn codec_for(&self, id: &CodecId) -> Option<&'static CodecDescriptor> {
        self.codecs.iter().copied().find(|d| (d.supports)(id))
    }

    /// Registered containers, in registration order.
    pub fn containers(&self) -> impl Iterator<Item = &'static ContainerDescriptor> {
        self.containers.iter().copied()
    }

    /// Registered codecs, in registration order.
    pub fn codecs(&self) -> impl Iterator<Item = &'static CodecDescriptor> {
        self.codecs.iter().copied()
    }
}
