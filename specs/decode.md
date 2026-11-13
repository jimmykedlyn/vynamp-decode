# Spec — vynamp-decode · decode

| Campo | Valor |
|---|---|
| Módulo | `vynamp-decode` |
| Crate(s) | 9: `vynamp-decode` (fachada), `vynamp-decode-core`, `-riff`, `-aiff`, `-pcm`, `-flac`, `-mpa`, `-mp3` e `vynamp-decode-testkit` (só teste) |
| Tipo | lib |
| Versão da spec | 0.4.0 |
| Data | 2026-11-13 |
| Estado | **§5 aprovada em 2026-11-13** pelo mantenedor, na abertura da `TASK-E01-04`. As demais seções seguem rascunho autorado a partir do código e dos testes que existem hoje: §3, §4, §6, §9 e §10 não foram aprovadas e não devem ser lidas como contrato. A aprovação é do mantenedor, por `GOVERNANCE.md` |
| Dono | a definir |
| Depende de | std; nenhuma dependência externa |
| É dependido por | engine; encode (para transcodificar sem segundo parser) |

> **Esqueleto gerado pelo passo 37, preenchido em 2026-11-11.**
> §1, §2 e §7 foram decididos em `01-arvore-final.md` §2 e valem como norma.
> §3 a §6 e §8 a §10 foram escritos a partir do que o módulo **é**, não do que se
> pretendia que fosse: cada API citada em §3 foi lida em `crates/*/src/`, e cada
> critério de §5 nomeia um teste que existe e roda. Onde a norma pede algo que o
> código não tem, isso está em §10, não inventado em §3.
> **Estar escrito não é estar aprovado:** quem move o estado para `aprovada` é o
> mantenedor, por `GOVERNANCE.md`.

## 1. Propósito

Transformar bytes de arquivo de midia em amostras PCM, com precisao de bit e duracao corretas.

**Não faz:** Nao escreve tag, nao monta biblioteca, nao toca audio, nao converte taxa.

## 2. Fronteira

| Permitido | Proibido |
|---|---|
| std; `vynamp-dsp` opcional atras de feature | qualquer crate de dominio `vynamp-*`; qualquer tipo de dominio; escrever arquivo |

Hoje o módulo usa **só `std`**: os nove crates não têm uma única dependência externa,
nem o `vynamp-dsp` que o §2 permite. A feature `resample` prevista não existe — está
declarada como lacuna em §10, não como contrato.

## 3. API pública

> Autorado em 2026-11-11 a partir de `crates/vynamp-decode/src/lib.rs` e
> `crates/vynamp-decode-core/src/lib.rs`. **Não aprovado.**

Nove crates, mas **uma superfície pública só**: a fachada `vynamp-decode` reexporta
todo o `vynamp-decode-core` (`pub use vynamp_decode_core::*`), e o consumidor nunca
precisa nomear um crate interno. Nada além do que está aqui é contrato.

### 3.1 Abertura

```rust
pub fn default_registry() -> Registry;
pub fn open(src: Box<dyn Source>, opts: Options) -> Result<Reader>;
pub fn open_path(path: &Path, opts: Options) -> Result<Reader>;
```

- **`default_registry()`** devolve um registro novo a cada chamada, contendo o que as
  features ligaram. Por padrão, quatro containers e quatro codecs:

  | Tipo | Nome | Extensões | Verificação |
  |---|---|---|---|
  | container | `riff` | `.wav .wave .w64 .rf64 .bw64` | `Oracle` |
  | container | `aiff` | `.aif .aiff .aifc` | `Oracle` |
  | container | `flac` | `.flac .fla` | `Conformance` |
  | container | `mpa` | `.mp3 .mp2 .mp1 .mpa` | `Oracle` |
  | codec | `pcm` | — | `Oracle` |
  | codec | `flac` | — | `Conformance` |
  | codec | `mpeg-audio` | — | `Oracle` |
  | codec | `mpeg-audio-layer1` | — | `Experimental` |

  A tabela não é documentação solta: `cargo run --example support_matrix` a imprime a
  partir do registro em tempo de execução.

- **`open_path`** usa a extensão do arquivo como **dica** de desempate; `open` não tem
  dica. Em nenhum dos dois a extensão decide o formato — quem decide é o conteúdo.

- **`open`/`open_path` decodificam a trilha 0.** Um arquivo sem trilha de áudio devolve
  `Unsupported("no audio track")`, não um leitor vazio.

### 3.2 `Registry` — o que existe e como se escolhe

```rust
impl Registry {
    pub fn new() -> Self;
    pub fn add_container(&mut self, d: &'static ContainerDescriptor);
    pub fn add_codec(&mut self, d: &'static CodecDescriptor);
    pub fn probe(&self, src: &mut dyn Source, hint_ext: Option<&str>, limits: &Limits)
        -> Result<&'static ContainerDescriptor>;
    pub fn codec_for(&self, id: &CodecId) -> Option<&'static CodecDescriptor>;
    pub fn containers(&self) -> impl Iterator<Item = &'static ContainerDescriptor>;
    pub fn codecs(&self) -> impl Iterator<Item = &'static CodecDescriptor>;
}
```

