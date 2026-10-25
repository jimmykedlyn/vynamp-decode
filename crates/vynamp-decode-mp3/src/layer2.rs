//! Layer II decoding (ISO/IEC 11172-3 section 2.4.3.3 and annex C.1.5.3, ISO/IEC 13818-3).
//!
//! A frame holds 36 time slots of 32 subband samples per channel, coded as 12 parts of three
//! samples per subband:
//!
//! - CRC (optional, 16 bits);
//! - bit allocation for the subbands below `sblimit` (the number of coded subbands, 8 to 30,
//!   fixed by the version, sample rate and bitrate per channel), the field being 2 to 4 bits
//!   wide depending on the subband. Subbands from the joint stereo `bound` on carry one
//!   allocation for both channels. The allocation code selects a quantization class;
//! - for every channel and subband with a nonzero class, a 2-bit `scfsi` that says how the
//!   three scalefactors (one per group of four parts) are transmitted;
//! - the scalefactors (6-bit indices, 63 not allowed, the factor being `2^(1 - index / 3)`):
//!   `scfsi` 0 sends three, 1 sends one for the first two groups and one for the last, 2
//!   sends one for all three, 3 sends one for the first and one for the last two;
//! - 12 parts, each holding for every subband and channel (above the bound one set for both
//!   channels, each channel still using its own scalefactor) three samples. A class of 3, 5
//!   or 9 levels packs the three samples into one code of 5, 7 or 10 bits
//!   (`c0 + n c1 + n^2 c2`); any other class is 3 to 16 bits per sample.
//!
//! A sample code `c` of a class with `n` levels (`n = 2^bits - 1` when ungrouped) is
//! requantized as `(c - (n - 1) / 2) * 2 / n * scalefactor`, which is the standard's "invert
//! the MSB, add `2^(1 - bits)`, multiply by `2^bits / (2^bits - 1)"` for ungrouped samples and
//! the same with its tabulated constants for grouped ones.
//!
//! The CRC-16 covers header bytes 2 and 3, the allocation field and the `scfsi` field, which
//! is not a whole number of bytes.

use crate::synth::Synthesis;
use crate::tables::{MAX_SAMPLES, scalefactors};
use vynamp_decode_core::{BitReader, Error, Result, crc16_mpeg};
use vynamp_decode_mpa::{ChannelMode, FrameHeader, MpegVersion};

/// Samples per channel in a Layer II frame.
pub(crate) const SAMPLES: usize = 1152;

/// Quantization class by allocation code, as rows of one table: 0 is "no samples",
/// 3 to 16 the number of bits of an ungrouped sample (`2^bits - 1` levels), and 17, 18 and 19
/// three samples grouped in one code of 3, 5 and 9 levels. Rows are referred to by offset.
const CLASSES: [u8; 76] = [
    // offset 0: 4 bits, low subbands of the 27/30 band tables.
    0, 17, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, //
    // offset 16: 4 bits.
    0, 17, 18, 3, 19, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 16, //
    // offset 32: 3 bits.
    0, 17, 18, 3, 19, 4, 5, 16, //
    // offset 40: 2 bits.
    0, 17, 18, 16, //
    // offset 44: 4 bits (8/12 band low rate tables), also the first 8 / 4 entries serve the
    // 3-bit / 2-bit fields of those tables and of the MPEG-2 tables.
    0, 17, 18, 19, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, //
    // offset 60: 4 bits (MPEG-2).
    0, 17, 18, 3, 19, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14,
];

/// A run of `count` subbands whose allocation field is `width` bits wide and whose codes
/// are looked up in `CLASSES` from `offset`.
struct Group {
    offset: u8,
    width: u8,
    count: u8,
}

const fn g(offset: u8, width: u8, count: u8) -> Group {
    Group {
        offset,
        width,
        count,
    }
}

