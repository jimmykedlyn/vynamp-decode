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
  frames. The demuxer reads `main_data_begin` of the goal frame and of the two frames before it
  (the IMDCT of a granule overlaps the previous one, and the polyphase filterbank keeps 16 slots
  of history, so the goal's first granule is exact only if the two granules before it were
  decoded from real data) and steps back over earlier frames, adding up their main data (frame
  length minus header, CRC and side information), until each of the three is covered; the
  earliest start is taken and the total is capped at 32 frames. Layers I and II have no
  reservoir; one frame warms the synthesis filterbank up. The facade test
  `decode_after_seek_equals_the_full_decode_slice` shows the output after a seek is identical
  to the full decode (the first version, goal frame only, was wrong by up to 0.35 at 8 kbit/s).
- Gapless is exact only if the LAME padding is at least 529; a smaller one saturates to 0.
- Frame count rules. The Xing/Info/VBRI frame count is the number of audio frames, the header
  frame excluded (LAME and FFmpeg write it so: the committed fixtures declare exactly the number
  of frames delivered, and their `bytes` field includes the header frame, so `bytes` cannot tell
  whether the count includes it). A writer that counts the header frame is recognised at open
  for constant bitrate Info headers by the audio size (the count or the count less one matches
  the bytes of audio at the stream's bitrate within four slots); `Track::frames` is then the
  audio frames. Where that cannot be checked, delivered == declared or delivered + 1 == declared
  is accepted at the end of the stream. Anything else (fewer frames, or more frames than
  declared, which a Reader that trusts the count would silently truncate) is Strict
  `Invalid { what: "mpeg frame count" }` and Resilient `Event::Clamped`. A count of 0 is
  treated as absent (`Track::frames` is `None`, no check, no gapless numbers). Note that a
  `Reader` stops at the declared end for tracks with padding, so the over-long case is reported
  to consumers that read the demuxer to its end.
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
- **Layer II tables and layout.** The bit-allocation tables (which allocation code means which
  quantization class, per subband group, for MPEG-1 tables a to d and the MPEG-2 LSF table),
  the table selection by version, sample rate and bitrate per channel, and the scalefactor
  transmission pattern by `scfsi` were transcribed from the public-domain (CC0) `minimp3`
  source, <https://github.com/lieff/minimp3> (`minimp3.h`, same licence header as above,
  checked 2026-10-24): `L12_subband_alloc_table`, `g_bitalloc_code_tab` and
  `L12_read_scalefactors`. Only the constants and rules were used, re-expressed in
  `layer2.rs`; the decoding code is original. They agree with the standard's tables as
  recalled independently (steps per allocation code and the field sizes 88, 94, 26, 38 and
  75 bits per channel are asserted in the `layer2` tests), and with FFmpeg's selection rule.
  Requantization is `(c - (n - 1) / 2) * 2 / n * scalefactor` for `n` levels (grouped
  classes 3, 5, 9 in codes of 5, 7, 10 bits, `c0 + n c1 + n^2 c2`). The CRC covers header
  bytes 2 and 3, the allocation field and the `scfsi` field (bit-granular).
- **Layer II test material.** `tests/fixtures/l2-*.mp2` were generated by this project with
  FFmpeg (`mp2`, and libtwolame for joint stereo with CRC) from a synthetic signal, so there
  is no third-party licence to carry; the `.f32` files are FFmpeg's `mp2float` decodes of the
  same files, used only as test oracle (`tools/fixtures/generate-mpeg-dev.ps1`).

## Decoder: Layer III bitstream layer (`vynamp-decode-mp3`, `src/layer3/`)

Side information, bit reservoir, scalefactors and Huffman decoding, from the public description
of ISO/IEC 11172-3 section 2.4 and ISO/IEC 13818-3. The decoding code is original. Constants
and two interpretation details were taken from the public-domain (CC0) `minimp3` source,
<https://github.com/lieff/minimp3> (`minimp3.h`, licence header checked 2026-10-24 as above):

