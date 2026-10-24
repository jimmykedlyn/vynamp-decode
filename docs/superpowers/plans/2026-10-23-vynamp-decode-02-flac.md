# vynamp-decode 02 — FLAC + diagnostics API — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Native FLAC (RFC 9639) demuxer + decoder, bit-exact, with seek, MD5 verification and Resilient recovery; plus the diagnostics/events API deferred from plan 01.

**Architecture:** Core gains `Event`s (drained from demuxer/decoder by `Reader`), `Packet.offset`, `Track.gapless`, a public ID3v2 skipper, an MSB-first `BitReader`, CRC-8/CRC-16 and MD5. New crate `vynamp-decode-flac` holds the demuxer (metadata blocks, frame sync, seek) and decoder (subframes, residuals, decorrelation). Facade registers it behind feature `flac` (default).

**Tech Stack:** Rust 1.99.0, std only. FFmpeg (`$env:FFMPEG`, dev only) for fixtures.

**Spec:** `docs/superpowers/specs/2026-10-23-vynamp-decode-design.md` (§5.2 modes/events, §5.5 verification, §9.1 levels, §9.6 gapless). Builds on plan 01 (`docs/superpowers/plans/2026-10-23-vynamp-decode-01-core-pcm.md`).

## Global Constraints

- Std only; no crates.io deps (dev included). `unsafe` forbidden (workspace lint).
- No panic on external input; checked arithmetic on header-derived values; check `Limits` before sizing any buffer.
- Strict (default): corrupt/truncated data → error, never partial audio as complete. Resilient: recover, and report every recovery as an `Event`.
- Gapless trim only in `Reader`; decoders/demuxers never trim.
- Clean-room: implement from RFC 9639 (and the FLAC format docs it supersedes); no Symphonia/FFmpeg/libFLAC code read. Sources in `docs/sources/flac.md`.
- `Invalid.offset` = absolute source byte position (decoders use `packet.offset` + position within the packet when known).
- Commits end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Gate before every commit: `cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked`.

## Rulings carried from plan 01 final review

- Public structs stay exhaustive (pre-1.0 semver; `#[non_exhaustive]` would block struct literals in sibling crates). Fields may be added in 0.x.
- No `Error::Eof`: end of stream is `Ok(false)`; truncation is `Invalid{what:"unexpected eof"}`. Document in `error.rs`.
- No `Track.time_base`: all positions are in frames at `sample_rate`.

## Review Focus

1. FLAC file with a leading ID3v2 tag (common from taggers) → opens and decodes identically to the untagged file. (Task 4)
2. Frame with bad CRC-16 (bit rot in the middle of a file) → Strict: `Invalid{what:"flac frame crc"}`; Resilient: that frame becomes silence of its block size, `Event::CorruptFrame` emitted, timeline continues. (Tasks 5, 8)
3. Seek on a file without SEEKTABLE, and to the last frame/past end → exact sample; no infinite scan. (Task 5)
4. 32-bit FLAC with side-channel coding (needs 33-bit intermediate) and wasted-bits → bit-exact. (Task 6)
5. File whose STREAMINFO total_samples = 0 (unknown) → decodes to EOF, `frames == None`, MD5 not verified, no error. (Tasks 4, 7)

---

### Task 1: Diagnostics API, packet offset, gapless kind (`vynamp-decode-core` + riff/aiff/pcm/facade)

**Files:** Create `crates/vynamp-decode-core/src/event.rs`; modify `traits.rs`, `packet.rs`, `track.rs`, `registry.rs`, `error.rs` (doc), riff/aiff/pcm crates, `crates/vynamp-decode/src/reader.rs`, facade tests.

