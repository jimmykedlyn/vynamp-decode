# Architecture

vynamp-decode is a std-only workspace: no crates.io dependencies, no `unsafe` in the
library crates.

## Crates

| Crate | Role |
|-------|------|
| `vynamp-decode-core` | Shared types and traits: `Source`, `Track`, `Packet`, `AudioBuffer`, `Demuxer`, `Decoder`, `Registry`, `Options`, `Error`. Depends on nothing. |
| `vynamp-decode-riff` | RIFF/WAVE, RF64, BW64 and Wave64 demuxer. |
| `vynamp-decode-aiff` | AIFF and AIFF-C demuxer. |
| `vynamp-decode-pcm` | Raw PCM decoder (8/16/24/32-bit integer, 32/64-bit float, both endians). |
| `vynamp-decode` | Facade: `default_registry`, `open`, `open_path` and the gapless-aware `Reader`. |
| `vynamp-decode-testkit` | Test-only: SHA-256, signal generators, container writers, allocation counter. Never a dependency of `vynamp-decode-core`. |

## Data flow

```text
Source -> Probe -> Demuxer -> Packet -> Decoder -> AudioBuffer -> Reader
```

1. A `Source` is a seekable byte stream (file, slice, vector).
2. The `Registry` probes the first bytes with each container's `probe`
   function; the file extension only breaks ties.
3. The winning container's `open` builds a `Demuxer` that exposes `Track`s and
   yields `Packet`s.
4. The codec registered for the track's `CodecId` builds a `Decoder` that turns
   packets into a planar `AudioBuffer`.
5. The `Reader` drives the loop and is the **only** place where encoder delay,
   encoder padding and seek preroll are trimmed. Demuxers and decoders report
   the full, untrimmed timeline; this keeps them simple and keeps gapless
   behaviour in one place.

Containers and codecs are described by static descriptors, so enabling a format
is a Cargo feature plus one `add_container` / `add_codec` call in
`default_registry`.
