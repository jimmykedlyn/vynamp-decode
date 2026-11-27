//! MP4 demuxer over hand-built files: sample tables, seek, chapters, limits and malformed boxes.

use vynamp_decode_core::{
    Chapter, CodecId, Demuxer, Error, GaplessKind, LimitKind, Limits, Mode, Options, Packet,
    SeekTo, Source, VecSource,
};
use vynamp_decode_mp4::{open_mp4, probe_mp4};

const RATE: u32 = 48_000;

fn bx(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut v = ((body.len() + 8) as u32).to_be_bytes().to_vec();
    v.extend_from_slice(kind);
    v.extend_from_slice(body);
    v
}

fn full(kind: &[u8; 4], version_flags: u32, body: &[u8]) -> Vec<u8> {
    bx(kind, &[&version_flags.to_be_bytes()[..], body].concat())
}

fn u32s(v: &[u32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_be_bytes()).collect()
}

fn alac_config(rate: u32, channels: u8) -> Vec<u8> {
    let mut c = 4096u32.to_be_bytes().to_vec();
    c.extend_from_slice(&[0, 16, 40, 10, 14, channels]);
    c.extend_from_slice(&255u16.to_be_bytes());
    c.extend_from_slice(&u32s(&[0, 0, rate]));
    c
}

fn alac_entry(rate: u32, channels: u8) -> Vec<u8> {
    let mut b = vec![0u8; 6];
    b.extend_from_slice(&1u16.to_be_bytes());
    b.extend_from_slice(&[0u8; 8]);
    b.extend_from_slice(&u16::from(channels).to_be_bytes());
    b.extend_from_slice(&16u16.to_be_bytes());
    b.extend_from_slice(&[0u8; 4]);
    b.extend_from_slice(&(rate << 16).to_be_bytes());
    b.extend(full(b"alac", 0, &alac_config(rate, channels)));
    bx(b"alac", &b)
}

/// A sample table: stts entries, per-sample sizes, samples per chunk (one stsc entry per
/// chunk) and chunk offsets.
struct Table {
    stts: Vec<(u32, u32)>,
    sizes: Vec<u32>,
    per_chunk: Vec<u32>,
    offsets: Vec<u32>,
}

fn stbl(entry: Vec<u8>, t: &Table) -> Vec<u8> {
    let mut stsd = u32s(&[1]);
    stsd.extend(entry);
    let mut stts = u32s(&[t.stts.len() as u32]);
    for &(n, d) in &t.stts {
        stts.extend(u32s(&[n, d]));
    }
    let mut stsc = u32s(&[t.per_chunk.len() as u32]);
    for (i, &n) in t.per_chunk.iter().enumerate() {
        stsc.extend(u32s(&[i as u32 + 1, n, 1]));
    }
    let mut stsz = u32s(&[0, t.sizes.len() as u32]);
    stsz.extend(u32s(&t.sizes));
    let mut stco = u32s(&[t.offsets.len() as u32]);
    stco.extend(u32s(&t.offsets));
    bx(
        b"stbl",
        &[
            full(b"stsd", 0, &stsd),
            full(b"stts", 0, &stts),
            full(b"stsc", 0, &stsc),
            full(b"stsz", 0, &stsz),
            full(b"stco", 0, &stco),
        ]
        .concat(),
    )
}

fn trak(id: u32, handler: &[u8; 4], timescale: u32, extra: Vec<u8>, stbl: Vec<u8>) -> Vec<u8> {
    let tkhd = full(b"tkhd", 0, &u32s(&[0, 0, id, 0, 0]));
    let mdhd = full(
        b"mdhd",
        0,
        &[&u32s(&[0, 0, timescale, 0])[..], &[0u8; 4]].concat(),
    );
    let hdlr = full(b"hdlr", 0, &[&[0u8; 4][..], handler, &[0u8; 13]].concat());
    let minf = bx(b"minf", &stbl);
    let mdia = bx(b"mdia", &[mdhd, hdlr, minf].concat());
    bx(b"trak", &[tkhd, extra, mdia].concat())
}

