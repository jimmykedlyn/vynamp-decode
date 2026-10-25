# Performance

The numbers below are **informative** (spec §9.4), not a gate: no CI check
depends on them, and they come from one development machine.

## Running the benchmark

```text
cargo run --release --example bench_decode -- <file> [iterations]
```

The example (`crates/vynamp-decode/examples/bench_decode.rs`) uses only
`std::time::Instant`. It runs one warm-up iteration, then `iterations` timed
ones (default 5). Each iteration opens the file with `open_path` and decodes
it to the end, so file I/O is included. It prints the min and median time and,
for each, throughput in MB/s of input bytes (1 MB = 10^6 bytes) and speed as a
multiple of realtime (decoded frames / sample rate / elapsed).

## Baseline

Machine: Intel Core i5-8350U @ 1.70 GHz, Windows 11 Pro 10.0.26300,
rustc 1.99.0 (b940084d7 2026-09-28), release profile, 200 iterations each.

| File | Input | Format | Min | Median |
|------|------:|--------|-----|--------|
| `tests/fixtures/flac/st16-44k1.flac` | 11,462 B | FLAC 16-bit stereo 44.1 kHz, 4,410 frames | 24.6 MB/s, 215x | 23.6 MB/s, 206x |
| `tests/fixtures/flac/st24-96k.flac` | 32,090 B | FLAC 24-bit stereo 96 kHz, 9,600 frames | 32.8 MB/s, 102x | 31.4 MB/s, 98x |
| `tests/fixtures/flac-conformance/subset/61 - predictor overflow check, 16-bit.flac` | 117,020 B | FLAC 16-bit 44.1 kHz, 227,247 frames | 23.2 MB/s, 1021x | 22.4 MB/s, 988x |
| `tests/fixtures/flac/ref/st16-44k1.wav` | 17,684 B | WAV PCM 16-bit stereo 44.1 kHz, 4,410 frames | 192.2 MB/s, 1087x | 150.4 MB/s, 850x |

Caveats:

- The oracle fixtures are 0.1 s long, so each iteration takes about a
  millisecond or less. Per-file open cost (file open, header parsing,
  allocation) is a large part of that, which understates steady-state
  throughput and makes the short files read lower than the longer one.
- The conformance file is a synthetic stress stream for predictor overflow,
  not typical music.
- Results vary with CPU frequency scaling and background load.

## MPEG audio

The oracle fixtures are too short to time, so the benchmark files are generated when
the benchmark is run and are **not committed** (they would exceed the fixture budget). Each is
30 s of the synthetic signal of the oracle corpus (tones, a gliding tone, noise, short bursts),
encoded by FFmpeg n8.1.3-23-g5c4f5744cd (libmp3lame and the native `mp2` encoder) into a scratch
directory outside the repository:

```text
S="aevalsrc=<rich0>|<rich1>:s=44100:d=30"   # the 'rich' expressions of tools/fixtures/generate.ps1
ffmpeg -f lavfi -i "$S" -c:a libmp3lame -b:a 128k cbr128-44k-30s.mp3
ffmpeg -f lavfi -i "$S" -c:a libmp3lame -q:a 2    vbr-v2-44k-30s.mp3
ffmpeg -f lavfi -i "$S" -c:a libmp3lame -b:a 320k cbr320-44k-30s.mp3
ffmpeg -f lavfi -i "$S" -c:a mp2 -b:a 192k        mp2-192k-44k-30s.mp2
ffmpeg -f lavfi -i "aevalsrc=<rich0>:s=22050:d=30" -c:a libmp3lame -b:a 48k mono-22k-48k-30s.mp3
```

Same machine and toolchain as above (Intel Core i5-8350U @ 1.70 GHz, Windows 11 Pro 10.0.26300,
rustc 1.99.0 (b940084d7 2026-09-28), release profile), 20 iterations each, single thread:

| File | Input | Format | Min | Median |
|------|------:|--------|-----|--------|
| `cbr128-44k-30s.mp3` | 481,115 B | Layer III CBR 128 kbit/s stereo 44.1 kHz, 1,323,000 frames | 3.8 MB/s, 234x | 3.1 MB/s, 196x |
| `vbr-v2-44k-30s.mp3` | 1,068,412 B | Layer III VBR -V2 stereo 44.1 kHz | 7.6 MB/s, 213x | 5.8 MB/s, 163x |
| `cbr320-44k-30s.mp3` | 1,202,721 B | Layer III CBR 320 kbit/s stereo 44.1 kHz | 8.0 MB/s, 199x | 6.3 MB/s, 158x |
| `mono-22k-48k-30s.mp3` | 180,628 B | Layer III MPEG-2 mono 22.05 kHz 48 kbit/s | 5.3 MB/s, 885x | 4.4 MB/s, 733x |
| `mp2-192k-44k-30s.mp2` | 720,352 B | Layer II 192 kbit/s stereo 44.1 kHz | 8.9 MB/s, 371x | 5.6 MB/s, 233x |

MP3 decoding of 44.1 kHz stereo therefore runs at roughly 160x to 235x realtime. The gap between
min and median (about 20 to 40 percent) is CPU frequency scaling and background load on a
laptop; the file I/O is included.

## Future work

A comparison against Symphonia on the same files would put these numbers in
context. It is not done here because the workspace takes no third-party
dependencies; it would have to live in a separate, out-of-workspace harness.
