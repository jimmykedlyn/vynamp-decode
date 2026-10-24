//! Frame header parsing and stream probing.
//!
//! The tables below are written out independently of the implementation, from the
//! bitrate/sample-rate tables of ISO/IEC 11172-3 and 13818-3.

use vynamp_decode_mpa::{ChannelMode, MpegVersion, parse_header, probe_mpa};

const V1_L1: [u32; 14] = [
    32, 64, 96, 128, 160, 192, 224, 256, 288, 320, 352, 384, 416, 448,
];
const V1_L2: [u32; 14] = [
    32, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384,
];
const V1_L3: [u32; 14] = [
    32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
];
const V2_L1: [u32; 14] = [
    32, 48, 56, 64, 80, 96, 112, 128, 144, 160, 176, 192, 224, 256,
];
const V2_L23: [u32; 14] = [8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160];

fn bitrates(v: MpegVersion, layer: u8) -> &'static [u32; 14] {
    match (v, layer) {
        (MpegVersion::V1, 1) => &V1_L1,
        (MpegVersion::V1, 2) => &V1_L2,
        (MpegVersion::V1, _) => &V1_L3,
        (_, 1) => &V2_L1,
        _ => &V2_L23,
    }
}

fn rates(v: MpegVersion) -> [u32; 3] {
    match v {
        MpegVersion::V1 => [44100, 48000, 32000],
        MpegVersion::V2 => [22050, 24000, 16000],
        MpegVersion::V25 => [11025, 12000, 8000],
    }
}

fn version_bits(v: MpegVersion) -> u8 {
    match v {
        MpegVersion::V1 => 3,
        MpegVersion::V2 => 2,
        MpegVersion::V25 => 0,
    }
}

/// Header bytes with the given fields, no CRC, stereo.
fn bytes(v: MpegVersion, layer: u8, br_idx: u8, sr_idx: u8, pad: bool) -> [u8; 4] {
    let layer_bits = 4 - layer;
    [
        0xFF,
        0xE0 | version_bits(v) << 3 | layer_bits << 1 | 1,
        br_idx << 4 | sr_idx << 2 | u8::from(pad) << 1,
        0x00,
    ]
}

#[test]
fn every_bitrate_and_rate_index_for_every_version_and_layer() {
    let mut checked = 0;
    for v in [MpegVersion::V1, MpegVersion::V2, MpegVersion::V25] {
        for layer in 1..=3u8 {
            for br_idx in 1..=14u8 {
                for sr_idx in 0..=2u8 {
                    for pad in [false, true] {
                        let h = parse_header(bytes(v, layer, br_idx, sr_idx, pad))
                            .unwrap_or_else(|| panic!("{v:?} L{layer} br{br_idx} sr{sr_idx}"));
                        let br = bitrates(v, layer)[usize::from(br_idx) - 1];
                        let sr = rates(v)[usize::from(sr_idx)];
                        assert_eq!(h.version, v);
                        assert_eq!(h.layer, layer);
                        assert_eq!(h.bitrate_kbps, br);
                        assert_eq!(h.sample_rate, sr);
                        assert_eq!(h.padding, pad);
                        assert!(!h.crc);
                        let samples = match (layer, v) {
                            (1, _) => 384,
                            (3, MpegVersion::V1) | (2, _) => 1152,
                            _ => 576,
                        };
                        assert_eq!(h.samples, samples);
                        let pad = usize::from(pad);
                        let len = match (layer, v) {
                            (1, _) => (12 * br as usize * 1000 / sr as usize + pad) * 4,
                            (3, MpegVersion::V2 | MpegVersion::V25) => {
                                72 * br as usize * 1000 / sr as usize + pad
                            }
                            _ => 144 * br as usize * 1000 / sr as usize + pad,
                        };
                        assert_eq!(h.frame_len, len, "{v:?} L{layer} br{br_idx} sr{sr_idx}");
                        checked += 1;
                    }
                }
            }
        }
    }
    assert_eq!(checked, 3 * 3 * 14 * 3 * 2);
}

#[test]
fn free_format_has_bitrate_zero_and_unknown_length() {
    let h = parse_header(bytes(MpegVersion::V1, 3, 0, 1, true)).unwrap();
    assert_eq!(h.bitrate_kbps, 0);
    assert_eq!(h.frame_len, 0);
    assert!(h.padding);
    assert_eq!(h.sample_rate, 48000);
}

