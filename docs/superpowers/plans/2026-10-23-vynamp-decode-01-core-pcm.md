# vynamp-decode 01 — Núcleo + containers PCM — Implementation Plan

Renamed from `agulha` to `vynamp-decode` on 2026-10-23 (maintainer decision).

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Repositório `vynamp-decode` utilizável: núcleo (tipos, Source, erros, limites, cancelamento, registry, probe), decoder PCM e demuxers WAV/RF64/BW64/Wave64/AIFF/AIFF-C, fachada `vynamp_decode::open`, testkit próprio, docs e CI.

**Architecture:** Workspace Cargo com um crate por responsabilidade (`vynamp-decode-core`, `vynamp-decode-pcm`, `vynamp-decode-riff`, `vynamp-decode-aiff`, fachada `vynamp-decode`, `vynamp-decode-testkit` não publicado). Demuxer produz `Packet` com bytes de N frames; `Decoder` converte para `AudioBuffer` planar; a fachada `Reader` aplica trim gapless uma única vez. Sem dependências de terceiros.

**Tech Stack:** Rust 1.99.0 (rust-toolchain.toml), edition 2024, resolver 3, somente `std`. Woodpecker CI (agentes `vynamp-linux` e `vynamp-windows` já existentes).

**Spec:** `docs/superpowers/specs/2026-10-23-vynamp-decode-design.md`

**Planos seguintes da Camada 1 (escritos depois deste, sobre as interfaces aqui fixadas):** 02 FLAC · 03 MPEG áudio (MP1/2/3 + Xing/LAME/VBRI) · 04 Metadados (ID3v1/v2, Vorbis comment, RIFF INFO, capa) + CUE · 05 Integração `vynamp-decode` → `vynamp-decode`.

## Global Constraints

- Somente `std`: nenhum `[dependencies]` nem `[dev-dependencies]` de crates.io em qualquer crate do workspace. (Exceção futura: `fuzz/` fora do workspace.)
- `#![forbid(unsafe_code)]` em todos os crates.
- Nenhum panic com entrada externa: todo parse retorna `Result`; índices verificados; aritmética com `checked_*`/`saturating_*` onde vem de cabeçalho.
- Licença `MIT OR Apache-2.0` em todos os `Cargo.toml`; `publish = false` só no testkit.
- Não copiar código de Symphonia/FFmpeg/SDKs; fontes consultadas registradas em `docs/sources/<formato>.md`.
- Decoders nunca aplicam trim gapless; só `Reader` (fachada) aplica, uma vez.
- Modo `Strict` é o padrão: dado truncado/corrompido = erro, nunca áudio parcial apresentado como completo.
- Commits terminam com `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`; branches `feature/<slug>` a partir de `develop`; PR para `develop`; merge pelo mantenedor.
- Gate local antes de cada commit: `cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked`.

## Review Focus

1. WAV gravado ao vivo/stream com `data` size 0 ou 0xFFFFFFFF em RIFF comum → Strict: `Invalid`; Resilient: lê até EOF com `frames = None`. (Task 5)
2. `data` declarado maior que o arquivo (truncado) → Strict: erro `Invalid` ao atingir EOF, sem retornar o último bloco parcial como sucesso. (Task 5, Task 7)
3. Chunk com tamanho ímpar (pad byte) e chunks desconhecidos entre `fmt` e `data`, ou `data` antes de `fmt` → parse correto / `Invalid` claro. (Task 5)
4. Cabeçalho com canais 0, taxa 0, block_align inconsistente com bits×canais, ou multiplicação que estoura u64 → `Invalid`/`Limit`, sem panic. (Tasks 4, 5, 6)
5. Seek para frame ≥ total ou no meio de um bloco → clamp/erro definido, posição exata ao frame depois. (Task 7)

---

### Task 1: Esqueleto do repositório, testkit e CI

