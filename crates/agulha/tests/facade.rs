use agulha::{Reader, default_registry, open};
use agulha_core::{
    AudioBuffer, CodecId, ContainerDescriptor, Demuxer, Endian, Error, Options, Packet,
    PcmEncoding, PcmSpec, Registry, Result, SeekTo, SliceSource, Source, Track, VecSource,
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
        Synth {
            tracks: vec![Track {
                id: 0,
                codec: CodecId::Pcm(SPEC16),
                sample_rate: 8000,
                channels: 1,
                channel_mask: 0,
                frames: Some(TOTAL),
                delay: 100,
                padding: 50,
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

static SYNTH: ContainerDescriptor = ContainerDescriptor {
    name: "synth",
    extensions: &["syn"],
    probe: |h| if h.starts_with(b"SYN") { 50 } else { 0 },
    open: synth_open,
};

fn synth_registry() -> Registry {
    let mut reg = Registry::new();
    reg.add_container(&SYNTH);
    for c in default_registry().codecs() {
        reg.add_codec(c);
    }
    reg
}

fn synth_reader(opts: Options) -> Reader {
    Reader::open_with(
        &synth_registry(),
        Box::new(SliceSource::new(b"SYNxxxxxxxx")),
        opts,
        None,
    )
    .unwrap()
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
    assert_eq!(reg.probe(&mut src, Some("bbb")).unwrap().name, "b");
    assert_eq!(reg.probe(&mut src, Some("bbx")).unwrap().name, "b");
    assert_eq!(src.position(), 0);
    assert_eq!(reg.probe(&mut src, Some("aaa")).unwrap().name, "a");
    // No matching hint: first registered among the best.
    assert_eq!(reg.probe(&mut src, Some("zzz")).unwrap().name, "a");
    assert_eq!(reg.probe(&mut src, None).unwrap().name, "a");
    // Hint of a lower-scoring descriptor does not override a higher score.
    assert_eq!(reg.probe(&mut src, Some("low")).unwrap().name, "a");
}

#[test]
fn probe_nothing_matches() {
    let mut reg = Registry::new();
    reg.add_container(&NONE);
    let mut src = VecSource::new(vec![0; 10]);
    assert_eq!(
        reg.probe(&mut src, None).err(),
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
