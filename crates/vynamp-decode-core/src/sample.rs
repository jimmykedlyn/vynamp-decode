//! Sample and PCM format descriptions.

/// In-memory sample format of an [`AudioBuffer`](crate::AudioBuffer).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleFormat {
    /// Unsigned 8-bit.
    U8,
    /// Signed 16-bit.
    I16,
    /// Signed 24-bit, stored sign-extended in an `i32`.
    I24,
    /// Signed 32-bit.
    I32,
    /// 32-bit float.
    F32,
    /// 64-bit float.
    F64,
}

/// Byte order of PCM samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Endian {
    /// Little endian.
    Little,
    /// Big endian.
    Big,
}

/// Numeric encoding of PCM samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PcmEncoding {
    /// Two's-complement signed integers.
    Signed,
    /// Unsigned integers.
    Unsigned,
    /// IEEE floating point.
    Float,
}

/// Description of raw PCM sample storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PcmSpec {
    /// Numeric encoding.
    pub encoding: PcmEncoding,
    /// Container width in bits (8, 16, 24, 32 or 64).
    pub bits: u8,
    /// Number of significant bits within the container.
    pub valid_bits: u8,
    /// Byte order.
    pub endian: Endian,
}
