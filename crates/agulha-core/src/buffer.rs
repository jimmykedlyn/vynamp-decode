//! Planar audio buffer.

use crate::{Error, LimitKind, Result, SampleFormat};

#[derive(Debug)]
enum Data {
    Empty,
    U8(Vec<u8>),
    I16(Vec<i16>),
    I32(Vec<i32>),
    F32(Vec<f32>),
    F64(Vec<f64>),
}

/// Planar audio buffer: channel `c` occupies `[c * stride, c * stride + frames)`
/// of the backing vector. Storage is reused across [`reset`](Self::reset) calls.
#[derive(Debug)]
pub struct AudioBuffer {
    format: SampleFormat,
    channels: u16,
    frames: usize,
    stride: usize,
    data: Data,
}

impl Default for AudioBuffer {
    fn default() -> Self {
        Self::new()
    }
}

macro_rules! plane {
    ($($get:ident, $get_mut:ident, $variant:ident, $ty:ty;)*) => {$(
        #[doc = concat!("Samples of channel `c` as `", stringify!($ty), "`, if the format matches.")]
        pub fn $get(&self, c: usize) -> Option<&[$ty]> {
            let Data::$variant(v) = &self.data else { return None };
            if c >= usize::from(self.channels) {
                return None;
            }
            v.get(c * self.stride..c * self.stride + self.frames)
        }

        #[doc = concat!("Mutable samples of channel `c` as `", stringify!($ty), "`, if the format matches.")]
        pub fn $get_mut(&mut self, c: usize) -> Option<&mut [$ty]> {
            let Data::$variant(v) = &mut self.data else { return None };
            if c >= usize::from(self.channels) {
                return None;
            }
            v.get_mut(c * self.stride..c * self.stride + self.frames)
        }
    )*};
}

fn prepare<T: Copy + Default>(v: &mut Vec<T>, need: usize) -> bool {
    if v.len() < need {
        if v.try_reserve_exact(need - v.len()).is_err() {
            return false;
        }
        v.resize(need, T::default());
    }
    true
}

fn shift<T: Copy>(v: &mut [T], channels: usize, frames: usize, stride: usize, n: usize) {
    for c in 0..channels {
        v[c * stride..c * stride + frames].copy_within(n.., 0);
    }
}

impl AudioBuffer {
    /// Creates an empty buffer without allocating.
    pub fn new() -> Self {
        Self {
            format: SampleFormat::F32,
            channels: 0,
            frames: 0,
            stride: 0,
            data: Data::Empty,
        }
    }

    /// Prepares the buffer for `frames` frames of `channels` channels in `format`.
    ///
    /// Reallocates only if the existing storage is too small or the storage
    /// type changes (I24 and I32 share storage). Contents are unspecified
    /// afterwards. If `channels * frames` overflows or the allocation fails,
    /// returns `Err(Error::Limit(LimitKind::Frames))` and leaves the buffer
    /// empty (`frames() == 0`).
    pub fn reset(&mut self, format: SampleFormat, channels: u16, frames: usize) -> Result<()> {
        self.channels = 0;
        self.frames = 0;
        self.stride = 0;
        let fail = Error::Limit(LimitKind::Frames);
        let need = usize::from(channels)
            .checked_mul(frames)
            .ok_or(fail.clone())?;
        let reusable = matches!(
            (&self.data, format),
            (Data::U8(_), SampleFormat::U8)
                | (Data::I16(_), SampleFormat::I16)
                | (Data::I32(_), SampleFormat::I24 | SampleFormat::I32)
                | (Data::F32(_), SampleFormat::F32)
                | (Data::F64(_), SampleFormat::F64)
        );
        if !reusable {
            self.data = match format {
                SampleFormat::U8 => Data::U8(Vec::new()),
                SampleFormat::I16 => Data::I16(Vec::new()),
                SampleFormat::I24 | SampleFormat::I32 => Data::I32(Vec::new()),
                SampleFormat::F32 => Data::F32(Vec::new()),
                SampleFormat::F64 => Data::F64(Vec::new()),
            };
        }
        let ok = match &mut self.data {
            Data::Empty => true,
            Data::U8(v) => prepare(v, need),
            Data::I16(v) => prepare(v, need),
            Data::I32(v) => prepare(v, need),
            Data::F32(v) => prepare(v, need),
            Data::F64(v) => prepare(v, need),
        };
        if !ok {
            self.data = Data::Empty;
            return Err(fail);
        }
        self.format = format;
        self.channels = channels;
        self.frames = frames;
        self.stride = frames;
        Ok(())
    }

