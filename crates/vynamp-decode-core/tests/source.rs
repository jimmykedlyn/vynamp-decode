use vynamp_decode_core::{ByteReader, Error, FileSource, SliceSource, Source, VecSource};

const D: [u8; 4] = [1, 2, 3, 4];

#[test]
fn endianness() {
    let mut s = SliceSource::new(&D);
    let mut r = ByteReader::new(&mut s);
    assert_eq!(r.u32_le().unwrap(), 0x0403_0201);
    let mut s = SliceSource::new(&D);
    let mut r = ByteReader::new(&mut s);
    assert_eq!(r.u32_be().unwrap(), 0x0102_0304);
    let mut s = SliceSource::new(&D);
    let mut r = ByteReader::new(&mut s);
    assert_eq!(r.u16_le().unwrap(), 0x0201);
    assert_eq!(r.u16_be().unwrap(), 0x0304);
    assert_eq!(r.position(), 4);
}

#[test]
fn wide_and_fourcc() {
    let d = [1u8, 2, 3, 4, 5, 6, 7, 8, b'R', b'I', b'F', b'F'];
    let mut s = VecSource::new(d.to_vec());
    let mut r = ByteReader::new(&mut s);
    assert_eq!(r.u64_le().unwrap(), 0x0807_0605_0403_0201);
    assert_eq!(r.fourcc().unwrap(), *b"RIFF");
    let mut s = SliceSource::new(&d);
    let mut r = ByteReader::new(&mut s);
    assert_eq!(r.u64_be().unwrap(), 0x0102_0304_0506_0708);
    assert_eq!(r.u8().unwrap(), b'R');
}

#[test]
fn eof_reports_offset() {
    let d = [9u8, 9];
    let mut s = SliceSource::new(&d);
    let mut r = ByteReader::new(&mut s);
    let mut buf = [0u8; 4];
    assert_eq!(
        r.read_exact(&mut buf),
        Err(Error::Invalid {
            what: "unexpected eof",
            offset: 2
        })
    );
}

#[test]
fn seek_past_end_then_read() {
    let mut s = SliceSource::new(&D);
    s.seek(100).unwrap();
    assert_eq!(s.position(), 100);
    let mut r = ByteReader::new(&mut s);
    assert!(matches!(r.u8(), Err(Error::Invalid { .. })));
}

#[test]
fn skip_and_len() {
    let mut s = SliceSource::new(&D);
    assert_eq!(s.len(), Some(4));
    let mut r = ByteReader::new(&mut s);
    r.skip(3).unwrap();
    assert_eq!(r.u8().unwrap(), 4);
    assert!(r.skip(1).is_err());
    assert!(r.skip(u64::MAX).is_err());
}

#[test]
fn file_source_matches_slice() {
    let data: Vec<u8> = (0..=255u8).cycle().take(200_000).collect();
    let path = std::env::temp_dir().join(format!(
        "vynamp-decode-core-source-{}.bin",
        std::process::id()
    ));
    std::fs::write(&path, &data).unwrap();
    let mut f = FileSource::open(&path).unwrap();
    assert_eq!(f.len(), Some(data.len() as u64));
    let mut a = vec![0u8; data.len()];
    ByteReader::new(&mut f).read_exact(&mut a).unwrap();
    assert_eq!(a, data);
    f.seek(70_000).unwrap();
    assert_eq!(f.position(), 70_000);
    let mut b = [0u8; 8];
    ByteReader::new(&mut f).read_exact(&mut b).unwrap();
    assert_eq!(&b, &data[70_000..70_008]);
    assert_eq!(f.position(), 70_008);
    drop(f);
    std::fs::remove_file(&path).unwrap();
    assert!(matches!(FileSource::open(&path), Err(Error::Io(_))));
}
