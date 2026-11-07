# vynamp-decode 03 — MPEG audio (Layer I/II/III) with exact gapless — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Native MPEG-1/2/2.5 audio decoding (Layers I, II, III), CBR/VBR/free-format, Xing/Info/LAME/VBRI headers, exact gapless trimming, exact seek with bit-reservoir preroll, Strict/Resilient behaviour — no third-party code.

**Architecture:** New crates `vynamp-decode-mpa` (elementary-stream demuxer: sync, headers, VBR headers, seek) and `vynamp-decode-mp3` (decoders for Layer I/II/III sharing the polyphase synthesis filterbank). Trailer detection moves from the FLAC crate to core so both share it. Output is `F32` planar. The Reader applies delay/padding from the track (gapless trim stays only in Reader).

**Tech Stack:** Rust 1.99.0, std only. FFmpeg (`$env:FFMPEG`, dev only; BtbN GPL build includes libmp3lame) for fixtures and as the reference decoder.

**Spec:** `docs/superpowers/specs/2026-10-23-vynamp-decode-design.md` (§5.5 verification, §9.1 levels, §9.6 gapless precedence). Plans 01/02 define the core API.

## Global Constraints

- Std only; no crates.io deps (dev included). `unsafe` forbidden.
- No panic on external input; checked arithmetic on header-derived values; `Limits` before sizing buffers.
- Strict (default): corrupt/truncated data → error, never partial audio as complete. Resilient: recover (silence for an undecodable frame), report every recovery as an `Event`.
- Gapless trim only in `Reader`; decoders/demuxers never trim — they set `Track.delay/padding/gapless`.
- Clean-room: algorithms implemented from the standard's published description (ISO/IEC 11172-3, 13818-3 and their public descriptions) and the LAME/Xing/VBRI header documentation. **Controller ruling:** constant tables (Huffman code tables, scalefactor band tables, Layer II allocation tables, synthesis window coefficients) may be taken from the public-domain (CC0) `minimp3` source or computed from their closed-form definitions; attribute in `docs/sources/mpeg-audio.md`. No GPL/LGPL code (FFmpeg, mpg123, libmad) read.
- `Invalid.offset` = absolute source byte position.
- Commits end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Gate before every commit: `cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked`.

## Verification standard (lossy)

ISO/IEC 11172-4 "full accuracy" criterion against a reference decoder's float output: RMS error < 2^-15/√12 (≈ 8.8e-6 FS) and max absolute error ≤ 2^-14 (≈ 6.1e-5 FS) per channel, after aligning both outputs exactly (same delay/padding handling). The reference is FFmpeg's float MP3/MP2 decoder output (`-c:a mp3float` / `mp2float`, written as f32 raw) committed next to each fixture. Tolerance is pre-fixed in the test; no per-file tuning.

## Review Focus

1. LAME-encoded VBR MP3 played gapless: output length = original PCM length exactly; first output sample aligned with the original first sample (±0). (Tasks 2, 8)
2. Seek into the middle of a VBR file with a large bit reservoir → output after seek identical (within tolerance) to the output of a full decode at that position. (Tasks 2, 7)
3. MP3 with ID3v2 at start, APEv2 + ID3v1 at end → no garbage frames decoded from tag bytes. (Tasks 1, 2)
4. Free-format and MPEG-2.5 (8 kHz) streams decode. (Tasks 2, 5)
5. Corrupt main_data / bad Huffman code mid-file → Strict Err; Resilient one silent frame + `CorruptFrame`, timeline continuous. (Tasks 5, 7)

---

### Task 1: Shared trailer detection + parameterised CRC-16 (core; FLAC uses it)

**Files:** Create `crates/vynamp-decode-core/src/trailer.rs`; modify `crc.rs`, `crates/vynamp-decode-flac/src/demux.rs` (use core version, delete its copy).

