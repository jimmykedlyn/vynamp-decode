//! Layer I decoding (ISO/IEC 11172-3 section 2.4.3.2 and annex C.3).
//!
//! A frame holds 12 time slots of 32 subband samples per channel:
//!
//! - CRC (optional, 16 bits), then 4-bit bit allocations for each subband (both channels
//!   below the joint stereo `bound`, one shared value from `bound` on); value 15 is forbidden
//!   and `n = allocation + 1` is the sample width (0 means the subband is silent);
//! - a 6-bit scalefactor index for every channel and subband with a nonzero allocation
//!   (63 is not allowed), the scalefactor being `2^(1 - index / 3)`;
//! - the samples, slot by slot, subband by subband, channel by channel (from `bound` on
//!   one sample is sent and used for both channels, with each channel's own scalefactor).
//!
//! A sample code `s` of `n` bits is requantized as
//! `(s - 2^(n-1) + 1) * 2 / (2^n - 1) * scalefactor` (the standard's "invert the MSB, add
//! 2^(1-n), multiply by 2^n / (2^n - 1)"). The all-ones code is not used by encoders; here it
//! decodes by the same formula instead of being rejected.
//!
//! The CRC-16 covers header bytes 2 and 3 and the allocation field (not the scalefactors).

use crate::synth::Synthesis;
use crate::tables::{MAX_SAMPLES, scalefactors};
use vynamp_decode_core::{BitReader, Error, Result, crc16_mpeg, crc16_update};
use vynamp_decode_mpa::{ChannelMode, FrameHeader};

/// Samples per channel in a Layer I frame.
pub(crate) const SAMPLES: usize = 384;

