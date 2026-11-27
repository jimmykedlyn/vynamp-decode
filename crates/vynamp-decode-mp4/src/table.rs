//! Sample tables (`stbl`): where each sample lives and when it plays.

use crate::boxes::Bx;
use vynamp_decode_core::{Error, LimitKind, Result};

/// One sample (an access unit; for ALAC, one codec frame).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Sample {
    /// Absolute source offset of the first byte.
    pub offset: u64,
    /// Size in bytes.
    pub size: u32,
    /// Start, in media timescale ticks.
    pub pts: u64,
    /// Duration, in media timescale ticks.
    pub duration: u32,
}

fn invalid(what: &'static str, b: &Bx<'_>) -> Error {
    Error::Invalid {
        what,
        offset: b.offset,
    }
}

fn need<'a>(stbl: &Bx<'a>, kind: &[u8; 4]) -> Result<Bx<'a>> {
    stbl.child(kind)?.ok_or(invalid("mp4 sample table", stbl))
}

/// Per-sample sizes from `stsz` or `stz2`. `media_bytes` bounds what a fixed-size `stsz`
/// may declare, since its count has no entries behind it.
fn sizes(stbl: &Bx<'_>, media_bytes: u64) -> Result<Vec<u32>> {
    if let Some(b) = stbl.child(b"stsz")? {
        let mut c = b.cur();
        c.version()?;
        let fixed = c.u32()?;
        if fixed != 0 {
            let n = c.u32()?;
            if u64::from(n) * u64::from(fixed) > media_bytes {
                return Err(invalid("mp4 sample count", &b));
            }
            return Ok(vec![fixed; n as usize]);
        }
        let n = c.count(4)?;
        return (0..n).map(|_| c.u32()).collect();
    }
    let b = stbl
        .child(b"stz2")?
        .ok_or(invalid("mp4 sample table", stbl))?;
    let mut c = b.cur();
    c.version()?;
    c.skip(3)?;
    let field = c.u8()?;
    let at = c.offset();
    let n = c.u32()? as usize;
    let bytes = match field {
        4 => n.div_ceil(2),
        8 => n,
        16 => n.saturating_mul(2),
        _ => return Err(invalid("mp4 stz2 field size", &b)),
    };
    let raw = c.take(bytes).map_err(|_| Error::Invalid {
        what: "mp4 entry count",
        offset: at,
    })?;
    Ok((0..n)
        .map(|i| match field {
            4 => u32::from((raw[i / 2] >> (4 * (1 - i % 2))) & 0x0F),
            8 => u32::from(raw[i]),
            _ => u32::from(u16::from_be_bytes([raw[2 * i], raw[2 * i + 1]])),
        })
        .collect())
}

/// Chunk offsets from `stco` or `co64`.
fn chunk_offsets(stbl: &Bx<'_>) -> Result<Vec<u64>> {
    if let Some(b) = stbl.child(b"stco")? {
        let mut c = b.cur();
        c.version()?;
        let n = c.count(4)?;
        return (0..n).map(|_| c.u32().map(u64::from)).collect();
    }
    let b = stbl
        .child(b"co64")?
        .ok_or(invalid("mp4 sample table", stbl))?;
    let mut c = b.cur();
    c.version()?;
    let n = c.count(8)?;
    (0..n).map(|_| c.u64()).collect()
}

/// Builds the sample list of `stbl`; samples must fit in `media_bytes`. The tables must
/// agree on the sample count; a total duration past `u64` is `Limit(Frames)`.
pub(crate) fn samples(stbl: &Bx<'_>, media_bytes: u64) -> Result<Vec<Sample>> {
    let sizes = sizes(stbl, media_bytes)?;
    let n = sizes.len();

    let stts = need(stbl, b"stts")?;
    let mut c = stts.cur();
    c.version()?;
    let entries = c.count(8)?;
    let mut runs = Vec::with_capacity(entries);
    let mut timed: u64 = 0;
    for _ in 0..entries {
        let (count, delta) = (c.u32()?, c.u32()?);
        timed += u64::from(count);
        runs.push((count, delta));
    }
    if timed != n as u64 {
        return Err(invalid("mp4 sample count", &stts));
    }

    let stsc = need(stbl, b"stsc")?;
    let mut c = stsc.cur();
    c.version()?;
    let entries = c.count(12)?;
    let mut chunk_map = Vec::with_capacity(entries);
    for _ in 0..entries {
        let (first, per_chunk) = (c.u32()?, c.u32()?);
        c.skip(4)?;
        chunk_map.push((first, per_chunk));
    }
    let chunks = chunk_offsets(stbl)?;

    let mut out = Vec::with_capacity(n);
    let mut sizes_it = sizes.iter();
    let mut runs_it = runs.into_iter().flat_map(|(k, d)| (0..k).map(move |_| d));
    let mut pts: u64 = 0;
    'chunks: for (ci, &chunk_start) in chunks.iter().enumerate() {
        let chunk_no = ci as u64 + 1;
        // The entry with the largest first_chunk <= chunk_no applies.
        let per_chunk = chunk_map
            .iter()
            .take_while(|&&(first, _)| u64::from(first) <= chunk_no)
            .last()
            .map_or(0, |&(_, k)| k);
        let mut offset = chunk_start;
        for _ in 0..per_chunk {
            let Some(&size) = sizes_it.next() else {
                break 'chunks;
            };
            // `timed == n` above: one duration per size.
            let duration = runs_it.next().unwrap_or(0);
            out.push(Sample {
                offset,
                size,
                pts,
                duration,
            });
            offset = offset
                .checked_add(u64::from(size))
                .ok_or(invalid("mp4 chunk offset", stbl))?;
            pts = pts
                .checked_add(u64::from(duration))
                .ok_or(Error::Limit(LimitKind::Frames))?;
        }
    }
    if out.len() != n {
        return Err(invalid("mp4 sample count", &stsc));
    }
    Ok(out)
}
