//! Subframes: CONSTANT, VERBATIM, FIXED and LPC (RFC 9639 sections 9.2.1 to 9.2.6).

use crate::residual::{decode_residual, invalid};
use vynamp_decode_core::{BitReader, Result};

/// Largest LPC order.
const MAX_ORDER: usize = 32;

/// FIXED predictor coefficients, most recent sample first (RFC 9639 section 9.2.5).
const FIXED: [&[i64]; 5] = [&[], &[1], &[2, -1], &[3, -3, 1], &[4, -6, 4, -1]];

/// Decodes one subframe of `out.len()` samples at `bps` bits (already including the
/// extra bit of a side channel). Error offsets are relative to the reader's slice.
pub(crate) fn decode_subframe(r: &mut BitReader, out: &mut [i64], bps: u32) -> Result<()> {
    if r.read_bits(1)? != 0 {
        return Err(invalid(r, "flac subframe header"));
    }
    let kind = r.read_bits(6)?;
    let wasted = if r.read_bits(1)? == 1 {
        r.read_unary()? + 1
    } else {
        0
    };
    // The wasted bits must leave at least one bit per sample.
    if wasted >= bps {
        return Err(invalid(r, "flac wasted bits"));
    }
    let bps = bps - wasted;

    match kind {
        0 => out.fill(r.read_signed_i64(bps)?),
        1 => {
            for o in out.iter_mut() {
                *o = r.read_signed_i64(bps)?;
            }
        }
        8..=12 => {
            let order = (kind - 8) as usize;
            let Some(&coefs) = FIXED.get(order) else {
                return Err(invalid(r, "flac subframe type"));
            };
            let mut rev = [0i64; 4];
            reverse_into(&mut rev[..order], coefs);
            predict(r, out, &rev[..order], 0, bps)?;
        }
        32..=63 => {
            let order = (kind - 31) as usize;
            decode_lpc(r, out, order, bps)?;
        }
        _ => return Err(invalid(r, "flac subframe type")),
    }

    if wasted > 0 {
        for o in out.iter_mut() {
            *o <<= wasted;
        }
    }
    Ok(())
}

/// Copies `coefs` into `dst` in reverse order (oldest sample's coefficient first).
fn reverse_into(dst: &mut [i64], coefs: &[i64]) {
    for (d, c) in dst.iter_mut().zip(coefs.iter().rev()) {
        *d = *c;
    }
}

fn decode_lpc(r: &mut BitReader, out: &mut [i64], order: usize, bps: u32) -> Result<()> {
    if order > out.len() {
        return Err(invalid(r, "flac predictor order"));
    }
    // Warm-up samples come before the coefficients; read them straight into place.
    for o in &mut out[..order] {
        *o = r.read_signed_i64(bps)?;
    }
    let precision = r.read_bits(4)? + 1;
    if precision == 16 {
        return Err(invalid(r, "flac lpc precision"));
    }
    let shift = r.read_signed(5)?;
    // "While the prediction right shift is signed two's complement, this number MUST
    // NOT be negative".
    let Ok(shift) = u32::try_from(shift) else {
        return Err(invalid(r, "flac lpc shift"));
    };
    let mut rev = [0i64; MAX_ORDER];
    for j in 0..order {
        rev[order - 1 - j] = i64::from(r.read_signed(precision)?);
    }
    predict_after_warmup(r, out, &rev[..order], shift, bps)
}

/// FIXED subframe tail: warm-up samples, then residual and prediction.
fn predict(
    r: &mut BitReader,
    out: &mut [i64],
    rev_coefs: &[i64],
    shift: u32,
    bps: u32,
) -> Result<()> {
    let order = rev_coefs.len();
    if order > out.len() {
        return Err(invalid(r, "flac predictor order"));
    }
    for o in &mut out[..order] {
        *o = r.read_signed_i64(bps)?;
    }
    predict_after_warmup(r, out, rev_coefs, shift, bps)
}

/// Reads the residual into `out[order..]` and adds the prediction in place.
///
/// Every reconstructed sample must fit `bps` bits; this also keeps the i64
/// accumulator far from overflow (32 coefficients of 15 bits times 33-bit samples).
fn predict_after_warmup(
    r: &mut BitReader,
    out: &mut [i64],
    rev_coefs: &[i64],
    shift: u32,
    bps: u32,
) -> Result<()> {
    let order = rev_coefs.len();
    decode_residual(r, out, order)?;
    let hi = (1i64 << (bps - 1)) - 1;
    let lo = -hi - 1;
    for i in order..out.len() {
        let acc: i64 = out[i - order..i]
            .iter()
            .zip(rev_coefs)
            .map(|(s, c)| s * c)
            .sum();
        let v = out[i] + (acc >> shift);
        if v < lo || v > hi {
            return Err(invalid(r, "flac sample range"));
        }
        out[i] = v;
    }
    Ok(())
}