**Files:**
- Create: `Cargo.toml` (workspace: members `crates/*`; `[workspace.package]` edition 2024, rust-version "1.99", license "MIT OR Apache-2.0", repository "https://github.com/jimmykedlyn/vynamp-decode"; `[workspace.lints.rust] unsafe_code = "forbid"`)
- Create: `rust-toolchain.toml` (igual ao Vynamp: channel 1.99.0, profile minimal, rustfmt+clippy), `.gitattributes` (`* text=auto eol=lf`, `*.wav *.aif *.aiff *.w64 *.bin binary`), `.gitignore` (`/target`)
- Create: `LICENSE-MIT`, `LICENSE-APACHE`, `README.md` (o que é, status "pré-alpha, privado", tabela de suporte gerada depois), `CONTRIBUTING.md` (regras globais acima, clean-room, TDD), `SECURITY.md`, `LEGAL.md` (política de features `encumbered`/`reverse-engineered`/`crates-copyleft` do spec §9.7–9.8), `CHANGELOG.md`, `docs/sources/README.md`
- Create: `crates/vynamp-decode-testkit/` (`publish = false`): `src/lib.rs`, `src/sha256.rs`, `src/signal.rs`, `src/alloc.rs`
- Create: `.woodpecker/linux.yaml`, `.woodpecker/windows.yaml`, `.woodpecker/gate.yaml` (copiar a estrutura do Vynamp: labels `host: vynamp-linux` / `vynamp-windows`, imagem `docker.io/library/rust:1.99.0-bookworm@sha256:114c7a4425406451c2866b6aafe69fe29b1b298832db1277d411ac73c82d04d6`, eventos push/pull_request/manual; passos = gate local; `gate.yaml` com `depends_on: [linux, windows]`, sem `runs_on`)

**Interfaces:**
- Produces: `vynamp_decode_testkit::sha256(data: &[u8]) -> [u8; 32]`, `vynamp_decode_testkit::hex(bytes: &[u8]) -> String`, `vynamp_decode_testkit::signal::sine_i32(rate: u32, freq: f64, frames: usize, bits: u8) -> Vec<i32>` (amplitude 0.5 FS, arredondamento para inteiro), `signal::sine_f64(rate, freq, frames) -> Vec<f64>`, `vynamp_decode_testkit::alloc::{CountingAlloc, allocations_on_this_thread() -> u64}` (contador thread-local `const`-inicializado; o alocador global só é instalado por testes que declaram `#[global_allocator]`).

- [ ] **Step 1: testes do testkit** em `crates/vynamp-decode-testkit/src/sha256.rs`:
```rust
#[test] fn sha256_empty() { assert_eq!(hex(&sha256(b"")), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"); }
#[test] fn sha256_abc() { assert_eq!(hex(&sha256(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"); }
#[test] fn sha256_two_blocks() { assert_eq!(hex(&sha256(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")), "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"); }
```
e em `signal.rs`: `sine_i32(48000, 1000.0, 48, 16)[0] == 0` e `max == 16384` (±1).
- [ ] **Step 2:** `cargo test -p vynamp-decode-testkit` → FAIL (não compila).
- [ ] **Step 3:** implementar SHA-256 (FIPS 180-4), gerador de seno, contador de alocação.
- [ ] **Step 4:** `cargo test -p vynamp-decode-testkit` → PASS; gate local completo → PASS.
- [ ] **Step 5:** commit `chore: workspace skeleton, testkit, docs and Woodpecker CI`.
- [ ] **Step 6 (operacional):** ativar o repositório `jimmykedlyn/vynamp-decode` no servidor Woodpecker local (mesmo procedimento usado no Vynamp; scripts em `vynamp repo infra/ci/woodpecker`). Se exigir login OAuth no navegador, registrar o passo para o mantenedor e seguir; o CI Gate é exigido antes de declarar PR pronto.

### Task 2: Tipos de erro, limites e cancelamento (`vynamp-decode-core`)

**Files:** Create `crates/vynamp-decode-core/{Cargo.toml, src/lib.rs, src/error.rs, src/limits.rs, src/cancel.rs}`