/// MPEG-1 tables a and b (27 and 30 subbands; the counts add up to 30).
const MPEG1_FULL: [Group; 4] = [g(0, 4, 3), g(16, 4, 8), g(32, 3, 12), g(40, 2, 7)];
/// MPEG-1 tables c and d (8 and 12 subbands).
const MPEG1_LOW: [Group; 2] = [g(44, 4, 2), g(44, 3, 10)];
/// The MPEG-2 (LSF) table of 30 subbands.
const LSF: [Group; 3] = [g(60, 4, 4), g(44, 3, 7), g(44, 2, 19)];

/// The allocation groups and the number of coded subbands (`sblimit`) for `header` and a frame
/// of `frame_len` bytes.
///
/// MPEG-1 by bitrate per channel and sample rate (free format: the bitrate the frame length
/// stands for, `(frame_len - padding) * sample rate / 144000` kbit/s, rounded):
/// below 56 kbit/s the low rate tables (12 subbands at 32 kHz, 8 otherwise); from 96 kbit/s
/// 30 subbands except at 48 kHz; otherwise 27.
fn select(header: &FrameHeader, frame_len: usize) -> (&'static [Group], usize) {
    if header.version != MpegVersion::V1 {
        return (&LSF, 30);
    }
    let kbps = match header.bitrate_kbps {
        0 => {
            let body = frame_len.saturating_sub(usize::from(header.padding)) as u64;
            let total = (body * u64::from(header.sample_rate) + 72_000) / 144_000;
            let total = u32::try_from(total).unwrap_or(u32::MAX);
            if header.channel_mode == ChannelMode::Mono {
                total
            } else {
                total / 2
            }
        }
        k if header.channel_mode == ChannelMode::Mono => k,
        k => k / 2,
    };
    if kbps < 56 {
        (&MPEG1_LOW, if header.sample_rate == 32000 { 12 } else { 8 })
    } else if kbps >= 96 && header.sample_rate != 48000 {
        (&MPEG1_FULL, 30)
    } else {
        (&MPEG1_FULL, 27)
    }
}

/// CRC-16 (polynomial 0x8005, MSB first) continued over the first `nbits` bits of `bytes`.
fn crc_bits(mut crc: u16, bytes: &[u8], nbits: usize) -> u16 {
    for i in 0..nbits {
        let bit = u16::from(bytes[i / 8] >> (7 - i % 8)) & 1;
        let feedback = (crc >> 15) ^ bit;
        crc <<= 1;
        if feedback != 0 {
            crc ^= 0x8005;
        }
    }
    crc
}

/// Reads the three samples of one subband of one part and requantizes them with unit
/// scalefactor.
fn read_triplet(r: &mut BitReader, class: u8) -> Result<[f32; 3]> {
    let mut out = [0.0f32; 3];
    if class >= 17 {
        let (levels, bits) = match class {
            17 => (3u32, 5),
            18 => (5, 7),
            _ => (9, 10),
        };
        let mut code = r.read_bits(bits)?;
        for o in &mut out {
            *o = ((code % levels) as i32 * 2 - (levels as i32 - 1)) as f32 / levels as f32;
            code /= levels;
        }
    } else {
        let bits = u32::from(class);
        let half = (1i32 << (bits - 1)) - 1;
        let scale = 2.0 / ((1u32 << bits) - 1) as f32;
        for o in &mut out {
            *o = (r.read_bits(bits)? as i32 - half) as f32 * scale;
        }
    }
    Ok(out)
}