- **`probe` lê até 4 096 bytes** (`PROBE_BYTES`) depois de pular tags ID3v2 iniciais e
  escolhe o descritor de **maior score**. Empate vai para o descritor cuja lista de
  extensões contém `hint_ext` (sem diferenciar caixa); sem dica, o primeiro registrado.
- **A fonte volta à posição 0** ao fim de `probe`, dê certo ou errado. Quem trata as
  tags é o demuxer, não o probe.
- **Nada casou:** `Unsupported("container")`. Nunca `panic!`.

### 3.3 `Reader` — a timeline já trimada

```rust
impl Reader {
    pub fn open_with(registry: &Registry, src: Box<dyn Source>,
                     opts: Options, hint_ext: Option<&str>) -> Result<Self>;
    pub fn track(&self) -> &Track;
    pub fn frames(&self) -> Option<u64>;
    pub fn position(&self) -> u64;
    pub fn take_events(&mut self) -> Vec<Event>;
    pub fn events_dropped(&self) -> u64;
    pub fn read(&mut self, out: &mut AudioBuffer) -> Result<bool>;
    pub fn seek(&mut self, frame: u64) -> Result<()>;
}
```

`open_with` é a porta para um registro que não é o padrão — é assim que o teste monta
containers sintéticos sem tocar no registro de produção.

- **`read` devolve `Ok(true)` com pelo menos um frame, ou `Ok(false)` no fim**, e nesse
  caso deixa `out` vazio. Não há erro de fim de fluxo: o enum `Error` **não tem**
  variante de EOF, e isso é deliberado.
- **`frames()` é o comprimento da timeline trimada**, não o declarado no arquivo:
  declaração menos `delay` e `padding`, saturando em zero. `None` quando o container não
  declara comprimento.
- **`position()` é o índice do próximo frame na timeline trimada** — anda com `read` e
  salta com `seek`.
- **`seek(frame)` recebe frame da timeline trimada.** Internamente soma o `delay` da
  faixa, pede ao demuxer e reaquece o decodificador com `set_preroll`; a posição final
  segue **onde o demuxer pousou**, não onde foi pedido. Um seek além do fim pousa no
  fim.
- **Eventos de diagnóstico:** `take_events` esvazia a fila; ela comporta 4 096 eventos e
  o excedente é contado em `events_dropped`, porque uma fila ilimitada num arquivo
  corrompido é um vetor de memória. `take_events` também zera o contador.
- **`Reader` é `Send`.** Não é `Sync`, e não precisa ser: um leitor pertence a uma
  thread.

### 3.4 `Track` — o que o demuxer sabe da faixa

```rust
pub struct Track {
    pub id: u32,
    pub codec: CodecId,
    pub sample_rate: u32,
    pub channels: u16,
    pub channel_mask: u32,
    pub frames: Option<u64>,
    pub delay: u32,
    pub padding: u32,
    pub gapless: GaplessKind,
    pub codec_private: Vec<u8>,
}
```

`gapless: GaplessKind` diz **quanta confiança** merecem `delay`, `padding` e `frames`:
`Exact` (lossless, por construção), `FromMetadata` (veio do encoder) e `Unknown`. Quem
vai além de simplesmente tocar — o engine, ao decidir se pode emendar faixas — consulta
esse campo em vez de assumir que um número declarado é verdade.

### 3.5 `AudioBuffer` — planar, reaproveitado

Plano por canal: o canal `c` ocupa `[c·stride, c·stride + frames)`. `reset` reaproveita
o armazenamento já alocado em vez de realocar a cada pacote. `format()`, `channels()`,
`frames()`, `truncate_front(n)`, `truncate_back(n)` e `to_f32_interleaved(&mut Vec<f32>)`
completam a superfície.

### 3.6 `Source` — de onde vêm os bytes

```rust
pub trait Source: Send {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize>;
    fn seek(&mut self, pos: u64) -> Result<()>;
    fn position(&self) -> u64;
    fn len(&self) -> Option<u64>;
    fn is_empty(&self) -> bool;
}
```

Três implementações: `FileSource` (arquivo), `VecSource` (buffer próprio) e
`SliceSource<'a>` (emprestado, para teste). A trait existe para que o consumidor possa
decodificar de um cache, de um pipe com seek ou de memória sem que o módulo saiba disso.

### 3.7 O que é reexportado do núcleo

`Error`, `LimitKind`, `Result`, `Limits`, `Mode`, `Options`, `Event`, `CodecId`,
`GaplessKind`, `SampleFormat`, `PcmSpec`, `PcmEncoding`, `Endian`, `Packet`, `SeekTo`,
`Demuxer`, `Decoder`, `CancelSource`, `CancelToken`, `Md5`, `Verification`,
`ContainerDescriptor`, `CodecDescriptor`, `OpenFn`, `skip_id3v2`, `ByteReader`,
`BitReader`, `crc8`, `crc16`, `crc16_update`, `crc16_mpeg`, `Trailer`, `TrailerKind`,
`find_trailers`.