- **Huffman code tables** (`layer3/codes.rs`): ISO/IEC 11172-3 annex B tables 1 to 3, 5 to 13,
  15, 16 and 24 (tables 16 to 23 and 24 to 31 share the code of 16 and 24 and differ in
  `linbits`: 1 2 3 4 6 8 10 13 and 4 5 6 7 8 9 11 13) and the two count1 tables. `minimp3` stores
  them as packed multi-level lookup trees (`tabs`, `tabindex`, `tab32`, `tab33`). A throw-away
  script (not committed) walked those trees bit by bit and wrote every symbol's code length and
  code value into `codes.rs` as plain `(length, code)` pairs in the standard's `x * ysize + y`
  order; `huffman.rs` builds its own two-level lookup tables from them (8 root bits). Checks in
  the tests: every table is a complete (Kraft sum exactly 1) prefix-free code; tables 1 and 2
  equal the standard's printed `hlen`/`hcod`; count1 table B is the 4-bit complement; every
  `(x, y)` of every table, with all sign combinations and the escape range, round-trips through
  an encoder written from the standard's description; the 3056 granules of the committed
  fixtures (44.1, 48, 22.05 and 8 kHz, CBR and VBR, long, short and start/stop blocks, count1
  tables A and B, every escape table 16 to 31) consume exactly `part2_3_length` bits. The
  fixtures do not use tables 8 to 13; those rest on the transcription, the Kraft/prefix checks
  and the round trips.
- **Scalefactor band widths** (`layer3/tables.rs`: long, short and mixed, 8 rows for 11.025/12,
  8, 22.05, 24, 16, 44.1, 48 and 32 kHz): from `g_scf_long`, `g_scf_short` and `g_scf_mixed`.
  Every row adds up to 576 lines (tested). The 8 kHz mixed row is taken as is and has three
  entries more than it has scalefactors.
- **MPEG-2/2.5 scalefactor partitions** (`LSF_PARTITIONS`, the `nr_of_sfb` values of the
  standard for long, mixed and short blocks and for the intensity right channel) and the
  MPEG-1 `slen` pairs and partitions (`SLEN_V1`, `V1_PARTITIONS`): from `g_scf_partitions` and
  `g_scfc_decode`. The `scalefac_compress` ranges (< 400, < 500, < 512 and, for the intensity
  right channel, on half the value, < 180, < 244, < 256) are the standard's.
- `pretab` (`PRETAB`, 22 entries): the standard's table; `minimp3`'s `g_preamp` carries the same
  ten non-zero values for bands 11 to 20.
- **Interpretation details taken from `minimp3`:** a short or mixed block's region 0 is
  `region0_count + 1` entries of the transmission-order width list (nine for short: 36 lines
  at every rate except 8 kHz, where it is 72; eight for mixed: 36 lines in MPEG-1, 48 in
  MPEG-2, 64 at 8 kHz). The 8 kHz short-block figure was confirmed by the committed 8 kHz fixture:
  with a fixed 36 the granule overran its `part2_3_length`. The mixed-block MPEG-2 figures have
  no fixture. A count1 quadruple that would end beyond `part2_3_length` is dropped as stuffing.
  The `preflag` of MPEG-2/2.5 is `scalefac_compress >= 500`, but never for the right channel of an
  intensity stereo frame (ISO/IEC 13818-3 gives that channel its own ranges).
- Bit reservoir: `main_data_begin` (9 bits in MPEG-1, 8 in MPEG-2/2.5) counts bytes back from the
  first main data byte of the frame; the decoder keeps the last 4 KiB of main data, glues the
  needed tail in front of the frame's own and reads the granules one after the other. A granule
  that starts before the first byte the decoder has seen is reported as not decodable (the
  synthesis outputs silence). The CRC covers header bytes 2 and 3 and the side information.
- Test material: the `vynamp-decode-mpa` fixtures `tests/fixtures/*.mp3` (generated by this
  project with FFmpeg/libmp3lame, see `tools/fixtures/generate-mpeg-dev.ps1`) plus bitstreams
  built in the tests by a small encoder written from the standard's description.

## Decoder: Layer III synthesis path (`vynamp-decode-mp3`)

