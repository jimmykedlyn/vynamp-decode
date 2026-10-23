//! Container and codec registry with format probing.

use crate::{CodecId, Decoder, Demuxer, Error, Options, Result, Source, Track};

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
    /// Reads up to 4096 bytes from the start and picks the highest non-zero
    /// score; ties go to the descriptor whose extensions contain `hint_ext`
    /// (case-insensitive), else to the first registered. The source is left
    /// at position 0. Returns `Unsupported("container")` if nothing matches.
    pub fn probe(
        &self,
        src: &mut dyn Source,
        hint_ext: Option<&str>,
    ) -> Result<&'static ContainerDescriptor> {
        src.seek(0)?;
        let mut head = vec![0u8; PROBE_BYTES];
        let mut got = 0;
        while got < head.len() {
            let n = src.read(&mut head[got..])?;
            if n == 0 {
                break;
            }
            got += n;
        }
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