`Demuxer` e `Decoder` são públicos porque o registry é público: sem eles não se
registra um formato novo. Mas **nenhum tipo de domínio do produto aparece aqui** — não
há `SampleRate`, `MediaTime` nem `AudioBlock` do engine; o decode fala `u32`, `u64` e
`f32`/`i32`.

## 4. Requisitos

Um requisito por comportamento que o código precisa manter, com ID estável. O bloco
`DECODE-REQ-01..29` é desta spec; `formatos.md` usa `30..59` e
`registro-capacidades.md` usa `60..89`, para que os três arquivos não colidam.

| ID | Requisito | REQ de produto | TASK | Onde é provado |
|---|---|---|---|---|
| `DECODE-REQ-01` | O formato é escolhido pelo conteúdo; a extensão só desempata | `REQ-E01-04` | TASK-E01-04 | `renamed_files_probe_by_content_not_by_extension`, `probe_tie_resolved_by_extension`, `open_path_uses_the_extension_and_probe` |
| `DECODE-REQ-02` | Entrada que não casa com nenhum container falha com `Unsupported("container")` e sem `panic!` | `REQ-E01-04` | TASK-E01-04 | `random_bytes_are_unsupported`, `probe_nothing_matches` |
| `DECODE-REQ-03` | Cada codec é resolvido pelo `CodecId` da faixa, e a lista de formatos acompanha as features ligadas | `REQ-E01-04` | TASK-E01-04 | `codec_lookup`, `registry_enumerates_names_extensions_and_verification` |
| `DECODE-REQ-04` | A saída é trimada: `delay` descartado no início e `padding` no fim, atravessando pacotes | `REQ-E01-01` | TASK-E01-01 | `gapless_delay_and_padding_span_packets`, `lame_tagged_files_decode_to_exactly_the_encoded_pcm_length` |
| `DECODE-REQ-05` | `frames()` e `position()` contam a timeline trimada, não a do arquivo | `REQ-E01-01` | TASK-E01-01 | `frames_and_position_follow_the_trimmed_timeline` |
| `DECODE-REQ-06` | O comprimento só é imposto quando a faixa declara `padding`; uma declaração seca é limite inferior | `REQ-E01-01` | TASK-E01-01 | `resilient_delivers_samples_beyond_a_too_small_declared_total`, `declared_1000_present_400_strict` |
| `DECODE-REQ-07` | `seek` entrega exatamente as amostras de um decode completo a partir do mesmo ponto | `REQ-E01-01` | TASK-E01-01 | `seek_lands_on_requested_output_frame`, `decode_after_seek_equals_the_full_decode_slice` |
| `DECODE-REQ-08` | Um pacote que decodifica menos frames que cobre não desloca os seguintes | `REQ-E01-01` | TASK-E01-01 | `short_decoder_output_does_not_shift_later_packets` |
| `DECODE-REQ-09` | Fim de fluxo é `Ok(false)` com buffer vazio; não existe variante de erro para EOF | `REQ-E01-01` | TASK-E01-01 | `read_leaves_no_stale_frames_at_end` |
| `DECODE-REQ-10` | Erro é dado: `Error` é `Copy`-comparável, `Display` e `std::error::Error` | `REQ-E01-04` | TASK-E01-04 | `wrong_size_error_carries_packet_offset`, `invalid_what` |
| `DECODE-REQ-11` | Cada limite declarado em `Limits` que o formato usa é aplicado e devolve `Limit(kind)` | `REQ-E01-04` | TASK-E01-04 | `channel_limit`, `frame_limit`, `limits`, `frame_limit_is_enforced`, `declared_length_beyond_max_frames_is_a_typed_error` |
| `DECODE-REQ-12` | Cancelamento é consultado antes de cada pacote e devolve `Cancelled` | `REQ-E01-04` | TASK-E01-18 | `cancel_is_checked_before_each_packet`, `cancel_between_packets` |
| `DECODE-REQ-13` | `Mode::Strict` falha no primeiro defeito; `Mode::Resilient` recupera e relata em `Event` | `REQ-E01-04` | TASK-E01-04 | `truncated_strict_and_resilient`, `resilient_corrupt_middle_frame_keeps_length_and_reports_once`, `truncated_streaminfo_is_a_typed_error` |
| `DECODE-REQ-14` | A fila de eventos é limitada e o excedente é contado, não perdido em silêncio | `REQ-E01-04` | TASK-E01-04 | `event_queue_is_capped_and_counts_drops`, `demuxer_queue_drops_are_counted_by_the_reader` |
| `DECODE-REQ-15` | Eventos são drenados também no fim do fluxo e no erro | `REQ-E01-04` | TASK-E01-04 | `events_are_drained_at_end_of_stream_and_on_error` |
| `DECODE-REQ-16` | Cada formato declara seu nível de verificação, e o nível é o que os testes sustentam | `REQ-E17-01` | TASK-E17-01 | `builtin_formats_have_the_expected_verification_level` |
| `DECODE-REQ-17` | O decodificador PCM e os decodificadores MPEG não alocam depois do aquecimento | `REQ-E01-07` | TASK-E01-07 | `no_allocations_after_warmup`, `decoding_does_not_allocate`, `layer_two_decoding_does_not_allocate`, `layer_three_decoding_does_not_allocate` |
| `DECODE-REQ-18` | O buffer de pacote é reaproveitado entre chamadas | `REQ-E01-07` | TASK-E01-07 | `packet_buffer_is_reused` |
| `DECODE-REQ-19` | Formatos com MD5 no cabeçalho conferem o hash e tratam divergência como erro | `REQ-E01-01` | TASK-E01-01 | `subset_files_decode_strictly_and_verify_their_md5` |
| `DECODE-REQ-20` | Um stream cortado de outro maior é rebaseado em vez de recusado, e o rebase é relatado | `REQ-E01-01` | TASK-E01-01 | `a_stream_without_its_leading_frames_is_rebased_and_decodes_strictly`, `a_stream_cut_out_of_a_longer_one_starts_silently_and_seeks_in_strict_mode` |
| `DECODE-REQ-21` | Informação de gapless conflitante é relatada, e a escolha é determinística | `REQ-E17-02` | TASK-E17-02 | `the_reader_reports_conflicting_gapless_information`, `a_tag_that_does_not_fit_the_frame_count_is_a_gapless_conflict` |
| `DECODE-REQ-22` | `Reader` é `Send` | `REQ-E01-04` | TASK-E01-04 | `reader_is_send` |
| `DECODE-REQ-23` | Tags ID3v2/APEv2/ID3v1 em volta do fluxo não mudam as amostras | `REQ-E01-04` | TASK-E01-04 | `id3v2_apev2_and_id3v1_wrappers_do_not_change_the_output`, `id3v2_prefix_does_not_change_the_samples` |
| `DECODE-REQ-24` | Bytes aleatórios, truncados ou com cabeçalho impossível nunca causam `panic!` | `REQ-E01-08` | TASK-E01-08 | `garbage_never_panics`, `faulty_files_are_rejected_in_strict_mode`, `byte_flip_and_truncation_smoke` |