- Requantization, short block reordering, middle/side and intensity stereo (MPEG-1 and
  ISO/IEC 13818-3), alias reduction (coefficients `c_i` of the standard), IMDCT 36/12 with window
  types 0-3, mixed blocks, overlap-add and frequency inversion were written from the public
  descriptions of the standard. The IMDCT uses a DCT-IV via a 9-point (3x3) or 3-point complex
  DFT, derived in `imdct.rs` and tested against the direct O(n^2) definition.
- **Mixed block region 0 in MPEG-2/2.5.** 36 lines (72 at 8 kHz), as FFmpeg (the oracle) does,
  not the 48 (64 at 8 kHz) that eight entries of the `minimp3` width list give. MPEG-1 is 36.
  Mixed blocks are not emitted by libmp3lame, so this rests on the ruling and a unit test.
- **8 kHz mixed layout.** Only the first 3 entries are long bands (36 lines); `Layout::n_long`
  is 3 there.
- **Intensity, illegal positions.** MPEG-1: position >= 7. MPEG-2/2.5: all-ones value of the
  partition width, and every position of a partition with slen 0 (`is_max = 0`, as the ISO
  reference decoder treats it). Illegal bands fall back to MS (if on) or L/R.
- **Oracle alignment.** Fixtures are libmp3lame encodes written without Xing/LAME tag
  (`-write_xing 0`, empty ID3v2 header cut off). With a tag FFmpeg's demuxer makes the decoder
  skip the encoder delay + 529 (528 + 1) samples and the padding (a tagged 0.3 s encode decodes to
  exactly 13230 frames); without it `mp3float` outputs every sample of every frame, which is
  compared with our untrimmed output at offset 0. Lags of +-1 are asserted to be far worse.
- LAME emits neither intensity stereo nor mixed blocks: both are checked on hand-assembled
  frames (intensity by linearity against a plain-stereo decode of the same left channel; all
  block types against an f64 reference chain).

### Layer III fix round 1 (rulings)

- **Mixed-block intensity bound.** The long bands of a mixed block may be intensity only if
  every short window of the right channel is zero (ISO 11172-3 reference decoder and FFmpeg:
  the non-zero search over the short windows gates the long loop); test
  `mixed_long_bands_are_intensity_only_if_every_short_window_is_empty`.
- **8 kHz (MPEG-2.5) mixed layout.** Follows FFmpeg (MPEG-2.5 is not an ISO standard):
  six 12-line long bands (72 lines, requantized without subblock gain, not reordered; the
  reordering starts at line 72), then the short bands from band 3. The IMDCT and alias
  reduction still treat only the two lowest subbands as long. The old `minimp3`-derived row
  (three long entries and an "entries 33-35 take 30-32" anomaly) is gone; test
  `the_8khz_mixed_layout_is_six_long_bands_then_the_short_bands_from_band_3`.
- **Intensity positions of slen 0 (M5).** Kept as in the ISO reference software (`is_max = 0`
  makes every position of such a partition illegal). FFmpeg may differ (it derives its own
  `sf_max`); we follow the ISO reference because it is the specification's own decoder, and
  only streams from unusual encoders reach this case.
- **Joint stereo with one undecodable channel.** If either channel of a granule needs preroll
  while middle/side or intensity is on, both channels of that granule are silent (test
  `a_joint_stereo_granule_with_one_undecodable_channel_is_silent_in_both`).
- **Different block layouts in the two channels.** Intensity is skipped and full-range
  middle/side is applied; FFmpeg uses the right channel's layout instead. Only streams with
  encoder errors reach this.