#[test]
fn reserved_and_invalid_values_are_rejected() {
    let good = bytes(MpegVersion::V1, 3, 9, 1, false);
    assert!(parse_header(good).is_some());
    // Sync: all eleven bits are required.
    for (i, mask) in [(0usize, 0x01u8), (1, 0x80), (1, 0x40), (1, 0x20)] {
        let mut b = good;
        b[i] &= !mask;
        assert_eq!(parse_header(b), None, "sync bit {i}/{mask:#x}");
    }
    let mut b = good;
    b[1] = (b[1] & !0x18) | 0x08; // version bits 01 (reserved)
    assert_eq!(parse_header(b), None);
    let mut b = good;
    b[1] &= !0x06; // layer bits 00 (reserved)
    assert_eq!(parse_header(b), None);
    let mut b = good;
    b[2] |= 0xF0; // bitrate index 15 (forbidden)
    assert_eq!(parse_header(b), None);
    let mut b = good;
    b[2] |= 0x0C; // sample rate index 3 (reserved)
    assert_eq!(parse_header(b), None);
}

#[test]
fn other_fields() {
    // MPEG-1 Layer III, CRC present (protection bit 0), joint stereo, mode_ext 2, emphasis 1.
    let h = parse_header([0xFF, 0xFA, 0x90, 0x40 | 0x20 | 0x01]).unwrap();
    assert!(h.crc);
    assert_eq!(h.channel_mode, ChannelMode::JointStereo);
    assert_eq!(h.mode_ext, 2);
    assert_eq!(h.emphasis, 1);
    for (bits, mode) in [
        (0u8, ChannelMode::Stereo),
        (1, ChannelMode::JointStereo),
        (2, ChannelMode::DualChannel),
        (3, ChannelMode::Mono),
    ] {
        let h = parse_header([0xFF, 0xFB, 0x90, bits << 6]).unwrap();
        assert_eq!(h.channel_mode, mode);
    }
}

// ------------------------------------------------------------------ probing

fn frame(first: [u8; 4], len: usize) -> Vec<u8> {
    let mut f = vec![0u8; len];
    f[..4].copy_from_slice(&first);
    f
}

/// 128 kbps, 48 kHz Layer III frames are 384 bytes.
const H48: [u8; 4] = [0xFF, 0xFB, 0x94, 0x00];
/// 128 kbps, 44.1 kHz.
const H44: [u8; 4] = [0xFF, 0xFB, 0x90, 0x00];

#[test]
fn probe_scores() {
    let mut three = Vec::new();
    for _ in 0..3 {
        three.extend(frame(H48, 384));
    }
    assert_eq!(probe_mpa(&three), 90);
    // One frame only, then nothing.
    assert_eq!(probe_mpa(&frame(H48, 384)), 30);
    // A header alone.
    assert_eq!(probe_mpa(&H48), 30);
    // Two frames.
    let mut two = frame(H48, 384);
    two.extend(frame(H48, 384));
    assert_eq!(probe_mpa(&two), 60);
    // The second frame has another sample rate: only one counts, on either side.
    let mut mixed = frame(H48, 384);
    mixed.extend(frame(H44, 417));
    assert_eq!(probe_mpa(&mixed), 30);
    assert_eq!(probe_mpa(&[]), 0);
    assert_eq!(probe_mpa(&[0u8; 4096]), 0);
    assert_eq!(probe_mpa(b"RIFF\0\0\0\0WAVEfmt "), 0);
    assert_eq!(probe_mpa(b"fLaC\0\0\0\"\x10\0\x10\0"), 0);
}

#[test]
fn probe_tolerates_leading_junk_and_a_cut_last_frame() {
    let mut v = vec![0u8; 100];
    for _ in 0..4 {
        v.extend(frame(H48, 384));
    }
    v.truncate(v.len() - 200); // the last frame is cut
    assert_eq!(probe_mpa(&v), 90);
}

#[test]
fn probe_gives_no_score_to_a_free_format_header_away_from_the_start() {
    // A free-format header (bitrate index 0) has no length to follow, so away from the start
    // it is as likely to be chance as any lone header: zero bytes after a damaged ID3 tag.
    let mut v = b"ID3\x04\x00\x00\xFF\xFF\xFF\xFF".to_vec();
    v.resize(2000, 0);
    assert_eq!(probe_mpa(&v), 0);
    // At the very start it still counts a little (the demuxer measures the frame length).
    let mut free = vec![0xFF, 0xFB, 0x00, 0x00];
    free.resize(2000, 0);
    assert_eq!(probe_mpa(&free), 30);
}
