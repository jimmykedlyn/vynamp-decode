//! The synthesis path end to end on hand-built frames: every block type against a reference
//! chain written from the definitions in `f64`, and intensity stereo (which libmp3lame never
//! emits) through the linearity of everything after requantization.

use super::*;
use crate::reference::RefSynth;
use vynamp_decode_core::{AudioBuffer, CodecId, Decoder, Options, Packet};

/// Start or stop block (`block_type` 1 or 3): long bands, window switching side information.
fn ws_info(block_type: u8, compress: u16, bv: u16, tables: [u8; 2]) -> GranuleInfo {
    GranuleInfo {
        big_values: bv,
        global_gain: 175,
        scalefac_compress: compress,
        window_switching: true,
        block_type,
        table_select: [tables[0], tables[1], 0],
        region0_count: 7,
        region1_count: 13,
        ..GranuleInfo::default()
    }
}

fn loud(mut info: GranuleInfo) -> GranuleInfo {
    info.global_gain = 175;
    info
}

/// Scalefactors `(value, bits)` for MPEG-1 `scalefac_compress` 8 (slen 2 then 1): the first
/// `n1` values take two bits, the next `n2` one bit.
fn v1_sf(n1: usize, n2: usize) -> Vec<(u32, u32)> {
    (0..n1)
        .map(|i| (((i * 3 + 1) % 4) as u32, 2))
        .chain((0..n2).map(|i| (((i * 5 + 2) % 2) as u32, 1)))
        .collect()
}

/// Decodes `frame` with the production decoder, as `Mp3`.
fn decode_pcm(frame: &[u8]) -> Vec<Vec<f32>> {
    let h = parse_header(frame[..4].try_into().unwrap()).unwrap();
    let mut d = crate::MpegDecoder::new(
        &crate::common::track(CodecId::Mp3, h.sample_rate, h.channels()),
        &Options::default(),
    )
    .unwrap();
    let mut out = AudioBuffer::new();
    let p = Packet {
        track: 0,
        pts: 0,
        frames: h.samples,
        offset: 0,
        data: frame.to_vec(),
    };
    d.decode(&p, &mut out).unwrap();
    (0..usize::from(h.channels()))
        .map(|c| out.plane_f32(c).unwrap().to_vec())
        .collect()
}

/// The reference decoder for the synthesis chain, written from the definitions in `f64`
/// (the references of the step modules, the literal reordering and middle/side, and the
/// matrixing/window filterbank), fed with the Huffman output of `decode_granules`.
fn reference_pcm(frame: &[u8]) -> Vec<Vec<f64>> {
    use super::super::{alias, imdct, requant};
    let h = parse_header(frame[..4].try_into().unwrap()).unwrap();
    let mut out = FrameData::new();
    decode_granules(&mut Layer3::default(), frame, &h, 0, &mut out).unwrap();
    let ms = h.channel_mode == vynamp_decode_mpa::ChannelMode::JointStereo && h.mode_ext & 2 != 0;
    let channels = out.channels;
    let mut overlap: Vec<_> = (0..channels)
        .map(|_| imdct::tests::Reference::new())
        .collect();
    let mut filter: Vec<_> = (0..channels).map(|_| RefSynth::new()).collect();
    let mut pcm = vec![Vec::new(); channels];
    for gr in 0..out.granules {
        let mut x: Vec<Vec<f64>> = (0..channels)
            .map(|ch| requant::tests::reference(&out.gr[gr][ch]))
            .collect();
        if ms {
            let [a, b] = &mut x[..] else { unreachable!() };
            for (m, s) in a.iter_mut().zip(b.iter_mut()) {
                (*m, *s) = (
                    (*m + *s) * std::f64::consts::FRAC_1_SQRT_2,
                    (*m - *s) * std::f64::consts::FRAC_1_SQRT_2,
                );
            }
        }
        for ch in 0..channels {
            let g = &out.gr[gr][ch];
            let mut xs: [f32; 576] = std::array::from_fn(|i| x[ch][i] as f32);
            if g.info.block_type == 2 {
                // Frequency line `f` of window `w` of a short band goes to `first + 3 f + w`.
                let src = xs;
                let first = g.layout.bound(g.layout.n_long);
                let (mut sent, mut f0) = (first, 0);
                for band in g.layout.widths[g.layout.n_long..].chunks(3) {
                    let w = usize::from(band[0]);
                    for win in 0..3 {
                        for k in 0..w {
                            xs[first + 3 * (f0 + k) + win] = src[sent + win * w + k];
                        }
                    }
                    sent += 3 * w;
                    f0 += w;
                }
            }
            let aliased = match (g.info.block_type, g.info.mixed_block) {
                (2, false) => xs.map(f64::from).to_vec(),
                (2, true) => alias::tests::reference(&xs, 1),
                _ => alias::tests::reference(&xs, 31),
            };
            let xs: [f32; 576] = std::array::from_fn(|i| aliased[i] as f32);
            for slot in overlap[ch].granule(&xs, &g.info) {
                pcm[ch].extend(filter[ch].slot(&slot));
            }
        }
    }
    pcm
}