**Interfaces:**
- Produces:
```rust
#[non_exhaustive] #[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    CorruptFrame { pts: u64, offset: u64, frames: u32 }, // Resilient replaced/skipped a frame
    Clamped { what: &'static str, declared: u64, actual: u64 }, // Resilient clamped a declared size
    ChecksumMismatch { what: &'static str },               // e.g. "flac md5"
    GaplessConflict { what: &'static str },                 // reserved for plan 03
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum GaplessKind { Exact, FromMetadata, Unknown }
// Track gains: pub gapless: GaplessKind   (riff/aiff/flac = Exact)
// Packet gains: pub offset: u64          (absolute byte position of the packet's first byte)
// Demuxer and Decoder gain: fn drain_events(&mut self, out: &mut Vec<Event>) {}  (default no-op)
pub fn skip_id3v2(src: &mut dyn Source, limits: &Limits) -> Result<u64>; // now pub; returns offset after tag(s); leaves src at that offset
// Reader gains: pub fn take_events(&mut self) -> Vec<Event>  (std::mem::take)
// Reader: after each packet, drains demuxer then decoder events into its Vec.
// Reader: position resync — before trimming a decoded packet, set the demuxer-timeline pos to packet.pts.
```
- riff/aiff: emit `Event::Clamped` where Resilient clamps today (RIFF unknown size bounded by container; AIFF COMM > SSND). PCM decoder errors use `offset: packet.offset`.
- [ ] **Step 1: tests:** Resilient AIFF with COMM frames > SSND → `reader.take_events() == [Clamped{what:"comm frames", declared, actual}]`; second `take_events()` → empty; Strict unchanged. Synthetic demuxer whose decoder outputs fewer frames than `packet.frames` for one packet → following packets still trimmed by `pts` (output frame indices match pts − delay). `skip_id3v2` public test: returns 0 when no tag. PCM wrong-size packet error carries `packet.offset`.
- [ ] **Steps 2–5:** FAIL → implement → PASS + gate → commit `feat(core): diagnostics events, packet offset, gapless kind and pts resync`.

### Task 2: BitReader and CRCs (`vynamp-decode-core`)

**Files:** Create `crates/vynamp-decode-core/src/bits.rs`, `src/crc.rs`.

**Interfaces:**
```rust
pub struct BitReader<'a> { /* &'a [u8], bit position */ }
impl<'a> BitReader<'a> {
  pub fn new(data: &'a [u8]) -> Self;
  pub fn read_bits(&mut self, n: u32) -> Result<u32>;      // 0..=32, MSB-first; past end → Invalid{what:"bitstream eof", offset: byte index}
  pub fn read_bits_u64(&mut self, n: u32) -> Result<u64>;  // 0..=64
  pub fn read_signed(&mut self, n: u32) -> Result<i32>;    // two's complement, 1..=32
  pub fn read_signed_i64(&mut self, n: u32) -> Result<i64>; // 1..=33 (FLAC side channel at 32 bps)
  pub fn read_unary(&mut self) -> Result<u32>;             // count of 0 bits before a 1; > 2^20 zeros → Invalid{what:"unary overflow"}
  pub fn read_rice_signed(&mut self, k: u32) -> Result<i32>; // zigzag-decoded Rice; overflow → Invalid{what:"rice overflow"}
  pub fn read_utf8_u64(&mut self) -> Result<u64>;          // FLAC "UTF-8"-coded number, up to 7 bytes / 36 bits; malformed → Invalid{what:"utf8 number"}
  pub fn align_byte(&mut self); pub fn byte_pos(&self) -> usize; pub fn bits_left(&self) -> u64;
}
pub fn crc8(data: &[u8]) -> u8;   // poly 0x07, init 0, no reflection (FLAC frame header)
pub fn crc16(data: &[u8]) -> u16; // poly 0x8005, init 0, no reflection (FLAC frame footer)
```
- [ ] **Step 1: tests:** `read_bits` across byte boundaries on `[0b1010_1100, 0b0101_0011]` (3 bits → 5, 7 bits → 0b1100010, …); `read_signed(4)` of `0b1111` → −1; `read_signed_i64(33)` of all ones → −1; unary of `0b0001_xxxx` → 3; Rice k=2 for zigzag values 0,−1,1,−2 encoded by hand; utf8 numbers 0, 0x7F, 0x80 (`0xC2 0x80`), 36-bit max (`0xFE 0x83 0xBF×5`), invalid continuation → Invalid; crc8(b"123456789") == 0xF4; crc16(b"123456789") == 0xFEE8; reading past end → Invalid, never panic.
- [ ] **Steps 2–5:** FAIL → implement → PASS + gate → commit `feat(core): MSB-first bit reader and FLAC CRCs`.

### Task 3: MD5 (`vynamp-decode-core`)

**Files:** Create `crates/vynamp-decode-core/src/md5.rs`.

**Interfaces:** `pub struct Md5; impl Md5 { pub fn new() -> Self; pub fn update(&mut self, data: &[u8]); pub fn finish(self) -> [u8; 16] }` (RFC 1321, streaming).
- [ ] **Step 1: tests:** RFC 1321 test suite: `""` → d41d8cd98f00b204e9800998ecf8427e, `"a"` → 0cc175b9c0f1b6a831c399e269772661, `"abc"` → 900150983cd24fb0d6963f7d28e17f72, `"message digest"` → f96b697d7cb7938d525a2f31aaf161d0, a–z → c3fcd3d76192e4007dfb496cca67e13b, 62-char alnum → d174ab98d277d9f5a5611c2c9f419d9f, 80 digits → 57edf4a22be3c955ac49da2e2107b67a; streaming in random chunk sizes equals one-shot.
- [ ] **Steps 2–5:** FAIL → implement → PASS + gate → commit `feat(core): streaming MD5`.

