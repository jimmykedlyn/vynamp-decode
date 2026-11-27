//! ALAC (Apple Lossless) decoder.
//!
//! The track's `codec_private` is the 24-byte `ALACSpecificConfig`. Mono and stereo
//! streams of 16 and 24 bits are decoded; other depths and channel counts are
//! `Unsupported`. Output is planar `I16` for 16 bits and `I24` (sign-extended in `i32`)
//! for 24 bits.
//!
//! A packet is one ALAC frame: elements (`SCE`, `CPE`, `LFE`) up to the `END` tag. In
//! `Mode::Strict` a malformed frame is an `Invalid` error; in `Mode::Resilient` it becomes
//! silence of `Packet::frames` frames (at most the configured frame length) and an
//! `Event::CorruptFrame`.
#![warn(missing_docs)]

use vynamp_decode_core::{
    AudioBuffer, CodecId, Decoder, Error, Event, Mode, Options, Packet, Result, SampleFormat, Track,
};

/// Length of an `ALACSpecificConfig`.
const CONFIG_LEN: usize = 24;
/// Largest frame length accepted (the bound of the FFmpeg decoder).
const MAX_FRAME_LENGTH: u32 = 4096 * 4096;
/// Element tags.
const TAG_SCE: u32 = 0;
const TAG_CPE: u32 = 1;
const TAG_LFE: u32 = 3;
const TAG_END: u32 = 7;
/// Upper bound on the residuals one bit of a compressed channel can cover.
///
/// Each step of `rice` decodes one residual (at least 1 bit) and, while the history is
/// below 128, a zero run. A run read without escape has `x <= 8` and a parameter
/// `k = 7 - log2(h) + ((h + 16) >> 6) <= 7` for `h < 128`, so it covers at most
/// `8 * 127 + 126 = 1142` samples, reading at least `x + 1` unary bits and `k - 1` more:
/// under 80 samples per bit. Only the escape (nine `1` bits then a 16-bit length) reaches
/// 65535: a step then covers at most 1 + 65535 samples for at least 1 + 9 + 16 = 26 bits.
/// The densest step is therefore 65536 / 26 < 2521 samples per bit, and a valid channel of
/// `s` samples needs at least `s / 2521` bits.
const MAX_SAMPLES_PER_RICE_BIT: u64 = 2521;

fn invalid(what: &'static str, offset: u64) -> Error {
    Error::Invalid { what, offset }
}

/// The fields of `ALACSpecificConfig` the decoder uses.
#[derive(Debug, Clone, Copy)]
struct Config {
    frame_length: u32,
    bit_depth: u32,
    /// `pb`: rice history multiplier.
    history_mult: u32,
    /// `mb`: initial rice history.
    initial_history: u32,
    /// `kb`: rice parameter limit.
    rice_limit: u32,
    channels: u16,
}

fn parse_config(c: &[u8]) -> Result<Config> {
    if c.len() != CONFIG_LEN {
        return Err(invalid("alac config", 0));
    }
    let frame_length = u32::from_be_bytes([c[0], c[1], c[2], c[3]]);
    if frame_length == 0 || frame_length > MAX_FRAME_LENGTH {
        return Err(invalid("alac frame length", 0));
    }
    if c[4] != 0 {
        return Err(Error::Unsupported("alac version"));
    }
    let bit_depth = u32::from(c[5]);
    if !matches!(bit_depth, 16 | 24) {
        return Err(Error::Unsupported("alac bit depth"));
    }
    if c[8] == 0 {
        return Err(invalid("alac rice limit", 8));
    }
    let channels = u16::from(c[9]);
    if !matches!(channels, 1 | 2) {
        return Err(Error::Unsupported("alac channels"));
    }
    Ok(Config {
        frame_length,
        bit_depth,
        history_mult: u32::from(c[6]),
        initial_history: u32::from(c[7]),
        rice_limit: u32::from(c[8]),
        channels,
    })
}

/// MSB-first bit reader. Reads past the end yield zeros and are caught by `overrun`.
struct Bits<'a> {
    d: &'a [u8],
    /// Position in bits.
    pos: u64,
}

impl<'a> Bits<'a> {
    fn new(d: &'a [u8]) -> Self {
        Self { d, pos: 0 }
    }

    fn len(&self) -> u64 {
        self.d.len() as u64 * 8
    }

    fn left(&self) -> u64 {
        self.len().saturating_sub(self.pos)
    }

    fn overrun(&self) -> bool {
        self.pos > self.len()
    }

    fn eof(&self) -> Error {
        invalid("bitstream eof", self.pos / 8)
    }

