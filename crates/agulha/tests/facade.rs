use agulha::{Reader, default_registry, open};
use agulha_core::{
    AudioBuffer, CodecId, ContainerDescriptor, Demuxer, Endian, Error, LimitKind, Limits, Options,
    Packet, PcmEncoding, PcmSpec, Registry, Result, SeekTo, SliceSource, Source, Track, VecSource,
};
use agulha_testkit::writers::{WavVariant, aiff, wav};

const SPEC16: PcmSpec = PcmSpec {
    encoding: PcmEncoding::Signed,
    bits: 16,
    valid_bits: 16,
    endian: Endian::Little,
};

fn samples(frames: usize, channels: usize) -> Vec<i16> {
    (0..frames * channels)
        .map(|i| (i as i32 * 37 % 60000 - 30000) as i16)
        .collect()
}

fn drain_i16(r: &mut Reader, ch: usize) -> Vec<Vec<i16>> {
    let mut out = vec![Vec::new(); ch];
    let mut buf = AudioBuffer::new();
    while r.read(&mut buf).unwrap() {
        assert!(buf.frames() > 0);
        for (c, o) in out.iter_mut().enumerate() {
            o.extend_from_slice(buf.plane_i16(c).unwrap());
        }
    }
    out
}

#[test]
fn wav_decodes_bit_exact() {
    let s = samples(10_000, 2);
    let bytes: Vec<u8> = s.iter().flat_map(|v| v.to_le_bytes()).collect();
    let file = wav(SPEC16, 44100, 2, &bytes, WavVariant::Riff);
    let mut r = open(Box::new(VecSource::new(file)), Options::default()).unwrap();
    assert_eq!(r.track().frames, Some(10_000));
    let got = drain_i16(&mut r, 2);
    for (c, g) in got.iter().enumerate() {
        let want: Vec<i16> = s.iter().skip(c).step_by(2).copied().collect();
        assert_eq!(*g, want);
    }
}

#[test]
fn aiff_decodes_bit_exact() {
    let s = samples(5_000, 1);
    let bytes: Vec<u8> = s.iter().flat_map(|v| v.to_be_bytes()).collect();
    let spec = PcmSpec {
        endian: Endian::Big,
        ..SPEC16
    };
    let file = aiff(spec, 48000, 1, &bytes, None);
    let mut r = open(Box::new(VecSource::new(file)), Options::default()).unwrap();
    assert_eq!(drain_i16(&mut r, 1)[0], s);
}

#[test]
fn random_bytes_are_unsupported() {
    let junk: Vec<u8> = (0..5000u32)
        .map(|i| (i.wrapping_mul(2654435761) >> 13) as u8)
        .collect();
    let e = open(Box::new(VecSource::new(junk)), Options::default())
        .err()
        .unwrap();
    assert_eq!(e, Error::Unsupported("container"));
}

// ---- synthetic demuxer: delay/padding/preroll ----

const TOTAL: u64 = 1000;
const PK: u64 = 64;

struct Synth {
    tracks: Vec<Track>,
    next: u64,
}

impl Synth {
    fn new() -> Self {
        Self::with(Some(TOTAL), 100, 50)
    }

    /// The demuxer always holds `TOTAL` frames; `frames`, `delay` and
    /// `padding` are what the track declares.
    fn with(frames: Option<u64>, delay: u32, padding: u32) -> Self {
        Synth {
            tracks: vec![Track {
                id: 0,
                codec: CodecId::Pcm(SPEC16),
                sample_rate: 8000,
                channels: 1,
                channel_mask: 0,
                frames,
                delay,
                padding,
                codec_private: Vec::new(),
            }],
            next: 0,
        }
    }
}

impl Demuxer for Synth {
    fn tracks(&self) -> &[Track] {
        &self.tracks
    }
    fn next_packet(&mut self, out: &mut Packet) -> Result<bool> {
        if self.next >= TOTAL {
            return Ok(false);
        }
        let n = PK.min(TOTAL - self.next);
        out.track = 0;
        out.pts = self.next;
        out.frames = n as u32;
        out.data.clear();
        for f in self.next..self.next + n {
            out.data.extend((f as i16).to_le_bytes());
        }
        self.next += n;
        Ok(true)
    }
    fn seek(&mut self, _track: u32, frame: u64) -> Result<SeekTo> {
        // Land on a packet boundary at or before the target, to force preroll.
        let start = frame / PK * PK;
        self.next = start;
        Ok(SeekTo {
            frame: start,
            preroll: frame - start,
        })
    }
}

