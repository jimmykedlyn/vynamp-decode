//! Xing / Info / LAME / VBRI header parsing on synthetic frames.

use vynamp_decode_mpa::{LameInfo, VbrKind, parse_vbr};

/// MPEG-1 Layer III, 128 kbps, 48 kHz, stereo, no CRC: 384 bytes.
fn blank_frame() -> Vec<u8> {
    let mut f = vec![0u8; 384];
    f[..4].copy_from_slice(&[0xFF, 0xFB, 0x94, 0x00]);
    f
}

/// CRC-16/ARC (reflected 0x8005, initial value 0): what the LAME tag uses. Written out
/// independently of the implementation.
fn lame_crc(data: &[u8]) -> u16 {
    let mut crc = 0u16;
    for &b in data {
        crc ^= u16::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xA001
            } else {
                crc >> 1
            };
        }
    }
    crc
}

/// Stereo MPEG-1 frame with an Info/Xing header at 36 and a LAME tag at 156.
fn xing_frame(tag: &[u8; 4], frames: u32, bytes: u32, delay: u16, padding: u16) -> Vec<u8> {
    let mut f = blank_frame();
    f[36..40].copy_from_slice(tag);
    f[40..44].copy_from_slice(&0xFu32.to_be_bytes());
    f[44..48].copy_from_slice(&frames.to_be_bytes());
    f[48..52].copy_from_slice(&bytes.to_be_bytes());
    for i in 0..100 {
        f[52 + i] = (i * 255 / 99) as u8;
    }
    f[152..156].copy_from_slice(&75u32.to_be_bytes());
    f[156..165].copy_from_slice(b"LAME3.100");
    f[165] = 0x04; // revision 0, method 4
    f[177] = (delay >> 4) as u8;
    f[178] = ((delay & 0xF) << 4) as u8 | (padding >> 8) as u8;
    f[179] = padding as u8;
    let crc = lame_crc(&f[..190]);
    f[190..192].copy_from_slice(&crc.to_be_bytes());
    f
}

#[test]
fn xing_with_valid_lame_tag() {
    let f = xing_frame(b"Xing", 1000, 400_000, 576, 1152);
    let v = parse_vbr(&f).unwrap();
    assert_eq!(v.kind, VbrKind::Xing);
    assert_eq!(v.frames, Some(1000));
    assert_eq!(v.bytes, Some(400_000));
    let toc = v.toc.as_ref().unwrap();
    assert_eq!(toc[0], 0);
    assert_eq!(toc[99], 255);
    assert_eq!(
        v.lame,
        Some(LameInfo {
            encoder_delay: 576,
            padding: 1152,
            crc_ok: true
        })
    );
}

#[test]
fn info_kind_and_bad_crc() {
    let mut f = xing_frame(b"Info", 10, 4000, 100, 200);
    let v = parse_vbr(&f).unwrap();
    assert_eq!(v.kind, VbrKind::Info);
    f[170] ^= 0x01; // inside the CRC-covered area
    let v = parse_vbr(&f).unwrap();
    let lame = v.lame.unwrap();
    assert!(!lame.crc_ok);
}

#[test]
fn flags_select_the_fields() {
    let mut f = blank_frame();
    f[36..40].copy_from_slice(b"Xing");
    f[40..44].copy_from_slice(&1u32.to_be_bytes()); // frames only
    f[44..48].copy_from_slice(&7u32.to_be_bytes());
    let v = parse_vbr(&f).unwrap();
    assert_eq!((v.frames, v.bytes, v.toc.is_some()), (Some(7), None, false));
    assert_eq!(v.lame, None);
    f[40..44].copy_from_slice(&2u32.to_be_bytes()); // bytes only
    f[44..48].copy_from_slice(&9u32.to_be_bytes());
    let v = parse_vbr(&f).unwrap();
    assert_eq!((v.frames, v.bytes), (None, Some(9)));
}

