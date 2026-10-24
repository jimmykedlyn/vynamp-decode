//! MPEG audio decoder (ISO/IEC 11172-3 and 13818-3).
//!
//! `MpegDecoder` decodes the packets of `vynamp-decode-mpa` (one whole frame per packet,
//! header included) into planar `F32` audio scaled so that full scale is plus or minus 1.0.
//! It dispatches on the track's codec: `Mp1` (Layer I), `Mp2` (Layer II) and `Mp3` (Layer III).
//!
//! The polyphase synthesis filterbank in the `synth` module is shared by all layers. Decoding
//! allocates nothing per packet once the output buffer has grown to the frame size.
//!
//! Layer III reads its main data through a bit reservoir that spans frames. A granule whose
//! data starts before the first frame the decoder has seen decodes to silence. That is
//! expected, and silent, in two places only:
//!
//! - inside the preroll announced with [`Decoder::set_preroll`] after a seek (packets that
//!   end at or before the announced position; the caller discards their output), and
//! - at the start of the stream (a first packet with `pts == 0`) or right after a gap of lost
//!   frames, if the first frame decoded points back before itself: until the reservoir holds
//!   the 511 bytes that `main_data_begin` can reach (the stream was cut out of a longer one,
//!   or the frames it needs were lost and already reported by the demuxer).
//!
//! Anywhere else the stream is damaged: `Mode::Strict` returns `Invalid { what: "mp3
//! reservoir" }` and `Mode::Resilient` replaces the frame by silence and reports a
//! `CorruptFrame`.
//!
//! Errors carry the absolute source offset (`Packet::offset` plus the position inside the
//! frame). In `Mode::Strict` a damaged frame (bad header, CRC mismatch, forbidden bit
//! allocation, truncated data) is an error; in `Mode::Resilient` it is replaced by silence of
//! its length and reported as `Event::CorruptFrame`. An empty packet (the demuxer's
//! Resilient gap marker) decodes to silence of `Packet::frames` frames. A Layer I/II
//! decoder's output lags its encoder's input by 481 samples (the filterbank delay); the
//! `Track::delay` is set by the demuxer.
#![warn(missing_docs)]

mod layer1;
mod layer2;
mod layer3;
mod synth;
mod tables;

#[cfg(test)]
#[path = "../tests/common/mod.rs"]
mod common;
#[cfg(test)]
mod reference;

use layer3::Layer3Decoder;
use synth::Synthesis;
use tables::MAX_SAMPLES;
use vynamp_decode_core::{
    AudioBuffer, CodecId, Decoder, Error, Event, Mode, Options, Packet, Result, SampleFormat, Track,
};
use vynamp_decode_mpa::parse_header;

/// The furthest back `main_data_begin` can reach into earlier frames' main data (9 bits).
const MAX_RESERVOIR_REACH: usize = 511;

/// Largest number of frames (per channel) a packet can decode to.
const MAX_FRAMES: u32 = MAX_SAMPLES as u32;

/// MPEG audio decoder for Layers I, II and III.
pub struct MpegDecoder {
    layer: u8,
    channels: u16,
    sample_rate: u32,
    mode: Mode,
    max_frames: u64,
    synth: [Synthesis; 2],
    layer3: Option<Box<Layer3Decoder>>,
    pcm: Box<[[f32; MAX_SAMPLES]; 2]>,
    events: Vec<Event>,
    /// End of the announced seek preroll (see [`Decoder::set_preroll`]).
    preroll_until: Option<u64>,
    /// No Layer III frame was decoded since the start, the last reset or the last gap.
    fresh: bool,
    /// The fresh state follows a gap of lost frames.
    after_gap: bool,
    /// The first frame of the fresh state pointed back before itself: missing reservoir data
    /// is expected until the reservoir holds `MAX_RESERVOIR_REACH` bytes.
    lead_in: bool,
}

impl MpegDecoder {
    /// Creates a decoder for an `Mp1`, `Mp2` or `Mp3` track.
    ///
    /// # Errors
    /// `Unsupported` for another codec; `Invalid` or `Limit` for a channel count or sample
    /// rate outside 1 or 2 channels and `opts.limits`.
    pub fn new(track: &Track, opts: &Options) -> Result<Self> {
        let layer = match track.codec {
            CodecId::Mp1 => 1,
            CodecId::Mp2 => 2,
            CodecId::Mp3 => 3,
            _ => return Err(Error::Unsupported("not an mpeg audio track")),
        };
        opts.limits.check_channels(track.channels)?;
        opts.limits.check_sample_rate(track.sample_rate)?;
        if track.channels > 2 {
            return Err(Error::Invalid {
                what: "channels",
                offset: 0,
            });
        }
        Ok(Self {
            layer,
            channels: track.channels,
            sample_rate: track.sample_rate,
            mode: opts.mode,
            max_frames: opts.limits.max_frames,
            synth: [Synthesis::new(), Synthesis::new()],
            layer3: (layer == 3).then(|| Box::new(Layer3Decoder::new())),
            pcm: Box::new([[0.0; MAX_SAMPLES]; 2]),
            events: Vec::new(),
            preroll_until: None,
            fresh: true,
            after_gap: false,
            lead_in: false,
        })
    }