fn synth_open(_: Box<dyn Source>, _: &Options) -> Result<Box<dyn Demuxer>> {
    Ok(Box::new(Synth::new()))
}

fn synth_open_short(_: Box<dyn Source>, _: &Options) -> Result<Box<dyn Demuxer>> {
    Ok(Box::new(Synth::with(Some(100), 80, 40)))
}

fn synth_open_unknown_len(_: Box<dyn Source>, _: &Options) -> Result<Box<dyn Demuxer>> {
    Ok(Box::new(Synth::with(None, 100, 50)))
}

static SYNTH: ContainerDescriptor = ContainerDescriptor {
    name: "synth",
    extensions: &["syn"],
    probe: |h| if h.starts_with(b"SYN") { 50 } else { 0 },
    open: synth_open,
};

static SYNTH_SHORT: ContainerDescriptor = ContainerDescriptor {
    name: "synth-short",
    extensions: &[],
    probe: |h| if h.starts_with(b"SHO") { 50 } else { 0 },
    open: synth_open_short,
};

static SYNTH_UNKNOWN: ContainerDescriptor = ContainerDescriptor {
    name: "synth-unknown",
    extensions: &[],
    probe: |h| if h.starts_with(b"UNK") { 50 } else { 0 },
    open: synth_open_unknown_len,
};

fn synth_registry() -> Registry {
    let mut reg = Registry::new();
    reg.add_container(&SYNTH);
    reg.add_container(&SYNTH_SHORT);
    reg.add_container(&SYNTH_UNKNOWN);
    for c in default_registry().codecs() {
        reg.add_codec(c);
    }
    reg
}

fn synth_reader_with(magic: &'static [u8], opts: Options) -> Reader {
    Reader::open_with(
        &synth_registry(),
        Box::new(SliceSource::new(magic)),
        opts,
        None,
    )
    .unwrap()
}

fn synth_reader(opts: Options) -> Reader {
    synth_reader_with(b"SYNxxxxxxxx", opts)
}

#[test]
fn gapless_delay_and_padding_span_packets() {
    let mut r = synth_reader(Options::default());
    let got = drain_i16(&mut r, 1);
    assert_eq!(got[0].len() as u64, TOTAL - 150);
    let want: Vec<i16> = (100..950).collect();
    assert_eq!(got[0], want);
}

#[test]
fn seek_lands_on_requested_output_frame() {
    let mut r = synth_reader(Options::default());
    // Output frame 500 == source sample 600 (decoder lands on 448, preroll 152).
    r.seek(500).unwrap();
    let got = drain_i16(&mut r, 1);
    assert_eq!(got[0][0], 600);
    assert_eq!(got[0].len(), 350);
    // Seek to 0 re-applies the delay.
    r.seek(0).unwrap();
    let got = drain_i16(&mut r, 1);
    assert_eq!(got[0][0], 100);
    assert_eq!(got[0].len(), 850);
}

#[test]
fn cancel_is_checked_before_each_packet() {
    let src = agulha_core::CancelSource::new();
    let opts = Options {
        cancel: src.token(),
        ..Options::default()
    };
    let mut r = synth_reader(opts);
    src.cancel();
    let mut buf = AudioBuffer::new();
    assert_eq!(r.read(&mut buf), Err(Error::Cancelled));
}

#[test]
fn read_leaves_no_stale_frames_at_end() {
    let mut r = synth_reader(Options::default());
    let mut buf = AudioBuffer::new();
    let mut total = 0usize;
    loop {
        let more = r.read(&mut buf).unwrap();
        // Consume before checking `more`: a stale buffer would be double counted.
        total += buf.frames();
        if !more {
            break;
        }
    }
    assert_eq!(buf.frames(), 0);
    assert_eq!(total as u64, TOTAL - 150);
    // Repeated reads after the end stay empty.
    assert!(!r.read(&mut buf).unwrap());
    assert_eq!(buf.frames(), 0);
}