#[test]
fn xing_offsets_follow_version_and_channels() {
    // MPEG-1 mono: side info 17 bytes, Xing at 21.
    let mut f = vec![0u8; 200];
    f[..4].copy_from_slice(&[0xFF, 0xFB, 0x94, 0xC0]);
    f[21..25].copy_from_slice(b"Info");
    f[25..29].copy_from_slice(&1u32.to_be_bytes());
    f[29..33].copy_from_slice(&5u32.to_be_bytes());
    assert_eq!(parse_vbr(&f).unwrap().frames, Some(5));
    // MPEG-2 stereo: 17 bytes. MPEG-2 mono: 9 bytes.
    let mut f = vec![0u8; 200];
    f[..4].copy_from_slice(&[0xFF, 0xF3, 0x94, 0x00]);
    f[21..25].copy_from_slice(b"Xing");
    f[25..29].copy_from_slice(&1u32.to_be_bytes());
    f[29..33].copy_from_slice(&6u32.to_be_bytes());
    assert_eq!(parse_vbr(&f).unwrap().frames, Some(6));
    f[..4].copy_from_slice(&[0xFF, 0xF3, 0x94, 0xC0]);
    f[21..25].fill(0);
    f[13..17].copy_from_slice(b"Xing");
    f[17..21].copy_from_slice(&1u32.to_be_bytes());
    f[21..25].copy_from_slice(&8u32.to_be_bytes());
    assert_eq!(parse_vbr(&f).unwrap().frames, Some(8));
}

#[test]
fn xing_after_a_header_crc() {
    // Protection bit 0: two CRC bytes sit between the header and the side info.
    let mut f = vec![0u8; 200];
    f[..4].copy_from_slice(&[0xFF, 0xFA, 0x94, 0x00]);
    f[38..42].copy_from_slice(b"Xing");
    f[42..46].copy_from_slice(&1u32.to_be_bytes());
    f[46..50].copy_from_slice(&11u32.to_be_bytes());
    assert_eq!(parse_vbr(&f).unwrap().frames, Some(11));
}

#[test]
fn vbri() {
    let mut f = blank_frame();
    f[36..40].copy_from_slice(b"VBRI");
    f[40..42].copy_from_slice(&1u16.to_be_bytes()); // version
    f[42..44].copy_from_slice(&0x0123u16.to_be_bytes()); // delay
    f[44..46].copy_from_slice(&75u16.to_be_bytes()); // quality
    f[46..50].copy_from_slice(&123_456u32.to_be_bytes());
    f[50..54].copy_from_slice(&300u32.to_be_bytes());
    f[54..56].copy_from_slice(&3u16.to_be_bytes()); // entries
    f[56..58].copy_from_slice(&2u16.to_be_bytes()); // scale
    f[58..60].copy_from_slice(&2u16.to_be_bytes()); // entry size
    f[60..62].copy_from_slice(&100u16.to_be_bytes()); // frames per entry
    for (i, v) in [10u16, 20, 30].iter().enumerate() {
        f[62 + 2 * i..64 + 2 * i].copy_from_slice(&v.to_be_bytes());
    }
    let v = parse_vbr(&f).unwrap();
    assert_eq!(v.kind, VbrKind::Vbri);
    assert_eq!(v.frames, Some(300));
    assert_eq!(v.bytes, Some(123_456));
    assert_eq!(v.vbri_table, vec![20, 40, 60]);
    assert_eq!(v.vbri_frames_per_entry, 100);
    assert!(v.toc.is_none());
    // A table that does not fit in the frame is rejected, not read out of bounds.
    f[54..56].copy_from_slice(&5000u16.to_be_bytes());
    assert_eq!(parse_vbr(&f), None);
}

#[test]
fn no_header_and_short_input() {
    assert_eq!(parse_vbr(&blank_frame()), None);
    assert_eq!(parse_vbr(&[]), None);
    assert_eq!(parse_vbr(&[0xFF, 0xFB, 0x94]), None);
    // A truncated frame must never panic, whatever it cuts.
    let f = xing_frame(b"Xing", 1, 1, 1, 1);
    for n in 0..f.len() {
        let _ = parse_vbr(&f[..n]);
    }
}

#[test]
fn lame_crc_variant_is_crc16_arc() {
    // The check value of CRC-16/ARC, so the helper above really is that variant.
    assert_eq!(lame_crc(b"123456789"), 0xBB3D);
}
