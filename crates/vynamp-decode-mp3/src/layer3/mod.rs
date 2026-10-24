//! Layer III bitstream layer: side information, bit reservoir, scalefactors and Huffman
//! decoding (ISO/IEC 11172-3 section 2.4.2 and 2.4.3.4, ISO/IEC 13818-3).
//!
//! [`decode_granules`] takes one frame and produces, for every granule and channel, the integer
//! spectral lines (before requantization), the raw scalefactors and the side information that
//! the synthesis path needs. [`Layer3Decoder`] runs the rest of the chain on that output:
//! requantization, joint stereo, short block reordering, alias reduction, the inverse MDCT
//! with overlap-add, and the polyphase synthesis shared with Layers I and II. Each module of
//! the chain documents its step.
//!
//! # Frame layout and the reservoir
//!
//! A frame is the header, an optional CRC, the side information and then "main data". The main
//! data of a granule (its scalefactors followed by its Huffman codes, `part2_3_length` bits in
//! all) does not have to sit in the frame whose side information describes it: the first
//! granule of a frame starts `main_data_begin` bytes before the frame's own main data, in the
//! unused tail of earlier frames. The decoder therefore keeps the main data of the last frames
//! (the reservoir), glues the needed tail in front of the current frame's main data and reads
//! the granules from that buffer one after the other.
//!
//! If the reservoir does not reach back far enough (the first frames of a stream, or the first
//! frames after a seek) the granules that start in the missing part cannot be decoded; they
//! are reported with [`GranuleStatus::NeedsPreroll`] and the synthesis outputs silence for
//! them. That is expected inside the preroll the demuxer requests after a seek; outside it
//! the caller treats it as a damaged stream (Strict: `Invalid { what: "mp3 reservoir" }`,
//! Resilient: `CorruptFrame`). A frame that fails to decode still adds its main data to the
//! reservoir, because the following frames may point into it.

mod alias;
mod bits;
mod codes;
mod huffman;
mod imdct;
mod requant;
mod reservoir;
mod scalefactors;
mod sideinfo;
mod stereo;
mod tables;
#[cfg(test)]
mod testenc;
#[cfg(test)]
mod tests;

use crate::synth::Synthesis;
use bits::Bits;
use imdct::Overlap;
use reservoir::Reservoir;
use scalefactors::Params;
use sideinfo::SideInfo;
use vynamp_decode_core::{Error, Result, crc16_mpeg, crc16_update};
use vynamp_decode_mpa::{ChannelMode, FrameHeader, MpegVersion};

pub(crate) use scalefactors::Layout;
pub(crate) use sideinfo::GranuleInfo;

/// Whether the data of a granule was available.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GranuleStatus {
    /// Decoded; the lines and scalefactors are valid.
    Decoded,
    /// The granule starts in main data that precedes the first frame the decoder has seen
    /// (the start of the stream or just after a seek). Its lines are zero; the output for it
    /// is silence. Expected inside the preroll the demuxer asked for.
    NeedsPreroll,
}

/// One granule of one channel.
#[derive(Clone)]
pub(crate) struct Granule {
    pub status: GranuleStatus,
    pub info: GranuleInfo,
    pub layout: Layout,
    /// Raw scalefactors by layout entry (see [`Layout`]); 0 where none is transmitted.
    pub scalefac: [u8; 40],
    /// Bit `i` set: entry `i` is the all-ones value of an intensity stereo right channel in
    /// MPEG-2/2.5, an illegal intensity position. (In MPEG-1 the illegal position is the
    /// scalefactor value 7.)
    pub illegal_is_pos: u64,
    /// The `preflag` in effect (parsed in MPEG-1, derived in MPEG-2/2.5).
    pub preflag: bool,
    /// Spectral lines in transmission order, signed, before requantization.
    pub xr: [i16; 576],
    /// Lines before the all-zero tail.
    pub used_lines: usize,
    /// Bits of part 2 (scalefactors) consumed.
    pub part2_bits: usize,
    /// Bits of part 2 and 3 consumed; equals `info.part2_3_length` unless the count1 region
    /// ended on stuffing bits.
    pub bits_used: usize,
}