**Interfaces:**
- Produces:
```rust
pub type Result<T> = core::result::Result<T, Error>;
#[non_exhaustive] #[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error { Io(std::io::ErrorKind), Invalid { what: &'static str, offset: u64 }, Unsupported(&'static str), Limit(LimitKind), Cancelled }
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum LimitKind { Bytes, Frames, Channels, SampleRate, Metadata, Depth }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits { pub max_bytes: u64, pub max_frames: u64, pub max_channels: u16, pub max_sample_rate: u32, pub max_metadata_bytes: u32, pub max_depth: u8 }
// Default: max_bytes = 64 GiB, max_frames = 66_355_200_000 (24 h a 768 kHz), max_channels = 32, max_sample_rate = 768_000, max_metadata_bytes = 16 MiB, max_depth = 16
impl Limits { pub fn check_channels(&self, n: u16) -> Result<()>; pub fn check_sample_rate(&self, r: u32) -> Result<()>; pub fn check_frames(&self, f: u64) -> Result<()>; pub fn check_bytes(&self, b: u64) -> Result<()> }
pub struct CancelSource { /* Arc<AtomicU64> */ }
impl CancelSource { pub fn new() -> Self; pub fn token(&self) -> CancelToken; pub fn cancel(&self) }
#[derive(Clone)] pub struct CancelToken { /* Arc<AtomicU64>, expected: u64 */ }
impl CancelToken { pub fn never() -> Self; pub fn check(&self) -> Result<()> } // Acquire load; diferente do esperado → Err(Cancelled)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)] pub enum Mode { #[default] Strict, Resilient }
#[derive(Clone)] pub struct Options { pub limits: Limits, pub cancel: CancelToken, pub mode: Mode } // Default: Limits::default(), CancelToken::never(), Strict
```
- [ ] **Step 1: testes** em `src/limits.rs`/`src/cancel.rs`: `check_channels(0)` → `Invalid{what:"channels",..}`; `check_channels(33)` → `Limit(Channels)`; `check_sample_rate(0)` → `Invalid`; `check_sample_rate(768_001)` → `Limit(SampleRate)`; token após `cancel()` → `Err(Cancelled)`; token criado depois do `cancel()` → `Ok`; `CancelToken::never().check()` → `Ok`.
- [ ] **Step 2:** rodar → FAIL. **Step 3:** implementar. **Step 4:** rodar → PASS + gate. **Step 5:** commit `feat(core): errors, limits, cancellation and options`.

### Task 3: Source e leitor de bytes limitado (`vynamp-decode-core`)

**Files:** Create `crates/vynamp-decode-core/src/source.rs`, `src/bytes.rs`

**Interfaces:**
- Produces:
```rust
pub trait Source { fn read(&mut self, buf: &mut [u8]) -> Result<usize>; fn seek(&mut self, pos: u64) -> Result<()>; fn position(&self) -> u64; fn len(&self) -> Option<u64>; }
pub struct SliceSource<'a>; impl<'a> SliceSource<'a> { pub fn new(data: &'a [u8]) -> Self }
pub struct VecSource; impl VecSource { pub fn new(data: Vec<u8>) -> Self }
pub struct FileSource; impl FileSource { pub fn open(path: &std::path::Path) -> Result<Self> } // BufReader interno 64 KiB
pub struct ByteReader<'s> { /* &'s mut dyn Source */ }
impl<'s> ByteReader<'s> {
  pub fn new(src: &'s mut dyn Source) -> Self;
  pub fn read_exact(&mut self, buf: &mut [u8]) -> Result<()>;   // EOF antes de completar → Invalid{what:"unexpected eof", offset}
  pub fn skip(&mut self, n: u64) -> Result<()>;
  pub fn u8(&mut self) -> Result<u8>; pub fn u16_le(&mut self) -> Result<u16>; pub fn u16_be(..); pub fn u32_le(..); pub fn u32_be(..); pub fn u64_le(..); pub fn u64_be(..);
  pub fn fourcc(&mut self) -> Result<[u8; 4]>; pub fn position(&self) -> u64;
}
```
- [ ] **Step 1: testes** (`tests/source.rs` do crate): leitura LE/BE de `[0x01,0x02,0x03,0x04]` → `u32_le == 0x04030201`, `u32_be == 0x01020304`; `read_exact` além do fim → `Invalid{what:"unexpected eof", offset: 2}` com fonte de 2 bytes; `seek` além do fim seguido de `u8` → `Invalid`; `FileSource` em arquivo temporário (`std::env::temp_dir()`) lê o mesmo que `SliceSource`.
- [ ] **Steps 2–5:** FAIL → implementar → PASS + gate → commit `feat(core): sources and bounded byte reader`.

### Task 4: Buffer de áudio, track, packet e traits (`vynamp-decode-core`)