/// Decodes one Layer I frame into the first `SAMPLES` entries of each channel of `pcm`.
///
/// `data` is the whole frame including its header; `base` is the absolute position of its
/// first byte. The filterbank history in `synth` advances even if the frame turns out to be
/// damaged part way; the caller resets it in that case.
pub(crate) fn decode_frame(
    data: &[u8],
    header: &FrameHeader,
    base: u64,
    synth: &mut [Synthesis; 2],
    pcm: &mut [[f32; MAX_SAMPLES]; 2],
) -> Result<()> {
    let nch = usize::from(header.channels());
    let bound = if header.channel_mode == ChannelMode::JointStereo {
        4 + 4 * usize::from(header.mode_ext)
    } else {
        32
    };
    let start = if header.crc { 6 } else { 4 };
    let body = data.get(start..).ok_or(Error::Invalid {
        what: "unexpected eof",
        offset: base + data.len() as u64,
    })?;
    let at = |e: Error| match e {
        Error::Invalid { what, offset } => Error::Invalid {
            what,
            offset: base + start as u64 + offset,
        },
        other => other,
    };
    let mut r = BitReader::new(body);

    // Allocation by subband, then channel.
    let mut alloc = [[0u8; 2]; 32];
    let mut forbidden = None;
    let mut nibble = 0usize;
    for (sb, al) in alloc.iter_mut().enumerate() {
        let coded = if sb >= bound { 1 } else { nch };
        for a in al.iter_mut().take(coded) {
            *a = r.read_bits(4).map_err(at)? as u8;
            if *a == 15 && forbidden.is_none() {
                forbidden = Some(nibble / 2);
            }
            nibble += 1;
        }
        if coded == 1 {
            al[1] = al[0];
        }
    }
    if header.crc {
        // The allocation field is a whole number of bytes: 4 bits times 32..=64.
        let field = &body[..nibble / 2];
        let crc = crc16_update(crc16_mpeg(&data[2..4]), field);
        if crc != u16::from_be_bytes([data[4], data[5]]) {
            return Err(Error::Invalid {
                what: "mpeg crc",
                offset: base + 4,
            });
        }
    }
    if let Some(byte) = forbidden {
        return Err(Error::Invalid {
            what: "mpeg allocation",
            offset: base + (start + byte) as u64,
        });
    }

    // factor = scalefactor * 2 / (2^n - 1), so a sample is (s - 2^(n-1) + 1) * factor.
    let table = scalefactors();
    let mut factor = [[0.0f32; 32]; 2];
    for sb in 0..32 {
        for ch in 0..nch {
            let a = alloc[sb][ch];
            if a != 0 {
                let index = r.read_bits(6).map_err(at)?;
                if index == 63 {
                    return Err(at(Error::Invalid {
                        what: "mpeg scalefactor",
                        offset: r.byte_pos() as u64 - 1,
                    }));
                }
                factor[ch][sb] = table[index as usize] * 2.0 / ((1u32 << (a + 1)) - 1) as f32;
            }
        }
    }

    for slot in 0..12 {
        let mut sub = [[0.0f32; 32]; 2];
        for sb in 0..32 {
            let mut code = 0i32;
            for ch in 0..nch {
                let a = alloc[sb][ch];
                if a == 0 {
                    continue;
                }
                let n = u32::from(a) + 1;
                // Above the bound channel 1 reuses the sample just read for channel 0.
                if sb < bound || ch == 0 {
                    code = r.read_bits(n).map_err(at)? as i32;
                }
                sub[ch][sb] = (code - (1i32 << (n - 1)) + 1) as f32 * factor[ch][sb];
            }
        }
        for ch in 0..nch {
            let (chunks, _) = pcm[ch].as_chunks_mut::<32>();
            synth[ch].synth(&sub[ch], &mut chunks[slot]);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::MpegDecoder;
    use crate::common::{L1Spec, Rng, random_l1, track};
    use crate::reference::{RefAnalysis, RefSynth, dequant, l1_subbands};
    use vynamp_decode_core::{AudioBuffer, CodecId, Decoder, Options, Packet};

    fn decoder(channels: u16, rate: u32) -> MpegDecoder {
        MpegDecoder::new(&track(CodecId::Mp1, rate, channels), &Options::default()).unwrap()
    }

    fn decode(d: &mut MpegDecoder, frame: Vec<u8>, pts: u64) -> AudioBuffer {
        let mut out = AudioBuffer::new();
        let packet = Packet {
            pts,
            frames: 384,
            data: frame,
            ..Packet::default()
        };
        d.decode(&packet, &mut out).unwrap();
        out
    }

    /// Decodes `specs` with the decoder and with the reference; returns the largest difference.
    fn compare(specs: &[L1Spec], rate: u32) -> f64 {
        let nch = specs[0].channels();
        let mut d = decoder(nch as u16, rate);
        let mut refs: Vec<RefSynth> = (0..nch).map(|_| RefSynth::new()).collect();
        let mut worst = 0.0f64;
        let mut peak = 0.0f64;
        for (n, spec) in specs.iter().enumerate() {
            let out = decode(&mut d, spec.build(), (n * 384) as u64);
            let sb = l1_subbands(spec);
            for ch in 0..nch {
                let got = out.plane_f32(ch).unwrap();
                assert_eq!(got.len(), 384);
                for slot in 0..12 {
                    let want = refs[ch].slot(&sb[ch][slot]);
                    for j in 0..32 {
                        let w = want[j];
                        peak = peak.max(w.abs());
                        worst = worst.max((f64::from(got[slot * 32 + j]) - w).abs());
                    }
                }
            }
        }
        assert!(peak > 0.05, "test signal too quiet: {peak}");
        worst
    }

    #[test]
    fn random_frames_match_reference_in_every_channel_mode() {
        let mut rng = Rng(0xC0FF_EE11);
        let cases: [(u8, u8); 7] = [(3, 0), (0, 0), (2, 0), (1, 0), (1, 1), (1, 2), (1, 3)];
        for (mode, ext) in cases {
            for crc in [false, true] {
                let specs: Vec<L1Spec> = (0..4)
                    .map(|_| random_l1(&mut rng, mode, ext, crc))
                    .collect();
                let e = compare(&specs, 32000);
                assert!(e < 5e-6, "mode {mode} ext {ext} crc {crc}: max error {e}");
            }
        }
    }

    #[test]
    fn mpeg2_layer1_matches_reference() {
        let mut rng = Rng(7);
        let mut s = random_l1(&mut rng, 3, 0, false);
        s.mpeg1 = false;
        s.rate_idx = 0; // 22.05 kHz
        s.bitrate_idx = 14; // 224 kbit/s: 48 * 224000 / 22050 = 487 bytes
        let e = compare(&[s], 22050);
        assert!(e < 5e-6, "max error {e}");
    }

    #[test]
    fn requantization_endpoints() {
        // 2-bit samples: codes 0, 1, 2 are -2/3, 0, +2/3 of the scalefactor 2.0 (index 0).
        assert!((dequant(0, 2, 0) + 2.0 * 2.0 / 3.0).abs() < 1e-12);
        assert!(dequant(1, 2, 0).abs() < 1e-12);
        assert!((dequant(2, 2, 0) - 2.0 * 2.0 / 3.0).abs() < 1e-12);
        // 15-bit: symmetric, just below full scale.
        let top = dequant(32766, 15, 3);
        assert!((top - (1.0 - 1.0 / 32767.0)).abs() < 1e-9, "{top}");
        assert!((dequant(0, 15, 3) + top).abs() < 1e-12);
    }

    /// Quantizes `input` (mono) with the standard analysis filterbank into Layer I frames
    /// (15-bit samples in the 16 lowest subbands, which hold the test signal).
    fn encode(input: &[f64], rate_idx: u8) -> Vec<Vec<u8>> {
        let mut an = RefAnalysis::new();
        let mut frames = Vec::new();
        for chunk in input.chunks(384) {
            let mut sub = [[0.0f64; 32]; 12];
            for (slot, s) in sub.iter_mut().enumerate() {
                let mut x = [0.0; 32];
                for (i, v) in x.iter_mut().enumerate() {
                    *v = chunk.get(slot * 32 + i).copied().unwrap_or(0.0);
                }
                *s = an.slot(&x);
            }
            let mut spec = L1Spec::silent(3);
            spec.rate_idx = rate_idx;
            spec.bitrate_idx = 14;
            for sb in 0..16 {
                let peak = sub.iter().map(|s| s[sb].abs()).fold(0.0, f64::max);
                let scf = (0..63u8)
                    .rev()
                    .find(|&i| 2f64.powf(1.0 - f64::from(i) / 3.0) >= peak)
                    .unwrap_or(0);
                spec.alloc[0][sb] = 14;
                spec.scf[0][sb] = scf;
                for (slot, s) in sub.iter().enumerate() {
                    let scale = 2f64.powf(1.0 - f64::from(scf) / 3.0);
                    let level = (s[sb] * 32767.0 / (2.0 * scale)).round() as i64;
                    spec.samples[slot][sb][0] = (level - 1 + 16384).clamp(0, 32766) as u32;
                }
            }
            frames.push(spec.build());
        }
        frames
    }

    /// Measures the decoder delay of Layer I/II: a signal through the standard's analysis
    /// filterbank, a high-resolution Layer I frame sequence and this decoder comes back
    /// delayed by the 481 samples of the two filterbanks.
    #[test]
    fn measured_filterbank_delay_is_481() {
        let n_frames = 14;
        let total = n_frames * 384;
        let tau = 2.0 * std::f64::consts::PI;
        let input: Vec<f64> = (0..total)
            .map(|n| {
                let t = n as f64 / 32000.0;
                let env = if n < 2000 { 1.0 } else { 0.5 };
                0.4 * env * (tau * 440.0 * t).sin()
                    + 0.2 * (tau * 1700.0 * t).sin()
                    + 0.1 * (tau * 5300.0 * t).sin()
            })
            .collect();
        let frames = encode(&input, 2);
        let mut d = decoder(1, 32000);
        let mut decoded = Vec::new();
        for (n, f) in frames.into_iter().enumerate() {
            let out = decode(&mut d, f, (n * 384) as u64);
            decoded.extend_from_slice(out.plane_f32(0).unwrap());
        }
        assert_eq!(decoded.len(), total);
        let err_at = |lag: usize| -> f64 {
            (0..total - 1000)
                .map(|n| (f64::from(decoded[n + lag]) - input[n]).powi(2))
                .sum::<f64>()
                / (total - 1000) as f64
        };
        let best = (0..1000)
            .min_by(|&a, &b| err_at(a).total_cmp(&err_at(b)))
            .unwrap();
        println!(
            "measured Layer I decoder delay: {best} samples, rms error {:e}",
            err_at(best).sqrt()
        );
        assert_eq!(best, 481);
        // The prototype filter is not perfect reconstruction (ripple of about 0.07 dB), so the
        // residual is a fraction of a percent of the 0.3 rms signal, not quantization noise.
        assert!(err_at(best).sqrt() < 2e-3, "rms {}", err_at(best).sqrt());
    }
}
