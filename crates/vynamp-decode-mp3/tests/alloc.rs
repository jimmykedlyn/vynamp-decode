//! No allocation per packet once the decoder and the output buffer are warm.

mod common;

use common::{Rng, fixture, random_l1, split_frames, track};
use vynamp_decode_core::{AudioBuffer, CodecId, Decoder, Options, Packet};
use vynamp_decode_mp3::MpegDecoder;
use vynamp_decode_testkit::alloc::{CountingAlloc, allocations_on_this_thread};

#[global_allocator]
static A: CountingAlloc = CountingAlloc;

#[test]
fn decoding_does_not_allocate() {
    let mut rng = Rng(5);
    for (mode, channels) in [(3u8, 1u16), (0, 2), (1, 2)] {
        let mut d =
            MpegDecoder::new(&track(CodecId::Mp1, 32000, channels), &Options::default()).unwrap();
        let packets: Vec<Packet> = (0..20)
            .map(|i| Packet {
                pts: i * 384,
                frames: 384,
                data: random_l1(&mut rng, mode, 1, i % 2 == 0).build(),
                ..Packet::default()
            })
            .collect();
        let mut out = AudioBuffer::new();
        d.decode(&packets[0], &mut out).unwrap();
        let before = allocations_on_this_thread();
        for p in &packets {
            d.decode(p, &mut out).unwrap();
        }
        assert_eq!(allocations_on_this_thread(), before, "mode {mode}");
    }
}

#[test]
fn layer_two_decoding_does_not_allocate() {
    for (name, rate, channels) in [
        ("l2-stereo-48k-192k", 48000, 2u16),
        ("l2-joint-44k-128k", 44100, 2),
        ("l2-mono-48k-64k", 48000, 1),
        ("l2-lsf-24k-64k", 24000, 2),
    ] {
        let data = fixture(&format!("{name}.mp2"));
        let packets: Vec<Packet> = split_frames(&data)
            .into_iter()
            .map(|(offset, f)| Packet {
                frames: 1152,
                offset,
                data: f.to_vec(),
                ..Packet::default()
            })
            .collect();
        let mut d =
            MpegDecoder::new(&track(CodecId::Mp2, rate, channels), &Options::default()).unwrap();
        let mut out = AudioBuffer::new();
        d.decode(&packets[0], &mut out).unwrap();
        let before = allocations_on_this_thread();
        for p in &packets {
            d.decode(p, &mut out).unwrap();
        }
        assert_eq!(allocations_on_this_thread(), before, "{name}");
    }
}