**Interfaces:**
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum TrailerKind { Id3v1, ApeV2, Lyrics3v2, Id3v2Appended }
#[derive(Debug, Clone, PartialEq, Eq)] pub struct Trailer { pub kind: TrailerKind, pub offset: u64, pub len: u64 }
/// Scans backwards from the source end; returns trailers outermost-last and the end of the audio payload. Leaves `src` position unspecified.
pub fn find_trailers(src: &mut dyn Source, payload_start: u64) -> Result<(u64 /*audio_end*/, Vec<Trailer>)>;
pub fn crc16_mpeg(data: &[u8]) -> u16; // poly 0x8005, init 0xFFFF (MPEG audio header CRC)
```
- Behaviour identical to the FLAC implementation from plan 02 (ID3v1, APEv2 with/without header, Lyrics3v2, 3DI footer, any common order, ≤ 16 iterations); now returns the trailer ranges (plan 04 will read them).
- [ ] **Step 1: tests** (move FLAC trailer tests into core `tests/trailer.rs`, add range assertions); `crc16_mpeg(b"123456789") == 0xAEE7` (CRC-16/CMS check value: poly 0x8005, init 0xFFFF, not reflected, no xorout). FLAC tests unchanged and passing.
- [ ] **Steps 2–5:** FAIL → implement → PASS + gate (incl. FLAC conformance env-var run) → commit `refactor(core): shared trailer detection and MPEG CRC-16`.

### Task 2: MPEG audio demuxer (`vynamp-decode-mpa`)

**Files:** Create `crates/vynamp-decode-mpa/{Cargo.toml, src/lib.rs, src/header.rs, src/vbr.rs, src/demux.rs}`, `docs/sources/mpeg-audio.md`, fixtures dir `tests/fixtures/` + dev generator `tools/fixtures/generate-mpeg-dev.ps1`.

**Interfaces:**
```rust
pub struct FrameHeader { pub version: MpegVersion /*V1,V2,V25*/, pub layer: u8 /*1..=3*/, pub crc: bool, pub bitrate_kbps: u32 /*0 = free*/, pub sample_rate: u32, pub padding: bool, pub channel_mode: ChannelMode, pub mode_ext: u8, pub emphasis: u8, pub frame_len: usize /*bytes incl. header*/, pub samples: u32 /*384, 1152, 576*/ }
pub fn parse_header(b: [u8; 4]) -> Option<FrameHeader>; // None for invalid sync/reserved values
pub struct VbrInfo { pub kind: VbrKind /*Xing,Info,Vbri*/, pub frames: Option<u32>, pub bytes: Option<u32>, pub toc: Option<Box<[u8; 100]>>, pub vbri_table: Vec<u32>, pub lame: Option<LameInfo> }
pub struct LameInfo { pub encoder_delay: u16, pub padding: u16, pub crc_ok: bool /*LAME tag CRC over first 190 bytes*/ }
pub fn probe_mpa(header: &[u8]) -> u8; // ≥ 3 consecutive valid frames with consistent version/layer/rate → 90; 1 frame → 30
pub struct MpaDemuxer; pub fn open_mpa(src: Box<dyn Source>, opts: &Options) -> Result<MpaDemuxer>; impl Demuxer for MpaDemuxer;
```
- Open: `skip_id3v2`; `find_trailers`; sync on the first header followed by a consistent next header (false-sync guard: same version/layer/sample rate, and for Layer III a plausible frame length); free format: measure the frame length from the distance to the next sync with the same header bits (bounded scan ≤ 2 × max free-format frame) — record it.
- VBR headers inside the first frame's side-info area (Xing/Info offset by version+channels, VBRI at +32): fill `Track.frames`; with a LAME tag whose CRC is valid, `delay = encoder_delay + 529` (529 = MP3 decoder delay) and `padding = lame_padding.saturating_sub(529)`; so gapless output = frames × samples_per_frame − delay − padding. Document the arithmetic and assert it on fixtures (output length must equal the original PCM length). `gapless = FromMetadata`. Without LAME: `delay = 529`, `padding = 0`, `gapless = Unknown` (Layer I/II: decoder delay per their filterbank — 240+1 for Layer I/II? derive from the synthesis filter (481 samples) and verify against FFmpeg fixture alignment; record the value). The Xing/Info frame itself is not audio: it is consumed and not emitted as a packet.
- Packets: one frame per packet (`data` = full frame incl. header), `pts` = frame index × samples. Layer III packets also need the bit reservoir — the decoder keeps it; the demuxer just delivers frames in order.
- Seek(frame): target frame index = frame / samples; preroll for Layer III = start at `max(0, idx − 2)` frames earlier (bit reservoir ≤ 511 bytes spans at most 2 previous frames at low bitrates — verify with fixtures; make the constant 2 configurable internally) so `SeekTo{frame: start_idx × samples, preroll: frame − start_idx × samples}`. Byte position: Xing TOC (interpolated) or VBRI table, else CBR arithmetic, else bisection with header sync; always land on a verified header and then count frames by scanning when the index is ambiguous (VBR without table → scan from the nearest known point, bounded).
- Strict: lost sync mid-stream → `Invalid{what:"mpeg sync"}`; truncated last frame → `Invalid{what:"unexpected eof"}`. Resilient: resync, emit `CorruptFrame` with the lost samples (estimated from header), silence packet (empty data) for the gap, bounded like FLAC (≤ 8 frames silence per gap).
- [ ] **Step 1: tests** (`tests/demux.rs`): header parse table tests (all bitrate/sample-rate indices for V1/V2/V2.5 × layers; reserved values → None); fixtures (FFmpeg: libmp3lame CBR 128k, VBR -q:a 2, 22.05 kHz MPEG-2, 8 kHz MPEG-2.5, mono; `mp2` encoder 192k; free-format via a test helper that rewrites bitrate index 0 on a CBR file only if the decoder can still validate — else synthetic headers): frame counts equal FFmpeg's, LAME delay/padding parsed, ID3v2 + APEv2 + ID3v1 wrapped file yields identical packets, seek tests (every 10th frame, last frame, past end) land on the expected frame with the expected preroll, garbage/truncation smoke in both modes.
- [ ] **Steps 2–5:** FAIL → implement → PASS + gate → commit `feat(mpa): MPEG audio elementary stream demuxer with VBR headers and seek`.

### Task 3: Polyphase synthesis filterbank + Layer I decoder (`vynamp-decode-mp3`)

**Files:** Create `crates/vynamp-decode-mp3/{Cargo.toml, src/lib.rs, src/synth.rs, src/layer1.rs, src/tables.rs}`.

**Interfaces:** `pub struct MpegDecoder; impl MpegDecoder { pub fn new(track: &Track, opts: &Options) -> Result<Self> }` + `impl Decoder` (dispatches on `CodecId::Mp1/Mp2/Mp3`); `pub(crate) struct Synthesis` — 32-subband polyphase synthesis (matrixing + 512-tap window D[i] from its definition/table), state per channel, `fn synth(&mut self, subbands: &[f32; 32], out: &mut [f32; 32])`.
- Layer I: allocation, scalefactors, sample requantization per the standard; CRC check when `crc` bit set (Strict → Invalid{what:"mpeg crc"}).
- [ ] **Step 1: tests:** synthesis impulse response vs the closed-form cosine matrix (single subband impulse → expected output within 1e-6); Layer I: there is no FFmpeg Layer I encoder — use hand-assembled frames for unit tests and, if available, public-domain/permissive Layer I test streams (search; if none, Mp1 verification = Experimental and document).
- [ ] **Steps 2–5:** FAIL → implement → PASS + gate → commit `feat(mp3): polyphase synthesis and Layer I`.

### Task 4: Layer II decoder

**Files:** Create `crates/vynamp-decode-mp3/src/layer2.rs`; tables in `tables.rs`.
- Allocation tables by (version, bitrate per channel, sample rate) per the standard; scfsi; grouped samples (3/5/9 levels); requantization; joint stereo bound; CRC.
- [ ] **Step 1: tests:** FFmpeg `mp2` fixtures (mono/stereo/joint stereo, 48k 192k, 44.1k 128k, MPEG-2 LSF 24k 64k) vs `mp2float` reference within the ISO tolerance; hand-assembled frame for grouped-sample decoding.
- [ ] **Steps 2–5:** FAIL → implement → PASS + gate → commit `feat(mp3): Layer II decoding`.

### Task 5: Layer III bitstream: side info, bit reservoir, scalefactors, Huffman

**Files:** Create `crates/vynamp-decode-mp3/src/layer3/{mod.rs, sideinfo.rs, reservoir.rs, scalefactors.rs, huffman.rs}`, tables.
- Side info (MPEG-1: 2 granules; LSF: 1 granule), main_data_begin; reservoir buffer (≤ 511 bytes back, 4 KiB capacity) — insufficient reservoir (e.g. after seek/first frame) → the granule is *not decodable*: output silence for it and mark `preroll-needed` (expected after seek; NOT an error/event when within the preroll frames the demuxer requested; outside preroll → Strict Invalid{what:"mp3 reservoir"}, Resilient CorruptFrame).
- Scalefactors MPEG-1 (slen tables, scfsi) and MPEG-2 LSF (incl. intensity-stereo right channel variant); Huffman big_values (tables 0–31 with linbits), count1 (A/B), region boundaries; overrun of part2_3_length → Invalid.
- [ ] **Step 1: tests:** per-table Huffman decode round trips from hand-built bitstreams; side-info parse of fixture frames (values cross-checked against frame sizes); reservoir underflow behaviour; garbage packets never panic (fixed seed).
- [ ] **Steps 2–5:** FAIL → implement → PASS + gate → commit `feat(mp3): Layer III side info, reservoir, scalefactors and Huffman`.

### Task 6: Layer III synthesis path

**Files:** Create `crates/vynamp-decode-mp3/src/layer3/{requant.rs, stereo.rs, imdct.rs, alias.rs}`.
- Requantization (x^(4/3) table + global gain/scalefac/subblock gain), reordering of short blocks, MS stereo, intensity stereo (MPEG-1 and LSF), alias reduction (8 butterflies), IMDCT 36/12 with window types 0–3 and mixed blocks, overlap-add, frequency inversion, then the shared polyphase synthesis.
- [ ] **Step 1: tests:** IMDCT vs direct O(n²) definition (1e-5); end-to-end FFmpeg libmp3lame fixtures (CBR 128k/320k, VBR V0/V2/V5, joint stereo with MS, a forced intensity-stereo file if LAME supports it — `-joint_stereo 1` does MS only; intensity → hand-assembled or document), MPEG-2 22.05k, MPEG-2.5 8k, mono: all within the ISO tolerance vs `mp3float` reference after exact alignment.
- [ ] **Steps 2–5:** FAIL → implement → PASS + gate → commit `feat(mp3): Layer III requantization, stereo, IMDCT and synthesis`.

### Task 7: Resilience, seek correctness, gapless end-to-end

**Files:** Modify decoder/demuxer; facade registration in `crates/vynamp-decode/{Cargo.toml, src/lib.rs}` (features `mpa`, `mp3`, default on).
- Resilient: an undecodable frame (Huffman/CRC/range) → silence of its samples + `CorruptFrame`; reservoir is reset sensibly. Gapless: Reader output length for LAME fixtures == original PCM length; first sample aligned. Conflicting metadata (LAME delay vs encoder-reported) → `GaplessConflict` event; no LAME → `GaplessKind::Unknown`, only the 529 decoder delay trimmed.
- [ ] **Step 1: tests:** gapless: encode `tools/fixtures/source-sine.wav` derivatives with libmp3lame → `open_path` output frames == source frames exactly, and samples within tolerance of the source after alignment; seek-equivalence: for 10 positions, decode-after-seek == full-decode slice within tolerance; corrupt-frame Resilient/Strict; ID3/APE-wrapped files; feature combos compile.
- [ ] **Steps 2–5:** FAIL → implement → PASS + gate → commit `feat: register MPEG audio; Resilient recovery and gapless end-to-end`.

### Task 8: Corpus, conformance evaluation, docs, benchmark

**Files:** Extend `tools/fixtures/generate.ps1` (MPEG section: libmp3lame CBR/VBR/ABR at several rates and modes, mp2, plus `mp3float`/`mp2float` f32 references and the original PCM), `crates/vynamp-decode/tests/oracle_mpeg.rs`, README matrix, book (verification, performance), CHANGELOG, `docs/sources/mpeg-audio.md`.
- Conformance: evaluate whether the ISO/IEC 11172-4 / 13818-4 compliance bitstreams are available under a license permitting redistribution; if yes, commit a subset ≤ 8 MiB and set `Conformance` for the layers covered; otherwise `Oracle` (Layer II/III) and `Experimental` (Layer I if no real-world fixtures) — document.
- Benchmark: add MP3 numbers (multi-second files) to book/src/performance.md.
- [ ] **Step 1:** oracle tests FAIL (fixtures absent) → generate/commit (≤ 4 MiB) → PASS + gate + cargo doc → commit `test: MPEG audio oracle corpus, conformance evaluation, docs and benchmark`.