impl Granule {
    fn empty() -> Self {
        Self {
            status: GranuleStatus::NeedsPreroll,
            info: GranuleInfo::default(),
            layout: Layout::new(0, MpegVersion::V1, &GranuleInfo::default()),
            scalefac: [0; 40],
            illegal_is_pos: 0,
            preflag: false,
            xr: [0; 576],
            used_lines: 0,
            part2_bits: 0,
            bits_used: 0,
        }
    }

    /// Resets to a granule without data.
    fn clear(&mut self, status: GranuleStatus, info: GranuleInfo, layout: Layout) {
        self.status = status;
        self.info = info;
        self.layout = layout;
        self.scalefac = [0; 40];
        self.illegal_is_pos = 0;
        self.preflag = false;
        self.xr = [0; 576];
        self.used_lines = 0;
        self.part2_bits = 0;
        self.bits_used = 0;
    }
}

/// The output of [`decode_granules`] for one frame.
pub(crate) struct FrameData {
    pub channels: usize,
    /// 2 for MPEG-1, 1 for MPEG-2/2.5.
    pub granules: usize,
    /// By granule, then channel.
    pub gr: [[Granule; 2]; 2],
}

impl FrameData {
    pub(crate) fn new() -> Box<Self> {
        Box::new(Self {
            channels: 0,
            granules: 0,
            gr: [
                [Granule::empty(), Granule::empty()],
                [Granule::empty(), Granule::empty()],
            ],
        })
    }

    /// Some granule could not be decoded for lack of reservoir data. (The decoder turns such
    /// granules into silence; the tests use this to check where they occur.)
    #[cfg(test)]
    pub(crate) fn needs_preroll(&self) -> bool {
        (0..self.granules).any(|g| {
            (0..self.channels).any(|c| self.gr[g][c].status == GranuleStatus::NeedsPreroll)
        })
    }
}

/// The state kept between frames: the bit reservoir and a scratch buffer.
#[derive(Default)]
pub(crate) struct Layer3 {
    reservoir: Reservoir,
    work: Vec<u8>,
}

impl Layer3 {
    /// Forgets the reservoir (after a seek).
    pub(crate) fn reset(&mut self) {
        self.reservoir.clear();
    }
}

/// Decoder state of a Layer III stream: the bit reservoir, the overlap of the inverse MDCT of
/// each channel and the scratch buffers of the synthesis path. Nothing is allocated per frame
/// once the reservoir buffers have reached their size.
pub(crate) struct Layer3Decoder {
    bits: Layer3,
    frame: Box<FrameData>,
    overlap: [Overlap; 2],
    /// Requantized lines of both channels.
    xr: Box<[[f32; 576]; 2]>,
}

impl Layer3Decoder {
    pub(crate) fn new() -> Self {
        requant::warm_up();
        stereo::warm_up();
        alias::warm_up();
        imdct::warm_up();
        Self {
            bits: Layer3::default(),
            frame: FrameData::new(),
            overlap: [Overlap::new(), Overlap::new()],
            xr: Box::new([[0.0; 576]; 2]),
        }
    }

    /// Forgets the reservoir and the overlap (after a seek).
    pub(crate) fn reset(&mut self) {
        self.bits.reset();
        self.clear_overlap();
    }

    /// Forgets the overlap only (after a frame was replaced by silence).
    pub(crate) fn clear_overlap(&mut self) {
        for o in &mut self.overlap {
            o.clear();
        }
    }

