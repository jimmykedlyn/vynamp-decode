//! No allocation per packet once the decoder and the output buffer are warm.

mod common;

use common::{Rng, random_l1, track};
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