    /// The next `n` (0..=32) bits, without consuming them.
    fn peek(&self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        let byte = (self.pos / 8) as usize;
        let mut w = 0u64;
        for i in 0..5 {
            let b = byte
                .checked_add(i)
                .and_then(|at| self.d.get(at))
                .copied()
                .unwrap_or(0);
            w = (w << 8) | u64::from(b);
        }
        // `w` holds 40 bits; drop the bits before `pos` and keep the top `n`.
        let sh = (self.pos % 8) as u32;
        ((w << (24 + sh)) >> (64 - n)) as u32
    }

    fn skip(&mut self, n: u32) {
        self.pos += u64::from(n);
    }

    fn bits(&mut self, n: u32) -> u32 {
        let v = self.peek(n);
        self.skip(n);
        v
    }

    /// `n` (1..=32) bits as a two's-complement value.
    fn signed(&mut self, n: u32) -> i32 {
        sext(self.bits(n) as i32, n)
    }

    /// Count of `1` bits before a `0`, at most 9 (the terminating `0` is consumed).
    fn ones_0_9(&mut self) -> u32 {
        let mut n = 0;
        while n < 9 && self.bits(1) == 1 {
            n += 1;
        }
        n
    }
}

/// Sign-extends the low `bits` (1..=32) of `v`.
fn sext(v: i32, bits: u32) -> i32 {
    let sh = 32 - bits;
    v.wrapping_shl(sh) >> sh
}

/// Floor of log2, with `log2(0) == 0`.
fn log2(v: u32) -> u32 {
    v.checked_ilog2().unwrap_or(0)
}

/// One adaptive-Golomb value with parameter `k` (>= 1); escapes to `bps` raw bits.
fn scalar(b: &mut Bits<'_>, k: u32, bps: u32) -> u32 {
    let mut x = b.ones_0_9();
    if x > 8 {
        return b.bits(bps);
    }
    if k != 1 {
        let extra = b.peek(k);
        x = (x << k) - x;
        if extra > 1 {
            x += extra - 1;
            b.skip(k);
        } else {
            b.skip(k - 1);
        }
    }
    x
}

/// Decodes `out.len()` prediction residuals.
fn rice(b: &mut Bits<'_>, out: &mut [i32], bps: u32, mult: u32, cfg: &Config) -> Result<()> {
    let n = out.len();
    let mut history = cfg.initial_history;
    let mut sign_mod = 0u32;
    let mut i = 0;
    while i < n {
        if b.left() == 0 {
            return Err(b.eof());
        }
        let k = log2((history >> 9) + 3).min(cfg.rice_limit);
        let x = scalar(b, k, bps).wrapping_add(sign_mod);
        sign_mod = 0;
        out[i] = ((x >> 1) as i32) ^ (x & 1).wrapping_neg() as i32;
        history = if x > 0xffff {
            0xffff
        } else {
            history
                .wrapping_add(x.wrapping_mul(mult))
                .wrapping_sub(history.wrapping_mul(mult) >> 9)
        };
        if history < 128 && i + 1 < n {
            // A run of zero residuals.
            let k = (7 - log2(history) + ((history + 16) >> 6)).min(cfg.rice_limit);
            let block = scalar(b, k, 16) as usize;
            if block > 0 {
                if block >= n - i {
                    return Err(invalid("alac zero run", b.pos / 8));
                }
                out[i + 1..=i + block].fill(0);
                i += block;
            }
            if block <= 0xffff {
                sign_mod = 1;
            }
            history = 0;
        }
        i += 1;
    }
    if b.overrun() {
        return Err(b.eof());
    }
    Ok(())
}

/// First-order prediction in place: `buf[i] += buf[i - 1]`.
fn first_order(buf: &mut [i32], bps: u32) {
    for i in 1..buf.len() {
        buf[i] = sext(buf[i - 1].wrapping_add(buf[i]), bps);
    }
}

/// Adaptive LPC reconstruction of `out` from the residuals `err`.
fn lpc(err: &[i32], out: &mut [i32], bps: u32, coefs: &mut [i16], quant: u32) {
    let n = out.len();
    let order = coefs.len();
    out[0] = err[0];
    if n <= 1 {
        return;
    }
    if order == 0 {
        out[1..].copy_from_slice(&err[1..]);
        return;
    }
    if order == 31 {
        out.copy_from_slice(err);
        first_order(out, bps);
        return;
    }
    let mut i = 1;
    while i <= order && i < n {
        out[i] = sext(out[i - 1].wrapping_add(err[i]), bps);
        i += 1;
    }
    for i in i..n {
        let d = out[i - order - 1];
        let mut val = 0i32;
        for (j, &c) in coefs.iter().enumerate() {
            val = val.wrapping_add(
                out[i - order + j]
                    .wrapping_sub(d)
                    .wrapping_mul(i32::from(c)),
            );
        }
        let val = ((i64::from(val) + (1i64 << (quant - 1))) >> quant) as i32;
        let mut e = err[i];
        out[i] = sext(val.wrapping_add(d).wrapping_add(e), bps);
        let s = e.signum();
        if s != 0 {
            let mut j = 0;
            while j < order && e.wrapping_mul(s) > 0 {
                let v = d.wrapping_sub(out[i - order + j]);
                let sign = v.signum() * s;
                coefs[j] = coefs[j].wrapping_sub(sign as i16);
                let v = v.wrapping_mul(sign);
                e = e.wrapping_sub((v >> quant).wrapping_mul(j as i32 + 1));
                j += 1;
            }
        }
    }
}

