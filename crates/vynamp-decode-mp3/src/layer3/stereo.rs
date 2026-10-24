//! Joint stereo: middle/side and intensity (ISO/IEC 11172-3 section 2.4.3.4.9, ISO/IEC 13818-3).
//!
//! Both tools work on the requantized lines in transmission order, before the short block
//! reordering, so that the scalefactor bands of the layout are contiguous ranges.
//!
//! # Middle/side
//!
//! `L = (M + S) / sqrt 2`, `R = (M - S) / sqrt 2` on every line that is not in an intensity
//! band.
//!
//! # Intensity
//!
//! The right channel of an intensity frame carries intensity positions as its scalefactors
//! instead of spectrum above a bound. The bound is found from the right channel's data: a
//! band is an intensity band if it lies beyond the last band that has a nonzero line. Long
//! blocks have one such bound; short blocks one per window (the bands of window `w` are every
//! third entry from `n_long + w`); in a mixed block the long bands form one group and each
//! window another. A right channel without any nonzero line is intensity from its first band
//! on. In an intensity band the left channel's lines `x` are split: `L = x kl`, `R = x kr`.
//!
//! * MPEG-1: position `p` in 0 to 6, `kl = tan(p pi / 12) / (1 + tan(p pi / 12))`,
//!   `kr = 1 / (1 + tan(p pi / 12))`. Position 7 (and anything above) is illegal.
//! * MPEG-2/2.5: with `i0 = 2^-0.25` (`2^-0.5` if bit 0 of the right channel's
//!   `scalefac_compress` is set): position 0 gives `kl = kr = 1`, an odd position
//!   `kl = i0^((p + 1) / 2)`, `kr = 1`, an even one `kl = 1`, `kr = i0^(p / 2)`. The scalefactor
//!   reader marks the illegal positions (all ones, and every position of a partition that
//!   sends none, whose `is_max` is 0).
//!
//! A band with an illegal position is not intensity: it is processed as middle/side if that is
//! on, and left alone otherwise. The last band of each kind has no scalefactor and takes the
//! position of the band before it (in the same window).
//!
//! Channels whose granules have different band structures (a long block against a short one)
//! cannot be matched band by band; intensity is skipped for such a granule.

use super::Granule;
use std::f64::consts::{FRAC_1_SQRT_2, PI};
use std::sync::OnceLock;
use vynamp_decode_mpa::MpegVersion;

/// The joint stereo tools in effect for a frame.
#[derive(Clone, Copy, Debug)]
pub(super) struct Stereo {
    pub version: MpegVersion,
    pub ms: bool,
    pub intensity: bool,
}

/// Gains `(left, right)` by position.
struct Gains {
    /// MPEG-1 positions 0 to 6.
    v1: [(f32, f32); 7],
    /// MPEG-2/2.5 by `intensity_scale` (0 or 1) and position (clamped to 63).
    lsf: [[(f32, f32); 64]; 2],
}

fn gains() -> &'static Gains {
    static GAINS: OnceLock<Gains> = OnceLock::new();
    GAINS.get_or_init(|| {
        let mut v1 = [(1.0f32, 0.0f32); 7];
        for (p, g) in v1.iter_mut().enumerate() {
            let (s, c) = (p as f64 * PI / 12.0).sin_cos();
            // tan / (1 + tan) and 1 / (1 + tan), written so that position 6 is exact.
            *g = ((s / (s + c)) as f32, (c / (s + c)) as f32);
        }
        let mut lsf = [[(1.0f32, 1.0f32); 64]; 2];
        for (scale, row) in lsf.iter_mut().enumerate() {
            let i0 = (-0.25 * (scale + 1) as f64).exp2();
            for (p, g) in row.iter_mut().enumerate().skip(1) {
                *g = if p % 2 == 1 {
                    (i0.powi(p.div_ceil(2) as i32) as f32, 1.0)
                } else {
                    (1.0, i0.powi((p / 2) as i32) as f32)
                };
            }
        }
        Gains { v1, lsf }
    })
}

/// Builds the tables now so that decoding never initializes them.
pub(super) fn warm_up() {
    gains();
}

/// What happens to the lines of one entry.
#[derive(Clone, Copy)]
enum Action {
    /// Middle/side if that is on, nothing otherwise.
    Plain,
    Intensity(f32, f32),
}

/// Applies the joint stereo tools to the requantized lines of both channels (transmission
/// order).
pub(super) fn process(l: &mut [f32; 576], r: &mut [f32; 576], g: [&Granule; 2], s: Stereo) {
    if !s.ms && !s.intensity {
        return;
    }
    let used = g[0].used_lines.max(g[1].used_lines).min(576);
    let (l0, l1) = (&g[0].layout, &g[1].layout);
    if !s.intensity || l0.n_long != l1.n_long || l0.n_short != l1.n_short {
        if s.ms {
            middle_side(&mut l[..used], &mut r[..used]);
        }
        return;
    }
    let layout = &l1.widths;
    // The last entry with a nonzero right line in each group of bands: the long bands and
    // the three windows.
    let mut last = [None::<usize>; 4];
    let mut start = 0;
    for (e, &w) in layout.iter().enumerate() {
        let end = start + usize::from(w);
        if r[start..end].iter().any(|&v| v != 0.0) {
            last[group(e, l1.n_long)] = Some(e);
        }
        start = end;
    }
    let gains = gains();
    let scale = usize::from(g[1].info.scalefac_compress & 1);
    let mut start = 0;
    for (e, &w) in layout.iter().enumerate() {
        let end = start + usize::from(w);
        if start >= used {
            break;
        }
        let end = end.min(used);
        let beyond = last[group(e, l1.n_long)].is_none_or(|b| e > b);
        let mut action = Action::Plain;
        if beyond {
            let mut src = e;
            while src >= l1.n_sf && src > 0 {
                src -= if src < l1.n_long { 1 } else { 3 };
            }
            let pos = usize::from(g[1].scalefac[src.min(39)]);
            if s.version == MpegVersion::V1 {
                if pos < 7 {
                    let (kl, kr) = gains.v1[pos];
                    action = Action::Intensity(kl, kr);
                }
            } else if g[1].illegal_is_pos >> src & 1 == 0 {
                let (kl, kr) = gains.lsf[scale][pos.min(63)];
                action = Action::Intensity(kl, kr);
            }
        }
        match action {
            Action::Intensity(kl, kr) => {
                for (a, b) in l[start..end].iter_mut().zip(&mut r[start..end]) {
                    let x = *a;
                    *a = x * kl;
                    *b = x * kr;
                }
            }
            Action::Plain if s.ms => middle_side(&mut l[start..end], &mut r[start..end]),
            Action::Plain => {}
        }
        start += usize::from(w);
    }
}

/// Group of band entries that share an intensity bound: 0 for the long bands, 1 to 3 for the
/// windows of the short bands.
fn group(entry: usize, n_long: usize) -> usize {
    if entry < n_long {
        0
    } else {
        1 + (entry - n_long) % 3
    }
}

fn middle_side(l: &mut [f32], r: &mut [f32]) {
    const K: f32 = FRAC_1_SQRT_2 as f32;
    for (a, b) in l.iter_mut().zip(r.iter_mut()) {
        let (m, s) = (*a, *b);
        *a = (m + s) * K;
        *b = (m - s) * K;
    }
}

#[cfg(test)]
mod tests;