/// Audio sample payloads: three packets of distinct bytes.
const AUDIO: [&[u8]; 3] = [b"first-packet", b"second-pkt", b"third"];
/// Chapter samples of the text track: length-prefixed UTF-8.
const TITLES: [&str; 2] = ["Intro", "Second"];

struct Spec {
    chap_track: bool,
    chpl: Option<Vec<(u64, &'static str)>>,
    edts: Option<Vec<u8>>,
    /// Audio stts (count, delta).
    stts: Vec<(u32, u32)>,
    /// Boxes appended to moov after the tracks.
    moov_tail: Vec<u8>,
}

impl Default for Spec {
    fn default() -> Self {
        Self {
            chap_track: true,
            chpl: None,
            edts: None,
            stts: vec![(2, 4096), (1, 1000)],
            moov_tail: Vec::new(),
        }
    }
}

/// ftyp, mdat (audio packets then chapter samples), moov.
fn file(spec: &Spec) -> Vec<u8> {
    let ftyp = bx(b"ftyp", b"M4B \0\0\0\0M4B isom");
    let mut payload = Vec::new();
    let data_start = (ftyp.len() + 8) as u32;
    let mut audio_off = Vec::new();
    for a in AUDIO {
        audio_off.push(data_start + payload.len() as u32);
        payload.extend_from_slice(a);
    }
    let mut text_off = Vec::new();
    let mut text_sizes = Vec::new();
    for t in TITLES {
        text_off.push(data_start + payload.len() as u32);
        let mut s = (t.len() as u16).to_be_bytes().to_vec();
        s.extend_from_slice(t.as_bytes());
        text_sizes.push(s.len() as u32);
        payload.extend(s);
    }
    let mdat = bx(b"mdat", &payload);

    let audio_table = Table {
        stts: spec.stts.clone(),
        sizes: AUDIO.iter().map(|a| a.len() as u32).collect(),
        // Two chunks: the first holds two samples, the second one.
        per_chunk: vec![2, 1],
        offsets: vec![audio_off[0], audio_off[2]],
    };
    let mut extra = Vec::new();
    if spec.chap_track {
        extra.extend(bx(b"tref", &bx(b"chap", &u32s(&[2]))));
    }
    if let Some(e) = &spec.edts {
        extra.extend(bx(b"edts", e));
    }
    let audio = trak(
        1,
        b"soun",
        RATE,
        extra,
        stbl(alac_entry(RATE, 2), &audio_table),
    );
    let text_table = Table {
        stts: vec![(1, 200), (1, 100)],
        sizes: text_sizes,
        per_chunk: vec![1, 1],
        offsets: text_off,
    };
    let text = trak(
        2,
        b"text",
        1000,
        Vec::new(),
        stbl(bx(b"text", &[0u8; 8]), &text_table),
    );
    let mvhd = full(b"mvhd", 0, &u32s(&[0, 0, 1000, 200]));
    let mut moov_body = [mvhd, audio, text].concat();
    if let Some(ch) = &spec.chpl {
        let mut b = vec![0u8; 4];
        b.push(ch.len() as u8);
        for (start, title) in ch {
            b.extend_from_slice(&start.to_be_bytes());
            b.push(title.len() as u8);
            b.extend_from_slice(title.as_bytes());
        }
        moov_body.extend(bx(b"udta", &full(b"chpl", 0x0100_0000, &b)));
    }
    moov_body.extend_from_slice(&spec.moov_tail);
    let moov = bx(b"moov", &moov_body);
    [ftyp, mdat, moov].concat()
}

fn open(data: Vec<u8>) -> vynamp_decode_mp4::Mp4Demuxer {
    open_mp4(Box::new(VecSource::new(data)), &Options::default()).expect("open")
}

fn open_err(data: Vec<u8>, opts: &Options) -> Error {
    match open_mp4(Box::new(VecSource::new(data)), opts) {
        Ok(_) => panic!("open succeeded"),
        Err(e) => e,
    }
}

fn packets(d: &mut dyn Demuxer) -> Vec<(u64, u32, Vec<u8>)> {
    let mut out = Vec::new();
    let mut p = Packet::default();
    while d.next_packet(&mut p).expect("packet") {
        out.push((p.pts, p.frames, p.data.clone()));
    }
    out
}

#[test]
fn probe_needs_an_ftyp_box() {
    assert_eq!(probe_mp4(&file(&Spec::default())), 100);
    assert_eq!(probe_mp4(b"\0\0\0\x08free\0\0\0\x08ftyp"), 0);
    assert_eq!(probe_mp4(b"RIFF\0\0\0\0WAVE"), 0);
    assert_eq!(probe_mp4(b"\0\0"), 0);
}

#[test]
fn the_audio_track_is_described_from_the_sample_entry_and_tables() {
    let d = open(file(&Spec::default()));
    let t = &d.tracks()[0];
    assert_eq!(
        d.tracks().len(),
        1,
        "the chapter text track is not an audio track"
    );
    assert_eq!(t.codec, CodecId::Alac);
    assert_eq!((t.sample_rate, t.channels), (RATE, 2));
    assert_eq!(t.frames, Some(2 * 4096 + 1000));
    assert_eq!((t.delay, t.padding), (0, 0));
    assert_eq!(t.gapless, GaplessKind::Exact);
    assert_eq!(t.codec_private, alac_config(RATE, 2));
}

#[test]
fn packets_follow_the_sample_tables_across_chunks() {
    let mut d = open(file(&Spec::default()));
    let got = packets(&mut d);
    let want: Vec<(u64, u32, Vec<u8>)> = vec![
        (0, 4096, AUDIO[0].to_vec()),
        (4096, 4096, AUDIO[1].to_vec()),
        (8192, 1000, AUDIO[2].to_vec()),
    ];
    assert_eq!(got, want);
}

#[test]
fn seek_lands_on_the_packet_holding_the_frame() {
    let mut d = open(file(&Spec::default()));
    let id = d.tracks()[0].id;
    assert_eq!(
        d.seek(id, 5000).unwrap(),
        SeekTo {
            frame: 4096,
            preroll: 904
        }
    );
    assert_eq!(
        packets(&mut d),
        vec![
            (4096, 4096, AUDIO[1].to_vec()),
            (8192, 1000, AUDIO[2].to_vec())
        ]
    );
    assert_eq!(
        d.seek(id, 8192).unwrap(),
        SeekTo {
            frame: 8192,
            preroll: 0
        }
    );
    assert_eq!(packets(&mut d), vec![(8192, 1000, AUDIO[2].to_vec())]);
    // Past the end: lands at the end, nothing left.
    assert_eq!(
        d.seek(id, 1 << 40).unwrap(),
        SeekTo {
            frame: 9192,
            preroll: 0
        }
    );
    assert!(packets(&mut d).is_empty());
    // And back to the start.
    d.seek(id, 0).unwrap();
    assert_eq!(packets(&mut d).len(), 3);
}

#[test]
fn chapters_come_from_the_text_track_named_by_tref_chap() {
    let d = open(file(&Spec::default()));
    let want = vec![
        Chapter {
            title: "Intro".into(),
            start_frame: 0,
        },
        Chapter {
            title: "Second".into(),
            start_frame: 200 * u64::from(RATE) / 1000,
        },
    ];
    assert_eq!(d.chapters(), want.as_slice());
}

#[test]
fn chpl_takes_precedence_over_the_text_track() {
    // 150 ms in units of 100 ns.
    let spec = Spec {
        chpl: Some(vec![(1_500_000, "Nero")]),
        ..Spec::default()
    };
    let d = open(file(&spec));
    assert_eq!(
        d.chapters(),
        &[Chapter {
            title: "Nero".into(),
            start_frame: 7200,
        }]
    );
}

#[test]
fn no_chapters_without_tref_or_chpl() {
    let spec = Spec {
        chap_track: false,
        ..Spec::default()
    };
    assert!(open(file(&spec)).chapters().is_empty());
}

#[test]
fn a_non_identity_edit_list_makes_gapless_unknown() {
    // One entry starting at media time 2112: the container trims a priming delay that this
    // demuxer does not apply, so it must not claim the length is exact.
    let elst = full(b"elst", 0, &u32s(&[1, 150, 2112, 0x0001_0000]));
    let spec = Spec {
        edts: Some(elst),
        ..Spec::default()
    };
    assert_eq!(open(file(&spec)).tracks()[0].gapless, GaplessKind::Unknown);
    // A single entry from media time 0 covering the media (9192 frames = 191.5 ms -> 192
    // ticks of the 1 kHz movie timescale) is the identity.
    let elst = full(b"elst", 0, &u32s(&[1, 192, 0, 0x0001_0000]));
    let spec = Spec {
        edts: Some(elst),
        ..Spec::default()
    };
    assert_eq!(open(file(&spec)).tracks()[0].gapless, GaplessKind::Exact);
}

#[test]
fn truncated_moov_is_a_typed_error() {
    let f = file(&Spec::default());
    for cut in [f.len() - 1, f.len() - 40, f.len() - 200] {
        let e = open_err(f[..cut].to_vec(), &Options::default());
        assert!(matches!(e, Error::Invalid { .. }), "cut {cut}: {e:?}");
    }
}

#[test]
fn a_box_smaller_than_its_header_is_invalid() {
    let mut f = file(&Spec::default());
    // The first box inside moov is mvhd; give it a size of 4.
    let moov = f.windows(4).position(|w| w == b"moov").unwrap() - 4;
    f[moov + 8..moov + 12].copy_from_slice(&4u32.to_be_bytes());
    let e = open_err(f, &Options::default());
    assert!(
        matches!(
            e,
            Error::Invalid {
                what: "mp4 box size",
                ..
            }
        ),
        "{e:?}"
    );
}

#[test]
fn moov_larger_than_the_metadata_limit_is_a_limit_error() {
    let opts = Options {
        limits: Limits {
            max_metadata_bytes: 64,
            ..Limits::default()
        },
        ..Options::default()
    };
    assert_eq!(
        open_err(file(&Spec::default()), &opts),
        Error::Limit(LimitKind::Metadata)
    );
}

#[test]
fn a_length_beyond_max_frames_is_a_limit_error() {
    let opts = Options {
        limits: Limits {
            max_frames: 9000,
            ..Limits::default()
        },
        ..Options::default()
    };
    assert_eq!(
        open_err(file(&Spec::default()), &opts),
        Error::Limit(LimitKind::Frames)
    );
}

#[test]
fn tables_that_disagree_on_the_sample_count_are_invalid() {
    let spec = Spec {
        stts: vec![(2, 4096)],
        ..Spec::default()
    };
    let e = open_err(file(&spec), &Options::default());
    assert!(matches!(e, Error::Invalid { .. }), "{e:?}");
}

#[test]
fn a_file_without_moov_is_invalid() {
    let f = file(&Spec::default());
    let moov = f.windows(4).position(|w| w == b"moov").unwrap() - 4;
    let e = open_err(f[..moov].to_vec(), &Options::default());
    assert!(
        matches!(
            e,
            Error::Invalid {
                what: "mp4 moov missing",
                ..
            }
        ),
        "{e:?}"
    );
}

#[test]
fn a_non_alac_sample_entry_is_unsupported() {
    let mut f = file(&Spec::default());
    // The audio sample entry is the first `alac` fourcc after stsd.
    let stsd = f.windows(4).position(|w| w == b"stsd").unwrap();
    let entry = stsd + f[stsd..].windows(4).position(|w| w == b"alac").unwrap();
    f[entry..entry + 4].copy_from_slice(b"mp4a");
    assert_eq!(
        open_err(f, &Options::default()),
        Error::Unsupported("mp4 codec")
    );
}

#[test]
fn invalid_utf8_in_a_chapter_title_is_strict_invalid_and_resilient_lossy() {
    let mut f = file(&Spec::default());
    let at = f.windows(5).position(|w| w == b"Intro").unwrap();
    f[at] = 0xFF;
    let e = open_err(f.clone(), &Options::default());
    assert!(
        matches!(
            e,
            Error::Invalid {
                what: "mp4 chapter title",
                ..
            }
        ),
        "{e:?}"
    );
    let opts = Options {
        mode: Mode::Resilient,
        ..Options::default()
    };
    let d = open_mp4(Box::new(VecSource::new(f)), &opts).expect("resilient open");
    assert_eq!(d.chapters()[0].title, "\u{FFFD}ntro");
}

#[test]
fn a_packet_beyond_the_end_of_the_file_is_invalid() {
    let mut f = file(&Spec::default());
    // Grow the last stsz entry past the end of the file.
    let stsz = f.windows(4).position(|w| w == b"stsz").unwrap();
    let last = stsz + 4 + 4 + 8 + 2 * 4;
    f[last..last + 4].copy_from_slice(&0x0100_0000u32.to_be_bytes());
    let mut d = open(f);
    let mut p = Packet::default();
    assert!(d.next_packet(&mut p).unwrap());
    assert!(d.next_packet(&mut p).unwrap());
    let e = d.next_packet(&mut p).unwrap_err();
    assert!(matches!(e, Error::Invalid { .. }), "{e:?}");
}

/// A file holding only ftyp and a moov with one ALAC track described by `t`; no media.
fn audio_only(t: &Table) -> Vec<u8> {
    let ftyp = bx(b"ftyp", b"M4A \0\0\0\0M4A isom");
    let audio = trak(1, b"soun", RATE, Vec::new(), stbl(alac_entry(RATE, 2), t));
    let mvhd = full(b"mvhd", 0, &u32s(&[0, 0, 1000, 0]));
    [ftyp, bx(b"moov", &[mvhd, audio].concat())].concat()
}

/// A source that does not know its length, like a pipe.
struct NoLen(VecSource);

impl Source for NoLen {
    fn read(&mut self, buf: &mut [u8]) -> vynamp_decode_core::Result<usize> {
        self.0.read(buf)
    }
    fn seek(&mut self, pos: u64) -> vynamp_decode_core::Result<()> {
        self.0.seek(pos)
    }
    fn position(&self) -> u64 {
        self.0.position()
    }
    fn len(&self) -> Option<u64> {
        None
    }
}

#[test]
fn a_fixed_size_stsz_count_is_bounded_by_the_limits_without_a_source_length() {
    // A fixed-size stsz declares its count with no entries behind it. Without a source
    // length nothing else bounds it, so a forged count must hit a limit before the sample
    // table is allocated: an explicit table that large could not fit in `max_metadata_bytes`.
    let mut f = audio_only(&Table {
        stts: vec![(1, 4096)],
        sizes: Vec::new(),
        per_chunk: vec![1],
        offsets: vec![0],
    });
    let stsz = f.windows(4).position(|w| w == b"stsz").unwrap();
    f[stsz + 8..stsz + 12].copy_from_slice(&1u32.to_be_bytes());
    let count = Limits::default().max_metadata_bytes / 4 + 1;
    f[stsz + 12..stsz + 16].copy_from_slice(&count.to_be_bytes());
    let e = match open_mp4(Box::new(NoLen(VecSource::new(f))), &Options::default()) {
        Ok(_) => panic!("open succeeded"),
        Err(e) => e,
    };
    assert_eq!(e, Error::Limit(LimitKind::Metadata));
}

#[test]
fn a_large_stsc_opens_in_linear_time() {
    // One stsc entry per chunk: a per-chunk scan of stsc is quadratic and takes tens of
    // seconds here; a cursor over the sorted entries takes milliseconds. The bound is loose
    // on purpose (debug build, loaded CI machine).
    const N: usize = 200_000;
    let f = audio_only(&Table {
        stts: vec![(N as u32, 4096)],
        sizes: vec![1; N],
        per_chunk: vec![1; N],
        offsets: vec![0; N],
    });
    let start = std::time::Instant::now();
    let d = open(f);
    let took = start.elapsed();
    assert_eq!(d.tracks()[0].frames, Some(N as u64 * 4096));
    assert!(
        took < std::time::Duration::from_secs(5),
        "open took {took:?}"
    );
}

#[test]
fn an_stsc_out_of_order_is_invalid() {
    let mut f = file(&Spec::default());
    // The audio stsc has entries (1, 2, 1) and (2, 1, 1); make the second start at chunk 1.
    let stsc = f.windows(4).position(|w| w == b"stsc").unwrap();
    let second_first = stsc + 4 + 8 + 12;
    f[second_first..second_first + 4].copy_from_slice(&1u32.to_be_bytes());
    let e = open_err(f, &Options::default());
    assert!(
        matches!(
            e,
            Error::Invalid {
                what: "mp4 stsc order",
                ..
            }
        ),
        "{e:?}"
    );
}

#[test]
fn chapters_are_returned_in_start_order() {
    // chpl lists its entries in file order, which a writer need not keep sorted.
    let spec = Spec {
        chpl: Some(vec![(3_000_000, "Second"), (1_500_000, "First")]),
        ..Spec::default()
    };
    let d = open(file(&spec));
    let got: Vec<(&str, u64)> = d
        .chapters()
        .iter()
        .map(|c| (c.title.as_str(), c.start_frame))
        .collect();
    assert_eq!(got, vec![("First", 7200), ("Second", 14_400)]);
}

#[test]
fn a_track_without_tkhd_that_is_not_used_does_not_fail_the_open() {
    // A trak with no tkhd and no media is neither the audio track nor the chapter track.
    let spec = Spec {
        moov_tail: bx(b"trak", &bx(b"free", &[])),
        ..Spec::default()
    };
    let d = open(file(&spec));
    assert_eq!(d.chapters().len(), 2);
}

#[test]
fn a_sample_larger_than_a_source_without_length_fails_without_allocating_its_size() {
    // A fixed-size stsz of one 4 GiB sample. Without a source length nothing bounds the
    // size up front, so the packet must be read in pieces and fail at the real end of the
    // data instead of first allocating what the table declares.
    let mut f = audio_only(&Table {
        stts: vec![(1, 4096)],
        sizes: Vec::new(),
        per_chunk: vec![1],
        offsets: vec![0],
    });
    let stsz = f.windows(4).position(|w| w == b"stsz").unwrap();
    f[stsz + 8..stsz + 12].copy_from_slice(&u32::MAX.to_be_bytes());
    f[stsz + 12..stsz + 16].copy_from_slice(&1u32.to_be_bytes());
    let mut d = open_mp4(Box::new(NoLen(VecSource::new(f))), &Options::default())
        .expect("open without length");
    let mut p = Packet::default();
    let e = d.next_packet(&mut p).unwrap_err();
    assert!(
        matches!(
            e,
            Error::Invalid {
                what: "unexpected eof",
                ..
            }
        ),
        "{e:?}"
    );
    assert!(
        p.data.capacity() <= 2 << 20,
        "allocated {} bytes for a file of a few hundred",
        p.data.capacity()
    );
}