/// Predictor parameters of one channel of a compressed element.
#[derive(Clone, Copy, Default)]
struct Predictor {
    mode: u32,
    quant: u32,
    mult: u32,
    order: usize,
    coefs: [i16; 32],
}

/// Decodes ALAC frames into planar audio.
pub struct AlacDecoder {
    cfg: Config,
    format: SampleFormat,
    mode: Mode,
    /// Per channel: residuals, reconstructed samples and the low "extra" bits.
    err: [Vec<i32>; 2],
    out: [Vec<i32>; 2],
    extra: [Vec<i32>; 2],
    events: Vec<Event>,
}

impl AlacDecoder {
    /// Creates a decoder for an ALAC `track`, configured by its `codec_private`.
    pub fn new(track: &Track, opts: &Options) -> Result<Self> {
        if track.codec != CodecId::Alac {
            return Err(Error::Unsupported("codec"));
        }
        let cfg = parse_config(&track.codec_private)?;
        opts.limits.check_channels(cfg.channels)?;
        if track.channels != cfg.channels {
            return Err(invalid("alac channels", 9));
        }
        Ok(Self {
            cfg,
            format: if cfg.bit_depth == 16 {
                SampleFormat::I16
            } else {
                SampleFormat::I24
            },
            mode: opts.mode,
            err: Default::default(),
            out: Default::default(),
            extra: Default::default(),
            events: Vec::new(),
        })
    }

    /// Decodes one frame into `self.out`; returns its length in frames.
    fn frame(&mut self, data: &[u8]) -> Result<usize> {
        let mut b = Bits::new(data);
        let mut ch = 0usize;
        let mut len = None;
        loop {
            if b.left() < 3 {
                return Err(invalid("alac end tag", b.pos / 8));
            }
            let n = match b.bits(3) {
                TAG_END => break,
                TAG_SCE | TAG_LFE => 1,
                TAG_CPE => 2,
                _ => return Err(invalid("alac element", b.pos / 8)),
            };
            if ch + n > usize::from(self.cfg.channels) {
                return Err(invalid("alac element channels", b.pos / 8));
            }
            self.element(&mut b, ch, n, &mut len)?;
            ch += n;
        }
        match len {
            Some(len) if ch == usize::from(self.cfg.channels) => Ok(len),
            _ => Err(invalid("alac element channels", b.pos / 8)),
        }
    }