### Task 4: FLAC stream and metadata blocks (`vynamp-decode-flac`)

**Files:** Create `crates/vynamp-decode-flac/{Cargo.toml, src/lib.rs, src/metadata.rs}`, `docs/sources/flac.md`; extend testkit with `writers::flac_metadata(...)` helpers only if needed for malformed tests.

**Interfaces:**
```rust
pub fn probe_flac(header: &[u8]) -> u8; // b"fLaC" at 0 → 100
pub struct StreamInfo { pub min_block: u16, pub max_block: u16, pub min_frame: u32, pub max_frame: u32, pub sample_rate: u32, pub channels: u8, pub bits_per_sample: u8, pub total_samples: u64 /*0 = unknown*/, pub md5: [u8; 16] }
pub struct SeekPoint { pub sample: u64, pub offset: u64 /*from first frame*/, pub frames: u16 }
pub enum BlockKind { StreamInfo, Padding, Application, SeekTable, VorbisComment, CueSheet, Picture, Reserved(u8) }
pub struct RawBlock { pub kind: BlockKind, pub data: Vec<u8> } // kept for plan 04 (VorbisComment, CueSheet, Picture); Padding not stored
pub(crate) fn read_metadata(r: &mut ByteReader, limits: &Limits) -> Result<(StreamInfo, Vec<SeekPoint>, Vec<RawBlock>, u64 /*first frame offset*/)>;
```
- Open: `skip_id3v2` first, then `fLaC`; STREAMINFO must be the first block (else `Invalid{what:"streaminfo first"}`), exactly 34 bytes; sample_rate 0 → `Invalid`; channels 1–8; bps 4–32; min_block ≥ 16 (except a single-frame stream) and ≤ max_block. Total of non-padding block bytes bounded by `limits.max_metadata_bytes` → `Limit(Metadata)`; Padding skipped without reading. Seek points with sample = 0xFFFFFFFFFFFFFFFF are placeholders (ignored). Track: `codec = CodecId::Flac`, `frames = Some(total) or None`, `codec_private` = 34-byte STREAMINFO body, `gapless = Exact`, `delay = padding = 0`.
- [ ] **Step 1: tests** (`tests/metadata.rs`, using FFmpeg fixtures from Task 9's generator where convenient OR hand-built byte arrays): STREAMINFO fields parsed from a hand-built block (rate 44100, 2 ch, 16 bps, total 4417, md5 bytes); block order error; size limit; ID3v2-prefixed stream opens with same Track; `total_samples = 0` → `frames == None`; truncated metadata → `Invalid{what:"unexpected eof"}`; byte-flip/truncation smoke over first 128 bytes, no panic.
- [ ] **Steps 2–5:** FAIL → implement → PASS + gate → commit `feat(flac): stream marker and metadata blocks`.

### Task 5: FLAC frame header, packetization and seek (`vynamp-decode-flac`)

**Files:** Create `crates/vynamp-decode-flac/src/frame.rs`, `src/demux.rs`.

**Interfaces:**
```rust
pub struct FrameHeader { pub blocking_variable: bool, pub block_size: u32, pub sample_rate: u32, pub channel_assignment: ChannelAssignment, pub bits_per_sample: u8, pub first_sample: u64, pub header_len: usize }
pub enum ChannelAssignment { Independent(u8), LeftSide, RightSide, MidSide }
pub(crate) fn parse_frame_header(bytes: &[u8], info: &StreamInfo) -> Result<FrameHeader>; // validates reserved bits, CRC-8, "get from STREAMINFO" codes
pub struct FlacDemuxer; pub fn open_flac(src: Box<dyn Source>, opts: &Options) -> Result<FlacDemuxer>; impl Demuxer for FlacDemuxer;
```
- Packetization: read forward in a reusable buffer (grow up to `max_frame` from STREAMINFO when known, else up to 16 MiB hard cap → `Limit(Bytes)`); a frame starts at a sync code (`0xFFF8`/`0xFFF9`, 14 sync bits + reserved 0 + blocking bit) with a header that passes CRC-8 and is consistent with STREAMINFO; it ends where the next valid header begins (or EOF). The whole frame (header..footer) must pass CRC-16. `Packet{pts: first_sample, frames: block_size, offset, data}` (data = full frame bytes). Fixed-blocksize streams: `first_sample = frame_number × fixed block size` (use STREAMINFO min_block for fixed streams; last frame may be shorter).
- CRC-16 failure: Strict → `Invalid{what:"flac frame crc", offset}`; Resilient → emit packet with the header-derived `frames` and empty `data` flagged for the decoder to produce silence (`data.is_empty()` = silence frame), plus `Event::CorruptFrame{pts, offset, frames}`. Garbage between frames: Strict → `Invalid{what:"flac sync"}`; Resilient → resync to next valid header and emit `CorruptFrame` for the skipped samples if the next header's `first_sample` shows a gap (silence packet covering the gap, capped at `limits.max_frames`).
- Seek(frame): clamp to total; candidate byte offset from SEEKTABLE (largest point ≤ target) or else bisection over the byte range [first_frame, len) syncing to the next valid header (≤ 64 iterations); then scan forward frame by frame until the frame containing `target`; return `SeekTo{frame: frame.first_sample, preroll: target − first_sample}`. Cancel checked per scanned frame.
- [ ] **Step 1: tests** (`tests/demux.rs`): header parse of hand-built header bytes incl. utf8 frame number, 8/16-bit block-size and rate codes, CRC-8 mismatch → Invalid; packet stream from an FFmpeg fixture: pts sequence contiguous and sums to total; single-bit flip inside frame 3 → Strict `Invalid{what:"flac frame crc"}` at that frame's offset, Resilient yields silence packet + one `CorruptFrame` event and all other packets unchanged; seek to 0, middle, last frame, total−1, total (→ next_packet false) on files with and without SEEKTABLE (generate one with `-seekable 0` or strip the block in a test helper); cancel during seek scan → Cancelled; byte-flip smoke (first 4 KiB of frame data) + truncation at frame boundaries, both modes, no panic.
- [ ] **Steps 2–5:** FAIL → implement → PASS + gate → commit `feat(flac): frame sync, packetization and exact seek`.

### Task 6: FLAC subframe decoding (`vynamp-decode-flac`)

**Files:** Create `crates/vynamp-decode-flac/src/decode.rs`, `src/subframe.rs`, `src/residual.rs`.

**Interfaces:** `pub struct FlacDecoder; impl FlacDecoder { pub fn new(track: &Track) -> Result<Self> }` (parses STREAMINFO from `codec_private`) + `impl Decoder`.
- Output format: bps ≤ 8 → U8 (`x + 128`), ≤ 16 → I16, ≤ 24 → I24, ≤ 32 → I32; samples not shifted (container holds the value at its declared bps).
- Subframes: header zero bit, type (CONSTANT, VERBATIM, FIXED order 0–4, LPC order 1–32; reserved → Invalid), wasted bits (unary k → shift left by k after decode). Effective bps = frame bps − wasted + 1 for the side channel of LeftSide/RightSide/MidSide. LPC: precision 1–15 bits (0b1111 → Invalid), shift: signed 5-bit (negative → Invalid), prediction accumulated in i64, residual added in i64, result must fit effective bps else `Invalid{what:"flac sample range"}`.
- Residual: coding method 0 (4-bit Rice) / 1 (5-bit Rice); partition order; escape parameter (all ones) → raw n-bit signed samples (n = 5 bits; n = 0 → zeros); partition sample counts validated (block_size >> order must be > predictor order for partition 0) → else Invalid.
- Decorrelation in i64: LeftSide R = L − S; RightSide L = R + S; MidSide: M = (M << 1) | (S & 1), L = (M + S) >> 1, R = (M − S) >> 1.
- Silence packet (`data.is_empty()`): output `packet.frames` frames of zeros (U8 → 128).
- CRC is the demuxer's job; the decoder trusts packet framing but still never panics on garbage (bit reader errors → Invalid with `offset: packet.offset + byte_pos`).
- No allocation per packet after warm-up (scratch buffers sized from STREAMINFO max_block × channels, checked against Limits).
- [ ] **Step 1: tests** (`tests/decode.rs`): hand-assembled single frames for CONSTANT, VERBATIM, FIXED orders 0–4, LPC order 2, Rice escape partition, wasted bits; every channel assignment on a 2-channel synthetic signal; 32-bit side channel extreme values (L = i32::MIN, R = i32::MAX); zero-alloc after warm-up (testkit alloc); garbage packet data → Err, never panic (random bytes, fixed seed, 1000 iterations).
- [ ] **Steps 2–5:** FAIL → implement → PASS + gate → commit `feat(flac): subframe, residual and channel decorrelation decoding`.

### Task 7: MD5 verification (`vynamp-decode-flac`)

**Files:** Modify `crates/vynamp-decode-flac/src/decode.rs`.

- Decoder hashes every decoded sample as little-endian signed integers of `ceil(bps/8)` bytes, interleaved, per RFC 9639. Verification only when: STREAMINFO md5 ≠ all-zero, total_samples > 0, decoding started at sample 0, and no seek/reset happened (`reset()` disables it for the rest of the stream). When the decoded count reaches total_samples: mismatch → Strict: `Err(Invalid{what:"flac md5", offset: packet.offset})` from that `decode` call; Resilient: `Event::ChecksumMismatch{what:"flac md5"}`. Silence packets (Resilient recovery) disable verification.
- [ ] **Step 1: tests:** FFmpeg fixture full decode → no error/no event; same fixture with one byte of the STREAMINFO MD5 flipped → Strict error on the last packet, Resilient event; after a seek, no verification and no error; md5 all-zero → skipped.
- [ ] **Steps 2–5:** FAIL → implement → PASS + gate → commit `feat(flac): STREAMINFO MD5 verification`.

### Task 8: Facade registration, Resilient end-to-end

**Files:** Modify `crates/vynamp-decode/{Cargo.toml, src/lib.rs}`, `crates/vynamp-decode/tests/facade.rs`.

- Feature `flac` (default) registers `ContainerDescriptor{name:"flac", extensions:&["flac","fla"], verification: Oracle until Task 9 decides}` and `CodecDescriptor{name:"flac", ...}`.
- [ ] **Step 1: tests:** `open_path` on an ID3v2-prefixed FLAC fixture decodes identically to the plain one; Resilient end-to-end with a corrupted middle frame → total frames unchanged, `take_events()` contains exactly one `CorruptFrame`; `--no-default-features --features flac` builds and its tests pass.
- [ ] **Steps 2–5:** FAIL → implement → PASS + gate (incl. feature combos) → commit `feat: register FLAC in the facade`.

### Task 9: FLAC oracle and conformance corpus, docs

**Files:** Extend `tools/fixtures/generate.ps1` (FLAC section), create `tests/fixtures/flac/*` + `MANIFEST.txt`, `crates/vynamp-decode/tests/oracle_flac.rs`; optional `tests/fixtures/flac-conformance/` (see below); update README matrix, book (`src/verification.md`), CHANGELOG, `docs/sources/flac.md`.

- Oracle (lossless ⇒ the source itself is the oracle): encode `tools/fixtures/source-sine.wav`-derived PCM with FFmpeg at: bps 8, 16, 24 (and 32 with `-strict experimental` if the pinned FFmpeg supports it; otherwise document); channels 1, 2, 6; rates 44100, 96000, 192000; `-compression_level` 0, 5, 8, 12; `-lpc_type` fixed and levinson/cholesky; `-frame_size` 192, 4096, 16384; one file with `-seekable 0` (no SEEKTABLE) if supported, else built by a test helper that strips the block; one with an ID3v2 prefix (written by the test or generator). Decoded output must equal the source samples narrowed exactly as FFmpeg narrows (plan 01 rule) — zero tolerance. Total fixture size ≤ 4 MiB.
- Conformance: evaluate the IETF CELLAR `flac-test-files` repository (github.com/ietf-wg-cellar/flac-test-files). If its license permits redistribution, commit a subset ≤ 8 MiB (the "subset" directory files covering every block size/channel/bps/coding feature, plus "faulty" ones as Strict-must-error cases) with its license file and per-file expected MD5 from the repo; test that every subset file decodes and matches its STREAMINFO MD5. If all pass, set FLAC `verification = Conformance`; otherwise `Oracle` and list failures in the CHANGELOG. If the license does not permit redistribution, do not commit them; document the manual procedure in the book and keep `Oracle`.
- [ ] **Step 1: test** `oracle_flac.rs` (and `conformance_flac.rs` if committed) → FAIL (fixtures absent). **Step 2:** generate/commit fixtures, docs. **Step 3:** PASS + gate + `cargo doc` (`RUSTDOCFLAGS=-D warnings`). **Step 4:** commit `test: FLAC oracle and conformance corpus, docs`.

### Task 10: Benchmark example

**Files:** Create `crates/vynamp-decode/examples/bench_decode.rs`, book `src/performance.md`.

- `cargo run --release --example bench_decode -- <file> [iterations]`: decodes the file N times (default 5) with std `Instant`, prints min/median MB/s and × realtime. No crate dependency. Book page records the numbers for the FLAC fixtures on the dev machine (informative; spec §9.4).
- [ ] **Step 1:** implement + run on 2 fixtures. **Step 2:** gate + `cargo build --examples --release --locked`. **Step 3:** commit `docs: decode benchmark example and baseline`.
