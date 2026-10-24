//! Hand assembly of MPEG audio Layer I frames for the tests.
#![allow(dead_code)]

use vynamp_decode_core::{CodecId, GaplessKind, Track, crc16_mpeg};
use vynamp_decode_mpa::parse_header;

/// MSB-first bit writer.
#[derive(Default)]
pub struct BitWriter {
    pub bytes: Vec<u8>,
    nbits: usize,
}

impl BitWriter {
    pub fn put(&mut self, value: u32, n: u32) {
        for i in (0..n).rev() {
            if self.nbits.is_multiple_of(8) {
                self.bytes.push(0);
            }
            let bit = ((value >> i) & 1) as u8;
            let last = self.bytes.len() - 1;
            self.bytes[last] |= bit << (7 - self.nbits % 8);
            self.nbits += 1;
        }
    }
}

/// Everything that goes into a Layer I frame.
#[derive(Clone)]
pub struct L1Spec {
    pub mpeg1: bool,
    pub rate_idx: u8,
    pub bitrate_idx: u8,
    pub padding: bool,
    /// 0 stereo, 1 joint stereo, 2 dual channel, 3 mono.
    pub mode: u8,
    pub mode_ext: u8,
    pub crc: bool,
    /// Coded allocation (0..=15) per channel and subband; above the bound channel 0 is shared.
    pub alloc: [[u8; 32]; 2],
    pub scf: [[u8; 32]; 2],
    /// Raw `alloc + 1`-bit sample codes indexed `[slot][subband][channel]` (above the bound
    /// only channel 0 is transmitted and used).
    pub samples: [[[u32; 2]; 32]; 12],
}

impl L1Spec {
    /// Silent MPEG-1 48 kHz 384 kbit/s frame (all allocations zero).
    pub fn silent(mode: u8) -> Self {
        Self {
            mpeg1: true,
            rate_idx: 1,
            bitrate_idx: 11,
            padding: false,
            mode,
            mode_ext: 0,
            crc: false,
            alloc: [[0; 32]; 2],
            scf: [[0; 32]; 2],
            samples: [[[0; 2]; 32]; 12],
        }
    }

    pub fn channels(&self) -> usize {
        if self.mode == 3 { 1 } else { 2 }
    }

    /// First subband with a shared allocation.
    pub fn bound(&self) -> usize {
        if self.mode == 1 {
            4 + 4 * usize::from(self.mode_ext)
        } else {
            32
        }
    }

    /// Coded allocation in effect for `ch` in `sb`.
    pub fn alloc_of(&self, ch: usize, sb: usize) -> u8 {
        if sb < self.bound() {
            self.alloc[ch][sb]
        } else {
            self.alloc[0][sb]
        }
    }

    pub fn header(&self) -> [u8; 4] {
        let version = if self.mpeg1 { 3 } else { 2 };
        [
            0xFF,
            0xE0 | (version << 3) | (3 << 1) | u8::from(!self.crc),
            (self.bitrate_idx << 4) | (self.rate_idx << 2) | (u8::from(self.padding) << 1),
            (self.mode << 6) | (self.mode_ext << 4),
        ]
    }

    /// The whole frame, zero padded to its length, with a valid CRC when `crc` is set.
    pub fn build(&self) -> Vec<u8> {
        let header = self.header();
        let frame_len = parse_header(header).expect("valid header").frame_len;
        let nch = self.channels();
        let bound = self.bound();
        let mut alloc = BitWriter::default();
        for sb in 0..32 {
            if sb < bound {
                for ch in 0..nch {
                    alloc.put(u32::from(self.alloc[ch][sb]), 4);
                }
            } else {
                alloc.put(u32::from(self.alloc[0][sb]), 4);
            }
        }
        let mut rest = BitWriter::default();
        for sb in 0..32 {
            for ch in 0..nch {
                if self.alloc_of(ch, sb) != 0 {
                    rest.put(u32::from(self.scf[ch][sb]), 6);
                }
            }
        }
        for slot in 0..12 {
            for sb in 0..32 {
                // Above the bound one sample serves both channels.
                for ch in 0..if sb < bound { nch } else { 1 } {
                    let a = self.alloc_of(ch, sb);
                    if a != 0 {
                        rest.put(self.samples[slot][sb][ch], u32::from(a) + 1);
                    }
                }
            }
        }
        let mut frame = header.to_vec();
        if self.crc {
            let mut c = crc16_mpeg(&header[2..4]);
            c = vynamp_decode_core::crc16_update(c, &alloc.bytes);
            frame.extend_from_slice(&c.to_be_bytes());
        }
        frame.extend_from_slice(&alloc.bytes);
        frame.extend_from_slice(&rest.bytes);
        assert!(frame.len() <= frame_len, "{} > {frame_len}", frame.len());
        frame.resize(frame_len, 0);
        frame
    }
}

pub fn track(codec: CodecId, sample_rate: u32, channels: u16) -> Track {
    Track {
        id: 0,
        codec,
        sample_rate,
        channels,
        channel_mask: 0,
        frames: None,
        delay: 0,
        padding: 0,
        gapless: GaplessKind::Unknown,
        codec_private: Vec::new(),
    }
}

/// Deterministic pseudo random numbers (xorshift32).
pub struct Rng(pub u32);

impl Rng {
    pub fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }

    pub fn below(&mut self, n: u32) -> u32 {
        self.next() % n
    }

    /// Uniform in [-1, 1).
    pub fn unit(&mut self) -> f64 {
        f64::from(self.next()) / 2_147_483_648.0 - 1.0
    }
}

/// A random but valid frame: allocations 0..=14 (biased small), random scalefactors and
/// sample codes. Sample codes never use the all-ones value.
pub fn random_l1(rng: &mut Rng, mode: u8, mode_ext: u8, crc: bool) -> L1Spec {
    let mut s = L1Spec::silent(mode);
    s.mode_ext = mode_ext;
    s.crc = crc;
    s.bitrate_idx = 14;
    s.rate_idx = 2; // 32 kHz: 12 * 448000 / 32000 * 4 = 672 bytes
    for ch in 0..2 {
        for sb in 0..32 {
            s.alloc[ch][sb] = if rng.below(4) == 0 {
                0
            } else {
                rng.below(6) as u8
            };
            s.scf[ch][sb] = rng.below(63) as u8;
        }
    }
    for slot in 0..12 {
        for sb in 0..32 {
            for ch in 0..2 {
                let nb = u32::from(s.alloc_of(ch.min(s.channels() - 1), sb)) + 1;
                s.samples[slot][sb][ch] = rng.below((1u32 << nb) - 1);
            }
        }
    }
    s
}
