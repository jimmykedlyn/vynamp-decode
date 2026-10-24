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

## Future work

A comparison against Symphonia on the same files would put these numbers in
context. It is not done here because the workspace takes no third-party
dependencies; it would have to live in a separate, out-of-workspace harness.