#[test]
fn frames_and_position_follow_the_trimmed_timeline() {
    let mut r = synth_reader(Options::default());
    assert_eq!(r.frames(), Some(TOTAL - 150));
    assert_eq!(r.position(), 0);
    let mut buf = AudioBuffer::new();
    assert!(r.read(&mut buf).unwrap());
    assert_eq!(r.position(), buf.frames() as u64);
    r.seek(500).unwrap();
    assert_eq!(r.position(), 500);
    let mut n = 0u64;
    while r.read(&mut buf).unwrap() {
        n += buf.frames() as u64;
        assert_eq!(r.position(), 500 + n);
    }
    assert_eq!(r.position(), r.frames().unwrap());
}

#[test]
fn delay_plus_padding_beyond_frames_is_empty() {
    let mut r = synth_reader_with(b"SHOxxxx", Options::default());
    assert_eq!(r.frames(), Some(0));
    let mut buf = AudioBuffer::new();
    assert!(!r.read(&mut buf).unwrap());
    assert_eq!(buf.frames(), 0);
    assert_eq!(r.position(), 0);
}

#[test]
fn unknown_length_still_trims_delay() {
    let mut r = synth_reader_with(b"UNKxxxx", Options::default());
    assert_eq!(r.frames(), None);
    let got = drain_i16(&mut r, 1);
    assert_eq!(got[0].len() as u64, TOTAL - 100);
    assert_eq!(got[0][0], 100);
    assert_eq!(r.frames(), None);
    assert_eq!(r.position(), TOTAL - 100);
}

#[test]
fn seek_past_end_clamps_position() {
    let mut r = synth_reader(Options::default());
    r.seek(10_000).unwrap();
    assert_eq!(r.position(), TOTAL - 150);
    let mut buf = AudioBuffer::new();
    assert!(!r.read(&mut buf).unwrap());
    assert_eq!(buf.frames(), 0);
    // Seeking back works afterwards.
    r.seek(0).unwrap();
    assert_eq!(r.position(), 0);
    assert!(r.read(&mut buf).unwrap());
}

#[test]
fn cancel_mid_stream() {
    let src = agulha_core::CancelSource::new();
    let opts = Options {
        cancel: src.token(),
        ..Options::default()
    };
    let mut r = synth_reader(opts);
    let mut buf = AudioBuffer::new();
    assert!(r.read(&mut buf).unwrap());
    src.cancel();
    assert_eq!(r.read(&mut buf), Err(Error::Cancelled));
}

// ---- ID3v2 skipping in probe ----

/// An ID3v2 tag header (10 bytes) followed by `size` zero bytes (and a
/// 10-byte footer if `footer`).
fn id3v2(size: usize, footer: bool) -> Vec<u8> {
    let mut t = b"ID3\x04\x00".to_vec();
    t.push(if footer { 0x10 } else { 0 });
    t.extend([
        ((size >> 21) & 0x7F) as u8,
        ((size >> 14) & 0x7F) as u8,
        ((size >> 7) & 0x7F) as u8,
        (size & 0x7F) as u8,
    ]);
    t.resize(10 + size, 0);
    if footer {
        t.extend(b"3DI\x04\x00\x10\x00\x00\x00\x00");
    }
    t
}

fn small_wav() -> Vec<u8> {
    wav(SPEC16, 8000, 1, &[0u8; 64], WavVariant::Riff)
}

fn probe_name(prefix: Vec<u8>, limits: &Limits) -> Result<&'static str> {
    let mut file = prefix;
    file.extend(small_wav());
    let mut src = VecSource::new(file);
    let r = default_registry().probe(&mut src, None, limits);
    assert_eq!(src.position(), 0, "probe must rewind the source");
    r.map(|d| d.name)
}

#[cfg(feature = "riff")]
#[test]
fn probe_skips_large_id3v2_tag() {
    let l = Limits::default();
    assert_eq!(probe_name(id3v2(300_000, false), &l), Ok("riff"));
    assert_eq!(probe_name(id3v2(300_000, true), &l), Ok("riff"));
    // Several tags in a row.
    let mut two = id3v2(5000, false);
    two.extend(id3v2(70_000, true));
    assert_eq!(probe_name(two, &l), Ok("riff"));
    // Empty tag.
    assert_eq!(probe_name(id3v2(0, false), &l), Ok("riff"));
}

