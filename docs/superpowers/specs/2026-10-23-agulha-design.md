# agulha — biblioteca própria de decodificação de áudio (design)

Data: 2026-10-23 · Autor: Claude (dono único do Vynamp) · Status: aprovado (mantenedor delegou a revisão final a Claude, 2026-10-23)

## 1. Intenção

O Vynamp hoje decodifica com a Symphonia (crate de terceiros) e usa FFmpeg só como oráculo de teste. O mantenedor quer uma biblioteca **própria**, sem bibliotecas de terceiros, modular, documentada e pronta para ser aberta (open source) para outras pessoas. Pode se inspirar na arquitetura da Symphonia, mas deve **melhorá-la** e ser código nosso. Cobertura desejada: **o mais completa possível**, incluindo **CUE**.

"agulha" (a agulha lê o sulco e vira som) — nome escolhido por Claude; o mantenedor pode trocar antes da publicação.

## 2. Objetivos

1. Decodificar os formatos do catálogo do Vynamp (`repo/docs/audio/02-formats.md`, 68 entradas) em camadas entregáveis (§6).
2. Zero dependências de terceiros em tempo de execução: apenas `std` do Rust.
3. Melhor que a Symphonia em pontos verificáveis:
   - **Exatidão**: lossless bit-exato contra referência; lossy dentro da métrica de conformidade de cada especificação.
   - **Gapless exato** (delay/padding aplicados uma única vez, exposto na API) e **seek exato ao sample** (com pre-roll quando o codec exige).
   - **Robustez**: nenhum panic, loop infinito ou alocação desproporcional com entrada malformada; limites configuráveis; cancelamento cooperativo; fuzzing contínuo por demuxer/decoder.
   - **CUE completo** (§5.6).
   - **Uso em tempo real**: buffers reutilizáveis, sem alocação por pacote no caminho quente após o aquecimento.
4. Modular: cada container e cada codec em crate próprio; fachada `agulha` com *features*.
5. Documentação completa para terceiros (§8).
6. Repositório privado `jimmykedlyn/agulha` no GitHub, CI no Woodpecker já existente.

## 3. Não-objetivos

- Codificar (encoders) — fora do escopo.
- Reprodução/saída de áudio, resampling, DSP — pertencem ao Vynamp/`vitrola`.
- DRM ou remoção de proteção (SACD protegido, WMA DRM, etc.) — nunca.
- `no_std` na primeira versão (pode vir depois).
- Interface gráfica de qualquer tipo: é uma biblioteca.

## 4. Anti-objetivos (falha mesmo que "funcione")

- Copiar ou traduzir código da Symphonia (MPL-2.0), FFmpeg (LGPL/GPL) ou SDKs proprietários. Implementação a partir das **especificações** e documentação pública; a Symphonia é referência de arquitetura apenas. Registrar fontes consultadas por codec (`docs/sources/<codec>.md`).
- Saída silenciosamente errada: na dúvida, erro `Unsupported`/`Invalid`, nunca áudio parcial apresentado como completo (lição do spike E01-01).
- Declarar suporte a formato sem testes de conformidade/oráculo.
- Panic em entrada do usuário.

## 5. Decisões de arquitetura (de Claude, salvo onde indicado)

### 5.1 Linguagem e regras
- Rust estável, mesma toolchain do Vynamp; edition 2024.
- `#![forbid(unsafe_code)]` em todos os crates de container/codec. Desempenho via código seguro e autovetorização; `unsafe` só se um benchmark provar necessidade, isolado num crate `agulha-simd` opcional, revisado por Opus.
- Ferramentas de desenvolvimento (cargo-fuzz, criterion-like benchmarks, FFmpeg/decoders de referência para gerar fixtures) são permitidas **fora** dos crates publicados; nenhuma entra como dependência normal.

### 5.2 Camadas da API
```
Source (Read+Seek próprio) → Probe → Demuxer (container) → Packet → Decoder (codec) → AudioBuffer
                                   ↘ Metadata (tags, capa, capítulos, ReplayGain, CUE)
```
- `AudioBuffer`: planar, tipos de sample `u8/i16/i24/i32/f32/f64`, conversão explícita (nunca implícita com perda).
- `Track`: codec params, `time_base`, `frames` (se conhecido), `delay`, `padding`, layout de canais.
- Erros: `Io`, `Invalid`, `Unsupported`, `Limit`, `Cancelled`, `Eof` — sem `String` como único conteúdo; contexto estruturado.
- Modos: `Strict` (padrão; quadro ruim = erro) e `Resilient` (pula quadro corrompido e reporta via evento).
- Limites (`Limits`): bytes, frames, canais, taxa, tamanho de metadado/capa, profundidade de recursão de containers.
- Cancelamento: token de geração (padrão do spike E01-01) verificado por pacote.
- Registry de codecs/containers com capacidades declaradas (substitui o registry experimental do Vynamp).

