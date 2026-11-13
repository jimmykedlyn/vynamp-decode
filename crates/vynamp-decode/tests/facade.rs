use vynamp_decode::{Reader, default_registry, open};
use vynamp_decode_core::{
    CodecId, ContainerDescriptor, Demuxer, Endian, Error, Limits, Options, PcmEncoding, PcmSpec,
    Registry, Result, Source, VecSource, Verification,
};

const SPEC16: PcmSpec = PcmSpec {
    encoding: PcmEncoding::Signed,
    bits: 16,
    valid_bits: 16,
    endian: Endian::Little,
};

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

/// Tests that decode through the PCM codec.
#[cfg(feature = "pcm")]
mod pcm_decoding {
    use super::*;
    use vynamp_decode_core::{
        AudioBuffer, CodecDescriptor, Decoder, Event, GaplessKind, Packet, SampleFormat, SeekTo,
        SliceSource, Track,
    };
    #[cfg(feature = "aiff")]
    use vynamp_decode_testkit::writers::aiff;
    #[cfg(feature = "riff")]
    use vynamp_decode_testkit::writers::{WavVariant, wav};

    #[cfg(any(feature = "riff", feature = "aiff"))]
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

    #[cfg(feature = "riff")]
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

    #[cfg(feature = "riff")]
    #[test]
    fn wav_seek_past_the_end_lands_at_the_declared_length() {
        let bytes = vec![0u8; 2 * 10_000];
        let file = wav(SPEC16, 44100, 1, &bytes, WavVariant::Riff);
        let mut r = open(Box::new(VecSource::new(file)), Options::default()).unwrap();
        let len = r.frames().unwrap();
        let mut buf = AudioBuffer::new();
        for t in [len + 1000, u64::MAX, len] {
            r.seek(t).unwrap();
            assert_eq!(r.position(), len, "seek {t}");
            assert!(!r.read(&mut buf).unwrap(), "seek {t}");
        }
    }