    /// Current sample format.
    pub fn format(&self) -> SampleFormat {
        self.format
    }

    /// Channel count.
    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// Frames per channel.
    pub fn frames(&self) -> usize {
        self.frames
    }

    plane! {
        plane_u8, plane_u8_mut, U8, u8;
        plane_i16, plane_i16_mut, I16, i16;
        plane_i32, plane_i32_mut, I32, i32;
        plane_f32, plane_f32_mut, F32, f32;
        plane_f64, plane_f64_mut, F64, f64;
    }

    /// Drops the first `n` frames of every channel (clamped to the buffer length).
    pub fn truncate_front(&mut self, n: usize) {
        let n = n.min(self.frames);
        if n == 0 {
            return;
        }
        let (ch, fr, st) = (usize::from(self.channels), self.frames, self.stride);
        match &mut self.data {
            Data::Empty => {}
            Data::U8(v) => shift(v, ch, fr, st, n),
            Data::I16(v) => shift(v, ch, fr, st, n),
            Data::I32(v) => shift(v, ch, fr, st, n),
            Data::F32(v) => shift(v, ch, fr, st, n),
            Data::F64(v) => shift(v, ch, fr, st, n),
        }
        self.frames -= n;
    }

    /// Drops the last `n` frames of every channel (clamped to the buffer length).
    pub fn truncate_back(&mut self, n: usize) {
        self.frames -= n.min(self.frames);
    }