Os 17 requisitos de produto do módulo (`REQ-E01-*`, `REQ-E12-05/06`, `REQ-E17-*`)
cobrem também os formatos e o registro de capacidades, que são o assunto das outras duas
specs. Nenhum deles fica sem dono: `formatos.md` §4 e `registro-capacidades.md` §4
fecham o mapa.

## 5. Critérios de aceite

> Autorado em 2026-11-11 a partir dos critérios que as TASKs **já tinham** e dos testes
> que **já existem**; **aprovado em 2026-11-13**, na abertura da `TASK-E01-04`, pelo
> mantenedor. A aprovação vale pelo conteúdo desta tabela no commit que a registra — ela
> não antecipa critério nenhum que venha a ser acrescentado depois.

Cada linha nomeia um teste que roda hoje na suíte do módulo, sem hardware de áudio e sem
rede.

| ID | Given | When | Then | Teste |
|---|---|---|---|---|
| `DECODE-AC-01` | um FLAC renomeado para `.mp3`, um MP2 renomeado para `.wav` e um WAV renomeado para `.flac`, cada um como arquivo no disco | o probe lê o cabeçalho sem consultar a extensão | o container e o codec corretos são escolhidos nos três, e os três decodificam até o fim em `Mode::Strict` | `renamed_files_probe_by_content_not_by_extension`, `probe_tie_resolved_by_extension`, `open_path_uses_the_extension_and_probe` |
| `DECODE-AC-02` | bytes aleatórios sem container conhecido | `open` é chamado | devolve `Unsupported("container")`, sem `panic!` | `random_bytes_are_unsupported` |
| `DECODE-AC-03` | um registry vazio | `probe` roda | devolve `Unsupported("container")` e a fonte volta à posição 0 | `probe_nothing_matches` |
| `DECODE-AC-04` | o registro padrão | a lista de containers e codecs é consultada | o conjunto bate exatamente com as features ligadas | `codec_lookup` |
| `DECODE-AC-05` | uma faixa com `delay` e `padding` declarados | ela é lida até o fim | a saída tem `frames − delay − padding` frames e começa no primeiro frame audível | `gapless_delay_and_padding_span_packets` |
| `DECODE-AC-06` | um MP3 com tag LAME de delay/padding | ele é decodificado | a contagem de frames é exatamente a do PCM codificado | `lame_tagged_files_decode_to_exactly_the_encoded_pcm_length` |
| `DECODE-AC-07` | um arquivo com gapless | `frames()` e `position()` são lidos ao longo da leitura | os dois seguem a timeline trimada | `frames_and_position_follow_the_trimmed_timeline` |
| `DECODE-AC-08` | uma faixa que declara menos frames do que tem, **sem** padding | ela é lida em `Mode::Resilient` | o áudio real é entregue inteiro e o clamp é relatado | `resilient_delivers_samples_beyond_a_too_small_declared_total` |
| `DECODE-AC-09` | um arquivo gapless | `seek` é chamado para um frame da timeline trimada | a primeira amostra lida é a daquele frame | `seek_lands_on_requested_output_frame` |
| `DECODE-AC-10` | qualquer arquivo | um seek é feito e depois o resto é decodificado | a saída é idêntica à fatia correspondente do decode completo | `decode_after_seek_equals_the_full_decode_slice` |
| `DECODE-AC-11` | um decodificador que devolve menos frames que o pacote cobre | o stream continua | os pacotes seguintes não deslizam | `short_decoder_output_does_not_shift_later_packets` |
| `DECODE-AC-12` | o fim do fluxo | `read` é chamado de novo | devolve `Ok(false)` e o buffer fica vazio, sem frames velhos | `read_leaves_no_stale_frames_at_end` |
| `DECODE-AC-13` | um token de cancelamento acionado | `read` é chamado | devolve `Err(Cancelled)` antes de consumir outro pacote | `cancel_is_checked_before_each_packet` |
| `DECODE-AC-14` | um demuxer que emite mais de 4 096 eventos | os eventos são drenados | 4 096 são entregues e o excedente aparece em `events_dropped` | `event_queue_is_capped_and_counts_drops` |
| `DECODE-AC-15` | um stream com defeito no meio | ele é lido em `Mode::Strict` e em `Mode::Resilient` | `Strict` falha com erro tipado; `Resilient` mantém o comprimento e relata **uma vez** | `truncated_strict_and_resilient`, `resilient_corrupt_middle_frame_keeps_length_and_reports_once` |
| `DECODE-AC-16` | um arquivo de cada nível de verificação | o registro é consultado | `flac` é `Conformance`, `mpeg-audio-layer1` é `Experimental`, o resto é `Oracle` | `builtin_formats_have_the_expected_verification_level` |
| `DECODE-AC-17` | um decodificador já aquecido | `decode` roda em regime | **zero** alocações no caminho quente | `no_allocations_after_warmup`, `decoding_does_not_allocate` |
| `DECODE-AC-18` | o corpus oficial de conformidade FLAC (`subset`) | cada arquivo é aberto em `Mode::Strict` | decodifica até o fim e o MD5 de STREAMINFO confere | `subset_files_decode_strictly_and_verify_their_md5` |
| `DECODE-AC-19` | o corpus oficial `faulty` | cada arquivo é aberto em `Mode::Strict` | todos devolvem `Err` e nenhum causa `panic!` | `faulty_files_are_rejected_in_strict_mode` |
| `DECODE-AC-20` | o manifesto PCM com hash por arquivo | cada fixture é decodificada | o PCM bate com o sinal de origem e com o hash fixado | `manifest_files_decode_to_the_source`, `source_matches_the_independent_signal_and_pinned_hash` |
| `DECODE-AC-21` | o corpus MPEG com referência por arquivo | cada fixture é decodificada | o erro fica dentro do critério de precisão total da ISO/IEC 11172-4 | `every_fixture_matches_its_checksum_and_the_reference_within_the_iso_criterion` |
| `DECODE-AC-22` | um FLAC com frames de side channel e wasted bits | ele é decodificado | o PCM é bit a bit o da referência | `side_channel_frames_decode_bit_exactly`, `wasted_bits_are_signalled_and_decoded` |
| `DECODE-AC-23` | tags ID3v2/APEv2/ID3v1 em volta do fluxo | ele é decodificado | a saída é idêntica à do mesmo arquivo sem tags | `id3v2_apev2_and_id3v1_wrappers_do_not_change_the_output` |
| `DECODE-AC-24` | um stream cortado do meio de outro | ele é aberto | a timeline é rebaseada e o rebase é relatado | `a_stream_without_its_leading_frames_is_rebased_and_decodes_strictly` |
| `DECODE-AC-25` | dois relatos de gapless que não fecham | a faixa é aberta | um `GaplessConflict` é relatado e a escolha é determinística | `the_reader_reports_conflicting_gapless_information` |
| `DECODE-AC-26` | `Limits` com canal e frame apertados | o arquivo é aberto | devolve `Limit(Channels)` / `Limit(Frames)`, não `panic!` | `channel_limit`, `frame_limit` |
| `DECODE-AC-27` | milhares de bytes corrompidos ao acaso | cada crate de decodificador os recebe | nenhum `panic!` | `garbage_never_panics`, `faulty_files_are_rejected_in_strict_mode` |
| `DECODE-AC-28` | o corpus gapless do MPEG em `crates/vynamp-decode/tests/fixtures/mpeg/`, com `triangulation.sha256` | cada fixture é hasheada e a lista do manifesto é comparada com a do diretório | o sha256 de cada arquivo bate com o do manifesto, o manifesto não é subconjunto do diretório e um byte trocado sai do valor fixado | `every_gapless_fixture_has_its_hash_in_the_manifest`, `a_flipped_byte_leaves_the_pinned_hash` |
| `DECODE-AC-29` | WAV PCM16, FLAC e MP3 derivados do mesmo tom de origem, e a referência do FLAC | cada um é decodificado | os dois lossless decodificam amostra a amostra para o tom de origem e são iguais entre si; o MP3 tem a duração certa e fica acima de 20 dB de SNR contra o mesmo tom | `wav_flac_and_mp3_of_the_same_tone_decode_against_one_reference` |
| `DECODE-AC-30` | um WAV PCM24 e um WAV PCM32 tirados do mesmo tom | cada um é decodificado | o PCM32 é o tom e o PCM24 é o tom deslocado de 8 bits, com os 8 bits baixos vindos do sinal e não zerados | `pcm24_and_pcm32_keep_every_valid_bit` |
| `DECODE-AC-31` | um FLAC lossless e um MP3 com tag LAME | a faixa é aberta e lida até o fim nos dois | o FLAC declara `delay`/`padding` zero e entrega `total_samples`; o MP3 declara os dois pela tag e entrega `frames − delay − padding` | `the_gapless_trim_is_declared_by_mpeg_and_absent_in_flac` |
| `DECODE-AC-32` | um FLAC cortado no meio do bloco `STREAMINFO`, e um `STREAMINFO` que declara 2³¹−1 frames com `max_frames` em 1 000 000 | o probe e o decode rodam sobre os dois | o truncado devolve `Invalid { what: "unexpected eof" }` no offset do corte e o declarado devolve `Limit(Frames)`; nenhum dos dois entra em `panic!` | `truncated_streaminfo_is_a_typed_error`, `declared_length_beyond_max_frames_is_a_typed_error` |
| `DECODE-AC-33` | o registro padrão | a enumeração de containers e codecs é consultada | sai com nome, extensões e nível de verificação, e o conjunto bate exatamente com as features ligadas | `registry_enumerates_names_extensions_and_verification` |

