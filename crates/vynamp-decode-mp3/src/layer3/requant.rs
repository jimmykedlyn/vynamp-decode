//! Requantization (ISO/IEC 11172-3 section 2.4.3.4.7).
//!
//! A line `x` of the granule becomes `sign(x) |x|^(4/3) 2^(e / 4)` where, in quarter steps,
//! `e = global_gain - 210 - 8 subblock_gain[window] - m (scalefac + preflag pretab)` with
//! `m = 2` (`scalefac_scale` off, a step of `2^-0.5`) or 4 (a step of `2^-1`). The
//! `subblock_gain` term applies to the short bands only, `preflag`/`pretab` to the long bands
//! only. Both factors come from tables built once: `x^(4/3)` for the 8207 magnitudes a line
//! can have (15 plus 13 escape bits is the most), and `2^(e / 4)` over every exponent that the
//! side information can produce.

use super::Granule;
use super::scalefactors::Layout;
use super::tables::PRETAB;
use std::sync::OnceLock;

/// Largest magnitude of a line: 15 + (2^13 - 1).
const MAX_MAGNITUDE: usize = 8206;
/// Index of exponent 0 in the gain table. The smallest exponent is
/// `0 - 210 - 8 * 7 - 4 * (31 + 3)` = -402; anything lower is clamped (it is far below the
/// smallest f32 anyway, whose exponent is -149 * 4).
const GAIN_OFFSET: i32 = 420;
/// Entries of the gain table: exponents -420 to 59 (the largest used is 255 - 210 = 45).
const GAIN_LEN: usize = 480;

struct Tables {
    pow43: [f32; MAX_MAGNITUDE + 1],
    gain: [f32; GAIN_LEN],
}

fn tables() -> &'static Tables {
    static TABLES: OnceLock<Tables> = OnceLock::new();
    TABLES.get_or_init(|| {
        let mut pow43 = [0.0f32; MAX_MAGNITUDE + 1];
        for (x, v) in pow43.iter_mut().enumerate() {
            *v = (x as f64).powf(4.0 / 3.0) as f32;
        }
        let mut gain = [0.0f32; GAIN_LEN];
        for (i, v) in gain.iter_mut().enumerate() {
            *v = ((i as i32 - GAIN_OFFSET) as f64 / 4.0).exp2() as f32;
        }
        Tables { pow43, gain }
    })
}

/// Builds the tables now so that decoding never initializes them.
pub(super) fn warm_up() {
    tables();
}

/// Requantizes the lines of `g` (transmission order) into `out`; the lines from
/// `g.used_lines` on are zero.
pub(super) fn requantize(g: &Granule, out: &mut [f32; 576]) {
    out.fill(0.0);
    let t = tables();
    let info = &g.info;
    let used = g.used_lines.min(576);
    let step = if info.scalefac_scale { 4 } else { 2 };
    let base = i32::from(info.global_gain) - 210;
    let layout = &g.layout;
    let mut line = 0;
    for (e, &w) in layout.widths.iter().enumerate() {
        if line >= used {
            break;
        }
        let mut sf = i32::from(g.scalefac[e]);
        let mut exp = base;
        if e < layout.n_long {
            if g.preflag {
                sf += i32::from(PRETAB.get(e).copied().unwrap_or(0));
            }
        } else {
            exp -= 8 * i32::from(info.subblock_gain[(e - layout.n_long) % 3]);
        }
        exp -= step * sf;
        let gain = t.gain[(exp + GAIN_OFFSET).clamp(0, GAIN_LEN as i32 - 1) as usize];
        let end = (line + usize::from(w)).min(used);
        for (o, &x) in out[line..end].iter_mut().zip(&g.xr[line..end]) {
            if x != 0 {
                let m = t.pow43[usize::from(x.unsigned_abs()).min(MAX_MAGNITUDE)] * gain;
                *o = if x < 0 { -m } else { m };
            }
        }
        line += usize::from(w);
    }
}

/// Reorders the lines of a short or mixed block from transmission order (per band: all lines
/// of window 0, then of window 1, then of window 2) into the order the IMDCT wants: three
/// interleaved windows, so that line `i` of window `w` of subband `sb` is at
/// `18 sb + 3 i + w`. The long part of a mixed block (lines 0 to 35) stays as it is.
pub(super) fn reorder(xr: &mut [f32; 576], layout: &Layout) {
    let src = *xr;
    // The three windows of a band occupy the same 3 * width lines before and after.
    let mut first = layout.bound(layout.n_long);
    let (bands, _) = layout.widths[layout.n_long..].as_chunks::<3>();
    for band in bands {
        let width = usize::from(band[0]);
        for w in 0..3 {
            for k in 0..width {
                xr[first + 3 * k + w] = src[first + w * width + k];
            }
        }
        first += 3 * width;
    }
}

#[cfg(test)]
pub(super) mod tests;