**Files:** Create `crates/vynamp-decode-core/src/{sample.rs, buffer.rs, track.rs, packet.rs, traits.rs}`

**Interfaces:**
- Produces:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum SampleFormat { U8, I16, I24, I32, F32, F64 } // I24 guardado em i32 com extensão de sinal
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum Endian { Little, Big }
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum PcmEncoding { Signed, Unsigned, Float }
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub struct PcmSpec { pub encoding: PcmEncoding, pub bits: u8 /*container: 8,16,24,32,64*/, pub valid_bits: u8, pub endian: Endian }
#[non_exhaustive] #[derive(Debug, Clone, PartialEq, Eq)] pub enum CodecId { Pcm(PcmSpec), Flac, Mp1, Mp2, Mp3 }
#[derive(Debug, Clone, PartialEq, Eq)] pub struct Track { pub id: u32, pub codec: CodecId, pub sample_rate: u32, pub channels: u16, pub channel_mask: u32, pub frames: Option<u64>, pub delay: u32, pub padding: u32, pub codec_private: Vec<u8> }
#[derive(Debug, Default)] pub struct Packet { pub track: u32, pub pts: u64, pub frames: u32, pub data: Vec<u8> }
pub struct AudioBuffer { /* format, channels, frames, capacity, data enum U8/I16/I32/F32/F64 em Vec, planar: canal c ocupa [c*capacity, c*capacity+frames) */ }
impl AudioBuffer {
  pub fn new() -> Self;                                         // vazio, sem alocação
  pub fn reset(&mut self, format: SampleFormat, channels: u16, frames: usize); // realoca só se capacidade insuficiente ou formato muda
  pub fn format(&self) -> SampleFormat; pub fn channels(&self) -> u16; pub fn frames(&self) -> usize;
  pub fn plane_u8(&self, c: usize) -> Option<&[u8]>; pub fn plane_i16(..) -> Option<&[i16]>; pub fn plane_i32(..) -> Option<&[i32]> /*I24 e I32*/; pub fn plane_f32(..); pub fn plane_f64(..);
  pub fn plane_u8_mut(&mut self, c: usize) -> Option<&mut [u8]>; /* idem para os demais */
  pub fn truncate_front(&mut self, n: usize); pub fn truncate_back(&mut self, n: usize); // usados pelo trim gapless
  pub fn to_f32_interleaved(&self, out: &mut Vec<f32>); // U8: (x-128)/128; I16: /32768; I24: /8388608; I32: /2147483648; F64: as f32
}
pub trait Demuxer { fn tracks(&self) -> &[Track]; fn next_packet(&mut self, out: &mut Packet) -> Result<bool>; fn seek(&mut self, track: u32, frame: u64) -> Result<SeekTo>; }
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub struct SeekTo { pub frame: u64, pub preroll: u64 } // frame = início do próximo pacote; preroll = frames a descartar após decodificar
pub trait Decoder { fn decode(&mut self, packet: &Packet, out: &mut AudioBuffer) -> Result<()>; fn reset(&mut self); }
```
- [ ] **Step 1: testes** (`src/buffer.rs`): `reset(I16, 2, 4)` então escrever planos e conferir layout planar; segundo `reset` com menos frames não muda o ponteiro do Vec (`as_ptr` igual) — sem realocação; `truncate_front(1)` em [1,2,3,4] → [2,3,4] por canal; `to_f32_interleaved` de I24 `[-8388608, 8388607]` → `[-1.0, 0.99999988]` (tolerância 1e-7); U8 `[0,128,255]` → `[-1.0, 0.0, 0.9921875]`.
- [ ] **Steps 2–5:** FAIL → implementar → PASS + gate → commit `feat(core): audio buffer, track, packet and codec traits`.

### Task 5: Decoder PCM (`vynamp-decode-pcm`)

**Files:** Create `crates/vynamp-decode-pcm/{Cargo.toml, src/lib.rs}`

**Interfaces:**
- Consumes: Task 4.
- Produces: `pub struct PcmDecoder; impl PcmDecoder { pub fn new(track: &Track) -> Result<Self> }` + `impl Decoder`. Mapeamento: Unsigned 8 → U8; Signed 8 → U8 (`x ^ 0x80`, sem perda); Signed 16 → I16; Signed 24 → I24 (extensão de sinal); Signed 32 → I32; Float 32 → F32; Float 64 → F64; demais combinações → `Unsupported("pcm spec")`. `packet.data.len()` deve ser `frames * channels * bits/8`, senão `Invalid{what:"pcm packet size"}`. `valid_bits` não altera samples.

- [ ] **Step 1: testes** (`tests/pcm.rs`): para cada spec (u8, s8, s16 LE/BE, s24 LE/BE, s32 LE/BE, f32 LE/BE, f64 LE/BE) codificar à mão 2 canais × 3 frames com valores-limite (mín, 0, máx; float −1.0, 0.0, 0.5) e conferir planos exatos; pacote com tamanho errado → `Invalid`; `Signed 12` → `Unsupported`; teste de alocação: após primeiro `decode` (aquecimento), 100 decodificações do mesmo tamanho fazem 0 alocações (`vynamp_decode_testkit::alloc`).
- [ ] **Steps 2–5:** FAIL → implementar → PASS + gate → commit `feat(pcm): integer and float PCM decoder`.

### Task 6: Demuxer RIFF — WAV, RF64/BW64, Wave64 (`vynamp-decode-riff`)

**Files:** Create `crates/vynamp-decode-riff/{Cargo.toml, src/lib.rs, src/wav.rs, src/w64.rs, src/fmt.rs}`, `crates/vynamp-decode-testkit/src/writers.rs`, `docs/sources/riff.md`

**Interfaces:**
- Consumes: Tasks 2–4.
- Produces: `pub fn open_riff(src: Box<dyn Source>, opts: &Options) -> Result<RiffDemuxer>` (detecta RIFF/RF64/BW64/Wave64), `pub fn probe_riff(header: &[u8]) -> u8` (100 se assinatura completa, 0 senão), `impl Demuxer for RiffDemuxer` (1 track, id 0). Pacotes de até 4096 frames. `fmt`: tag 1 (PCM int), 3 (IEEE float), 0xFFFE extensible com SubFormat GUID PCM `00000001-0000-0010-8000-00aa00389b71` ou float `00000003-…`; demais → `Unsupported`. WAV 8 bits = Unsigned; demais = Signed; `channel_mask` do extensible. RF64/BW64: tamanhos de `ds64` quando o campo de 32 bits é 0xFFFFFFFF. Wave64: chunks por GUID (`riff` `66666972-912E-11CF-A5D6-28DB04C10000`, `wave` `65766177-ACF3-11D3-8CD1-00C04F8EDB8A`, `fmt ` `20746D66-ACF3-11D3-8CD1-00C04F8EDB8A`, `data` `61746164-ACF3-11D3-8CD1-00C04F8EDB8A`), tamanho u64 inclui cabeçalho de 24 bytes, alinhamento 8. Chunks desconhecidos pulados respeitando pad. `block_align` deve ser `channels * bits/8`, senão `Invalid`. Seek: exato, `preroll = 0`, frame ≥ total → `SeekTo{frame: total, preroll: 0}` e próximo `next_packet` → `false`.
- Produces (testkit): `writers::wav(spec: PcmSpec, rate: u32, channels: u16, interleaved_bytes: &[u8], variant: WavVariant) -> Vec<u8>` com `enum WavVariant { Riff, Extensible, Rf64, Bw64, Wave64, RiffStreaming /*data size 0xFFFFFFFF*/ }`.

- [ ] **Step 1: testes** (`tests/riff.rs`), todos com dados do `writers::wav` + `signal::sine_i32`: round-trip bit a bit para cada variante × {u8, s16, s24, s32, f32, f64} × {1, 2, 6 canais}; chunk desconhecido de tamanho ímpar antes de `data` → round-trip OK; `data` antes de `fmt` → `Invalid{what:"data before fmt"}`; channels 0 → `Invalid`; block_align errado → `Invalid`; `RiffStreaming` em Strict → `Invalid{what:"unknown data size"}`; em Resilient → todos os frames, `track.frames == None`; `data` declarado 1000 frames com 400 presentes → Strict: pacotes até o fim e depois `Err(Invalid{what:"unexpected eof"})` (nenhum `Ok(true)` com pacote parcial); `max_channels = 2` com arquivo de 6 canais → `Limit(Channels)`; cancelamento entre pacotes → `Cancelled`; seek para frame 1000 de 4800 → próximo pacote `pts == 1000` com samples idênticos ao original a partir de 1000.
- [ ] **Steps 2–5:** FAIL → implementar → PASS + gate → commit `feat(riff): WAV, RF64/BW64 and Wave64 demuxer`.

### Task 7: Demuxer AIFF / AIFF-C (`vynamp-decode-aiff`)

**Files:** Create `crates/vynamp-decode-aiff/{Cargo.toml, src/lib.rs, src/extended.rs}`, extend `crates/vynamp-decode-testkit/src/writers.rs`, `docs/sources/aiff.md`

**Interfaces:**
- Produces: `pub fn open_aiff(src: Box<dyn Source>, opts: &Options) -> Result<AiffDemuxer>`, `pub fn probe_aiff(header: &[u8]) -> u8`, `impl Demuxer`. `COMM` com taxa em IEEE 754 80-bit (`extended::to_u32(bytes: [u8;10]) -> Result<u32>`; não inteiro, zero, NaN/inf → `Invalid`). AIFF: Signed BE. AIFF-C compressionType: `NONE`/`twos` (Signed BE), `sowt` (Signed LE), `fl32`/`FL32` (Float 32 BE), `fl64`/`FL64` (Float 64 BE), `raw ` (Unsigned 8), `in24`/`in32` (Signed BE), `23ni`/`42ni` (Signed LE); outros (incl. `ulaw`/`alaw`) → `Unsupported`. `SSND`: respeitar `offset`; bits não múltiplos de 8 arredondados para cima no container, `valid_bits` = bits declarados. Pacotes até 4096 frames; seek exato.
- Produces (testkit): `writers::aiff(spec: PcmSpec, rate: u32, channels: u16, interleaved_bytes: &[u8], compression: Option<[u8;4]>) -> Vec<u8>`.

- [ ] **Step 1: testes** (`tests/aiff.rs`): `extended::to_u32` para 44100, 48000, 96000, 192000, 8000, 768000 (bytes canônicos escritos no teste) e taxa 0 → `Invalid`; round-trip AIFF s8/s16/s24/s32 e cada compressionType listado; `SSND` com offset 16 → samples corretos; `ulaw` → `Unsupported`; `COMM` ausente → `Invalid`; truncado em Strict → erro como na Task 6; seek exato.
- [ ] **Steps 2–5:** FAIL → implementar → PASS + gate → commit `feat(aiff): AIFF and AIFF-C demuxer`.

### Task 8: Registry, probe e fachada `vynamp-decode` com `Reader`

**Files:** Create `crates/vynamp-decode-core/src/registry.rs`, `crates/vynamp-decode/{Cargo.toml, src/lib.rs, src/reader.rs}`, `crates/vynamp-decode/tests/facade.rs`, `crates/vynamp-decode/examples/decode_to_raw.rs`

**Interfaces:**
- Consumes: Tasks 2–7.
- Produces:
```rust
// vynamp-decode-core
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum Verification { Conformance, Oracle, Experimental }
pub struct ContainerDescriptor { pub name: &'static str, pub extensions: &'static [&'static str], pub probe: fn(&[u8]) -> u8, pub open: fn(Box<dyn Source>, &Options) -> Result<Box<dyn Demuxer>> }
pub struct CodecDescriptor { pub name: &'static str, pub verification: Verification, pub supports: fn(&CodecId) -> bool, pub make: fn(&Track, &Options) -> Result<Box<dyn Decoder>> }
pub struct Registry { /* Vec<&'static ContainerDescriptor>, Vec<&'static CodecDescriptor> */ }
impl Registry { pub fn new() -> Self; pub fn add_container(&mut self, d: &'static ContainerDescriptor); pub fn add_codec(&mut self, d: &'static CodecDescriptor); pub fn probe(&self, src: &mut dyn Source, hint_ext: Option<&str>) -> Result<&'static ContainerDescriptor>; pub fn codec_for(&self, id: &CodecId) -> Option<&'static CodecDescriptor>; pub fn containers(&self) -> impl Iterator<Item=&'static ContainerDescriptor>; pub fn codecs(&self) -> impl Iterator<Item=&'static CodecDescriptor> }
// probe: lê até 4096 bytes do início, escolhe maior score > 0 (empate → extensão casando vence), volta a fonte à posição 0; nenhum → Unsupported("container")
// vynamp-decode (fachada) — features: "riff", "aiff", "pcm" (todas default)
pub fn default_registry() -> Registry;
pub fn open(src: Box<dyn Source>, opts: Options) -> Result<Reader>;
pub fn open_path(path: &std::path::Path, opts: Options) -> Result<Reader>; // usa extensão como hint
pub struct Reader { /* demuxer, decoder da track 0, trim pendente */ }
impl Reader { pub fn track(&self) -> &Track; pub fn read(&mut self, out: &mut AudioBuffer) -> Result<bool>; pub fn seek(&mut self, frame: u64) -> Result<()>; }
// read: decodifica próximo pacote; descarta `delay` frames iniciais e `padding` finais (se `frames` conhecido) uma única vez; descarta `preroll` após seek; nunca devolve buffer com 0 frames e Ok(true); verifica cancel antes de cada pacote
```
- [ ] **Step 1: testes** (`crates/vynamp-decode/tests/facade.rs`): `open` de WAV e AIFF gerados pelo testkit decodifica todos os frames bit a bit; `open` de bytes aleatórios → `Unsupported("container")`; Track sintética com `delay = 100, padding = 50` (via Demuxer de teste definido no próprio teste e `Registry` customizada) → `Reader` entrega `frames - 150` frames, primeiro sample = sample 100 da fonte; seek para 1000 → primeiro sample lido = sample 1000; probe com empate resolvido pela extensão; `examples/decode_to_raw.rs` compila (`cargo build --examples`).
- [ ] **Steps 2–5:** FAIL → implementar → PASS + gate → commit `feat: registry, probe and vynamp-decode facade with gapless-aware Reader`.

### Task 9: Corpus de oráculo externo, matriz de suporte e documentação

**Files:** Create `tools/fixtures/generate.ps1` (dev-only; usa `ffmpeg` do PATH para gerar arquivos reais de terceiros a partir de `tools/fixtures/source-sine.wav` gerado pelo testkit), `tests/fixtures/pcm/*.{wav,w64,aif,aifc}`, `tests/fixtures/pcm/MANIFEST.txt` (linhas `arquivo|sha256|rate|channels|spec|frames|gerador`), `crates/vynamp-decode/tests/oracle_pcm.rs`, `crates/vynamp-decode/examples/support_matrix.rs`, `book/` (mdBook sem dependências: `book.toml`, `src/SUMMARY.md`, `src/architecture.md`, `src/usage.md`, `src/adding-a-codec.md`, `src/verification.md`), atualizar `README.md`, `CHANGELOG.md`

**Interfaces:**
- Consumes: Task 8 (`default_registry`, `open`).
- Produces: `examples/support_matrix.rs` imprime tabela Markdown (container/codec, verificação) a partir do registry; README inclui a saída.

- [ ] **Step 1: teste** `oracle_pcm.rs`: para cada linha do MANIFEST, confere sha256 do arquivo (testkit) e que `vynamp_decode::open_path` decodifica `frames` frames bit a bit iguais à fonte (`source-sine.wav` decodificada pela própria vynamp-decode, que já é provada pelos round-trips sintéticos). Variantes geradas: `pcm_u8`, `pcm_s16le`, `pcm_s24le`, `pcm_s32le`, `pcm_f32le`, `pcm_f64le` em WAV, WAV extensible 6 canais, RF64 (`-rf64 always`), Wave64 (`-f w64`), AIFF s16/s24, AIFF-C `sowt` e `fl32`.
- [ ] **Step 2:** rodar → FAIL (fixtures ausentes). **Step 3:** rodar `tools/fixtures/generate.ps1`, commitar fixtures (< 2 MiB no total), escrever docs. **Step 4:** PASS + gate + `cargo doc --workspace --no-deps` sem warnings (`RUSTDOCFLAGS=-D warnings`). **Step 5:** commit `test: third-party PCM oracle corpus, support matrix and book`.
- [ ] **Step 6:** push da branch, PR para `develop` com template, revisão independente, CI Gate Woodpecker verde → mantenedor faz merge.
