use super::super::testenc::{header, side_info_bytes, v1_mono, v1_stereo, v2_mono, v2_stereo};
use super::*;
use vynamp_decode_mpa::FrameHeader;

fn long(n: u16) -> GranuleInfo {
    GranuleInfo {
        part2_3_length: 700 + n,
        big_values: 100 + n,
        global_gain: 150 + n as u8,
        scalefac_compress: n % 16,
        table_select: [1 + n as u8, 2 + n as u8, 3 + n as u8],
        region0_count: (n % 16) as u8,
        region1_count: (n % 8) as u8,
        scalefac_scale: n % 2 == 1,
        count1_table: n.is_multiple_of(3),
        ..GranuleInfo::default()
    }
}

fn short(n: u16, mixed: bool) -> GranuleInfo {
    GranuleInfo {
        part2_3_length: 900 + n,
        big_values: 50 + n,
        global_gain: 100 + n as u8,
        scalefac_compress: 5 + n,
        window_switching: true,
        block_type: 2,
        mixed_block: mixed,
        table_select: [7 + n as u8, 9, 0],
        subblock_gain: [1, 7, n as u8 % 8],
        // Fixed by the standard when window switching is on.
        region0_count: if mixed { 7 } else { 8 },
        region1_count: if mixed { 13 } else { 12 },
        preflag: true,
        ..GranuleInfo::default()
    }
}

fn roundtrip(h: &FrameHeader, si: &SideInfo) -> SideInfo {
    let bytes = side_info_bytes(si, h);
    parse(&bytes, h, 1000).expect("parse")
}

#[test]
fn mpeg1_stereo_round_trips() {
    let mut si = SideInfo {
        main_data_begin: 511,
        scfsi: [0b1010, 0b0101],
        ..SideInfo::default()
    };
    si.granules[0][0] = long(1);
    si.granules[0][1] = short(2, false);
    si.granules[1][0] = GranuleInfo {
        preflag: true,
        ..long(3)
    };
    si.granules[1][1] = short(4, true);
    assert_eq!(roundtrip(&v1_stereo(), &si), si);
}

#[test]
fn mpeg1_mono_has_one_channel_of_two_granules() {
    let mut si = SideInfo {
        main_data_begin: 3,
        scfsi: [0b1111, 0],
        ..SideInfo::default()
    };
    si.granules[0][0] = long(5);
    si.granules[1][0] = GranuleInfo {
        window_switching: true,
        block_type: 3,
        table_select: [4 + 1, 31, 0],
        subblock_gain: [0, 0, 0],
        region0_count: 7,
        region1_count: 13,
        ..long(6)
    };
    // Block types 1 and 3 do not use the mixed flag.
    si.granules[1][0].part2_3_length = 4095;
    si.granules[1][0].big_values = 288;
    assert_eq!(roundtrip(&v1_mono(), &si), si);
}

#[test]
fn mpeg2_has_one_granule_and_no_scfsi_or_preflag() {
    let mut si = SideInfo {
        main_data_begin: 255,
        ..SideInfo::default()
    };
    si.granules[0][0] = GranuleInfo {
        scalefac_compress: 511,
        ..long(7)
    };
    si.granules[0][1] = GranuleInfo {
        preflag: false,
        scalefac_compress: 300,
        ..short(1, true)
    };
    assert_eq!(roundtrip(&v2_stereo(), &si), si);
    let mut mono = SideInfo::default();
    mono.granules[0][0] = long(2);
    assert_eq!(roundtrip(&v2_mono(), &mono), mono);
    let h25 = header(0, 8, 2, 0, 0, false);
    assert_eq!(roundtrip(&h25, &si), si);
}

#[test]
fn the_mixed_flag_means_nothing_without_short_blocks() {
    let mut si = SideInfo::default();
    si.granules[0][0] = GranuleInfo {
        window_switching: true,
        block_type: 1,
        mixed_block: true,
        table_select: [3, 4, 0],
        region0_count: 7,
        region1_count: 13,
        ..long(1)
    };
    let h = v2_mono();
    let got = roundtrip(&h, &si);
    assert!(!got.granules[0][0].mixed_block);
    assert_eq!(got.granules[0][0].block_type, 1);
}

#[test]
fn bad_side_info_is_invalid_with_the_absolute_offset() {
    let h = v1_stereo();
    let mut si = SideInfo::default();
    si.granules[0][0] = long(1);
    si.granules[0][0].big_values = 289;
    let bytes = side_info_bytes(&si, &h);
    assert_eq!(
        parse(&bytes, &h, 1000),
        Err(vynamp_decode_core::Error::Invalid {
            what: "mp3 side info",
            offset: 1000,
        })
    );
    // Window switching with block type 0 is reserved.
    let mut si = SideInfo::default();
    si.granules[1][1] = short(1, false);
    si.granules[1][1].block_type = 0;
    let bytes = side_info_bytes(&si, &h);
    assert!(parse(&bytes, &h, 0).is_err());
    // Wrong length.
    assert!(parse(&bytes[..31], &h, 0).is_err());
    assert!(parse(&[], &h, 0).is_err());
}

#[test]
fn parsing_garbage_never_panics() {
    let mut seed = 0xBEEF_u32;
    let headers = [v1_stereo(), v1_mono(), v2_stereo(), v2_mono()];
    for i in 0..4000 {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let h = &headers[i % 4];
        let bytes: Vec<u8> = (0..h.side_info_len())
            .map(|k| (seed >> (k % 24)) as u8 ^ (k as u8).wrapping_mul(37))
            .collect();
        let _ = parse(&bytes, h, 0);
    }
}
