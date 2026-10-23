//! vynamp-decode: dependency-free audio decoding behind a single facade.
//!
//! [`open`] / [`open_path`] probe the container, build a decoder for the first
//! track and return a [`Reader`] that yields gapless (delay/padding trimmed)
//! audio.
#![warn(missing_docs)]

mod reader;

pub use reader::Reader;
pub use vynamp_decode_core::*;

/// Descriptor of the RIFF/WAVE, RF64, BW64 and Wave64 container.
#[cfg(feature = "riff")]
pub static RIFF: ContainerDescriptor = ContainerDescriptor {
    name: "riff",
    extensions: &["wav", "wave", "w64", "rf64", "bw64"],
    probe: vynamp_decode_riff::probe_riff,
    verification: Verification::Oracle,
    open: |src, opts| Ok(Box::new(vynamp_decode_riff::open_riff(src, opts)?)),
};

/// Descriptor of the AIFF and AIFF-C container.
#[cfg(feature = "aiff")]
pub static AIFF: ContainerDescriptor = ContainerDescriptor {
    name: "aiff",
    extensions: &["aif", "aiff", "aifc"],
    probe: vynamp_decode_aiff::probe_aiff,
    verification: Verification::Oracle,
    open: |src, opts| Ok(Box::new(vynamp_decode_aiff::open_aiff(src, opts)?)),
};

/// Descriptor of the raw PCM codec.
#[cfg(feature = "pcm")]
pub static PCM: CodecDescriptor = CodecDescriptor {
    name: "pcm",
    verification: Verification::Oracle,
    supports: |id| matches!(id, CodecId::Pcm(_)),
    make: |track, _| Ok(Box::new(vynamp_decode_pcm::PcmDecoder::new(track)?)),
};

/// Registry containing every container and codec enabled by features.
pub fn default_registry() -> Registry {
    #[allow(unused_mut)]
    let mut reg = Registry::new();
    #[cfg(feature = "riff")]
    reg.add_container(&RIFF);
    #[cfg(feature = "aiff")]
    reg.add_container(&AIFF);
    #[cfg(feature = "pcm")]
    reg.add_codec(&PCM);
    reg
}

/// Opens `src` with the [default registry](default_registry).
pub fn open(src: Box<dyn Source>, opts: Options) -> Result<Reader> {
    Reader::open_with(&default_registry(), src, opts, None)
}

/// Opens the file at `path`, using its extension as a probe hint.
pub fn open_path(path: &std::path::Path, opts: Options) -> Result<Reader> {
    let src = FileSource::open(path)?;
    let ext = path.extension().and_then(|e| e.to_str());
    Reader::open_with(&default_registry(), Box::new(src), opts, ext)
}