    /// Decodes one element of `n` channels into channels `ch0..ch0 + n`.
    fn element(
        &mut self,
        b: &mut Bits<'_>,
        ch0: usize,
        n: usize,
        len: &mut Option<usize>,
    ) -> Result<()> {
        let cfg = self.cfg;
        b.skip(16);
        let has_size = b.bits(1) == 1;
        let extra_bits = b.bits(2) * 8;
        let compressed = b.bits(1) == 0;
        let samples = if has_size {
            b.bits(32)
        } else {
            cfg.frame_length
        };
        if b.overrun() {
            return Err(b.eof());
        }
        if samples == 0 || samples > cfg.frame_length {
            return Err(invalid("alac frame length", b.pos / 8));
        }
        // Bound the declared count by what the rest of the packet can encode before
        // sizing the per-channel buffers by it: verbatim samples take `bit_depth` bits
        // each; each compressed channel needs its own residual bits, at most
        // `MAX_SAMPLES_PER_RICE_BIT` samples per bit.
        let fits = if compressed {
            u64::from(samples) * n as u64 <= b.left().saturating_mul(MAX_SAMPLES_PER_RICE_BIT)
        } else {
            u64::from(samples) * n as u64 * u64::from(cfg.bit_depth) <= b.left()
        };
        if !fits {
            return Err(invalid("alac frame size", b.pos / 8));
        }
        let samples = samples as usize;
        match *len {
            Some(l) if l != samples => return Err(invalid("alac frame length", b.pos / 8)),
            _ => *len = Some(samples),
        }
        let bps = (cfg.bit_depth + n as u32 - 1)
            .checked_sub(extra_bits)
            .filter(|b| (1..=32).contains(b))
            .ok_or(invalid("alac sample size", b.pos / 8))?;
        for c in ch0..ch0 + n {
            self.err[c].resize(samples, 0);
            self.out[c].resize(samples, 0);
            self.extra[c].resize(samples, 0);
        }

        let (mut shift, mut weight, mut extra_bits) = (0, 0, extra_bits);
        if compressed {
            shift = b.bits(8);
            weight = b.bits(8);
            if n == 2 && weight != 0 && shift > 31 {
                return Err(invalid("alac stereo shift", b.pos / 8));
            }
            let mut p = [Predictor::default(); 2];
            for pc in p.iter_mut().take(n) {
                pc.mode = b.bits(4);
                pc.quant = b.bits(4);
                pc.mult = b.bits(3);
                pc.order = b.bits(5) as usize;
                if pc.order as u32 >= cfg.frame_length || pc.quant == 0 {
                    return Err(invalid("alac predictor", b.pos / 8));
                }
                for i in (0..pc.order).rev() {
                    pc.coefs[i] = b.signed(16) as i16;
                }
            }
            if extra_bits > 0 {
                if b.left() < (samples * n) as u64 * u64::from(extra_bits) {
                    return Err(b.eof());
                }
                for i in 0..samples {
                    for c in ch0..ch0 + n {
                        self.extra[c][i] = b.bits(extra_bits) as i32;
                    }
                }
            }
            for (k, pc) in p.iter_mut().take(n).enumerate() {
                let c = ch0 + k;
                let mult = pc.mult * cfg.history_mult / 4;
                rice(b, &mut self.err[c], bps, mult, &cfg)?;
                // Mode 15 runs a first-order pass before the adaptive predictor; no known
                // encoder writes it. Other nonzero modes are undefined.
                match pc.mode {
                    0 => {}
                    15 => first_order(&mut self.err[c], bps),
                    _ => return Err(invalid("alac prediction type", b.pos / 8)),
                }
                lpc(
                    &self.err[c],
                    &mut self.out[c],
                    bps,
                    &mut pc.coefs[..pc.order],
                    pc.quant,
                );
            }
        } else {
            if b.left() < (samples * n) as u64 * u64::from(cfg.bit_depth) {
                return Err(b.eof());
            }
            for i in 0..samples {
                for c in ch0..ch0 + n {
                    self.out[c][i] = b.signed(cfg.bit_depth);
                }
            }
            extra_bits = 0;
        }
        if n == 2 && weight != 0 {
            let (l, r) = self.out.split_at_mut(1);
            for (a, b) in l[0][..samples].iter_mut().zip(&mut r[0][..samples]) {
                let x = a.wrapping_sub(b.wrapping_mul(weight as i32) >> shift);
                let y = b.wrapping_add(x);
                *a = y;
                *b = x;
            }
        }
        if extra_bits > 0 {
            for c in ch0..ch0 + n {
                for (o, &e) in self.out[c][..samples].iter_mut().zip(&self.extra[c]) {
                    *o = o.wrapping_shl(extra_bits) | e;
                }
            }
        }
        Ok(())
    }

    fn store(&self, len: usize, out: &mut AudioBuffer) -> Result<()> {
        out.reset(self.format, self.cfg.channels, len)?;
        for c in 0..usize::from(self.cfg.channels) {
            let src = &self.out[c][..len];
            if let Some(dst) = out.plane_i16_mut(c) {
                for (d, &s) in dst.iter_mut().zip(src) {
                    *d = s as i16;
                }
            } else if let Some(dst) = out.plane_i32_mut(c) {
                for (d, &s) in dst.iter_mut().zip(src) {
                    *d = sext(s, 24);
                }
            }
        }
        Ok(())
    }
}

impl Decoder for AlacDecoder {
    fn decode(&mut self, packet: &Packet, out: &mut AudioBuffer) -> Result<()> {
        let result = self.frame(&packet.data);
        match result {
            Ok(len) => self.store(len, out),
            // Resilient: a frame that cannot be decoded becomes silence of its length.
            Err(Error::Invalid { .. }) if self.mode == Mode::Resilient => {
                let frames = packet.frames.min(self.cfg.frame_length);
                out.reset(self.format, self.cfg.channels, frames as usize)?;
                for c in 0..usize::from(self.cfg.channels) {
                    if let Some(p) = out.plane_i16_mut(c) {
                        p.fill(0);
                    } else if let Some(p) = out.plane_i32_mut(c) {
                        p.fill(0);
                    }
                }
                self.events.push(Event::CorruptFrame {
                    pts: packet.pts,
                    offset: packet.offset,
                    frames,
                });
                Ok(())
            }
            Err(e) => {
                out.reset(self.format, self.cfg.channels, 0)?;
                Err(match e {
                    Error::Invalid { what, offset } => Error::Invalid {
                        what,
                        offset: packet.offset.saturating_add(offset),
                    },
                    other => other,
                })
            }
        }
    }

    fn reset(&mut self) {}

    fn drain_events(&mut self, out: &mut Vec<Event>) {
        out.append(&mut self.events);
    }
}
