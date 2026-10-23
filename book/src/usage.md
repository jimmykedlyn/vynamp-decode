# Usage

Decode a file with `open_path` and loop over `Reader::read`:

```rust,no_run
use vynamp_decode::{AudioBuffer, Options, open_path};

fn main() -> vynamp_decode::Result<()> {
    let mut reader = open_path(std::path::Path::new("song.wav"), Options::default())?;
    let track = reader.track().clone();
    println!("{} Hz, {} channels", track.sample_rate, track.channels);

    let mut buf = AudioBuffer::new();
    let mut interleaved = Vec::new();
    let mut frames = 0u64;
    while reader.read(&mut buf)? {
        buf.to_f32_interleaved(&mut interleaved);
        frames += buf.frames() as u64;
        // hand `interleaved` to your audio device here
    }
    println!("{frames} frames");
    Ok(())
}
```

- `read` returns `Ok(true)` with at least one frame, or `Ok(false)` at the end.
- Reuse the same `AudioBuffer`; its storage is recycled between calls.
- Planar access is available per format: `plane_i16(c)`, `plane_i32(c)` (24-bit
  samples are sign-extended, not shifted), `plane_f32(c)`, and so on. Check
  `buf.format()` first.
- `Reader::seek(frame)` seeks on the trimmed output timeline.
- Use `open` with a `Box<dyn Source>` (for example `VecSource`) when the data is
  not a file.

A complete example lives in `crates/vynamp-decode/examples/decode_to_raw.rs`; the
current support table is printed by `cargo run --example support_matrix`.