    /// Fills `out` with `frames` frames of silence (clamped to the largest frame) and
    /// clears the filterbank history.
    fn silence(&mut self, pts: u64, frames: u32, out: &mut AudioBuffer) -> Result<()> {
        let frames = frames.min(MAX_FRAMES);
        if pts.saturating_add(u64::from(frames)) > self.max_frames {
            return Err(Error::Limit(vynamp_decode_core::LimitKind::Frames));
        }
        out.reset(SampleFormat::F32, self.channels, frames as usize)?;
        for c in 0..usize::from(self.channels) {
            if let Some(p) = out.plane_f32_mut(c) {
                p.fill(0.0);
            }
        }
        for s in &mut self.synth {
            s.reset();
        }
        if let Some(l3) = &mut self.layer3 {
            l3.clear_overlap();
        }
        Ok(())
    }

    fn decode_frame(&mut self, packet: &Packet, out: &mut AudioBuffer) -> Result<()> {
        let bad_header = Error::Invalid {
            what: "mpeg header",
            offset: packet.offset,
        };
        let head: [u8; 4] =
            packet
                .data
                .get(..4)
                .and_then(|h| h.try_into().ok())
                .ok_or(Error::Invalid {
                    what: "unexpected eof",
                    offset: packet.offset,
                })?;
        let header = parse_header(head).ok_or(bad_header.clone())?;
        if header.layer != self.layer
            || header.channels() != self.channels
            || header.sample_rate != self.sample_rate
        {
            return Err(bad_header);
        }
        let frames = if self.layer == 1 {
            layer1::decode_frame(
                &packet.data,
                &header,
                packet.offset,
                &mut self.synth,
                &mut self.pcm,
            )?;
            layer1::SAMPLES
        } else if let Some(l3) = &mut self.layer3 {
            l3.decode(
                &packet.data,
                &header,
                packet.offset,
                &mut self.synth,
                &mut self.pcm,
            )?
        } else {
            layer2::decode_frame(
                &packet.data,
                &header,
                packet.offset,
                &mut self.synth,
                &mut self.pcm,
            )?;
            layer2::SAMPLES
        };
        self.check_reservoir(packet, frames)?;
        self.max_frames_check(packet.pts, frames)?;
        out.reset(SampleFormat::F32, self.channels, frames)?;
        for c in 0..usize::from(self.channels) {
            if let Some(p) = out.plane_f32_mut(c) {
                p.copy_from_slice(&self.pcm[c][..frames]);
            }
        }
        Ok(())
    }

    /// Decides whether granules silenced for lack of reservoir data are expected here (see the
    /// crate documentation).
    fn check_reservoir(&mut self, packet: &Packet, frames: usize) -> Result<()> {
        let Some(l3) = &self.layer3 else {
            return Ok(());
        };
        let starved = l3.starved();
        if self.fresh {
            self.fresh = false;
            self.lead_in = starved && (packet.pts == 0 || self.after_gap);
            self.after_gap = false;
        }
        if !starved {
            return Ok(());
        }
        let in_preroll = self
            .preroll_until
            .is_some_and(|until| packet.pts.saturating_add(frames as u64) <= until);
        let in_lead_in = self.lead_in && l3.reservoir_before() < MAX_RESERVOIR_REACH;
        if in_preroll || in_lead_in {
            Ok(())
        } else {
            Err(Error::Invalid {
                what: "mp3 reservoir",
                offset: packet.offset,
            })
        }
    }

    fn max_frames_check(&self, pts: u64, frames: usize) -> Result<()> {
        if pts.saturating_add(frames as u64) > self.max_frames {
            return Err(Error::Limit(vynamp_decode_core::LimitKind::Frames));
        }
        Ok(())
    }
}

impl Decoder for MpegDecoder {
    fn decode(&mut self, packet: &Packet, out: &mut AudioBuffer) -> Result<()> {
        if packet.data.is_empty() {
            // Lost frames: their main data is gone with them.
            if let Some(l3) = &mut self.layer3 {
                l3.reset();
            }
            self.fresh = true;
            self.after_gap = true;
            self.lead_in = false;
            return self.silence(packet.pts, packet.frames, out);
        }
        match self.decode_frame(packet, out) {
            Err(Error::Invalid { .. }) if self.mode == Mode::Resilient => {
                let frames = parse_header(
                    packet
                        .data
                        .get(..4)
                        .and_then(|h| h.try_into().ok())
                        .unwrap_or([0; 4]),
                )
                .map_or(packet.frames, |h| h.samples);
                self.silence(packet.pts, frames, out)?;
                self.events.push(Event::CorruptFrame {
                    pts: packet.pts,
                    offset: packet.offset,
                    frames: out.frames() as u32,
                });
                Ok(())
            }
            other => other,
        }
    }

    fn reset(&mut self) {
        for s in &mut self.synth {
            s.reset();
        }
        if let Some(l3) = &mut self.layer3 {
            l3.reset();
        }
        self.preroll_until = None;
        self.fresh = true;
        self.after_gap = false;
        self.lead_in = false;
    }

    fn set_preroll(&mut self, until: u64) {
        self.preroll_until = Some(until);
    }

    fn drain_events(&mut self, out: &mut Vec<Event>) {
        out.append(&mut self.events);
    }
}