- **8 kHz mixed blocks: four long subbands.** Superseding the fix-round remark above that the
  IMDCT and alias reduction keep two long subbands: the 72 long lines are four subbands, so at
  8 kHz the long IMDCT/window covers subbands 0-3 and alias reduction runs across boundaries
  1-3 (FFmpeg's MPEG-2.5 switch_point behaviour, per the controller ruling); every other rate
  keeps two subbands and boundary 1. Both counts come from `Layout::long_subbands()` (long
  lines / 18); test `long_regions_agree_for_every_sample_rate` and the 8 kHz f64 reference
  chain test.

## Corpus, conformance evaluation and benchmark (plan 03, task 8)

- **Conformance evaluation.** The ISO/IEC 11172-4 and 13818-4 compliance bitstreams (the
  `.bit`/`.mpg` test streams with their reference PCM) are ISO copyright material distributed
  with the standards; no licence permitting redistribution was found, so they were **not
  downloaded and not committed**. Without official vectors the support matrix says `oracle` for
  Layers II and III (`mpeg-audio` codec) and `experimental` for Layer I (`mpeg-audio-layer1`
  codec: FFmpeg has no Layer I encoder, so there is no real-world Layer I file and no third-party
  oracle; it is verified only against an independent `f64` reference on hand-assembled frames).
  The ISO full accuracy criterion (RMS < 2^-15/sqrt(12), maximum <= 2^-14) is applied against
  FFmpeg's `mp2float`/`mp3float` instead.
- **Oracle corpus** (`tests/fixtures/mpeg/`, 33 encodes plus 33 f32 references, about 3.0 MiB;
  test `crates/vynamp-decode/tests/oracle_mpeg.rs`). Generated by the MPEG section of
  `tools/fixtures/generate.ps1` with FFmpeg n8.1.3-23-g5c4f5744cd (win64 gpl build, 2026-10-22)
  from signals made by `aevalsrc` (nothing read from outside the repository). `MANIFEST.txt`
  records per file the SHA-256 of the file and of its reference, rate, channels, signal, layer,
  note and the exact encoder arguments; the common command shape is
  `ffmpeg -v error -y -f lavfi -i "aevalsrc=<expr>:s=<rate>:d=<secs>" -fflags +bitexact
  -flags:a +bitexact <args> <file>` followed by the removal of the empty ID3v2 header FFmpeg
  writes, and `ffmpeg -c:a mp2float|mp3float -i <file> -f f32le -c:a pcm_f32le <file>.f32`.
  Layer III arguments are `-c:a libmp3lame <rate/mode args> -write_xing 0`; Layer II uses `-c:a
  mp2` (and `libtwolame` with `-mode joint_stereo -error_protection 1` for the joint stereo CRC file).
- **Layer II tables.** The decoder does not expose the table it picked, so the test re-derives
  it from each file's header (version, rate, bitrate per channel; the same rule as the
  standard and `select` in `layer2.rs`) and requires it to equal the manifest's `table` note:
  A (27 subbands): 48 kHz stereo 128 kbit/s, 44.1 kHz mono 64; B (30): 44.1 kHz 384, 32 kHz 256;
  C (8 subbands, below 56 kbit/s per channel): 48 kHz 96 and 64 kbit/s stereo, 48 kHz joint stereo
  96 with CRC; D (12 subbands, 32 kHz below 56): 64 kbit/s stereo, 32 kbit/s mono; LSF: 24 kHz 64,
  22.05 kHz 96, 16 kHz mono 32. Layer III covers CBR 32/96/192/256/320, VBR `-q:a` 0/3/6/9, ABR
  128/192, mono, joint stereo, forced L/R, MPEG-2 (22.05/24/16 kHz) and MPEG-2.5 (11.025, 12 and
  8 kHz). Measured errors are far inside the criterion (RMS about 1.3e-7 to 4.8e-7 against a
  limit of 8.8e-7; maximum about 5e-7 to 3.2e-6 against 6.1e-5).
- **Frame count tolerance.** A VBR Xing count that includes the Info frame (delivered + 1) cannot
  be told from a correct one, so it is accepted in Strict mode too (test
  `a_vbr_count_that_includes_the_info_frame_is_tolerated_without_an_event`); the LAME tag's
  trailing padding then lies past the data and is not trimmed (the output keeps those samples).
- **CBR truncated by one frame.** A constant bitrate file cut by exactly one frame is
  normalised silently: the declared-1 candidate matches the audio size, so it is read as a count
  that included the Info frame.
- **Benchmark.** See `book/src/performance.md`; the multi-second files are generated at bench
  time (not committed).