    /// Decodes one frame into `pcm` (576 samples per granule and channel) through the
    /// per-channel `synth` filterbanks and returns the number of samples per channel.
    ///
    /// A granule that could not be decoded for lack of reservoir data
    /// ([`GranuleStatus::NeedsPreroll`]) becomes silence and restarts the channel's overlap and
    /// filterbank, so that the stream resumes cleanly.
    ///
    /// # Errors
    /// The errors of [`decode_granules`]; the filterbank state is untouched then.
    pub(crate) fn decode(
        &mut self,
        frame: &[u8],
        header: &FrameHeader,
        offset: u64,
        synth: &mut [Synthesis; 2],
        pcm: &mut [[f32; crate::tables::MAX_SAMPLES]; 2],
    ) -> Result<usize> {
        decode_granules(&mut self.bits, frame, header, offset, &mut self.frame)?;
        let channels = self.frame.channels;
        let joint = header.channel_mode == ChannelMode::JointStereo && channels == 2;
        let tools = stereo::Stereo {
            version: header.version,
            ms: joint && header.mode_ext & 2 != 0,
            intensity: joint && header.mode_ext & 1 != 0,
        };
        let mut slots = [[0.0f32; 32]; 18];
        let mut slot_pcm = [0.0f32; 32];
        for gr in 0..self.frame.granules {
            let granules = &self.frame.gr[gr];
            let mut present = [false; 2];
            for ch in 0..channels {
                present[ch] = granules[ch].status == GranuleStatus::Decoded;
                if present[ch] {
                    requant::requantize(&granules[ch], &mut self.xr[ch]);
                } else {
                    self.xr[ch].fill(0.0);
                }
            }
            // A joint stereo granule needs both channels: with one undecodable the pair is
            // silent.
            if (tools.ms || tools.intensity) && present != [true, true] {
                present = [false; 2];
            }
            let mut used = [granules[0].used_lines, granules[1].used_lines];
            if (tools.ms || tools.intensity) && present == [true, true] {
                let [l, r] = &mut *self.xr;
                stereo::process(l, r, [&granules[0], &granules[1]], tools);
                used = [used[0].max(used[1]); 2];
            }
            for ch in 0..channels {
                let out = &mut pcm[ch][gr * 576..(gr + 1) * 576];
                if !present[ch] {
                    self.overlap[ch].clear();
                    synth[ch].reset();
                    out.fill(0.0);
                    continue;
                }
                let info = &granules[ch].info;
                let xr = &mut self.xr[ch];
                if info.block_type == 2 {
                    requant::reorder(xr, &granules[ch].layout);
                }
                let long_sb = granules[ch].layout.long_subbands();
                alias::reduce(xr, used[ch], long_sb.saturating_sub(1));
                self.overlap[ch].granule(xr, info, long_sb, &mut slots);
                let (chunks, _) = out.as_chunks_mut::<32>();
                for (slot, o) in slots.iter().zip(chunks) {
                    synth[ch].synth(slot, &mut slot_pcm);
                    *o = slot_pcm;
                }
            }
        }
        Ok(self.frame.granules * 576)
    }
}

/// Whether a granule that starts `start_bits` after the first main data bit of the current
/// frame (negative: before it) lies within the `available` bytes of reservoir.
fn reaches(start_bits: i64, available: usize) -> bool {
    start_bits >= -8 * available as i64
}

/// Decodes the bitstream layer of one whole frame (`frame` starts at the header; `offset` is
/// its absolute position in the source). The contents of `out` are unspecified after an error.
///
/// # Errors
/// `Invalid` with `what` `"mp3 side info"` (too short or impossible side information),
/// `"mpeg crc"` or `"mp3 huffman"` (scalefactors or Huffman data that do not fit in
/// `part2_3_length` or the data of the frame, or an unusable table). A reservoir that does not
/// reach back far enough is not an error: the affected granules get
/// [`GranuleStatus::NeedsPreroll`] and the caller decides whether that is expected.
pub(crate) fn decode_granules(
    state: &mut Layer3,
    frame: &[u8],
    header: &FrameHeader,
    offset: u64,
    out: &mut FrameData,
) -> Result<()> {
    let side_start = 4 + if header.crc { 2 } else { 0 };
    let main_start = side_start + header.side_info_len();
    if frame.len() < main_start {
        return Err(Error::Invalid {
            what: "mp3 side info",
            offset: offset + frame.len() as u64,
        });
    }
    let result = decode_frame(state, frame, header, offset, main_start, out);
    // The main data counts for later frames whatever became of this one.
    state.reservoir.push(&frame[main_start..]);
    result
}

