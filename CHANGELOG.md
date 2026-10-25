# Changelog

## Unreleased

- Workspace skeleton, `vynamp-decode-testkit`, documentation and Woodpecker CI.
- `vynamp-decode` facade with gapless-aware `Reader`, RIFF/RF64/Wave64 and AIFF/AIFF-C
  demuxers and the PCM decoder.
- Third-party PCM oracle corpus (FFmpeg-generated, dev-only), `support_matrix`
  example and mdBook sources.
- FLAC (RFC 9639): native demuxer (frame sync, CRC-checked packetization, exact seek, Resilient
  recovery) and decoder (constant, verbatim, fixed and LPC subframes, Rice residuals, stereo
  decorrelation, wasted bits, up to 32 bits and 8 channels) with STREAMINFO MD5 verification.
  Legacy variable block size streams (pre-2007 layout) are recognised.
- FLAC oracle corpus (FFmpeg-generated, dev-only, 22 files with PCM references; the pinned FFmpeg
  cannot write 8-bit FLAC) and a conformance subset of the CC0 IETF `flac-test-files` (20 of the
  64 `subset` FLAC files committed, all 64 pass with the default limits when run from a clone; the
  `faulty` files are rejected in Strict mode except file 10, an invalid Vorbis comment block,
  which is validated in plan 04). FLAC is therefore `conformance`.
- Strict mode rejects more samples than STREAMINFO declares and non-final blocks below 16
  samples; Resilient reports `Event::Clamped` for the former and keeps the audio.
- Default `Limits::max_metadata_bytes` raised from 16 MiB to 64 MiB (large cover art).
- MPEG audio (Layers I, II and III; MPEG-1, 2 and 2.5): `mpa` demuxer and `mp3` decoder registered
  in the facade behind the default features `mpa` and `mp3`. Exact gapless output from a valid LAME
  tag (otherwise only the decoder delay of 529 samples, or 481 for Layers I and II, is trimmed),
  exact seeking (decode after a seek equals the slice of the full decode), Resilient recovery
  (`CorruptFrame` and silence for undecodable frames, `GaplessConflict` for a LAME tag that does not
  fit the frame count). A Xing/VBRI frame count must equal the frames delivered or exceed them by
  one. `Decoder::set_preroll` tells a decoder the seek preroll. Verification level `oracle`.
- MPEG audio oracle corpus (FFmpeg-generated, dev-only; 12 Layer II files covering allocation
  tables A, B, C, D and the MPEG-2 LSF table, 21 Layer III files covering CBR, VBR, ABR, mono,
  joint stereo, MPEG-2 and MPEG-2.5, each with the f32 decode of FFmpeg's `mp2float`/`mp3float`;
  3.0 MiB), checked against the ISO/IEC 11172-4 full accuracy criterion. The ISO compliance
  bitstreams are not redistributable and are not used: Layers II and III are `oracle`; Layer I is
  `experimental` and now has its own codec descriptor, `mpeg-audio-layer1`. MP3 decode benchmark
  (about 160x to 235x realtime for 44.1 kHz stereo) in the book.