### Critérios que esta spec **não** fixa

- **Formato a formato.** Quais containers, codecs e perfis existem é `formatos.md`;
  o que o registro de capacidades promete é `registro-capacidades.md`.
- **Desempenho.** Nenhum teste compara tempo de decode: o número depende da máquina e
  reprovaria em CI por variabilidade, não por regressão. Existe `examples/bench_decode.rs`
  como instrumento de bancada, e ele não é gate.
- **Fidelidade de codec com perdas por medição psicoacústica.** O critério é o da
  ISO/IEC 11172-4 contra referência externa; quem julga ouvindo não está no CI.

## 6. Erros e cancelamento

**Erro é dado, não `panic!`.** Toda a superfície devolve `Result`, e o enum é o mesmo para
todos os crates:

```rust
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Io(std::io::ErrorKind),
    Invalid { what: &'static str, offset: u64 },
    Unsupported(&'static str),
    Limit(LimitKind),
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitKind { Bytes, Frames, Channels, SampleRate, Metadata, Depth }
```

- **Não existe variante de fim de arquivo.** O fim é `Ok(false)` em `read` e em
  `next_packet`; dado truncado é `Invalid { what: "unexpected eof", offset }`. Um EOF
  como erro obrigaria todo chamador a distinguir "acabou" de "quebrou" num caminho que
  não é excepcional.