    #[cfg(feature = "aiff")]
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
                    gapless: GaplessKind::Exact,
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
        verification: Verification::Experimental,
        probe: |h| if h.starts_with(b"SYN") { 50 } else { 0 },
        open: synth_open,
    };

    static SYNTH_SHORT: ContainerDescriptor = ContainerDescriptor {
        name: "synth-short",
        extensions: &[],
        verification: Verification::Experimental,
        probe: |h| if h.starts_with(b"SHO") { 50 } else { 0 },
        open: synth_open_short,
    };

    static SYNTH_UNKNOWN: ContainerDescriptor = ContainerDescriptor {
        name: "synth-unknown",
        extensions: &[],
        verification: Verification::Experimental,
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

    // ---- events and pts resync ----

    #[cfg(feature = "aiff")]
    #[test]
    fn resilient_aiff_clamp_is_reported_once() {
        use vynamp_decode_core::Mode;
        let s = samples(11, 2);
        let bytes: Vec<u8> = s.iter().flat_map(|v| v.to_be_bytes()).collect();
        let spec = PcmSpec {
            endian: Endian::Big,
            ..SPEC16
        };
        let mut file = aiff(spec, 48000, 2, &bytes[..40], None);
        // COMM frame count lives at offset 22.
        file[22..26].copy_from_slice(&11u32.to_be_bytes());

        let resilient = Options {
            mode: Mode::Resilient,
            ..Options::default()
        };
        let mut r = open(Box::new(VecSource::new(file.clone())), resilient).unwrap();
        assert_eq!(drain_i16(&mut r, 2)[0].len(), 10);
        assert_eq!(
            r.take_events(),
            [Event::Clamped {
                what: "comm frames",
                declared: 11,
                actual: 10,
            }]
        );
        assert!(r.take_events().is_empty());

        let e = open(Box::new(VecSource::new(file)), Options::default())
            .err()
            .unwrap();
        assert!(matches!(
            e,
            Error::Invalid {
                what: "comm frames exceed ssnd",
                ..
            }
        ));
    }

    /// Stands in for a decoder that produces fewer frames than `packet.frames`
    /// for the packet at pts 64.
    struct ShortDecoder;

    impl Decoder for ShortDecoder {
        fn decode(&mut self, p: &Packet, out: &mut AudioBuffer) -> Result<()> {
            let n = if p.pts == 64 { 40 } else { p.frames as usize };
            out.reset(SampleFormat::I16, 1, n)?;
            let plane = out.plane_i16_mut(0).unwrap();
            for (i, v) in plane.iter_mut().enumerate() {
                *v = (p.pts + i as u64) as i16;
            }
            Ok(())
        }
        fn reset(&mut self) {}
    }

    const GAP_SPEC: PcmSpec = PcmSpec {
        valid_bits: 15,
        ..SPEC16
    };

    fn gap_open(_: Box<dyn Source>, _: &Options) -> Result<Box<dyn Demuxer>> {
        let mut s = Synth::with(Some(TOTAL), 200, 0);
        s.tracks[0].codec = CodecId::Pcm(GAP_SPEC);
        Ok(Box::new(s))
    }

    static GAP: ContainerDescriptor = ContainerDescriptor {
        name: "gap",
        extensions: &[],
        verification: Verification::Experimental,
        probe: |h| if h.starts_with(b"GAP") { 50 } else { 0 },
        open: gap_open,
    };

    static GAP_CODEC: CodecDescriptor = CodecDescriptor {
        name: "gap",
        verification: Verification::Experimental,
        supports: |id| *id == CodecId::Pcm(GAP_SPEC),
        make: |_, _| Ok(Box::new(ShortDecoder)),
    };

    #[test]
    fn short_decoder_output_does_not_shift_later_packets() {
        let mut reg = Registry::new();
        reg.add_container(&GAP);
        reg.add_codec(&GAP_CODEC);
        let mut r = Reader::open_with(
            &reg,
            Box::new(SliceSource::new(b"GAPxxxxxxxx")),
            Options::default(),
            None,
        )
        .unwrap();
        let got = drain_i16(&mut r, 1);
        // Delay 200: the first audible frame is source frame 200. Packet 64 is
        // short (frames 64..104, before the delay) and must not skew the rest.
        let want: Vec<i16> = (200..1000).collect();
        assert_eq!(got[0], want);
    }

    fn gap0_open(_: Box<dyn Source>, _: &Options) -> Result<Box<dyn Demuxer>> {
        let mut s = Synth::with(Some(TOTAL), 0, 0);
        s.tracks[0].codec = CodecId::Pcm(GAP_SPEC);
        Ok(Box::new(s))
    }

    static GAP0: ContainerDescriptor = ContainerDescriptor {
        name: "gap0",
        extensions: &[],
        verification: Verification::Experimental,
        probe: |h| if h.starts_with(b"GP0") { 50 } else { 0 },
        open: gap0_open,
    };

    #[test]
    fn short_decoder_output_in_audible_region_and_after_seek() {
        let mut reg = Registry::new();
        reg.add_container(&GAP0);
        reg.add_codec(&GAP_CODEC);
        let mut r = Reader::open_with(
            &reg,
            Box::new(SliceSource::new(b"GP0xxxxxxxx")),
            Options::default(),
            None,
        )
        .unwrap();
        // Packet 64 yields only 40 frames; later packets stay aligned by pts.
        let want: Vec<i16> = (0..104).chain(128..1000).collect();
        assert_eq!(drain_i16(&mut r, 1)[0], want);
        // Seek onto the short packet (lands at 64, preroll 6).
        r.seek(70).unwrap();
        let want: Vec<i16> = (70..104).chain(128..1000).collect();
        assert_eq!(drain_i16(&mut r, 1)[0], want);
        // Seek past it.
        r.seek(500).unwrap();
        let want: Vec<i16> = (500..1000).collect();
        assert_eq!(drain_i16(&mut r, 1)[0], want);
    }

    /// Queues events, then ends the stream (or fails) on the first packet.
    struct Noisy {
        tracks: Vec<Track>,
        pending: usize,
        fail: bool,
        /// Events the demuxer reports as dropped on its own queue.
        dropped: u64,
    }

    impl Demuxer for Noisy {
        fn tracks(&self) -> &[Track] {
            &self.tracks
        }
        fn next_packet(&mut self, _: &mut Packet) -> Result<bool> {
            self.pending = self.pending.max(1);
            if self.fail {
                Err(Error::Invalid {
                    what: "noisy",
                    offset: 0,
                })
            } else {
                Ok(false)
            }
        }
        fn seek(&mut self, _: u32, frame: u64) -> Result<SeekTo> {
            Ok(SeekTo { frame, preroll: 0 })
        }
        fn drain_events(&mut self, out: &mut Vec<Event>) {
            out.extend((0..self.pending).map(|_| Event::ChecksumMismatch { what: "noisy" }));
            self.pending = 0;
        }
        fn take_events_dropped(&mut self) -> u64 {
            std::mem::take(&mut self.dropped)
        }
    }

    fn noisy(pending: usize, fail: bool) -> Result<Box<dyn Demuxer>> {
        let mut s = Synth::new();
        s.tracks[0].codec = CodecId::Pcm(GAP_SPEC);
        Ok(Box::new(Noisy {
            tracks: s.tracks,
            pending,
            fail,
            dropped: if pending == 7 { 123 } else { 0 },
        }))
    }

    static NOISY_END: ContainerDescriptor = ContainerDescriptor {
        name: "noisy-end",
        extensions: &[],
        verification: Verification::Experimental,
        probe: |h| if h.starts_with(b"NEN") { 50 } else { 0 },
        open: |_, _| noisy(0, false),
    };

    static NOISY_ERR: ContainerDescriptor = ContainerDescriptor {
        name: "noisy-err",
        extensions: &[],
        verification: Verification::Experimental,
        probe: |h| if h.starts_with(b"NER") { 50 } else { 0 },
        open: |_, _| noisy(0, true),
    };

    static NOISY_MANY: ContainerDescriptor = ContainerDescriptor {
        name: "noisy-many",
        extensions: &[],
        verification: Verification::Experimental,
        probe: |h| if h.starts_with(b"NMA") { 50 } else { 0 },
        open: |_, _| noisy(5000, false),
    };

    static NOISY_DROPS: ContainerDescriptor = ContainerDescriptor {
        name: "noisy-drops",
        extensions: &[],
        verification: Verification::Experimental,
        probe: |h| if h.starts_with(b"NDR") { 50 } else { 0 },
        open: |_, _| noisy(7, false),
    };

    fn noisy_reader(magic: &'static [u8]) -> Reader {
        let mut reg = Registry::new();
        reg.add_container(&NOISY_END);
        reg.add_container(&NOISY_ERR);
        reg.add_container(&NOISY_MANY);
        reg.add_container(&NOISY_DROPS);
        reg.add_codec(&GAP_CODEC);
        Reader::open_with(
            &reg,
            Box::new(SliceSource::new(magic)),
            Options::default(),
            None,
        )
        .unwrap()
    }

    #[test]
    fn events_are_drained_at_end_of_stream_and_on_error() {
        let mut buf = AudioBuffer::new();
        let mut r = noisy_reader(b"NENxxxxxxxx");
        assert!(r.take_events().is_empty());
        assert!(!r.read(&mut buf).unwrap());
        assert_eq!(r.take_events().len(), 1);

        let mut r = noisy_reader(b"NERxxxxxxxx");
        assert!(r.read(&mut buf).is_err());
        assert_eq!(r.take_events().len(), 1);
    }

    #[test]
    fn event_queue_is_capped_and_counts_drops() {
        let mut r = noisy_reader(b"NMAxxxxxxxx");
        assert_eq!(r.events_dropped(), 5000 - 4096);
        assert_eq!(r.take_events().len(), 4096);
        assert_eq!(r.events_dropped(), 0);
        assert!(r.take_events().is_empty());
    }

    #[test]
    fn demuxer_queue_drops_are_counted_by_the_reader() {
        let mut r = noisy_reader(b"NDRxxxxxxxx");
        // 7 events fit; the demuxer says it dropped 123 more.
        assert_eq!(r.take_events().len(), 7);
        assert_eq!(r.events_dropped(), 0);
        let mut r = noisy_reader(b"NDRxxxxxxxx");
        assert_eq!(r.events_dropped(), 123);
        assert_eq!(r.take_events().len(), 7);
        assert_eq!(r.events_dropped(), 0);
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
        let src = vynamp_decode_core::CancelSource::new();
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
        let src = vynamp_decode_core::CancelSource::new();
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
}

/// ID3v2 skipping, exercised with a WAV payload (the riff probe is the vehicle).
#[cfg(feature = "riff")]
mod id3 {
    use super::*;
    use vynamp_decode_core::{LimitKind, Limits};
    use vynamp_decode_testkit::writers::{WavVariant, wav};

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
}

// ---- probe ties ----

fn tie_open(_: Box<dyn Source>, _: &Options) -> Result<Box<dyn Demuxer>> {
    Err(Error::Unsupported("tie"))
}
static TIE_A: ContainerDescriptor = ContainerDescriptor {
    name: "a",
    extensions: &["aaa"],
    verification: Verification::Experimental,
    probe: |_| 10,
    open: tie_open,
};
static TIE_B: ContainerDescriptor = ContainerDescriptor {
    name: "b",
    extensions: &["bbb", "BBX"],
    verification: Verification::Experimental,
    probe: |_| 10,
    open: tie_open,
};
static LOW: ContainerDescriptor = ContainerDescriptor {
    name: "low",
    extensions: &["low"],
    verification: Verification::Experimental,
    probe: |_| 1,
    open: tie_open,
};
static NONE: ContainerDescriptor = ContainerDescriptor {
    name: "none",
    extensions: &[],
    verification: Verification::Experimental,
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
    assert_eq!(
        reg.codec_for(&CodecId::Pcm(SPEC16)).is_some(),
        cfg!(feature = "pcm")
    );
    assert_eq!(
        reg.codec_for(&CodecId::Flac).is_some(),
        cfg!(feature = "flac")
    );
    for id in [CodecId::Mp1, CodecId::Mp2, CodecId::Mp3] {
        assert_eq!(
            reg.codec_for(&id).is_some(),
            cfg!(feature = "mp3"),
            "{id:?}"
        );
    }
    assert_eq!(
        reg.containers().count(),
        usize::from(cfg!(feature = "riff"))
            + usize::from(cfg!(feature = "aiff"))
            + usize::from(cfg!(feature = "flac"))
            + usize::from(cfg!(feature = "mpa"))
    );
}

#[test]
fn reader_is_send() {
    fn assert_send<T: Send>() {}
    assert_send::<Reader>();
}

#[test]
fn builtin_formats_have_the_expected_verification_level() {
    // FLAC has official conformance vectors (IETF flac-test-files); PCM has none; Layer I has
    // no real-world fixture and no third-party oracle (FFmpeg has no Layer I encoder).
    let want = |name: &str| match name {
        "flac" => Verification::Conformance,
        "mpeg-audio-layer1" => Verification::Experimental,
        _ => Verification::Oracle,
    };
    let reg = default_registry();
    assert!(reg.containers().all(|c| c.verification == want(c.name)));
    assert!(reg.codecs().all(|c| c.verification == want(c.name)));
}

/// `DECODE-AC-33`: the registry enumerates name, extensions and verification.
///
/// The set has to follow the features, not a hardcoded four: a build without a feature
/// does not enumerate it, and one with a single feature enumerates exactly that. The
/// profile column the `REQ-E01-04` acceptance used to ask for does not exist here by
/// design — see `specs/decode.md` §10.
///
/// Gated on the four container features because the extension guards below name a
/// container each. Without the gate the test passes on the default build and fails on
/// `--no-default-features --features riff`, which is the build the paragraph above claims
/// to cover -- a documented property that no build exercises is a claim, not a test.
#[cfg(all(feature = "riff", feature = "aiff", feature = "flac", feature = "mpa"))]
#[test]
fn registry_enumerates_names_extensions_and_verification() {
    let reg = default_registry();
    let containers: Vec<(&str, &[&str], Verification)> = reg
        .containers()
        .map(|c| (c.name, c.extensions, c.verification))
        .collect();
    let codecs: Vec<(&str, Verification)> =
        reg.codecs().map(|c| (c.name, c.verification)).collect();

    let mut want_containers: Vec<&str> = Vec::new();
    if cfg!(feature = "riff") {
        want_containers.push("riff");
    }
    if cfg!(feature = "aiff") {
        want_containers.push("aiff");
    }
    if cfg!(feature = "flac") {
        want_containers.push("flac");
    }
    if cfg!(feature = "mpa") {
        want_containers.push("mpa");
    }
    let mut want_codecs: Vec<&str> = Vec::new();
    if cfg!(feature = "pcm") {
        want_codecs.push("pcm");
    }
    if cfg!(feature = "flac") {
        want_codecs.push("flac");
    }
    if cfg!(feature = "mp3") {
        want_codecs.push("mpeg-audio");
        want_codecs.push("mpeg-audio-layer1");
    }

    assert_eq!(
        containers.iter().map(|c| c.0).collect::<Vec<_>>(),
        want_containers
    );
    assert_eq!(codecs.iter().map(|c| c.0).collect::<Vec<_>>(), want_codecs);

    // `DECODE-AC-33` names the verification level as part of the enumeration, so the third
    // column has to be asserted here and not only by the test that checks the levels
    // themselves -- otherwise it is data carried through the tuple and never read. The
    // expected value per format is the one `specs/decode.md` §10 justifies, repeated here
    // on purpose: the two tests agree through the spec, not through a shared helper, so a
    // wrong level has to be written twice to pass.
    let want = |name: &str| match name {
        "flac" => Verification::Conformance,
        "mpeg-audio-layer1" => Verification::Experimental,
        _ => Verification::Oracle,
    };
    for (name, _, level) in &containers {
        assert_eq!(*level, want(name), "container {name}");
    }
    for (name, level) in &codecs {
        assert_eq!(*level, want(name), "codec {name}");
    }

    // Extensions are part of what is enumerated, and are advertised without the dot.
    for (name, ext, _) in &containers {
        assert!(!ext.is_empty(), "{name} advertises no extension");
        for e in *ext {
            assert!(
                !e.starts_with('.'),
                "{name} advertises {e} with a leading dot"
            );
            assert_eq!(e.to_lowercase(), *e, "{name} advertises {e} in mixed case");
        }
    }
    // A container's extensions are what the hint matches, so they are the caller's map
    // from a path to a probe tiebreak. Guard the ones the docs promise.
    let ext_of = |n: &str| {
        containers
            .iter()
            .find(|c| c.0 == n)
            .map(|c| c.1)
            .unwrap_or(&[])
    };
    assert!(ext_of("riff").contains(&"wav"));
    assert!(ext_of("flac").contains(&"flac"));
    assert!(ext_of("mpa").contains(&"mp3"));
    assert!(ext_of("aiff").contains(&"aif"));
}

/// `DECODE-AC-01`: a real file on disk, renamed to the wrong extension, still probes by content.
///
/// The three cases are the three formats the acceptance names: FLAC as `.mp3`, MPEG audio
/// as `.wav`, WAV as `.flac`. Each must open in `Mode::Strict`, report the codec its bytes
/// say, and decode to the end — `open_path` must not let the extension decide.
///
/// The files are copied into a temporary directory with `std::env::temp_dir()` because the
/// workspace has no registry packages in its `Cargo.lock` (ADR-002) and adding a crate for
/// this would be the wrong trade. The name is unique per process to keep parallel runs apart,
/// and `TempDir` removes it on drop — including when an assertion below fails, which a plain
/// call at the end of the test would not do.
///
/// Gated on the three features the cases need: the fixtures are FLAC, MPEG audio and PCM,
/// and a build without one of them fails at `open_path` with `unsupported: container`
/// rather than proving anything about the extension.
#[cfg(all(feature = "flac", feature = "mpa", feature = "pcm"))]
#[test]
fn renamed_files_probe_by_content_not_by_extension() {
    /// Fixture path, renamed file name, and the predicate its bytes must satisfy.
    type Case = (&'static str, &'static str, fn(&CodecId) -> bool);

    /// Removes the directory when it goes out of scope, panic or not.
    struct TempDir(std::path::PathBuf);
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
    let dir = TempDir(std::env::temp_dir().join(format!("vynamp-ac01-{}", std::process::id())));
    std::fs::create_dir_all(&dir.0).expect("temp dir");

    let cases: [Case; 3] = [
        ("flac/mono16.flac", "renamed-as-mp3.mp3", |c| {
            matches!(c, CodecId::Flac)
        }),
        // Layer II exactly: the demuxer maps the layer field to the codec, so accepting
        // any MPEG codec here would let a wrong layer pass.
        ("mpeg/l2-A-44k-mono-64k.mp2", "renamed-as-wav.wav", |c| {
            matches!(c, CodecId::Mp2)
        }),
        ("pcm/sine-s16le.wav", "renamed-as-flac.flac", |c| {
            matches!(c, CodecId::Pcm(_))
        }),
    ];

    let mut results = Vec::new();
    for (source, renamed, want) in cases {
        let src = root.join(source);
        let data = std::fs::read(&src).unwrap_or_else(|e| panic!("{}: {e}", src.display()));
        let path = dir.0.join(renamed);
        std::fs::write(&path, &data).expect("write fixture copy");

        let opts = Options {
            mode: vynamp_decode_core::Mode::Strict,
            ..Options::default()
        };
        let mut r = vynamp_decode::open_path(&path, opts)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let codec = r.track().codec.clone();
        assert!(
            want(&codec),
            "{source} renamed to {renamed} opened as {codec:?}, which its bytes do not say"
        );
        let frames = r.frames();
        let mut buf = vynamp_decode_core::AudioBuffer::new();
        let mut total = 0u64;
        while r.read(&mut buf).expect("decode to the end") {
            total += buf.frames() as u64;
            assert!(buf.frames() > 0, "{source}: read returned an empty buffer");
        }
        // Nothing was silently dropped: the trimmed timeline is what came out.
        if let Some(f) = frames {
            assert_eq!(total, f, "{source}: decoded {total} of {f} frames");
        }
        assert!(total > 0, "{source} decoded no frames at all");
        results.push((source, codec, total));
    }

    // The loop pushes one entry per case, so a length check here could never fail. What is
    // worth asserting is that the three cases are the three distinct codecs the acceptance
    // names: a copy-paste that made two cases the same file would still push three.
    let got: Vec<&CodecId> = results.iter().map(|(_, c, _)| c).collect();
    assert!(
        matches!(
            got.as_slice(),
            [CodecId::Flac, CodecId::Mp2, CodecId::Pcm(_)]
        ),
        "the three cases must be three different codecs: {results:?}"
    );
}
