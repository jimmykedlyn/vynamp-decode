//! IEEE 754 80-bit extended precision numbers, as used for the AIFF sample rate.

use vynamp_decode_core::{Error, Result};

/// Converts a big-endian 80-bit extended number to a `u32`, rounding to the
/// nearest integer (halves round up).
///
/// Layout: 1 sign bit, 15-bit exponent (bias 16383), 64-bit significand with an
/// explicit integer bit. Fractional rates such as 22254.545454 Hz (classic Mac
/// 22 kHz) are accepted and rounded. Zero, negative values, NaN, infinity,
/// denormals, unnormalised numbers, values that round to zero (below 0.5) and
/// values that round above `u32::MAX` fail with
/// `Invalid { what: "sample rate", offset: 0 }` (callers re-base the offset).
pub fn to_u32(bytes: [u8; 10]) -> Result<u32> {
    let invalid = Error::Invalid {
        what: "sample rate",
        offset: 0,
    };
    let sign_exp = u16::from_be_bytes([bytes[0], bytes[1]]);
    let mantissa = u64::from_be_bytes([
        bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7], bytes[8], bytes[9],
    ]);
    let negative = sign_exp & 0x8000 != 0;
    let exp = i32::from(sign_exp & 0x7FFF);
    // Zero, denormals, infinity/NaN and unnormalised numbers (integer bit clear).
    if negative || exp == 0 || exp == 0x7FFF || mantissa >> 63 == 0 {
        return Err(invalid);
    }
    // value = mantissa * 2^(e - 63) with the integer bit at position 63.
    let e = exp - 16383;
    let rounded = match e {
        // [0.5, 1) rounds up to 1; anything smaller rounds to zero.
        -1 => 1,
        0..=62 => {
            let shift = (63 - e) as u32; // 1..=63
            let whole = mantissa >> shift;
            let half = 1u64 << (shift - 1);
            whole + u64::from(mantissa & ((1u64 << shift) - 1) >= half)
        }
        _ => return Err(invalid),
    };
    u32::try_from(rounded).map_err(|_| invalid)
}