- **`Invalid` carrega o offset absoluto** — é o que torna um arquivo corrompido
  diagnosticável em vez de só recusado. Quando a falha é de um chunk, o offset é o do
  chunk, não o do arquivo.
- **`Io(ErrorKind)`** guarda a *espécie* do erro de I/O e não o erro: `Error` é `Clone`,
  `PartialEq` e `Eq`, e `std::io::Error` não é nenhum dos três.
- **`Unsupported(&'static str)`** é recusa deliberada, com o que faltou em texto fixo
  (`"container"`, `"codec"`, `"no audio track"`, `"compression"`). É o que a UI mostra
  como "formato não suportado" sem inventar mensagem.
- **`Limit(LimitKind)`** é recurso, não corrupção: o arquivo pode ser válido e grande
  demais para o orçamento declarado.

**Limites e o que os aplica.** `Limits` declara `max_bytes`, `max_frames`,
`max_channels`, `max_sample_rate`, `max_metadata_bytes` e `max_depth`. Os cinco
primeiros são aplicados pelos demuxers; `max_depth` **não é aplicado por nenhum
demuxer hoje** — está declarado em §10, porque um limite que ninguém lê dá falsa
sensação de proteção. Zero é sempre `Invalid`, acima do teto é `Limit`.

**Cancelamento é explícito, não timeout.** `CancelSource::cancel()` aciona um token
compartilhado; `CancelToken::check()` é chamado antes de cada pacote e antes de abrir. Um
timeout esconderia a diferença entre "a máquina está lenta" e "o usuário desistiu", e a
diferença importa: o primeiro deve dar erro, o segundo não.

**Depois de um erro o `Reader` não é prometido utilizável.** `Error::Invalid` no meio de
um stream deixa o demuxer em estado indefinido; quem quer continuar usa `Mode::Resilient`,
que é a promessa de continuar — e é ela que é testada.

