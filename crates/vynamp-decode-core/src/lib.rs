//! Core types shared by all vynamp-decode crates.
#![warn(missing_docs)]

mod bits;
mod buffer;
mod bytes;
mod cancel;
mod crc;
mod error;
mod event;
mod limits;
mod md5;
mod packet;
mod registry;
mod sample;
mod source;
mod track;
mod traits;

pub use bits::BitReader;
pub use buffer::AudioBuffer;
pub use bytes::ByteReader;
pub use cancel::{CancelSource, CancelToken};
pub use crc::{crc8, crc16, crc16_update};
pub use error::{Error, LimitKind, Result};
pub use event::Event;
pub use limits::{Limits, Mode, Options};
pub use md5::Md5;
pub use packet::Packet;
pub use registry::{
    CodecDescriptor, ContainerDescriptor, OpenFn, Registry, Verification, skip_id3v2,
};
pub use sample::{Endian, PcmEncoding, PcmSpec, SampleFormat};
pub use source::{FileSource, SliceSource, Source, VecSource};
pub use track::{CodecId, GaplessKind, Track};
pub use traits::{Decoder, Demuxer, SeekTo};
