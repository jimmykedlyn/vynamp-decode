//! Core types shared by all agulha crates.
#![warn(missing_docs)]

mod buffer;
mod bytes;
mod cancel;
mod error;
mod limits;
mod packet;
mod sample;
mod source;
mod track;
mod traits;

pub use buffer::AudioBuffer;
pub use bytes::ByteReader;
pub use cancel::{CancelSource, CancelToken};
pub use error::{Error, LimitKind, Result};
pub use limits::{Limits, Mode, Options};
pub use packet::Packet;
pub use sample::{Endian, PcmEncoding, PcmSpec, SampleFormat};
pub use source::{FileSource, SliceSource, Source, VecSource};
pub use track::{CodecId, Track};
pub use traits::{Decoder, Demuxer, SeekTo};