/// Decodes one Layer II frame into the first `SAMPLES` entries of each channel of `pcm`.
///
/// `data` is the whole frame including its header; `base` is the absolute position of its
/// first byte. The filterbank history in `synth` advances even if the frame turns out to be
/// damaged part way; the caller resets it in that case.
// Channel and subband indices address several parallel arrays at once.
#[allow(clippy::needless_range_loop)]
pub(crate) fn decode_frame(
    data: &[u8],
    header: &FrameHeader,
    base: u64,
    synth: &mut [Synthesis; 2],
    pcm: &mut [[f32; MAX_SAMPLES]; 2],
) -> Result<()> {
    let nch = usize::from(header.channels());
    let (groups, sblimit) = select(header, data.len());
    let bound = if header.channel_mode == ChannelMode::JointStereo {
        (4 + 4 * usize::from(header.mode_ext)).min(sblimit)
    } else {
        sblimit
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

    // Quantization class by channel and subband; above the bound channel 1 shares channel 0's.
    let mut class = [[0u8; 32]; 2];
    let mut covered_bits = 0usize;
    let mut sb = 0;
    for grp in groups {
        for _ in 0..grp.count {
            if sb == sblimit {
                break;
            }
            let coded = if sb < bound { nch } else { 1 };
            for ch in 0..coded {
                let code = r.read_bits(u32::from(grp.width)).map_err(at)? as usize;
                class[ch][sb] = CLASSES
                    .get(usize::from(grp.offset) + code)
                    .copied()
                    .unwrap_or(0);
                covered_bits += usize::from(grp.width);
            }
            if coded == 1 {
                class[1][sb] = class[0][sb];
            }
            sb += 1;
        }
    }

    let mut scfsi = [[0u8; 32]; 2];
    for sb in 0..sblimit {
        for ch in 0..nch {
            if class[ch][sb] != 0 {
                scfsi[ch][sb] = r.read_bits(2).map_err(at)? as u8;
                covered_bits += 2;
            }
        }
    }
    if header.crc {
        let crc = crc_bits(crc16_mpeg(&data[2..4]), body, covered_bits);
        if crc != u16::from_be_bytes([data[4], data[5]]) {
            return Err(Error::Invalid {
                what: "mpeg crc",
                offset: base + 4,
            });
        }
    }

    // Scalefactor values by channel, subband and group of four parts.
    let table = scalefactors();
    let mut factor = [[[0.0f32; 3]; 32]; 2];
    let scalefactor = |r: &mut BitReader| -> Result<f32> {
        let index = r.read_bits(6).map_err(at)?;
        if index == 63 {
            return Err(at(Error::Invalid {
                what: "mpeg scalefactor",
                offset: r.byte_pos() as u64 - 1,
            }));
        }
        Ok(table[index as usize])
    };
    for sb in 0..sblimit {
        for ch in 0..nch {
            if class[ch][sb] == 0 {
                continue;
            }
            factor[ch][sb] = match scfsi[ch][sb] {
                0 => [
                    scalefactor(&mut r)?,
                    scalefactor(&mut r)?,
                    scalefactor(&mut r)?,
                ],
                1 => {
                    let a = scalefactor(&mut r)?;
                    [a, a, scalefactor(&mut r)?]
                }
                2 => {
                    let a = scalefactor(&mut r)?;
                    [a, a, a]
                }
                _ => {
                    let a = scalefactor(&mut r)?;
                    let b = scalefactor(&mut r)?;
                    [a, b, b]
                }
            };
        }
    }

    for part in 0..12 {
        // vals[channel][sample of the part][subband]
        let mut vals = [[[0.0f32; 32]; 3]; 2];
        for sb in 0..sblimit {
            let coded = if sb < bound { nch } else { 1 };
            for ch in 0..coded {
                if class[ch][sb] == 0 {
                    continue;
                }
                let raw = read_triplet(&mut r, class[ch][sb]).map_err(at)?;
                // Above the bound one code serves both channels.
                let last = if coded == 1 { nch } else { ch + 1 };
                for target in ch..last {
                    let f = factor[target][sb][part / 4];
                    for (k, v) in raw.iter().enumerate() {
                        vals[target][k][sb] = v * f;
                    }
                }
            }
        }
        for ch in 0..nch {
            let (chunks, _) = pcm[ch].as_chunks_mut::<32>();
            for k in 0..3 {
                synth[ch].synth(&vals[ch][k], &mut chunks[part * 3 + k]);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::needless_range_loop)]
mod tests {
    use super::select;
    use crate::MpegDecoder;
    use crate::common::{BitWriter, Rng, track};
    use crate::reference::{RefSynth, dequant_levels};
    use vynamp_decode_core::{AudioBuffer, CodecId, Decoder, Error, Mode, Options, Packet};
    use vynamp_decode_mpa::parse_header;

    /// Allocation field width and steps by allocation code for the four groups of MPEG-1
    /// tables a and b (standard table 3-B.2a): subbands 0-2, 3-10, 11-22 and 23-26.
    const STEPS: [&[u32]; 4] = [
        &[
            0, 3, 7, 15, 31, 63, 127, 255, 511, 1023, 2047, 4095, 8191, 16383, 32767, 65535,
        ],
        &[
            0, 3, 5, 7, 9, 15, 31, 63, 127, 255, 511, 1023, 2047, 4095, 8191, 65535,
        ],
        &[0, 3, 5, 7, 9, 15, 31, 65535],
        &[0, 3, 5, 65535],
    ];
    const WIDTHS: [u32; 4] = [4, 4, 3, 2];

    fn group_of(sb: usize) -> usize {
        match sb {
            0..=2 => 0,
            3..=10 => 1,
            11..=22 => 2,
            _ => 3,
        }
    }

    /// A mono MPEG-1 48 kHz 384 kbit/s Layer II frame (27 subbands), described by what the
    /// standard puts in it.
    struct Spec {
        crc: bool,
        /// Allocation code by subband.
        alloc: [usize; 27],
        scfsi: [u8; 27],
        /// The scalefactor indices in effect for the three groups of four parts.
        scf: [[u8; 3]; 27],
        /// Sample code `0..steps` by subband, part and sample of the part.
        codes: [[[u32; 3]; 12]; 27],
    }

    impl Spec {
        fn steps(&self, sb: usize) -> u32 {
            STEPS[group_of(sb)][self.alloc[sb]]
        }

        fn build(&self) -> Vec<u8> {
            let header = [0xFF, 0xFD ^ u8::from(self.crc), 0xE4, 0xC0];
            let mut alloc = BitWriter::default();
            let mut nbits = 0;
            for sb in 0..27 {
                alloc.put(self.alloc[sb] as u32, WIDTHS[group_of(sb)]);
                nbits += WIDTHS[group_of(sb)];
            }
            for sb in 0..27 {
                if self.steps(sb) != 0 {
                    alloc.put(u32::from(self.scfsi[sb]), 2);
                    nbits += 2;
                }
            }
            // The scalefactors follow the scfsi field without byte alignment.
            let mut rest = alloc;
            let alloc_bytes = rest.bytes.clone();
            for sb in 0..27 {
                if self.steps(sb) == 0 {
                    continue;
                }
                let s = self.scf[sb];
                match self.scfsi[sb] {
                    0 => s.iter().for_each(|&v| rest.put(u32::from(v), 6)),
                    1 => {
                        assert_eq!(s[0], s[1]);
                        rest.put(u32::from(s[0]), 6);
                        rest.put(u32::from(s[2]), 6);
                    }
                    2 => {
                        assert!(s[0] == s[1] && s[1] == s[2]);
                        rest.put(u32::from(s[0]), 6);
                    }
                    _ => {
                        assert_eq!(s[1], s[2]);
                        rest.put(u32::from(s[0]), 6);
                        rest.put(u32::from(s[1]), 6);
                    }
                }
            }
            for part in 0..12 {
                for sb in 0..27 {
                    let n = self.steps(sb);
                    let c = self.codes[sb][part];
                    match n {
                        0 => {}
                        3 => rest.put(c[0] + 3 * c[1] + 9 * c[2], 5),
                        5 => rest.put(c[0] + 5 * c[1] + 25 * c[2], 7),
                        9 => rest.put(c[0] + 9 * c[1] + 81 * c[2], 10),
                        _ => {
                            let bits = (n + 1).ilog2();
                            c.iter().for_each(|&v| rest.put(v, bits));
                        }
                    }
                }
            }
            let mut frame = header.to_vec();
            if self.crc {
                // Bitwise CRC-16 (0x8005, init 0xFFFF) over header bytes 2-3 and the
                // allocation and scfsi bits, which are the first `nbits` bits of `alloc`.
                let mut bits: Vec<u8> = Vec::new();
                for b in &header[2..4] {
                    (0..8).for_each(|i| bits.push((b >> (7 - i)) & 1));
                }
                (0..nbits as usize)
                    .for_each(|i| bits.push((alloc_bytes[i / 8] >> (7 - i % 8)) & 1));
                let mut crc = 0xFFFFu16;
                for bit in bits {
                    let fb = (crc >> 15) ^ u16::from(bit);
                    crc <<= 1;
                    if fb != 0 {
                        crc ^= 0x8005;
                    }
                }
                frame.extend_from_slice(&crc.to_be_bytes());
            }
            frame.extend_from_slice(&rest.bytes);
            assert!(frame.len() <= 1152, "{}", frame.len());
            frame.resize(1152, 0);
            frame
        }

        /// The 36 slots of 32 subband samples the standard's formulas give.
        fn subbands(&self) -> Vec<[f64; 32]> {
            let mut slots = vec![[0.0; 32]; 36];
            for sb in 0..27 {
                let n = self.steps(sb);
                if n == 0 {
                    continue;
                }
                for part in 0..12 {
                    for k in 0..3 {
                        slots[part * 3 + k][sb] =
                            dequant_levels(self.codes[sb][part][k], n, self.scf[sb][part / 4]);
                    }
                }
            }
            slots
        }
    }

    /// A frame using every class of the table (grouped 3, 5, 9 and ungrouped 3 to 16 bits) and
    /// every scfsi mode.
    fn busy(rng: &mut Rng, crc: bool) -> Spec {
        let mut s = Spec {
            crc,
            alloc: [0; 27],
            scfsi: [0; 27],
            scf: [[0; 3]; 27],
            codes: [[[0; 3]; 12]; 27],
        };
        for sb in 0..27 {
            // Every code of each group in turn, so all classes occur across the subbands.
            s.alloc[sb] = (sb * 5 + 1) % STEPS[group_of(sb)].len();
            s.scfsi[sb] = (sb % 4) as u8;
            let a = rng.below(40) as u8;
            s.scf[sb] = match s.scfsi[sb] {
                0 => [a, rng.below(40) as u8, rng.below(40) as u8],
                1 => [a, a, rng.below(40) as u8],
                2 => [a, a, a],
                _ => {
                    let b = rng.below(40) as u8;
                    [a, b, b]
                }
            };
            let n = s.steps(sb);
            if n != 0 {
                for part in 0..12 {
                    for k in 0..3 {
                        // The all-ones code of an ungrouped class is not used by encoders.
                        s.codes[sb][part][k] = rng.below(n);
                    }
                }
            }
        }
        s
    }

    fn decode(d: &mut MpegDecoder, frame: Vec<u8>) -> Result<AudioBuffer, Error> {
        let mut out = AudioBuffer::new();
        let packet = Packet {
            frames: 1152,
            data: frame,
            offset: 1000,
            ..Packet::default()
        };
        d.decode(&packet, &mut out).map(|()| out)
    }

    #[test]
    fn hand_assembled_frames_decode_like_the_standard_formulas() {
        let mut rng = Rng(0x2468_ACE1);
        let classes: std::collections::BTreeSet<u32> = (0..27)
            .map(|sb| busy(&mut Rng(1), false).steps(sb))
            .collect();
        for n in [3, 5, 9, 7, 15, 31, 65535] {
            assert!(
                classes.contains(&n),
                "class {n} missing from the test frame"
            );
        }
        for crc in [false, true] {
            let mut d =
                MpegDecoder::new(&track(CodecId::Mp2, 48000, 1), &Options::default()).unwrap();
            let mut reference = RefSynth::new();
            for _ in 0..3 {
                let spec = busy(&mut rng, crc);
                let out = decode(&mut d, spec.build()).unwrap();
                let got = out.plane_f32(0).unwrap();
                assert_eq!(got.len(), 1152);
                let mut worst = 0.0f64;
                let mut peak = 0.0f64;
                for (t, sub) in spec.subbands().iter().enumerate() {
                    let want = reference.slot(sub);
                    for j in 0..32 {
                        peak = peak.max(want[j].abs());
                        worst = worst.max((f64::from(got[t * 32 + j]) - want[j]).abs());
                    }
                }
                assert!(peak > 0.05, "test signal too quiet: {peak}");
                assert!(worst < 5e-6, "crc {crc}: max error {worst}");
            }
        }
    }

    #[test]
    fn grouped_codes_unpack_least_significant_first() {
        // One subband (sb 0, allocation code 1 = 3 levels grouped, scalefactor index 0 = 2.0)
        // with the codes (0, 1, 2) in the first part and zeros elsewhere: slots 0..3 of the
        // subband must be -2/3, 0, +2/3 times the scalefactor 2.0, and the decoder must agree.
        let mut spec = busy(&mut Rng(3), false);
        spec.alloc = [0; 27];
        spec.alloc[0] = 1;
        spec.scfsi[0] = 2;
        spec.scf[0] = [0; 3];
        spec.codes = [[[0; 3]; 12]; 27];
        spec.codes[0][0] = [0, 1, 2];
        let sub = spec.subbands();
        let third = 2.0 / 3.0 * 2.0;
        assert!((sub[0][0] + third).abs() < 1e-12);
        assert!(sub[1][0].abs() < 1e-12);
        assert!((sub[2][0] - third).abs() < 1e-12);
        // Packed code 0 + 3 * 1 + 9 * 2 = 21 = 0b10101 in 5 bits.
        let frame = spec.build();
        let mut d = MpegDecoder::new(&track(CodecId::Mp2, 48000, 1), &Options::default()).unwrap();
        let out = decode(&mut d, frame).unwrap();
        let mut reference = RefSynth::new();
        for (t, s) in sub.iter().enumerate() {
            let want = reference.slot(s);
            for j in 0..32 {
                assert!((f64::from(out.plane_f32(0).unwrap()[t * 32 + j]) - want[j]).abs() < 5e-6);
            }
        }
    }

    #[test]
    fn crc_covers_the_scfsi_field() {
        let spec = busy(&mut Rng(9), true);
        let good = spec.build();
        let mk = || MpegDecoder::new(&track(CodecId::Mp2, 48000, 1), &Options::default()).unwrap();
        decode(&mut mk(), good.clone()).unwrap();
        // Allocation field: 3 * 4 + 8 * 4 + 12 * 3 + 4 * 2 = 88 bits = 11 bytes after the
        // 6-byte header and CRC, so byte 17 starts the scfsi field.
        let mut bad = good.clone();
        bad[17] ^= 0x40;
        assert_eq!(
            decode(&mut mk(), bad).unwrap_err(),
            Error::Invalid {
                what: "mpeg crc",
                offset: 1004
            }
        );
        // The first scalefactor follows the scfsi field and is not covered: a changed value
        // still passes the CRC check (the byte after the allocated subbands' scfsi bits).
        let scfsi_bits = (0..27).filter(|&sb| spec.steps(sb) != 0).count() * 2;
        let mut sf = good;
        sf[17 + scfsi_bits.div_ceil(8)] ^= 0x01;
        // May decode or fail on index 63, but never with a CRC error.
        if let Err(e) = decode(&mut mk(), sf) {
            assert!(
                matches!(
                    e,
                    Error::Invalid {
                        what: "mpeg scalefactor",
                        ..
                    }
                ),
                "{e:?}"
            );
        }
    }

    #[test]
    fn free_format_selects_the_table_of_the_bitrate_its_length_stands_for() {
        // 44.1 kHz stereo, bitrate index 0. 417 bytes (418 padded) are 128 kbit/s: table A.
        let h = parse_header([0xFF, 0xFD, 0x00, 0x04]).unwrap();
        let hp = parse_header([0xFF, 0xFD, 0x02, 0x04]).unwrap();
        assert_eq!(select(&h, 417).1, 27);
        assert_eq!(select(&hp, 418).1, 27);
        // 1254 bytes are 384 kbit/s: table B; 208 bytes are 64 kbit/s: table C.
        assert_eq!(select(&h, 1254).1, 30);
        assert_eq!(select(&h, 208).1, 8);
    }

    #[test]
    fn allocation_tables_have_the_standard_sizes() {
        // Header: MPEG-1 Layer II, no CRC; bitrate index, sample rate index, channel mode.
        // Bitrate indices: 1:32 2:48 3:56 4:64 5:80 6:96 7:112 8:128 9:160 10:192 11:224 12:256
        // 13:320 14:384. Sample rates: 0:44.1k 1:48k 2:32k.
        let bits = |hdr: [u8; 4]| {
            let h = parse_header(hdr).unwrap();
            let (groups, sblimit) = select(&h, h.frame_len);
            let mut left = sblimit;
            let mut total = 0;
            for g in groups {
                let n = left.min(usize::from(g.count));
                total += n * usize::from(g.width);
                left -= n;
            }
            (sblimit, total)
        };
        let v1 =
            |bitrate: u8, rate: u8, mode: u8| [0xFF, 0xFD, bitrate << 4 | rate << 2, mode << 6];
        // 3 * 4 + 8 * 4 + 12 * 3 + 4 * 2 = 88 (table a), plus 3 * 2 = 94 (table b),
        // 2 * 4 + 6 * 3 = 26 (table c), 2 * 4 + 10 * 3 = 38 (table d).
        let cases = [
            (v1(10, 1, 0), (27, 88)), // 48 kHz stereo 192k: 96 per channel
            (v1(14, 1, 0), (27, 88)), // 48 kHz stereo 384k
            (v1(10, 0, 0), (30, 94)), // 44.1 kHz stereo 192k: 96 per channel
            (v1(8, 2, 0), (27, 88)),  // 32 kHz stereo 128k: 64 per channel
            (v1(10, 2, 0), (30, 94)), // 32 kHz stereo 192k
            (v1(7, 0, 0), (27, 88)),  // 44.1 kHz stereo 112k: 56 per channel
            (v1(6, 1, 0), (8, 26)),   // 48 kHz stereo 96k: 48 per channel
            (v1(6, 2, 0), (12, 38)),  // 32 kHz stereo 96k
            (v1(4, 0, 0), (8, 26)),   // 44.1 kHz stereo 64k: 32 per channel
            (v1(4, 1, 3), (27, 88)),  // 48 kHz mono 64k
            (v1(1, 0, 3), (8, 26)),   // 44.1 kHz mono 32k
            (v1(2, 2, 3), (12, 38)),  // 32 kHz mono 48k
            (v1(9, 0, 1), (27, 88)),  // 44.1 kHz joint stereo 160k: 80 per channel
        ];
        for (hdr, want) in cases {
            assert_eq!(bits(hdr), want, "{hdr:02x?}");
        }
        // MPEG-2 LSF: 4 * 4 + 7 * 3 + 19 * 2 = 75 bits, 30 subbands, whatever the bitrate.
        for bitrate in [1u8, 8, 14] {
            let hdr = [0xFF, 0xF5, bitrate << 4 | 2 << 2, 0];
            assert_eq!(bits(hdr), (30, 75), "bitrate index {bitrate}");
        }
    }

    #[test]
    fn garbage_payload_with_a_valid_header_never_panics() {
        let mut rng = Rng(77);
        for mode in [Mode::Strict, Mode::Resilient] {
            let o = Options {
                mode,
                ..Options::default()
            };
            let mut d = MpegDecoder::new(&track(CodecId::Mp2, 48000, 1), &o).unwrap();
            for _ in 0..200 {
                let mut f = vec![0xFF, 0xFD, 0xA4, 0xC0];
                f.extend((0..572).map(|_| rng.next() as u8));
                let _ = decode(&mut d, f);
            }
        }
    }
}
