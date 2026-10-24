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
//! data starts before the first frame the decoder has seen (the start of a stream, or a seek
//! without enough preroll) decodes to silence.
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
        self.max_frames_check(packet.pts, frames)?;
        out.reset(SampleFormat::F32, self.channels, frames)?;
        for c in 0..usize::from(self.channels) {
            if let Some(p) = out.plane_f32_mut(c) {
                p.copy_from_slice(&self.pcm[c][..frames]);
            }
        }
        Ok(())
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
    }

    fn drain_events(&mut self, out: &mut Vec<Event>) {
        out.append(&mut self.events);
    }
}