    /// Converts to interleaved `f32` in `[-1, 1)`, replacing the contents of `out`.
    pub fn to_f32_interleaved(&self, out: &mut Vec<f32>) {
        out.clear();
        let channels = usize::from(self.channels);
        out.reserve(channels * self.frames);
        let i32_scale = match self.format {
            SampleFormat::I24 => 8_388_608.0,
            _ => 2_147_483_648.0,
        };
        for f in 0..self.frames {
            for c in 0..channels {
                let i = c * self.stride + f;
                out.push(match &self.data {
                    Data::Empty => 0.0,
                    Data::U8(v) => (f32::from(v[i]) - 128.0) / 128.0,
                    Data::I16(v) => f32::from(v[i]) / 32768.0,
                    Data::I32(v) => (f64::from(v[i]) / i32_scale) as f32,
                    Data::F32(v) => v[i],
                    Data::F64(v) => v[i] as f32,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn planar_layout() {
        let mut b = AudioBuffer::new();
        b.reset(SampleFormat::I16, 2, 4).unwrap();
        b.plane_i16_mut(0).unwrap().copy_from_slice(&[1, 2, 3, 4]);
        b.plane_i16_mut(1).unwrap().copy_from_slice(&[5, 6, 7, 8]);
        assert_eq!(b.plane_i16(0).unwrap(), &[1, 2, 3, 4]);
        assert_eq!(b.plane_i16(1).unwrap(), &[5, 6, 7, 8]);
        assert!(b.plane_i16(2).is_none());
        assert!(b.plane_f32(0).is_none());
        assert_eq!(
            (b.format(), b.channels(), b.frames()),
            (SampleFormat::I16, 2, 4)
        );
    }

    #[test]
    fn reset_does_not_reallocate() {
        let mut b = AudioBuffer::new();
        b.reset(SampleFormat::I16, 2, 4).unwrap();
        let p = b.plane_i16(0).unwrap().as_ptr();
        b.reset(SampleFormat::I16, 2, 2).unwrap();
        assert_eq!(b.plane_i16(0).unwrap().as_ptr(), p);
        assert_eq!(b.frames(), 2);
    }

    #[test]
    fn truncate() {
        let mut b = AudioBuffer::new();
        b.reset(SampleFormat::I16, 2, 4).unwrap();
        b.plane_i16_mut(0).unwrap().copy_from_slice(&[1, 2, 3, 4]);
        b.plane_i16_mut(1).unwrap().copy_from_slice(&[1, 2, 3, 4]);
        b.truncate_front(1);
        assert_eq!(b.plane_i16(0).unwrap(), &[2, 3, 4]);
        assert_eq!(b.plane_i16(1).unwrap(), &[2, 3, 4]);
        b.truncate_back(1);
        assert_eq!(b.plane_i16(1).unwrap(), &[2, 3]);
        b.truncate_front(99);
        assert_eq!(b.frames(), 0);
    }

    #[test]
    fn to_f32() {
        let mut b = AudioBuffer::new();
        let mut out = Vec::new();
        b.reset(SampleFormat::I24, 1, 2).unwrap();
        b.plane_i32_mut(0)
            .unwrap()
            .copy_from_slice(&[-8_388_608, 8_388_607]);
        b.to_f32_interleaved(&mut out);
        assert_eq!(out[0], -1.0);
        assert!((out[1] - 0.999_999_9).abs() < 1e-7);
        b.reset(SampleFormat::U8, 1, 3).unwrap();
        b.plane_u8_mut(0).unwrap().copy_from_slice(&[0, 128, 255]);
        b.to_f32_interleaved(&mut out);
        assert_eq!(out, [-1.0, 0.0, 0.992_187_5]);
        b.reset(SampleFormat::I16, 2, 1).unwrap();
        b.plane_i16_mut(0).unwrap()[0] = 16384;
        b.plane_i16_mut(1).unwrap()[0] = -32768;
        b.to_f32_interleaved(&mut out);
        assert_eq!(out, [0.5, -1.0]);
    }

    #[test]
    fn reset_overflow_and_huge() {
        let mut b = AudioBuffer::new();
        b.reset(SampleFormat::I16, 2, 4).unwrap();
        assert_eq!(
            b.reset(SampleFormat::I16, 2, usize::MAX),
            Err(Error::Limit(LimitKind::Frames))
        );
        assert_eq!(b.frames(), 0);
        assert!(b.plane_i16(0).is_none());
        assert_eq!(
            b.reset(SampleFormat::F64, 2, usize::MAX / 2),
            Err(Error::Limit(LimitKind::Frames))
        );
        assert_eq!(b.frames(), 0);
        b.reset(SampleFormat::I16, 1, 2).unwrap();
        assert_eq!(b.frames(), 2);
    }

    #[test]
    fn i24_i32_share_storage() {
        let mut b = AudioBuffer::new();
        b.reset(SampleFormat::I24, 1, 4).unwrap();
        b.plane_i32_mut(0).unwrap().copy_from_slice(&[7, 8, 9, 10]);
        // Reused storage keeps its old contents; fresh storage would be zeroed.
        b.reset(SampleFormat::I32, 1, 4).unwrap();
        assert_eq!(b.plane_i32(0).unwrap(), &[7, 8, 9, 10]);
    }

    #[test]
    fn to_f32_other_formats() {
        let mut b = AudioBuffer::new();
        let mut out = Vec::new();
        b.reset(SampleFormat::I32, 1, 2).unwrap();
        b.plane_i32_mut(0).unwrap().copy_from_slice(&[i32::MIN, 0]);
        b.to_f32_interleaved(&mut out);
        assert_eq!(out, [-1.0, 0.0]);
        b.reset(SampleFormat::F32, 1, 2).unwrap();
        b.plane_f32_mut(0).unwrap().copy_from_slice(&[0.25, -0.5]);
        b.to_f32_interleaved(&mut out);
        assert_eq!(out, [0.25, -0.5]);
        b.reset(SampleFormat::F64, 1, 2).unwrap();
        b.plane_f64_mut(0).unwrap().copy_from_slice(&[0.125, -1.0]);
        b.to_f32_interleaved(&mut out);
        assert_eq!(out, [0.125, -1.0]);
    }
}
