# MPEG audio (Layer I, II, III)

Public descriptions consulted for `vynamp-decode-mpa` (the elementary stream demuxer). No
FFmpeg, mpg123, libmad, LAME or other GPL/LGPL source code was read or copied; the
implementation was written from the format descriptions below. FFmpeg is used only as a
dev-time tool: it writes the fixtures (`tools/fixtures/generate-mpeg-dev.ps1`) and
`ffprobe -count_packets` supplied the expected packet counts recorded in the tests.

- ISO/IEC 11172-3:1993 (MPEG-1 audio) and ISO/IEC 13818-3:1998 (MPEG-2 audio), as publicly
  described: the 32-bit frame header (11-bit sync, version, layer, protection bit,
  bitrate index, sample rate index, padding, channel mode, mode extension, emphasis), the
  bitrate tables for MPEG-1 Layers I, II and III and for MPEG-2/2.5 Layer I and Layers II/III,
  the sample rates (44.1, 48, 32 kHz; 22.05, 24, 16 kHz; 11.025, 12, 8 kHz), the frame length
  formulas (Layer I: `(12 * bitrate / sample rate + padding) * 4`; Layer II and MPEG-1
  Layer III: `144 * bitrate / sample rate + padding`; MPEG-2/2.5 Layer III:
  `72 * bitrate / sample rate + padding`), samples per frame (384, 1152, 1152 and 576), the
  Layer III side information sizes (17/32 bytes in MPEG-1, 9/17 in MPEG-2/2.5) and free format
  (bitrate index 0, length given by the distance between headers). MPEG-2.5 is the unofficial
  extension with version bits `00`. The tables are small and were typed in from these
  descriptions; the CC0 `minimp3` tables are not used by this crate (they are for the decoder
  tasks).
- Xing/Info header (Xing Technology, as documented publicly): `Xing` or `Info` after the side
  information, flags (frames, bytes, TOC, quality), 100-byte TOC. `Info` is the same layout for
  constant bitrate files.
- LAME tag (LAME project's public "LAME tag" description): the 36 bytes after the Xing fields:
  encoder string, revision/VBR method, low-pass, replay gain, flags, bitrate, the 12-bit
  encoder delay and 12-bit padding, music length, music CRC and tag CRC. **Tag CRC variant:**
  CRC-16/ARC (polynomial 0x8005 processed LSB first, i.e. 0xA001, initial value 0, no final
  XOR) over the frame bytes before the CRC field, which is the first 190 bytes of an MPEG-1
  stereo frame. Verified on three files written by LAME 3.99r and three written by FFmpeg's
  Lavc 57.48 muxer (read locally, not committed: the stored CRC equalled the computed one in
  all six). FFmpeg's muxer writes the CRC over the first 190 bytes with the CRC field zeroed
  even for layouts where the tag ends earlier (MPEG-2, mono; the committed 22.05 kHz
  fixture shows it), so the demuxer accepts either span.
- Gapless arithmetic: the decoder's own delay is 529 samples (polyphase filterbank 481 plus
  the IMDCT overlap, the usual figure for MP3 decoders). `delay = encoder_delay + 529` and
  `padding = tag padding - 529`; `frames * samples per frame - delay - padding` equals the
  length of the encoded PCM. This was checked against the PCM length on the committed fixtures
  (48, 44.1, 22.05 and 8 kHz; stereo and mono).
- VBRI (Fraunhofer): `VBRI` at offset 36 of the first frame, version, delay, quality, bytes,
  frames, table entry count, scale, entry size, frames per entry, table. The delay field is
  ignored.
- Seek preroll: a Layer III frame starts its main data `main_data_begin` bytes (9 bits in
  MPEG-1, 8 in MPEG-2/2.5, at most 511 bytes) before its own, inside the main data of earlier
  frames. The demuxer reads `main_data_begin` of the goal frame and steps back over earlier
  frames, adding up their main data (frame length minus header, CRC and side information),
  until it covers that; one more frame is added for the IMDCT overlap and the total is capped at
  32 frames. Layers I and II have no reservoir; one frame warms the synthesis filterbank up. The
  decoder task checks the figures against its output.
- Gapless is exact only if the LAME padding is at least 529; a smaller one saturates to 0.
- ID3v2, ID3v1, APEv2 and Lyrics3v2 tags are handled by the core `skip_id3v2` and
  `find_trailers`.

## Decoder: synthesis filterbank and Layer I (`vynamp-decode-mp3`)

- Polyphase synthesis (ISO/IEC 11172-3 annex C, as publicly described): matrixing
  `V[i] = sum_k cos((16 + i)(2k + 1) pi / 64) S[k]`, the 1024-entry `V` history, the
  `U` vector built from 32-entry halves of every 64-entry block, and
  `out[j] = sum D[j + 32 m] U[j + 32 m]`. The fast form used here is derived from the cosine
  symmetries (see `synth.rs`), not copied.
- **Synthesis window `D[i]` (512 coefficients, multiples of 2^-16).** Taken from the
  public-domain (CC0) `minimp3` source, <https://github.com/lieff/minimp3> (`minimp3.h`),
  whose licence header reads: "To the extent possible under law, the author(s) have dedicated
  all copyright and related and neighboring rights to this software to the public domain
  worldwide ... See <http://creativecommons.org/publicdomain/zero/1.0/>" (checked on
  2026-10-24). Only the numeric constants were used: `minimp3` stores 240 window values in a
  layout folded for SIMD plus 16 constants in its two output pairs; a throw-away script
  (not committed) ported its scalar synthesis stage and solved, by least squares over random
  input, for the coefficient of every `U` entry of the definition above, which yields `D[i]`
  in the standard's order. The integers came out exact (the largest deviation from an integer
  was 1e-3 after scaling `D[256]` to 75038). Eight entries (`D[16 + 64 n]`) multiply a
  matrixing output that is identically zero, so the solve cannot see them; they are set by the
  window's antisymmetry. The result is checked in `tables.rs`/`synth.rs` tests: antisymmetry,
  `D[256] = 75038`, and the Layer I delay test, which passes a signal through the standard's
  analysis filterbank (window `C = D / 32`) and this synthesis and recovers it with the
  expected 481-sample delay and an error at the filter's known ripple level.
- Layer I frame layout (allocation, scalefactor index `2^(1 - i / 3)`, sample requantization
  `(s - 2^(n-1) + 1) * 2 / (2^n - 1) * scalefactor`, joint stereo `bound = 4 + 4 * mode_ext`
  with one shared sample above it, CRC over header bytes 2 and 3 plus the allocation field), from the
  public description of the standard.
- **Layer I test material.** No Layer I encoder is available (FFmpeg has none) and no Layer I
  stream with a clear permissive licence was found (a search turned up only the ISO conformance
  streams, whose licence is unstated, which are therefore not used). Layer I is verified
  against an independent `f64` implementation of the definition on hand-assembled frames
  and the delay test above; its verification level stays **Experimental**.