fn decode_frame(
    state: &mut Layer3,
    frame: &[u8],
    header: &FrameHeader,
    offset: u64,
    main_start: usize,
    out: &mut FrameData,
) -> Result<()> {
    let side_start = main_start - header.side_info_len();
    let side = &frame[side_start..main_start];
    if header.crc {
        let crc = crc16_update(crc16_mpeg(&frame[2..4]), side);
        if crc != u16::from_be_bytes([frame[4], frame[5]]) {
            return Err(Error::Invalid {
                what: "mpeg crc",
                offset: offset + 4,
            });
        }
    }
    let si: SideInfo = sideinfo::parse(side, header, offset + side_start as u64)?;
    let row = tables::sfb_row(header.version, header.sample_rate).ok_or(Error::Invalid {
        what: "mp3 side info",
        offset,
    })?;
    let main = &frame[main_start..];
    let begin = usize::from(si.main_data_begin);
    // The reservoir bytes in front of the frame's main data that a granule can use: none are
    // needed beyond `begin`.
    let avail = state.reservoir.len().min(begin);
    state.work.clear();
    state.work.extend_from_slice(state.reservoir.tail(avail));
    state.work.extend_from_slice(main);
    let work = &state.work[..];

    let v1 = header.version == MpegVersion::V1;
    out.channels = usize::from(header.channels());
    out.granules = if v1 { 2 } else { 1 };
    // Absolute position of a bit of the work buffer, for errors: a byte inside the current
    // frame is exact; one in the reservoir belongs to an earlier frame and maps to the start
    // of this frame's main data.
    let at = |work_bit: usize| -> u64 {
        let byte = work_bit / 8;
        offset + main_start as u64 + byte.saturating_sub(avail) as u64
    };
    let main_bits = main.len() as i64 * 8;
    // Position of the next granule relative to the first bit of the frame's main data.
    let mut start = -(begin as i64) * 8;
    for gr in 0..out.granules {
        for ch in 0..out.channels {
            let info = si.granules[gr][ch];
            let begin_v = start;
            let end = begin_v + i64::from(info.part2_3_length);
            start = end;
            if end > main_bits {
                return Err(Error::Invalid {
                    what: "mp3 huffman",
                    offset: at(work.len() * 8),
                });
            }
            let layout = Layout::new(row, header.version, &info);
            // Granule 1 may reuse granule 0's scalefactors of long blocks.
            let scfsi = if gr == 1 && v1 { si.scfsi[ch] } else { 0 };
            let shares = scfsi != 0 && layout.n_short == 0;
            let usable = reaches(begin_v, avail)
                && !(shares && out.gr[0][ch].status != GranuleStatus::Decoded);
            if !usable {
                out.gr[gr][ch].clear(GranuleStatus::NeedsPreroll, info, layout);
                continue;
            }
            let first = (begin_v + avail as i64 * 8) as usize;
            let last = first + usize::from(info.part2_3_length);
            let prev = out.gr[0][ch].scalefac;
            let g = &mut out.gr[gr][ch];
            g.clear(GranuleStatus::Decoded, info, layout);
            let mut bits = Bits::new(work, first);
            let sf = scalefactors::read(
                &mut bits,
                &Params {
                    version: header.version,
                    gi: &info,
                    layout: &layout,
                    scfsi: if shares { scfsi } else { 0 },
                    intensity_right: header.channel_mode == ChannelMode::JointStereo
                        && header.mode_ext & 1 == 1
                        && ch == 1,
                },
                &prev,
            );
            if bits.pos() > last {
                return Err(Error::Invalid {
                    what: "mp3 huffman",
                    offset: at(bits.pos()),
                });
            }
            g.scalefac = sf.values;
            g.illegal_is_pos = sf.illegal;
            g.preflag = sf.preflag;
            g.part2_bits = bits.pos() - first;
            let spectrum = huffman::decode_spectrum(&mut bits, last, &info, &layout, &mut g.xr)
                .map_err(|pos| Error::Invalid {
                    what: "mp3 huffman",
                    offset: at(pos),
                })?;
            g.used_lines = spectrum.used_lines;
            g.bits_used = bits.pos() - first;
        }
    }
    Ok(())
}
