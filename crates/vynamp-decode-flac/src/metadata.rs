//! FLAC metadata blocks (RFC 9639 section 8).

use vynamp_decode_core::{BitReader, ByteReader, Error, LimitKind, Limits, Result};

/// Size of the STREAMINFO block body in bytes.
pub(crate) const STREAMINFO_LEN: usize = 34;

/// Size of one SEEKTABLE entry in bytes.
const SEEKPOINT_LEN: usize = 18;

/// Seek point sample number that marks a placeholder entry.
const PLACEHOLDER: u64 = u64::MAX;

/// Contents of the STREAMINFO block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamInfo {
    /// Minimum block size in samples.
    pub min_block: u16,
    /// Maximum block size in samples.
    pub max_block: u16,
    /// Minimum frame size in bytes (0 = unknown).
    pub min_frame: u32,
    /// Maximum frame size in bytes (0 = unknown).
    pub max_frame: u32,
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// Channel count (1 to 8).
    pub channels: u8,
    /// Bits per sample (4 to 32).
    pub bits_per_sample: u8,
    /// Total samples per channel (0 = unknown).
    pub total_samples: u64,
    /// MD5 of the unencoded audio (all zero = not computed).
    pub md5: [u8; 16],
}

/// One entry of the SEEKTABLE block (placeholders are dropped).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeekPoint {
    /// First sample number of the target frame.
    pub sample: u64,
    /// Byte offset of the target frame from the first frame.
    pub offset: u64,
    /// Samples in the target frame.
    pub frames: u16,
}

/// Metadata block type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    /// STREAMINFO (0).
    StreamInfo,
    /// PADDING (1).
    Padding,
    /// APPLICATION (2).
    Application,
    /// SEEKTABLE (3).
    SeekTable,
    /// VORBIS_COMMENT (4).
    VorbisComment,
    /// CUESHEET (5).
    CueSheet,
    /// PICTURE (6).
    Picture,
    /// Reserved type (7 to 126).
    Reserved(u8),
}

/// An unparsed metadata block, kept for tag and picture parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawBlock {
    /// Block type.
    pub kind: BlockKind,
    /// Block body.
    pub data: Vec<u8>,
}

impl StreamInfo {
    /// Parses and validates a 34-byte STREAMINFO body that starts at `offset`.
    fn parse(body: &[u8; STREAMINFO_LEN], offset: u64) -> Result<Self> {
        let bad = |what| Error::Invalid { what, offset };
        let mut b = BitReader::new(body);
        let min_block = b.read_bits(16)? as u16;
        let max_block = b.read_bits(16)? as u16;
        let min_frame = b.read_bits(24)?;
        let max_frame = b.read_bits(24)?;
        let sample_rate = b.read_bits(20)?;
        let channels = b.read_bits(3)? as u8 + 1;
        let bits_per_sample = b.read_bits(5)? as u8 + 1;
        let total_samples = b.read_bits_u64(36)?;
        let mut md5 = [0u8; 16];
        for m in &mut md5 {
            *m = b.read_bits(8)? as u8;
        }
        if sample_rate == 0 {
            return Err(bad("sample rate"));
        }
        if !(4..=32).contains(&bits_per_sample) {
            return Err(bad("bits per sample"));
        }
        // Blocks under 16 samples are only valid in a stream of one short frame.
        if max_block == 0 || min_block > max_block || (min_block < 16 && max_block > 16) {
            return Err(bad("block sizes"));
        }
        Ok(Self {
            min_block,
            max_block,
            min_frame,
            max_frame,
            sample_rate,
            channels,
            bits_per_sample,
            total_samples,
            md5,
        })
    }
}

fn kind_of(code: u8) -> Option<BlockKind> {
    Some(match code {
        0 => BlockKind::StreamInfo,
        1 => BlockKind::Padding,
        2 => BlockKind::Application,
        3 => BlockKind::SeekTable,
        4 => BlockKind::VorbisComment,
        5 => BlockKind::CueSheet,
        6 => BlockKind::Picture,
        7..=126 => BlockKind::Reserved(code),
        _ => return None,
    })
}

fn parse_seek_table(data: &[u8]) -> Vec<SeekPoint> {
    let be64 = |b: &[u8]| u64::from_be_bytes(b.try_into().unwrap_or([0; 8]));
    data.as_chunks::<SEEKPOINT_LEN>()
        .0
        .iter()
        .filter_map(|e| {
            let sample = be64(&e[..8]);
            (sample != PLACEHOLDER).then(|| SeekPoint {
                sample,
                offset: be64(&e[8..16]),
                frames: u16::from_be_bytes([e[16], e[17]]),
            })
        })
        .collect()
}

/// Reads the metadata blocks that follow the `fLaC` marker.
///
/// Returns the STREAMINFO, the seek points, every non-padding block (in file
/// order, STREAMINFO first) and the absolute offset of the first frame. The
/// total size of the non-padding blocks is bounded by
/// `limits.max_metadata_bytes`; padding is skipped without being read.
pub(crate) fn read_metadata(
    r: &mut ByteReader,
    limits: &Limits,
) -> Result<(StreamInfo, Vec<SeekPoint>, Vec<RawBlock>, u64)> {
    let mut info: Option<StreamInfo> = None;
    let mut seek = Vec::new();
    let mut blocks = Vec::new();
    let mut total = 0u64;
    loop {
        let start = r.position();
        let head = r.u8()?;
        let last = head & 0x80 != 0;
        let len = u64::from(r.u8()?) << 16 | u64::from(r.u16_be()?);
        let body = r.position();
        let Some(kind) = kind_of(head & 0x7F) else {
            return Err(Error::Invalid {
                what: "block type",
                offset: start,
            });
        };
        if info.is_none() && kind != BlockKind::StreamInfo {
            return Err(Error::Invalid {
                what: "streaminfo first",
                offset: start,
            });
        }
        match kind {
            BlockKind::StreamInfo if info.is_some() => {
                return Err(Error::Invalid {
                    what: "duplicate streaminfo",
                    offset: start,
                });
            }
            BlockKind::StreamInfo if len != STREAMINFO_LEN as u64 => {
                return Err(Error::Invalid {
                    what: "streaminfo length",
                    offset: start,
                });
            }
            BlockKind::SeekTable if len % SEEKPOINT_LEN as u64 != 0 => {
                return Err(Error::Invalid {
                    what: "seektable length",
                    offset: start,
                });
            }
            BlockKind::Padding => r.skip(len)?,
            _ => {
                total += len;
                if total > u64::from(limits.max_metadata_bytes) {
                    return Err(Error::Limit(LimitKind::Metadata));
                }
                let mut data = vec![0u8; len as usize];
                r.read_exact(&mut data)?;
                match kind {
                    BlockKind::StreamInfo => {
                        let body_arr: &[u8; STREAMINFO_LEN] =
                            data.as_slice().try_into().map_err(|_| Error::Invalid {
                                what: "streaminfo length",
                                offset: start,
                            })?;
                        info = Some(StreamInfo::parse(body_arr, body)?);
                    }
                    BlockKind::SeekTable => seek.extend(parse_seek_table(&data)),
                    _ => {}
                }
                blocks.push(RawBlock { kind, data });
            }
        }
        if last {
            break;
        }
    }
    // `info` is set: the first block is checked to be STREAMINFO above.
    let info = info.ok_or(Error::Invalid {
        what: "streaminfo first",
        offset: r.position(),
    })?;
    Ok((info, seek, blocks, r.position()))
}