fn assert_same_audio(frame: &[u8], what: &str) {
    let got = decode_pcm(frame);
    let want = reference_pcm(frame);
    for (ch, (g, w)) in got.iter().zip(&want).enumerate() {
        assert_eq!(g.len(), w.len(), "{what}");
        let peak = w.iter().fold(0.0f64, |m, v| m.max(v.abs()));
        assert!(peak > 0.02, "{what} ch{ch}: reference too quiet ({peak})");
        let err = g
            .iter()
            .zip(w)
            .map(|(&a, &b)| (f64::from(a) - b).abs())
            .fold(0.0, f64::max);
        assert!(
            err < 2e-5 * peak.max(1.0),
            "{what} ch{ch}: max error {err:e}, peak {peak}"
        );
    }
}

/// MPEG-1 44.1 kHz 320 kbit/s frame of the given mode.
fn v1_frame_header(mode: u8, mode_ext: u8) -> FrameHeader {
    header(3, 14, 0, mode, mode_ext, false)
}

#[test]
fn every_block_type_synthesizes_like_the_definition() {
    let mut seed = 99;
    // Kinds of granule: 0 long, 1 start, 2 short, 3 stop, 4 mixed.
    let make = |kind: usize, seed: &mut u32| match kind {
        0 => granule(
            loud(long_info(8, 70, [13, 15, 13], 5, 4)),
            [24, 62],
            v1_sf(11, 10),
            12,
            seed,
        ),
        1 => granule(
            ws_info(1, 8, 70, [13, 15]),
            [36, 576],
            v1_sf(11, 10),
            12,
            seed,
        ),
        2 => granule(
            loud(short_info(8, 80, [13, 15], false)),
            [36, 576],
            v1_sf(18, 18),
            12,
            seed,
        ),
        3 => granule(
            ws_info(3, 8, 70, [13, 15]),
            [36, 576],
            v1_sf(11, 10),
            12,
            seed,
        ),
        _ => granule(
            loud(short_info(8, 80, [13, 15], true)),
            [36, 576],
            v1_sf(17, 18),
            12,
            seed,
        ),
    };
    for (what, mode, ext, kinds) in [
        ("long, start, short, stop", 0u8, 0u8, [0usize, 1, 2, 3]),
        ("long and start with middle/side", 1, 2, [0, 1, 0, 1]),
        ("mixed", 0, 0, [4, 4, 4, 4]),
        ("short with middle/side", 1, 2, [2, 2, 2, 2]),
    ] {
        let mut specs = vec![Vec::new(), Vec::new()];
        for (n, &k) in kinds.iter().enumerate() {
            specs[n / 2].push(make(k, &mut seed));
        }
        let (si, w) = assemble(&mut specs, [0, 0], 0);
        let frame = frame_bytes(&v1_frame_header(mode, ext), &si, &w.bytes);
        assert_same_audio(&frame, what);
    }
}

