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

/// Descriptor of the native FLAC container.
#[cfg(feature = "flac")]
pub static FLAC: ContainerDescriptor = ContainerDescriptor {
    name: "flac",
    extensions: &["flac", "fla"],
    probe: vynamp_decode_flac::probe_flac,
    verification: Verification::Conformance,
    open: |src, opts| Ok(Box::new(vynamp_decode_flac::open_flac(src, opts)?)),
};

/// Descriptor of the MPEG audio elementary stream container: `.mp1`, `.mp2` and `.mp3` files,
/// with or without ID3v2/ID3v1/APEv2 tags and a Xing/Info/LAME or VBRI header.
#[cfg(feature = "mpa")]
pub static MPA: ContainerDescriptor = ContainerDescriptor {
    name: "mpa",
    extensions: &["mp3", "mp2", "mp1", "mpa"],
    probe: vynamp_decode_mpa::probe_mpa,
    verification: Verification::Oracle,
    open: |src, opts| Ok(Box::new(vynamp_decode_mpa::open_mpa(src, opts)?)),
};

/// Descriptor of the raw PCM codec.
#[cfg(feature = "pcm")]
pub static PCM: CodecDescriptor = CodecDescriptor {
    name: "pcm",
    verification: Verification::Oracle,
    supports: |id| matches!(id, CodecId::Pcm(_)),
    make: |track, _| Ok(Box::new(vynamp_decode_pcm::PcmDecoder::new(track)?)),
};

/// Descriptor of the FLAC codec.
#[cfg(feature = "flac")]
pub static FLAC_CODEC: CodecDescriptor = CodecDescriptor {
    name: "flac",
    verification: Verification::Conformance,
    supports: |id| matches!(id, CodecId::Flac),
    make: |track, opts| {
        Ok(Box::new(vynamp_decode_flac::FlacDecoder::with_options(
            track, opts,
        )?))
    },
};

/// Descriptor of the MPEG audio codec for Layers II and III of MPEG-1, MPEG-2 and MPEG-2.5.
///
/// Verified against FFmpeg's floating point decoders (`mp2float`, `mp3float`) under the ISO/IEC
/// 11172-4 full accuracy criterion (RMS error below 2^-15 / sqrt(12), maximum error at most
/// 2^-14) on the oracle corpus; the ISO conformance streams are not redistributable, hence
/// `Oracle` rather than `Conformance`.
#[cfg(feature = "mp3")]
pub static MPEG_AUDIO_CODEC: CodecDescriptor = CodecDescriptor {
    name: "mpeg-audio",
    verification: Verification::Oracle,
    supports: |id| matches!(id, CodecId::Mp2 | CodecId::Mp3),
    make: |track, opts| Ok(Box::new(vynamp_decode_mp3::MpegDecoder::new(track, opts)?)),
};

/// Descriptor of the MPEG audio codec for Layer I.
///
/// `Experimental`: FFmpeg has no Layer I encoder, so there is no real-world Layer I file and no
/// third-party oracle; the decoder is tested with hand-assembled frames against an independent
/// reference chain only.
#[cfg(feature = "mp3")]
pub static MPEG_AUDIO_LAYER1_CODEC: CodecDescriptor = CodecDescriptor {
    name: "mpeg-audio-layer1",
    verification: Verification::Experimental,
    supports: |id| matches!(id, CodecId::Mp1),
    make: |track, opts| Ok(Box::new(vynamp_decode_mp3::MpegDecoder::new(track, opts)?)),
};

/// Registry containing every container and codec enabled by features.
pub fn default_registry() -> Registry {
    #[allow(unused_mut)]
    let mut reg = Registry::new();
    #[cfg(feature = "riff")]
    reg.add_container(&RIFF);
    #[cfg(feature = "aiff")]
    reg.add_container(&AIFF);
    #[cfg(feature = "flac")]
    reg.add_container(&FLAC);
    #[cfg(feature = "mpa")]
    reg.add_container(&MPA);
    #[cfg(feature = "pcm")]
    reg.add_codec(&PCM);
    #[cfg(feature = "flac")]
    reg.add_codec(&FLAC_CODEC);
    #[cfg(feature = "mp3")]
    reg.add_codec(&MPEG_AUDIO_CODEC);
    #[cfg(feature = "mp3")]
    reg.add_codec(&MPEG_AUDIO_LAYER1_CODEC);
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