#[cfg(feature = "riff")]
#[test]
fn probe_malformed_id3v2_does_not_skip() {
    let l = Limits::default();
    for bad in [[0xFF; 4], [0, 0, 0, 0x80], [0x80, 0, 0, 0]] {
        let mut t = b"ID3\x04\x00\x00".to_vec();
        t.extend(bad);
        t.resize(2000, 0);
        assert_eq!(
            probe_name(t, &l),
            Err(Error::Unsupported("container")),
            "{bad:?}"
        );
    }
    // Bad version byte.
    let mut t = id3v2(100, false);
    t[3] = 0xFF;
    assert_eq!(probe_name(t, &l), Err(Error::Unsupported("container")));
    // Tag declaring more than the file holds.
    let mut t = id3v2(0, false);
    t[9] = 0x7F;
    assert_eq!(probe_name(t, &l), Err(Error::Unsupported("container")));
}

#[cfg(feature = "riff")]
#[test]
fn probe_id3v2_beyond_metadata_limit_is_a_limit_error() {
    let l = Limits {
        max_metadata_bytes: 100_000,
        ..Limits::default()
    };
    assert_eq!(probe_name(id3v2(99_980, false), &l), Ok("riff"));
    assert_eq!(
        probe_name(id3v2(100_000, false), &l),
        Err(Error::Limit(LimitKind::Metadata))
    );
    // Cumulative across several tags, footer included.
    let mut two = id3v2(60_000, true);
    two.extend(id3v2(60_000, false));
    assert_eq!(probe_name(two, &l), Err(Error::Limit(LimitKind::Metadata)));
}

// ---- probe ties ----

fn tie_open(_: Box<dyn Source>, _: &Options) -> Result<Box<dyn Demuxer>> {
    Err(Error::Unsupported("tie"))
}
static TIE_A: ContainerDescriptor = ContainerDescriptor {
    name: "a",
    extensions: &["aaa"],
    probe: |_| 10,
    open: tie_open,
};
static TIE_B: ContainerDescriptor = ContainerDescriptor {
    name: "b",
    extensions: &["bbb", "BBX"],
    probe: |_| 10,
    open: tie_open,
};
static LOW: ContainerDescriptor = ContainerDescriptor {
    name: "low",
    extensions: &["low"],
    probe: |_| 1,
    open: tie_open,
};
static NONE: ContainerDescriptor = ContainerDescriptor {
    name: "none",
    extensions: &[],
    probe: |_| 0,
    open: tie_open,
};

#[test]
fn probe_tie_resolved_by_extension() {
    let mut reg = Registry::new();
    for d in [&LOW, &TIE_A, &TIE_B, &NONE] {
        reg.add_container(d);
    }
    let mut src = VecSource::new(vec![1, 2, 3, 4]);
    assert_eq!(
        reg.probe(&mut src, Some("bbb"), &Limits::default())
            .unwrap()
            .name,
        "b"
    );
    assert_eq!(
        reg.probe(&mut src, Some("bbx"), &Limits::default())
            .unwrap()
            .name,
        "b"
    );
    assert_eq!(src.position(), 0);
    assert_eq!(
        reg.probe(&mut src, Some("aaa"), &Limits::default())
            .unwrap()
            .name,
        "a"
    );
    // No matching hint: first registered among the best.
    assert_eq!(
        reg.probe(&mut src, Some("zzz"), &Limits::default())
            .unwrap()
            .name,
        "a"
    );
    assert_eq!(
        reg.probe(&mut src, None, &Limits::default()).unwrap().name,
        "a"
    );
    // Hint of a lower-scoring descriptor does not override a higher score.
    assert_eq!(
        reg.probe(&mut src, Some("low"), &Limits::default())
            .unwrap()
            .name,
        "a"
    );
}

#[test]
fn probe_nothing_matches() {
    let mut reg = Registry::new();
    reg.add_container(&NONE);
    let mut src = VecSource::new(vec![0; 10]);
    assert_eq!(
        reg.probe(&mut src, None, &Limits::default()).err(),
        Some(Error::Unsupported("container"))
    );
    assert_eq!(src.position(), 0);
}

#[test]
fn codec_lookup() {
    let reg = default_registry();
    assert!(reg.codec_for(&CodecId::Pcm(SPEC16)).is_some());
    assert!(reg.codec_for(&CodecId::Flac).is_none());
    assert_eq!(reg.containers().count(), 2);
}

#[test]
fn reader_is_send() {
    fn assert_send<T: Send>() {}
    assert_send::<Reader>();
}