#[test]
fn mpeg2_mixed_and_long_blocks_synthesize_like_the_definition() {
    let mut seed = 123;
    // 22.05 kHz mono, one granule per frame. Mixed: compress 300 = slen (3, 3, 3, 0).
    let sf: Vec<u32> = (0..24).map(|i| (i * 3 + 1) % 8).collect();
    let mixed = granule(
        loud(short_info(300, 60, [13, 15], true)),
        [36, 576],
        sf_bits(&sf, &[3; 24]),
        10,
        &mut seed,
    );
    let sf: Vec<u32> = (0..21).map(|i| i % 2).collect();
    let b: Vec<u32> = (0..21).map(|i| if i < 11 { 1 } else { 2 }).collect();
    let long = granule(
        loud(long_info(505, 60, [13, 15, 13], 5, 4)),
        [36, 96],
        sf_bits(&sf, &b),
        10,
        &mut seed,
    );
    for (what, g) in [("mixed", mixed), ("long with preflag", long)] {
        let mut specs = vec![vec![g]];
        let (si, w) = assemble(&mut specs, [0, 0], 0);
        let frame = frame_bytes(&v2_mono(), &si, &w.bytes);
        assert_same_audio(&frame, &format!("22.05 kHz {what}"));
    }
}

/// An empty granule: no lines, no bits.
fn empty(seed: &mut u32) -> GrSpec {
    granule(long_info(0, 0, [0; 3], 0, 0), [576, 576], vec![], 0, seed)
}

/// Two frames with the same left channel: one plain stereo with a silent right channel, and
/// one with the header `intensity` and the right channel `right`. With an empty right channel
/// every band is intensity and every line of the left channel is split with the same gains,
/// so the output is the plain left channel's output scaled (everything after requantization
/// is linear).
fn intensity_frames(
    plain: &FrameHeader,
    intensity: &FrameHeader,
    right: &dyn Fn(&mut u32) -> GrSpec,
    granules: usize,
) -> (Vec<u8>, Vec<u8>) {
    // Regions of `long_info(.., 6, 5)`: 7 and 13 bands.
    let regions = if plain.version == MpegVersion::V1 {
        [30, 90]
    } else {
        [44, 140]
    };
    let build = |h: &FrameHeader, intens: bool| {
        let mut seed = 4711;
        let mut specs = vec![Vec::new(); granules];
        for spec in &mut specs {
            let l = granule(
                loud(long_info(0, 90, [13, 15, 13], 6, 5)),
                regions,
                vec![],
                14,
                &mut seed,
            );
            let r = if intens {
                right(&mut seed)
            } else {
                empty(&mut seed)
            };
            spec.extend([l, r]);
        }
        let (si, w) = assemble(&mut specs, [0, 0], 0);
        frame_bytes(h, &si, &w.bytes)
    };
    (build(plain, false), build(intensity, true))
}

fn scaled_matches(plain: &[Vec<f32>], got: &[Vec<f32>], kl: f64, kr: f64, what: &str) {
    let peak = plain[0].iter().fold(0.0f32, |m, v| m.max(v.abs()));
    assert!(peak > 0.05, "{what}: left channel too quiet ({peak})");
    for (i, &x) in plain[0].iter().enumerate() {
        for (ch, k) in [(0, kl), (1, kr)] {
            let e = (f64::from(got[ch][i]) - k * f64::from(x)).abs();
            assert!(
                e < 1e-5 * f64::from(peak),
                "{what} ch{ch} sample {i}: {e:e}"
            );
        }
    }
}

