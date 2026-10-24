//! Coded residual (RFC 9639 section 9.2.7).

use vynamp_decode_core::{BitReader, Error, Result};

/// Builds an `Invalid` error positioned at the reader's current byte
/// (relative to the slice the reader was created on).
pub(crate) fn invalid(r: &BitReader, what: &'static str) -> Error {
    Error::Invalid {
        what,
        offset: r.byte_pos() as u64,
    }
}

/// Decodes the residual of a subframe into `out[order..]`.
///
/// `out.len()` is the block size and `order` the predictor order (the number of
/// warm-up samples that precede the residual).
pub(crate) fn decode_residual(r: &mut BitReader, out: &mut [i64], order: usize) -> Result<()> {
    let block_size = out.len();
    let (param_bits, escape) = match r.read_bits(2)? {
        0 => (4, 0b1111),
        1 => (5, 0b11111),
        _ => return Err(invalid(r, "flac residual method")),
    };
    let partition_order = r.read_bits(4)?;
    let partitions = 1usize << partition_order;
    // "The partition order MUST be such that the block size is evenly divisible by the
    // number of partitions" and "(block size >> partition order) is larger than the
    // predictor order". Reference decoders accept equality (a first partition that
    // holds only the warm-up samples and no residual), so this does too.
    let per_partition = block_size >> partition_order;
    if !block_size.is_multiple_of(partitions) || per_partition < order {
        return Err(invalid(r, "flac partition order"));
    }

    let mut start = order;
    for p in 1..=partitions {
        let end = p * per_partition;
        let dst = &mut out[start..end];
        let param = r.read_bits(param_bits)?;
        if param == escape {
            let bits = r.read_bits(5)?;
            if bits == 0 {
                dst.fill(0);
            } else {
                for d in dst {
                    *d = i64::from(r.read_signed(bits)?);
                }
            }
        } else {
            for d in dst {
                let v = r.read_rice_signed(param)?;
                // RFC 9639 section 9.2.7.3: "residual sample values MUST NOT have an
                // absolute value equal to, or larger than, 2 to the power 31", which
                // excludes the most negative 32-bit value.
                if v == i32::MIN {
                    return Err(invalid(r, "flac residual range"));
                }
                *d = i64::from(v);
            }
        }
        start = end;
    }
    Ok(())
}