## 7. Fora de escopo

metadata, CUE e capas (C12: a spec anunciou, o crate nao existe — fica registrado como limitacao); encoders; saida de audio

## 8. Dependências

Depende de: std; nenhuma dependência externa

É dependido por: engine; encode (para transcodificar sem segundo parser)

O `vynamp-decode-testkit` é `dev-dependency` e nunca entra em build de produção: ele
existe para contar alocações (`unsafe impl GlobalAlloc`) e calcular hash nos testes, e é
o **único** crate do workspace com exceção ao `unsafe_code = "forbid"`.

## 9. Testes e fixtures

**510 `#[test]` em nove crates, todos rodando sem rede, sem hardware e sem FFmpeg no
PATH.** O FFmpeg é ferramenta de geração das fixtures, nunca dependência de execução: as
referências estão commitadas ao lado dos arquivos.

### Oráculos

| Corpus | O que é | Licença | Como se verifica |
|---|---|---|---|
| `tests/fixtures/flac-conformance/subset/` | 21 arquivos do `flac-test-files` do IETF CELLAR | CC0 | `Mode::Strict` até o fim, com o MD5 de STREAMINFO do próprio decodificador |
| `tests/fixtures/flac-conformance/faulty/` | 11 arquivos inválidos do mesmo repositório | CC0 | cada um devolve `Err` em `Strict` e nenhum causa `panic!` |
| `tests/fixtures/flac/` | 23 arquivos gerados + `ref/` com o PCM de referência + `MANIFEST.txt` com SHA-256 por arquivo | autoral | comparação bit a bit contra a referência |
| `tests/fixtures/mpeg/` | 67 arquivos cobrindo as tabelas de Layer II e os três modos de Layer III, com `MANIFEST.txt` e referência `.ref.f32` | autoral | erro RMS e máximo contra o critério de precisão total da ISO/IEC 11172-4 |
| `tests/fixtures/pcm/` | 13 arquivos de layout (endianness, 8/16/24/32 bits, f32/f64, RF64, Wave64, AIFC, 6 canais) + `MANIFEST.txt` | autoral | sinal de origem reconstruído independentemente e hash fixado |
| `crates/vynamp-decode/tests/fixtures/mpeg/` | 15 arquivos de gapless em cinco casos (`.src.wav`, `.mp3` com tag LAME e `.ref.f32` por caso) + `triangulation.sha256` | autoral | hash por arquivo conferido contra o manifesto, triangulação com o tom de origem e trim por tag LAME |

As fixtures são geradas por `tools/fixtures/generate.ps1`,
`generate-flac-dev.ps1` e `generate-mpeg-dev.ps1`. O `MANIFEST.txt` guarda, por arquivo,
o hash, os argumentos de geração e a versão exata do gerador — sem isso uma fixture
regenerada noutra versão viraria uma referência diferente sem ninguém notar. O corpus de
gapless do MPEG tem o manifesto em `triangulation.sha256`, no formato `sha256sum`, escrito
pelo próprio `generate-mpeg-dev.ps1`: ele nasceu sem manifesto e ganhou um em
`TASK-E01-01`.

### Testes que dependem de corpus externo

Dois, ambos com a variável de ambiente documentada:

- **`VYNAMP_FLAC_TEST_FILES`** — aponta para um clone do `flac-test-files` e faz
  `conformance_flac.rs` rodar o `subset` e o `faulty` **completos** em vez da seleção
  commitada. Sem ela, roda a seleção, que é o que dá para versionar.
- **`VYNAMP_BLACKBOX_OUT`** — diretório de saída dos dois testes `#[ignore]` de
  `vynamp-decode-mp3/src/layer3/tests/blackbox.rs`, que escrevem arquivos para
  comparação externa. O `#[ignore]` tem motivo escrito na própria anotação.

### Testes de borda que existem porque a borda já falhou

- `short_decoder_output_does_not_shift_later_packets` — um pacote que decodifica menos
  frames que cobre deslocava todos os seguintes.
- `read_leaves_no_stale_frames_at_end` — o buffer devolvia frames velhos na última
  chamada, e um chamador que somasse comprimentos contaria duas vezes.
- `partial_trailing_frame_is_dropped` — o último frame incompleto é descartado e a
  contagem da faixa reflete os frames inteiros, não os bytes.

As bordas de saturação de escala inteira são do `vynamp-dsp`
(`full_scale_narrows_without_wrapping_to_the_opposite_sign`); aqui o decode não satura,
porque não converte profundidade.

## 10. Limitações conhecidas

Limitação declarada não reprova a entrega; limitação omitida reprova.