### 5.3 Crates
`agulha-core` (tipos, Source, erros, limites, registry, conversão de samples, bit reader, CRC/MD5 próprios, IMDCT/FFT próprios), um crate por container (`agulha-riff`, `agulha-aiff`, `agulha-caf`, `agulha-au`, `agulha-flac`, `agulha-mpa`, `agulha-mp4`, `agulha-ogg`, `agulha-mkv`, `agulha-asf`, `agulha-dsd`, …), um por codec (`agulha-pcm`, `agulha-adpcm`, `agulha-mp3`, `agulha-aac`, `agulha-alac`, `agulha-vorbis`, `agulha-opus`, `agulha-wavpack`, `agulha-ape`, `agulha-tta`, `agulha-mpc`, `agulha-wma`, …), `agulha-meta` (ID3v1/v2, Vorbis comment, APE tag, MP4 atoms, RIFF INFO, capa, capítulos, ReplayGain), `agulha-cue`, e a fachada `agulha`.

### 5.4 Features e risco jurídico (decisão do mantenedor: "faça tudo")
- Todos os formatos serão implementados.
- `default`: formatos de especificação aberta ou patentes vencidas (PCM, FLAC, ALAC, MP1/2/3, AAC-LC, Vorbis, Opus, WavPack, TTA, Musepack, AC-3, DSD→PCM, …).
- Feature opcional `encumbered` (desligada por padrão) agrupa: HE-AAC v1/v2, xHE-AAC/USAC, E-AC-3, TrueHD/MLP, DTS/DTS-HD, AC-4, MPEG-H. Feature `reverse-engineered`: WMA família. APE em `default` se a revisão de licença confirmar implementação limpa; senão vai para opcional.
- `LEGAL.md` explica cada caso; não é aconselhamento jurídico; quem distribui é responsável por licenças de patente.

### 5.5 Verificação
- Corpus de fixtures com manifesto (schema/hash/origem/licença), reaproveitando o corpus do spike E01-01.
- Lossless: igualdade bit a bit contra referência. Lossy: vetores de conformidade oficiais quando existirem (ex.: conformidade MPEG-1/2 Layer III, vetores Opus RFC 6716/8251, Vorbis), senão erro máximo contra decoder de referência com tolerância pré-fixada por codec.
- Fuzzing por demuxer/decoder (alvo: horas acumuladas sem crash antes de cada release).
- Benchmarks contra Symphonia (dev-only) por formato: meta = não mais lento que a Symphonia; regressões > 10% bloqueiam.

### 5.6 CUE (`agulha-cue`)
- Parser de CUE externo e embutido (FLAC CUESHEET block, Vorbis comment `CUESHEET`, APE tag `Cuesheet`).
- `FILE` múltiplos, `TRACK`, `INDEX 00/01+`, `PREGAP`/`POSTGAP`, `REM` (GENRE, DATE, REPLAYGAIN_*, COMMENT, DISCID), `TITLE/PERFORMER/SONGWRITER/ISRC/CATALOG/FLAGS`.
- Codificação: BOM UTF-8/UTF-16, heurística UTF-8 válido, fallback Windows-1252 e Shift-JIS (tabelas próprias).
- Faixas virtuais com limites exatos ao sample (75 frames CD/s → samples na taxa do arquivo), gapless entre faixas virtuais, seek dentro da faixa virtual.
- Política de pregap (oculto / anexado à faixa anterior / anexado à seguinte) configurável; padrão: INDEX 01 como início da faixa.

