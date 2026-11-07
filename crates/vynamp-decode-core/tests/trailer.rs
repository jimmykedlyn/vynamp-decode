use vynamp_decode_core::{Trailer, TrailerKind, VecSource, crc16_mpeg, find_trailers};

fn synchsafe(n: u32) -> [u8; 4] {
    [
        ((n >> 21) & 0x7F) as u8,
        ((n >> 14) & 0x7F) as u8,
        ((n >> 7) & 0x7F) as u8,
        (n & 0x7F) as u8,
    ]
}

fn id3v2_with_footer(body_len: usize) -> Vec<u8> {
    let mut t = b"ID3\x04\x00\x10".to_vec();
    t.extend(synchsafe(body_len as u32));
    t.extend(vec![0xFFu8; body_len]);
    t.extend(b"3DI\x04\x00\x10");
    t.extend(synchsafe(body_len as u32));
    t
}

fn lyrics3v2(body_len: usize) -> Vec<u8> {
    let mut t = b"LYRICSBEGIN".to_vec();
    t.extend(vec![0xFFu8; body_len]);
    t.extend(format!("{:06}", 11 + body_len).bytes());
    t.extend(b"LYRICS200");
    t
}

fn id3v1() -> Vec<u8> {
    let mut tag = b"TAG".to_vec();
    tag.resize(128, b' ');
    tag
}

fn ape_footer(items: usize, with_header: bool) -> Vec<u8> {
    let size = (32 + items) as u32;
    let mut footer = b"APETAGEX".to_vec();
    footer.extend(2000u32.to_le_bytes());
    footer.extend(size.to_le_bytes());
    footer.extend(0u32.to_le_bytes());
    let flags: u32 = if with_header { 0x8000_0000 } else { 0 };
    footer.extend(flags.to_le_bytes());
    footer.extend([0u8; 8]);
    let mut tag = Vec::new();
    if with_header {
        let mut h = footer.clone();
        h[20..24].copy_from_slice(&(flags | 0x2000_0000).to_le_bytes());
        tag.extend(h);
    }
    tag.extend(vec![0xFFu8; items]);
    tag.extend(footer);
    tag
}

const AUDIO: usize = 1000;
const START: u64 = 4;

/// Audio of `AUDIO` bytes followed by `parts`; returns the detection result and each
/// part's expected range.
fn run(parts: &[(TrailerKind, Vec<u8>)]) -> (u64, Vec<Trailer>, Vec<Trailer>) {
    let mut file = vec![0xA5u8; AUDIO];
    let mut want = Vec::new();
    for (kind, p) in parts {
        want.push(Trailer {
            kind: *kind,
            offset: file.len() as u64,
            len: p.len() as u64,
        });
        file.extend(p);
    }
    let mut src = VecSource::new(file);
    let (end, got) = find_trailers(&mut src, START).unwrap();
    (end, got, want)
}

#[test]
fn no_trailer_ends_at_the_source_end() {
    let (end, got, _) = run(&[]);
    assert_eq!((end, got), (AUDIO as u64, vec![]));
}

#[test]
fn each_kind_reports_its_range() {
    use TrailerKind::*;
    for part in [
        (Id3v1, id3v1()),
        (ApeV2, ape_footer(7, false)),
        (ApeV2, ape_footer(7, true)),
        (Lyrics3v2, lyrics3v2(25)),
        (Id3v2Appended, id3v2_with_footer(40)),
    ] {
        let (end, got, want) = run(&[part]);
        assert_eq!(end, AUDIO as u64);
        assert_eq!(got, want);
    }
}

#[test]
fn trailers_combine_in_any_order_outermost_last() {
    use TrailerKind::*;
    let ape = (ApeV2, ape_footer(2, true));
    let orders: [Vec<(TrailerKind, Vec<u8>)>; 4] = [
        vec![
            ape.clone(),
            (Id3v2Appended, id3v2_with_footer(40)),
            (Id3v1, id3v1()),
        ],
        vec![
            (Id3v2Appended, id3v2_with_footer(40)),
            ape.clone(),
            (Id3v1, id3v1()),
        ],
        vec![(Lyrics3v2, lyrics3v2(30)), ape, (Id3v1, id3v1())],
        vec![(Id3v1, id3v1()), (Id3v2Appended, id3v2_with_footer(5))],
    ];
    for parts in orders {
        let (end, got, want) = run(&parts);
        assert_eq!(end, AUDIO as u64);
        // File order: the outermost (last) trailer is last.
        assert_eq!(got, want);
    }
}

#[test]
fn trailers_never_reach_into_the_payload_start() {
    // An ID3v1-looking tag that would begin before `payload_start` is not a trailer.
    let mut src = VecSource::new(id3v1());
    let (end, got) = find_trailers(&mut src, 8).unwrap();
    assert_eq!((end, got), (128, vec![]));
    let mut src = VecSource::new(id3v1());
    let (end, got) = find_trailers(&mut src, 0).unwrap();
    assert_eq!(end, 0);
    assert_eq!(got.len(), 1);
}

#[test]
fn at_most_sixteen_trailers_are_stripped() {
    let mut file = vec![0u8; 64];
    for _ in 0..20 {
        file.extend(id3v1());
    }
    let mut src = VecSource::new(file);
    let (end, got) = find_trailers(&mut src, 0).unwrap();
    assert_eq!(got.len(), 16);
    assert_eq!(end, 64 + 4 * 128);
}

#[test]
fn junk_is_not_a_trailer() {
    let mut file = vec![0xA5u8; AUDIO];
    file.extend(vec![0x11u8; 700]);
    let mut src = VecSource::new(file);
    let (end, got) = find_trailers(&mut src, START).unwrap();
    assert_eq!((end, got), (1700, vec![]));
}

#[test]
fn crc16_mpeg_check_value() {
    assert_eq!(crc16_mpeg(b"123456789"), 0xAEE7);
    assert_eq!(crc16_mpeg(b""), 0xFFFF);
}