| Limitação | Efeito | Quem cobre |
|---|---|---|
| **Quatro containers e quatro codecs.** RIFF/RF64/BW64/Wave64, AIFF/AIFC, FLAC, MPEG audio; PCM e FLAC e MP2/MP3/MP1 | Nada de Ogg, MP4, Matroska, WMA, WavPack, APE, DSD, AAC. Um `.m4a` cai em `Unsupported("container")` | TASK-E01-07 (codecs modernos), E01-08 (desktop e arquivos grandes), E01-09 (DSD), E17-02..17 (bindings) |
| **Nenhuma feature `resample`.** O §2 permite `vynamp-dsp` atrás dela, e ela não existe | O decode nunca reamostra: o consumidor recebe a taxa do arquivo. Quem precisa converter chama o `vynamp-dsp` | TASK-E03-01, no engine |
| **`Limits::max_depth` não é aplicado por nenhum demuxer** | Um container com aninhamento profundo não é recusado por profundidade; o limite existe no tipo e não protege nada | Reavaliado na `TASK-E01-04` (2026-11-13): RIFF, AIFF, FLAC e MPEG audio são containers planos — nenhum deles tem container dentro de container, então não há profundidade a limitar nesta TASK e nada foi implementado. A limitação continua aberta, e o dono é a primeira TASK que trouxer container aninhado: `TASK-E17-03` (bindings de container) ou `TASK-E01-07` |
| **Nenhum perfil no descritor.** `ContainerDescriptor` e `CodecDescriptor` têm `name`, `extensions` e `verification`; não têm `profiles`, nem lista de sample rates, nem limites | Quem enumera o registro sabe **o que abre**, não **em que perfil**: um decoder que aceita MP3 não anuncia "MPEG-1 Layer III 44,1 kHz estéreo até 320 kbps". A enumeração de `DECODE-AC-33` sai com nome, extensões e nível de verificação — não com perfis | `DECODE-REQ-70` / `TASK-E17-01`, que é a dona da capability por tuple. O `acceptance` do `REQ-E01-04` no umbrella dizia "decoder lista perfis reais": esse texto foi corrigido em 2026-11-13 para o que o registro entrega, porque perfil é tuple de runtime e não cabe no descritor. Consultar `config/format-catalog.json` em execução está proibido por `specs/registro-capacidades.md` |
| **`channel_mask` só é preenchido pelo RIFF** (WAVE `EXTENSIBLE`). AIFF, FLAC e MPEG devolvem 0 | Layout de canais além de mono/estéreo não é descrito para FLAC 5.1, que é suportado mas chega sem máscara | TASK-E01-09 |
| **Uma trilha só.** `open`/`open_path` decodificam a trilha 0 | Um `.mka`/`.mp4` com duas faixas de áudio não pode escolher a segunda — e esses containers nem abrem ainda | TASK-E17-03 |
| **Sem metadados, CUE nem capas** (`C12`: a spec anunciou, o crate não existe) | Não há API para ler título, artista ou capítulo. O FLAC lê os blocos de metadados do stream, mas só o que precisa para decodificar | Fica registrado como limitação; é `§7` |
| **Sem `SampleRate`, `MediaTime` ou `AudioBlock`** | Não há tipo de domínio aqui por decisão: o engine é dono deles e traduz | `AGENTS.md` §2; TASK-E03-01 |
| **MPEG Layer I é `Experimental`** | FFmpeg não tem encoder de Layer I: não existe arquivo real nem oráculo de terceiro. O decoder é testado com frames montados à mão contra uma cadeia de referência independente | Fica declarado no nível de verificação, visível em `support_matrix` |
| **Sem `Sync` no `Reader`** | Um leitor pertence a uma thread. Compartilhar exige um canal, não um `Mutex` em volta do `Reader` | Decisão: o decode é sequencial por natureza |
| **`Mode::Resilient` recupera, mas não reconstrói** | Um frame corrompido vira silêncio do tamanho do frame ou é saltado; os bytes perdidos não voltam | `Event::CorruptFrame` é o relatório; a decisão é do chamador |
| **Nenhum `application_ready`** | O módulo tem código e testes, não um aplicativo: nada aqui toca dispositivo de áudio | TASK-E02-01, no player |
| **FLAC não tem delay nem padding.** O `STREAMINFO` do RFC 9639 §8.2 tem 34 bytes: `min_block`, `max_block`, `min_frame`, `max_frame`, `sample_rate`, `channels`, `bits_per_sample`, `total_samples` e `md5`. Não há campo de encoder delay nem de padding, e o codec é lossless, sem priming | Um FLAC nunca carrega `delay`/`padding` a cortar: `Track::delay` e `Track::padding` são sempre 0 neste container (`vynamp-decode-flac/src/demux.rs` os fixa assim), e a saída tem exatamente os `total_samples` do STREAMINFO. O caso real de trim de gapless no corpus é MPEG com tag LAME (delay 1105, padding 344–656 nas fixtures) | Registrado aqui porque o `Given` do AC-2 da `TASK-E01-01` pedia "um FLAC com encoder delay e padding declarados no streaminfo", condição que o formato não oferece. Esse `Given` foi corrigido em 2026-11-13 — ele é texto autorado na execução, não requisito aprovado: o `acceptance` do backlog para a TASK nunca pediu o campo, e sim "delay/padding e PCM24/32 preservados quando perfil permite". A linha de §5 continua sendo o registro da limitação, que não deixa de existir. O trim por codec é `TASK-E17-02` |
