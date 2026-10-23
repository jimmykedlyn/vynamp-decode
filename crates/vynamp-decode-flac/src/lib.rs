//! FLAC demuxer (stream marker and metadata blocks).
//!
//! `open_flac` skips a leading ID3v2 tag, checks the `fLaC` marker and parses
//! the metadata blocks (RFC 9639). STREAMINFO must come first and is validated;
//! the total size of the non-padding blocks is bounded by
//! `Limits::max_metadata_bytes`, and padding is skipped without being read.
//! Seek table placeholders are dropped. A zero `total_samples` means the length
//! is unknown (`Track::frames == None`).
//!
//! Frame reading and seeking are not implemented yet: `next_packet` and `seek`
//! return `Unsupported("flac frames")`.
#![warn(missing_docs)]

mod metadata;

pub use metadata::{BlockKind, RawBlock, SeekPoint, StreamInfo};

use metadata::read_metadata;
use vynamp_decode_core::{
    ByteReader, CodecId, Demuxer, Error, GaplessKind, Options, Packet, Result, SeekTo, Source,
    Track, skip_id3v2,
};

/// Detects a FLAC stream marker in `header`: `fLaC` at offset 0 gives 100.
pub fn probe_flac(header: &[u8]) -> u8 {
    if header.starts_with(b"fLaC") { 100 } else { 0 }
}

/// Demuxer for native FLAC streams.
pub struct FlacDemuxer {
    // Used by the frame reader (next task).
    #[allow(dead_code)]
    src: Box<dyn Source>,
    tracks: Vec<Track>,
    info: StreamInfo,
    seek_points: Vec<SeekPoint>,
    blocks: Vec<RawBlock>,
    first_frame: u64,
}

impl FlacDemuxer {
    /// The parsed STREAMINFO block.
    pub fn stream_info(&self) -> &StreamInfo {
        &self.info
    }

    /// Seek table entries without placeholders (offsets are from the first frame).
    pub fn seek_points(&self) -> &[SeekPoint] {
        &self.seek_points
    }

    /// Non-padding metadata blocks in file order, STREAMINFO first.
    pub fn metadata_blocks(&self) -> &[RawBlock] {
        &self.blocks
    }

    /// Absolute byte offset of the first audio frame.
    pub fn first_frame_offset(&self) -> u64 {
        self.first_frame
    }
}

/// Opens a FLAC stream, parsing its metadata blocks.
pub fn open_flac(mut src: Box<dyn Source>, opts: &Options) -> Result<FlacDemuxer> {
    opts.cancel.check()?;
    if let Some(len) = src.len() {
        opts.limits.check_bytes(len)?;
    }
    let marker = skip_id3v2(src.as_mut(), &opts.limits)?;
    let (info, seek_points, blocks, first_frame) = {
        let mut r = ByteReader::new(src.as_mut());
        if &r.fourcc()? != b"fLaC" {
            return Err(Error::Invalid {
                what: "flac signature",
                offset: marker,
            });
        }
        read_metadata(&mut r, &opts.limits)?
    };
    opts.limits.check_sample_rate(info.sample_rate)?;
    opts.limits.check_channels(u16::from(info.channels))?;
    let frames = (info.total_samples != 0).then_some(info.total_samples);
    if let Some(f) = frames {
        opts.limits.check_frames(f)?;
    }
    let codec_private = blocks.first().map(|b| b.data.clone()).unwrap_or_default();
    let track = Track {
        id: 0,
        codec: CodecId::Flac,
        sample_rate: info.sample_rate,
        channels: u16::from(info.channels),
        channel_mask: 0,
        frames,
        delay: 0,
        padding: 0,
        gapless: GaplessKind::Exact,
        codec_private,
    };
    Ok(FlacDemuxer {
        src,
        tracks: vec![track],
        info,
        seek_points,
        blocks,
        first_frame,
    })
}

impl Demuxer for FlacDemuxer {
    fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    // Replaced by the frame reader in the next task.
    fn next_packet(&mut self, _out: &mut Packet) -> Result<bool> {
        Err(Error::Unsupported("flac frames"))
    }

    // Replaced by the frame seeker in the next task.
    fn seek(&mut self, _track: u32, _frame: u64) -> Result<SeekTo> {
        Err(Error::Unsupported("flac frames"))
    }
}