### 5.7 Integração com o Vynamp
- O crate `vynamp-decode` passa a usar `agulha` no lugar da Symphonia, preservando a API interna e os testes do spike E01-01 (PR #18) como regressão.
- Enquanto a Camada 1 não estiver pronta, PR #18 segue como está (spike); a troca é uma TASK própria.

## 6. Camadas de entrega
1. **Base**: core, WAV/RF64/Wave64, AIFF/AIFF-C, FLAC (nativo), MP1/MP2/MP3 (+Xing/LAME/VBRI), metadados ID3/Vorbis/RIFF, **CUE**. → troca da Symphonia no Vynamp.
2. **Diário**: MP4/M4A/M4B (AAC-LC, ALAC, capítulos), Ogg (Vorbis, Opus, FLAC), CAF, AU, ADPCM (IMA/MS), G.711, PCM raw.
3. **Audiófilo/legado**: WavPack (lossless/hybrid), APE, TTA, Musepack SV7/SV8, DSF/DFF→PCM, Matroska/WebM, MPEG-TS/LATM, WMA (feature), HE-AAC (feature).
4. **Especiais**: AC-3, E-AC-3/TrueHD/DTS/AC-4/MPEG-H/IAMF (feature), MIDI+SoundFont, tracker (MOD/XM/IT/S3M), TAK, OptimFROG, AMR, Speex, ATRAC, DST, SACD ISO sem proteção.

Cada camada fecha com: conformidade + fuzz + benchmark + docs atualizadas + release interna (tag pelo mantenedor).

## 7. Restrições
- Repositório privado; abertura pública é decisão futura do mantenedor.
- Licença: MIT OR Apache-2.0 (decisão de Claude, padrão Rust; trocável antes de abrir).
- Mesmas regras de entrega do Vynamp: PR por TASK, revisão por subagente de contexto limpo, CI Gate Woodpecker, merge pelo mantenedor.
- Windows, Linux, macOS (não verificado sem agente Mac), Android: código portátil, sem APIs de SO.

## 8. Documentação
README, guia (mdBook em `book/`): arquitetura, uso, como adicionar codec; rustdoc completo com exemplos testados; matriz de suporte gerada a partir do registry; `CONTRIBUTING.md`, `SECURITY.md`, `LEGAL.md`, `CHANGELOG.md`, `docs/sources/` (especificações consultadas por formato), ADRs.

## 9. Decisões complementares (de Claude, após revisão do construtor)

1. **Níveis de verificação** na matriz de suporte: `conformance` (vetores oficiais), `oracle` (saída de decoder de referência gerada offline e versionada como fixture, com tolerância pré-fixada) e `experimental`. Formatos sem vetores públicos são aceitos com `oracle`; nunca sem nenhum dos dois.
2. **Zero terceiros**: vale para os crates publicados (só `std`) e para dev-dependencies (harness de teste, gerador de propriedades, benchmark e contador de alocação próprios). Exceção única: diretório `fuzz/` fora do workspace publicado pode usar `cargo-fuzz`/`libfuzzer-sys`. MD5, CRC, FFT/IMDCT, tabelas de codificação: código próprio.
3. **API**: a `agulha` define sua própria API (ADR-001 do repositório); o `vynamp-decode` vira adaptador. Os testes do spike E01-01 viram testes de regressão do adaptador.
4. **Desempenho**: comparação com a Symphonia é informativa na Camada 1 (medida localmente, não no CI); gate duro = regressão > 10% contra a baseline da própria `agulha` na mesma máquina.
5. **Catálogo**: a primeira TASK cria a tabela entrada-do-catálogo → camada/crate/feature/nível; nenhuma das 68 entradas fica sem destino.
6. **Gapless/seek exato**: "exato" = saída do decoder de referência com delay/padding oficiais removidos. Precedência de metadados: edit list MP4 > iTunSMPB > LAME/Xing > VBRI; conflito é reportado como evento; sem metadado confiável → sem trim e flag `gapless_unknown`.
7. **Codecs de risco** (`encumbered`, `reverse-engineered`): construídos agora no repositório privado (decisão do mantenedor: "faça tudo"); avaliação jurídica é pré-requisito apenas para abrir o código.
8. **Formatos sem especificação pública** (TAK, OptimFROG, WMA, ATRAC, DST, partes de DTS-HD/APE) — recomendação de Claude aceita pelo mantenedor: construídos por último. Fonte permissiva (BSD/MIT/domínio público, ex.: SDK atual do Monkey's Audio após confirmar a licença vigente) pode ser estudada com crédito em `docs/sources/`. Quando a única fonte for código copyleft (LGPL/GPL), o crate fica em `crates-copyleft/`, com a **mesma licença da fonte**, fora do workspace padrão e desligado por padrão; o núcleo e os crates padrão permanecem MIT OR Apache-2.0 e nunca dependem deles. Nenhum código é copiado: reimplementação com registro do que foi estudado.

## 10. Deixado ao construtor
Estrutura interna de módulos, algoritmos internos (FFT/IMDCT, tabelas de Huffman, etc.), estratégia de pooling de buffers, formato exato dos manifestos de fixtures, detalhes do CI, divisão fina de TASKs por camada.