#[test]
fn mpeg1_intensity_stereo_splits_the_left_channel_by_position() {
    let plain_h = v1_frame_header(0, 0);
    let int_h = v1_frame_header(1, 1);
    let both_h = v1_frame_header(1, 3);
    for pos in 0..=7u32 {
        // Right channel scalefactors of three bits each (compress 13 = slen 3, 3).
        let right = move |seed: &mut u32| {
            granule(
                long_info(13, 0, [0; 3], 0, 0),
                [576, 576],
                vec![(pos, 3); 21],
                0,
                seed,
            )
        };
        let (plain, intensity) = intensity_frames(&plain_h, &int_h, &right, 2);
        let base = decode_pcm(&plain);
        let t = (f64::from(pos) * std::f64::consts::PI / 12.0).tan();
        // Positions 0 to 6 split by the tangent; 7 is illegal and leaves the channels alone
        // (the right channel stays silent).
        let (kl, kr) = match pos {
            6 | 7 => (1.0, 0.0),
            _ => (t / (1.0 + t), 1.0 / (1.0 + t)),
        };
        scaled_matches(
            &base,
            &decode_pcm(&intensity),
            kl,
            kr,
            &format!("position {pos}"),
        );
        // With middle/side on as well, an illegal position falls back to middle/side.
        if pos == 7 {
            let (_, both) = intensity_frames(&plain_h, &both_h, &right, 2);
            let k = std::f64::consts::FRAC_1_SQRT_2;
            scaled_matches(&base, &decode_pcm(&both), k, k, "illegal position with ms");
        }
    }
}

#[test]
fn mpeg2_intensity_stereo_uses_odd_even_positions_and_the_scale_bit() {
    let plain_h = v2_stereo();
    let int_h = v2_intensity();
    // Right channel: sfc = 86 -> slen (2, 2, 2, 0) with 7 7 7 0 entries, so every position
    // (0 to 2; 3 is illegal) fits in two bits. scalefac_compress = 2 * sfc + scale bit.
    for scale in 0..2u16 {
        let i0 = 2f64.powf(-0.25 * f64::from(scale + 1));
        for pos in 0..=3u32 {
            let right = move |seed: &mut u32| {
                granule(
                    long_info(172 + scale, 0, [0; 3], 0, 0),
                    [576, 576],
                    vec![(pos, 2); 21],
                    0,
                    seed,
                )
            };
            let (plain, intensity) = intensity_frames(&plain_h, &int_h, &right, 1);
            let base = decode_pcm(&plain);
            let (kl, kr) = match pos {
                0 => (1.0, 1.0),
                1 => (i0, 1.0),
                2 => (1.0, i0),
                // Illegal: no intensity, the right channel stays silent.
                _ => (1.0, 0.0),
            };
            scaled_matches(
                &base,
                &decode_pcm(&intensity),
                kl,
                kr,
                &format!("scale {scale} position {pos}"),
            );
        }
    }
}

#[test]
fn a_right_channel_without_positions_is_not_intensity() {
    // compress 0 on the right channel of an MPEG-2 intensity frame: no position is sent, all
    // are illegal, and the right channel stays silent.
    let (plain, intensity) = intensity_frames(&v2_stereo(), &v2_intensity(), &empty, 1);
    let (base, got) = (decode_pcm(&plain), decode_pcm(&intensity));
    scaled_matches(&base, &got, 1.0, 0.0, "no positions");
}

#[test]
fn granules_without_reservoir_data_are_silent() {
    // The third of the reservoir test frames reaches 300 bytes back; decoded on its own its
    // first granule has no data, the second starts inside the frame.
    let mut seed = 31337;
    let (frames, _) = reservoir_frames(&mut seed);
    let pcm = decode_pcm(&frames[2]);
    assert_eq!(pcm[0].len(), 1152);
    assert!(pcm[0][..576].iter().all(|&v| v == 0.0));
    assert!(pcm[0][576..].iter().any(|&v| v != 0.0));
}
