# Spec — vynamp-decode · formatos

| Campo | Valor |
|---|---|
| Módulo | `vynamp-decode` |
| Crate(s) | `vynamp-decode-riff`, `vynamp-decode-aiff`, `vynamp-decode-pcm`, `vynamp-decode-flac`, `vynamp-decode-mpa`, `vynamp-decode-mp3` |
| Tipo | lib |
| Versão da spec | 0.2.0 |
| Data | 2026-11-11 |
| Estado | **rascunho autorado, não aprovado** — §3 a §10 escritos a partir do código e dos testes existentes; a aprovação é do mantenedor, por `GOVERNANCE.md` |
| Dono | mantenedor do `vynamp-decode` |
| Depende de | `vynamp-decode-core` (contratos); `vynamp-dsp` (opcional, feature `resample`) |
| É dependido por | engine; encode (para transcodificar sem segundo parser) |

> Esqueleto gerado pelo passo 37, preenchido em 2026-11-11. §1, §2 e §7 vêm de
> `01-arvore-final.md` §2 e valem como norma. §3 descreve só a API pública que existe
> hoje nos crates de formato; os contratos comuns (`Demuxer`, `Decoder`, `Options`,
> erros) são de `specs/decode.md` e não são repetidos aqui. Formatos ainda não
> implementados aparecem em §4 como requisitos `planned`, sem teste, e em §10.

## 1. Propósito

Transformar bytes de arquivo de mídia em amostras PCM, com precisão de bit e duração corretas.

**Não faz:** não escreve tag, não monta biblioteca, não toca áudio, não converte taxa.

## 2. Fronteira

| Permitido | Proibido |
|---|---|
| herda a fronteira de `specs/decode.md` | herda as proibições de `specs/decode.md` |

## 3. API pública

Cada formato entrega um par **demuxer + decoder** sobre os traits de
`vynamp-decode-core` (`specs/decode.md` §3). Todo crate de formato expõe a mesma forma:
uma sonda de bytes, um demuxer e uma função `open_*` que o registro usa.

| Crate | Sonda | Demuxer / abertura | Decoder | Tipos públicos |
|---|---|---|---|---|
| `vynamp-decode-riff` | `probe_riff(&[u8]) -> u8` | `RiffDemuxer`, `open_riff(Box<dyn Source>, &Options)` | `PcmDecoder` | `PACKET_FRAMES` (= 4096) |
| `vynamp-decode-aiff` | `probe_aiff(&[u8]) -> u8` | `AiffDemuxer`, `open_aiff(Box<dyn Source>, &Options)` | `PcmDecoder` | — |
| `vynamp-decode-pcm` | — | — | `PcmDecoder::new(&Track)` | usa `PcmSpec { encoding, bits, valid_bits, endian }` e `PcmEncoding { Signed, Unsigned, Float }` do core |
| `vynamp-decode-flac` | `probe_flac(&[u8]) -> u8` | `FlacDemuxer` (`stream_info()`, `seek_points()`, `metadata_blocks()`, `first_frame_offset()`), `open_flac` | `FlacDecoder::new` / `FlacDecoder::with_options` | `StreamInfo`, `SeekPoint`, `BlockKind`, `RawBlock`, `ChannelAssignment`, `FrameHeader` |
| `vynamp-decode-mpa` | `probe_mpa(&[u8]) -> u8` | `MpaDemuxer` (`vbr_info()`, `free_format_len()`, `first_frame_offset()`, `uses_cbr_index()`), `open_mpa` | — (o decoder vive em `-mp3`) | `parse_header([u8; 4]) -> Option<FrameHeader>`, `FrameHeader`, `MpegVersion`, `ChannelMode`, `parse_vbr`, `VbrInfo`, `VbrKind`, `LameInfo` |
| `vynamp-decode-mp3` | — | — | `MpegDecoder::new(&Track, &Options)` (Layer I, II e III) | — |

Contratos fixados por esta spec:

- **Sonda.** `probe_*` devolve uma confiança de 0 a 255 e não consome a fonte; 0 quer
  dizer "não é este formato". O registro escolhe pela maior confiança, nunca pela
  extensão do arquivo.
- **Pacote PCM.** RIFF e AIFF emitem pacotes de no máximo `PACKET_FRAMES` quadros;
  quadro parcial no fim do dado é descartado, não preenchido.
- **Identidade de codec.** `CodecId` cobre hoje `Pcm`, `Flac`, `Mp1`, `Mp2`, `Mp3`. Um
  codec novo entra como variante nova, com spec e teste no mesmo PR.
- **Grau de verificação.** Cada formato declara `Verification` (`Conformance`,
  `Oracle` ou `Experimental`) no registro (`specs/registro-capacidades.md`).

## 4. Requisitos

Formatos implementados (provados por teste hoje):

| ID | Requisito | REQ de produto | TASK | Onde é provado |
|---|---|---|---|---|
| DECODE-REQ-30 | Ler WAV, RF64 e Wave64 com PCM u8, s16, s24, s32, f32 e f64, inclusive `WAVE_FORMAT_EXTENSIBLE` | REQ-E01-01 | TASK-E01-01 | `riff.rs::round_trip_all_variants` |
| DECODE-REQ-31 | Pular chunk desconhecido respeitando o byte de alinhamento ímpar (RIFF) e o alinhamento de 8 bytes (Wave64) | REQ-E01-01 | TASK-E01-01 | `riff.rs::odd_unknown_chunk_is_skipped`, `wave64_unknown_chunk_is_skipped` |
| DECODE-REQ-32 | Aceitar arquivo em streaming (tamanho de `data` desconhecido) limitando-o ao contêiner, e RF64 sem `ds64` | REQ-E01-01 | TASK-E01-01 | `riff.rs::streaming_data_size`, `rf64_without_ds64`, `riff_unknown_size_bounded_by_container_emits_clamped` |
| DECODE-REQ-33 | Ler AIFF e AIFF-C com as compressões NONE, twos, sowt, in24, in32, fl32/FL32, fl64/FL64 e `raw ` | REQ-E01-01 | TASK-E01-01 | `aiff.rs::round_trip_plain_aiff`, `round_trip_every_compression_type` |
| DECODE-REQ-34 | Converter a taxa em 80 bits estendidos do AIFF sem perder taxa fracionária | REQ-E01-01 | TASK-E01-01 | `aiff.rs::extended_canonical_rates`, `fractional_rate_is_accepted_in_both_modes` |
| DECODE-REQ-35 | Converter PCM inteiro e float, em ambos os endians, para a amostra interna sem alterar o valor; `valid_bits` não muda amostras | REQ-E01-01 | TASK-E01-01 | `pcm.rs::s16_both_endians` … `f64_both_endians`, `valid_bits_does_not_change_samples` |
| DECODE-REQ-36 | Decodificar FLAC nativo 16, 24 e 32 bits com todos os tipos de subframe e todas as atribuições de canal | REQ-E01-04 | TASK-E01-04 | `flac/decode.rs::fixed_orders_0_to_4`, `lpc_order_2`, `every_channel_assignment`, `side_channel_32_bit_extremes` |
| DECODE-REQ-37 | Buscar em FLAC por SEEKTABLE e sobreviver a SEEKTABLE errada; tamanho de bloco variável e total desconhecido | REQ-E01-04 | TASK-E01-04 | `flac/demux.rs::seek_with_seektable`, `seek_survives_a_wrong_seektable`, `variable_block_size_stream`, `unknown_total_decodes_to_eof_and_seeks` |
| DECODE-REQ-38 | Verificar o MD5 do STREAMINFO quando a decodificação é linear; desativar a verificação após busca | REQ-E01-04 | TASK-E01-04 | `flac/md5.rs::intact_streams_verify_silently`, `strict_reports_a_mismatch_on_the_last_packet`, `a_seek_disables_verification` |
| DECODE-REQ-39 | Ler FLAC precedido de ID3v2 com a mesma `Track` | REQ-E01-04 | TASK-E01-04 | `flac/metadata.rs::id3v2_prefix_opens_with_same_track` |
| DECODE-REQ-40 | Reconhecer quadros MPEG 1, 2 e 2.5 em todas as camadas, rejeitando valores reservados | REQ-E01-04 | TASK-E01-04 | `mpa/header.rs::every_bitrate_and_rate_index_for_every_version_and_layer`, `reserved_and_invalid_values_are_rejected` |
| DECODE-REQ-41 | Dar gapless exato a partir da tag LAME; consumir Xing/Info/VBRI sem emiti-los como áudio | REQ-E01-04 | TASK-E01-04 | `mpa/demux.rs::lame_tag_gives_exact_gapless_numbers`, `xing_and_info_headers_are_consumed_not_emitted`; `mpa/vbr.rs::vbri` |
| DECODE-REQ-42 | Ignorar ID3v2, APEv2 e ID3v1 sem alterar os pacotes; não sincronizar em lixo parecido com cabeçalho | REQ-E01-04 | TASK-E01-04 | `mpa/demux.rs::id3v2_apev2_and_id3v1_yield_identical_packets`, `false_sync_guard_skips_junk_that_looks_like_a_header` |
| DECODE-REQ-43 | Medir streams *free format* e manter a busca exata quando o cabeçalho Info mente | REQ-E01-04 | TASK-E01-04 | `mpa/demux.rs::free_format_streams_are_measured`, `seeks_stay_exact_when_the_info_header_lies` |
| DECODE-REQ-44 | Decodificar MP2 e MP3 dentro do critério ISO de precisão total | REQ-E01-04 | TASK-E01-04 | `mp2.rs::matches_ffmpeg_under_the_iso_full_accuracy_criterion`, `mp3.rs::matches_ffmpeg_under_the_iso_full_accuracy_criterion` |
| DECODE-REQ-45 | Decodificar MP3 Layer III sem alocar por quadro | REQ-E01-04 | TASK-E01-04 | `mp3/alloc.rs::layer_three_decoding_does_not_allocate` |

Formatos planejados (sem teste; nenhum entra no registro antes do teste):

| ID | Requisito | REQ de produto | TASK | Onde é provado |
|---|---|---|---|---|
| DECODE-REQ-46-AAC | AAC-LC em MP4 e ADTS; HE-AAC nunca anunciado como AAC-LC | REQ-E01-07 | TASK-E01-07 | planned, sem teste (`DECODE-AC-34`, `DECODE-AC-35` em `specs/decode.md`) |
| DECODE-REQ-46-MP4 | Demuxer MP4, ALAC lossless em MP4 e leitura de capítulos de M4B | REQ-E01-11 | TASK-E01-11 | planned, sem teste (`DECODE-AC-53`, `DECODE-AC-54`) |
| DECODE-REQ-46-VORBIS | Demuxer Ogg e Vorbis em Ogg | REQ-E01-12 | TASK-E01-12 | planned, sem teste (`DECODE-AC-55`) |
| DECODE-REQ-46-OPUS-CELT | Opus CELT-only fullband em Ogg, sobre o demuxer Ogg de `DECODE-REQ-46-VORBIS` | REQ-E01-13 | TASK-E01-13 | planned, sem teste (`DECODE-AC-56`) |
| DECODE-REQ-46-OPUS-SILK | Opus SILK-only e híbrido (SILK+CELT) em Ogg, sobre o demuxer Ogg de `DECODE-REQ-46-VORBIS` | REQ-E01-14 | TASK-E01-14 | planned, sem teste (`DECODE-AC-57`) |
| DECODE-REQ-47 | CAF, AU, G.711, ADPCM, FLAC em Ogg/MP4/Matroska, WavPack, APE, TTA, WMA, Musepack, WebM/Matroska; arquivos grandes | REQ-E01-08 | TASK-E01-08 | planned, sem teste |
| DECODE-REQ-48 | Mapa de canais completo (`channel_mask`) e DSD convertido para PCM | REQ-E01-09 | TASK-E01-09 | planned, sem teste |
| DECODE-REQ-49 | Módulos tracker, MIDI e codecs raros | REQ-E12-05 | TASK-E12-05 | planned, sem teste |
| DECODE-REQ-50 | Leitura de CD de áudio | REQ-E12-06 | TASK-E12-06 | planned, sem teste |
| DECODE-REQ-51 | Bindings de contêineres e seleção de faixa de áudio | REQ-E17-03 | TASK-E17-03 | planned, sem teste |
| DECODE-REQ-52 | xHE-AAC (USAC) com capacidade declarada honestamente | REQ-E17-04 | TASK-E17-04 | planned, sem teste |
| DECODE-REQ-53 | Certificação de lossless, híbridos e PCM especiais | REQ-E17-05 | TASK-E17-05 | planned, sem teste |
| DECODE-REQ-54 | MPEG-H com renderer e licença | REQ-E17-13 | TASK-E17-13 | planned, sem teste |
| DECODE-REQ-55 | AC-4 e apresentações de áudio | REQ-E17-14 | TASK-E17-14 | planned, sem teste |
| DECODE-REQ-56 | IAMF como modelo e renderer | REQ-E17-15 | TASK-E17-15 | planned, sem teste |
| DECODE-REQ-57 | AC-3, E-AC-3, DTS e TrueHD, inclusive passthrough | REQ-E17-16 | TASK-E17-16 | planned, sem teste |
| DECODE-REQ-58 | Codecs de fala, AAC low-delay e formatos legados | REQ-E17-17 | TASK-E17-17 | planned, sem teste |
| DECODE-REQ-59 | Codec presente para playback não autoriza anunciar encoding; a lista de "formatos disponíveis" sai do build real | REQ-E01-07 | TASK-E01-07 | regra de `reference/audio/02-formats.md`; sem critério ainda. A dona é a `TASK-E01-07`, que fecha o fatiamento depois das `TASK-E01-11..14`; estas registram só decode e não anunciam encoding, mas não são donas da prova |

O antigo `DECODE-REQ-46` ("AAC-LC em MP4 e ADTS, HE-AAC, ALAC em MP4, Vorbis e Opus em
Ogg; capítulos", todo da `TASK-E01-07`) foi dividido em 2026-11-24, seguindo o
fatiamento da `TASK-E01-07` em `TASK-E01-07/11/12/13/14` de 2026-11-22: cada sub-requisito acima tem um
nome próprio e uma TASK dona. O ID sem sufixo não é mais usado.

## 5. Critérios de aceite

| ID | Given | When | Then | Teste |
|---|---|---|---|---|
| DECODE-AC-30 | um WAV/RF64/Wave64 gerado em cada variante de amostra | é aberto e decodificado | as amostras são idênticas às gravadas | `riff.rs::round_trip_all_variants` |
| DECODE-AC-31 | um WAV com `data` antes de `fmt ` | é aberto | erro `Invalid`, sem `panic!` | `riff.rs::data_before_fmt` |
| DECODE-AC-32 | um WAV com format tag desconhecida | é aberto | erro `Unsupported` | `riff.rs::unsupported_format_tag` |
| DECODE-AC-33 | um WAV com quadro parcial no fim | é decodificado até o fim | o quadro parcial é descartado | `riff.rs::partial_trailing_frame_is_dropped` |
| DECODE-AC-34 | um WAV | busca a uma posição exata e ao fim | a próxima amostra é a da posição pedida | `riff.rs::seek_exact_and_end` |
| DECODE-AC-35 | bytes invertidos ou truncados num WAV válido | é aberto e decodificado | termina com erro ou EOF, nunca com `panic!` | `riff.rs::byte_flip_and_truncation_smoke` |
| DECODE-AC-36 | um AIFF-C com cada tipo de compressão suportado | é decodificado | as amostras são idênticas às gravadas | `aiff.rs::round_trip_every_compression_type` |
| DECODE-AC-37 | um AIFF-C com compressão não suportada | é aberto | erro `Unsupported` | `aiff.rs::unsupported_compression` |
| DECODE-AC-38 | um AIFF cujo COMM declara mais quadros que o SSND tem | é decodificado | a duração é limitada ao SSND e o evento `Event::Clamped` é emitido | `aiff.rs::comm_frames_beyond_ssnd_emits_clamped` |
| DECODE-AC-39 | um AIFF com `SSND.offset` diferente de zero | é decodificado | a primeira amostra vem depois do offset | `aiff.rs::ssnd_offset_is_respected` |
| DECODE-AC-40 | um `PcmSpec` fora do suportado, ou pacote de tamanho errado | é decodificado | erro `Unsupported` ou `Invalid` | `pcm.rs::unsupported_specs`, `wrong_packet_size_is_invalid` |
| DECODE-AC-41 | os vetores FLAC de referência estéreo 16 e 24 bits | são decodificados | a saída é igual, bit a bit, à fonte | `flac/oracle.rs::stereo_16_bit_equals_source`, `stereo_24_bit_equals_source` |
| DECODE-AC-42 | um subframe FLAC malformado | é decodificado | erro `Invalid`, sem `panic!` | `flac/decode.rs::rejects_malformed_subframes` |
| DECODE-AC-43 | um FLAC com SEEKTABLE errada | busca a uma posição | a posição final é exata | `flac/demux.rs::seek_survives_a_wrong_seektable` |
| DECODE-AC-44 | um FLAC com MD5 que não confere, em modo estrito | é decodificado até o fim | o erro sai no último pacote | `flac/md5.rs::strict_reports_a_mismatch_on_the_last_packet` |
| DECODE-AC-45 | um FLAC cuja taxa passa do limite | é aberto | erro `Limit` | `flac/metadata.rs::rate_beyond_limit` |
| DECODE-AC-46 | um MP3 com tag LAME | é decodificado | atraso e preenchimento saem exatos (gapless) | `mpa/demux.rs::lame_tag_gives_exact_gapless_numbers` |
| DECODE-AC-47 | o mesmo MP3 com e sem ID3v2/APEv2/ID3v1 | é demultiplexado | os pacotes são idênticos | `mpa/demux.rs::id3v2_apev2_and_id3v1_yield_identical_packets` |
| DECODE-AC-48 | lixo inicial e último quadro cortado | a sonda roda | o formato é reconhecido | `mpa/header.rs::probe_tolerates_leading_junk_and_a_cut_last_frame` |
| DECODE-AC-49 | um MP1 | busca a várias posições | as amostras são as de uma decodificação completa | `mp1.rs::layer_one_seeks_give_exactly_the_samples_of_a_full_decode` |
| DECODE-AC-50 | um MP2 com índice de scalefactor 63 | é decodificado | erro `Invalid` | `mp2.rs::scalefactor_index_63_is_invalid` |
| DECODE-AC-51 | um MP3 que começa dentro do bit reservoir | é decodificado | sai silêncio até o reservoir encher, sem erro | `mp3.rs::a_stream_that_starts_inside_the_reservoir_decodes_silently_until_it_is_filled` |
| DECODE-AC-52 | MP2 e MP3 de referência | são decodificados | a saída cumpre o critério ISO de precisão total contra o oráculo | `mp2.rs::matches_ffmpeg_under_the_iso_full_accuracy_criterion`, `mp3.rs::matches_ffmpeg_under_the_iso_full_accuracy_criterion` |
| DECODE-AC-53 | um ALAC em MP4 em `tests/fixtures/modern/alac/`, gerado pelo FFmpeg fixado (`-c:a alac`), e o PCM de origem usado para produzi-lo, ambos no `MANIFEST.txt` do diretório com sha256 e frame count | o arquivo é aberto por `vynamp_decode::open_path` e decodificado até o fim; o PCM resultante é comparado amostra a amostra contra o PCM de origem | o PCM decodificado é bit-exato ao da origem; `frames()` devolve a contagem bruta do manifesto (antes de trim); um seek para o frame da metade seguido de leitura até `Ok(false)` cai no mesmo slice do decode linear (TASK-E01-11-AC-1) | `mp4_alac.rs::alac_in_mp4_decodes_bit_exact_to_its_source` |
| DECODE-AC-54 | um M4B em `tests/fixtures/modern/alac/`, gerado pelo FFmpeg fixado (`-f ipod -map_metadata`) com um capítulo declarado via FFMETADATA, listado no `MANIFEST.txt` do diretório | o arquivo é aberto e os capítulos são enumerados pela API pública | a lista de capítulos (título + timestamp) é igual à do FFMETADATA de origem; `frames()` devolve a contagem bruta do manifesto; seek para o frame da metade cai no slice esperado (TASK-E01-11-AC-2) | `mp4_alac.rs::m4b_chapters_match_their_ffmetadata` |
| DECODE-AC-55 | um Vorbis em Ogg em `tests/fixtures/modern/vorbis/`, gerado pelo FFmpeg fixado (`-c:a libvorbis`), listado no `MANIFEST.txt` do diretório com sha256, taxa, canais e frames, mais o decode de referência produzido pelo mesmo FFmpeg (`ffmpeg -i in.ogg -f wav ref.wav`) | o arquivo é decodificado e o PCM é comparado contra o WAV de referência | SNR >= 90 dB contra a referência (tolerância explícita para lossy); `frames()` devolve a contagem bruta do manifesto; seek para o frame da metade cai no slice esperado (TASK-E01-12-AC-1) | `ogg_vorbis.rs::vorbis_in_ogg_matches_the_ffmpeg_reference` |
| DECODE-AC-56 | um Opus CELT-only fullband em Ogg em `tests/fixtures/modern/opus-celt/`, gerado pelo FFmpeg fixado (`-c:a libopus -application audio`), listado no `MANIFEST.txt` do diretório com sha256, taxa, canais e frames, mais o decode de referência produzido pelo mesmo FFmpeg | o arquivo é decodificado e o PCM é comparado contra o WAV de referência | SNR >= 90 dB contra a referência (tolerância explícita para lossy); `frames()` devolve a contagem bruta do manifesto (antes de pre-skip); seek para o frame da metade cai no slice esperado (TASK-E01-13-AC-1) | `opus_celt.rs::opus_celt_matches_the_ffmpeg_reference` |
| DECODE-AC-57 | um Opus SILK-only e um Opus híbrido em Ogg em `tests/fixtures/modern/opus-silk/`, gerados pelo FFmpeg fixado a partir da mesma origem mono de banda larga com semente fixa (`-f lavfi -i "anoisesrc=color=pink:amplitude=0.3:duration=5:sample_rate=48000:seed=42,volume='0.5+0.5*sin(2*PI*4*t)':eval=frame"`, ruído rosa de 5 s modulado em amplitude a 4 Hz; um tom puro não serve, porque o libopus troca para CELT) e com `-fflags +bitexact -serial_offset 0`, para que cada geração saia byte a byte igual: o SILK-only com `-ac 1 -ar 16000 -c:a libopus -application voip -b:a 16k`, o híbrido com `-ac 1 -c:a libopus -application voip -b:a 18k` (48 kHz fullband, bitrate baixo; 18k fica no meio da faixa de 14k a 22k em que todo pacote saiu híbrido na medição, e a 24k já apareceram pacotes CELT config 31 misturados); cada um listado no `MANIFEST.txt` do diretório com sha256, taxa, canais e frames, e com decode de referência do mesmo FFmpeg | o byte de TOC de cada pacote de áudio dos dois arquivos é lido do Ogg (RFC 6716 §3.1, os 5 bits altos dão o config), e depois cada arquivo é decodificado e o PCM é comparado contra o WAV de referência correspondente | todo pacote do arquivo SILK-only tem config de TOC entre 0 e 11 e todo pacote do arquivo híbrido tem config de TOC entre 12 e 15; SNR >= 80 dB contra a referência (tolerância explícita, menor que CELT pelo codec); `frames()` devolve a contagem bruta do manifesto; seek para o frame da metade cai no slice esperado (TASK-E01-14-AC-1) | `opus_silk_hybrid.rs::opus_silk_and_hybrid_match_the_ffmpeg_reference` |

> **`DECODE-AC-53` a `-57` autorados em 2026-11-24, não aprovados.** Vêm do fatiamento da
> `TASK-E01-07` (`DECODE-REQ-46-MP4`, `-VORBIS`, `-OPUS-CELT`, `-OPUS-SILK`); o `Then` de
> cada um é cópia literal da TASK correspondente, e os testes não existem ainda: a
> coluna `Teste` nomeia o que cada TASK cria. Cada TASK tem arquivo de teste e diretório
> de fixture próprios, para que rodem em paralelo sem disputar o mesmo arquivo. Os
> números 36 a 40 já são AIFF/PCM nesta tabela e não foram reaproveitados. Aprovação é
> do mantenedor, por `GOVERNANCE.md`.

### Critérios que esta spec **não** fixa

- Critério de aceite para os formatos planejados (DECODE-REQ-47 a 58): cada TASK
  escreve os seus antes de sair de `planned`.
- Desempenho (tempo de decodificação por segundo de áudio): é de `specs/decode.md`.

## 6. Erros e cancelamento

Herdados de `specs/decode.md` §6: todo crate de formato devolve `Unsupported` (formato
ou variante reconhecido mas não suportado), `Invalid` (dado malformado) ou `Limit`
(valor fora dos limites de `Options`), nunca `panic!`. Divergência recuperável entre
cabeçalho e dado (tamanho declarado maior que o contêiner) não é erro: o demuxer limita
a duração e emite `Event::Clamped`. Cancelamento é do chamador, entre pacotes; nenhum
crate de formato tem timeout.

## 7. Fora de escopo

metadata, CUE e capas (C12: a spec anunciou, o crate não existe — fica registrado como limitação); encoders; saída de áudio

## 8. Dependências

Depende de: `vynamp-dsp` (opcional, feature `resample`)

É dependido por: engine; encode (para transcodificar sem segundo parser)

## 9. Testes e fixtures

- Cada crate tem `tests/` próprio; RIFF, AIFF e PCM geram os arquivos no próprio teste
  (round-trip), sem fixture binária.
- FLAC, MPA e MP3 usam fixtures em `crates/<crate>/tests/fixtures/`, pequenas o
  bastante para o repositório.
- Oráculo numérico: FLAC compara bit a bit com a fonte (`flac/oracle.rs`); MP2 e MP3
  comparam com saída de referência do FFmpeg pelo critério ISO de precisão total.
- Teste que depende de arquivo grande fica atrás de `#[ignore]` com motivo escrito e
  variável de ambiente documentada no próprio teste.
- `cargo test --workspace` em `libs/decode` roda tudo o que não é `#[ignore]`.
- Fixtures dos codecs modernos: cada TASK tem script de geração próprio em
  `tools/fixtures/`, que escreve só no seu diretório de `tests/fixtures/modern/` e o
  `MANIFEST.txt` dele, sem tocar `generate.ps1` nem o diretório de outra TASK:
  `generate-alac.ps1` → `modern/alac/` (`TASK-E01-11`), `generate-vorbis.ps1` →
  `modern/vorbis/` (`TASK-E01-12`), `generate-opus-celt.ps1` → `modern/opus-celt/`
  (`TASK-E01-13`), `generate-opus-silk.ps1` → `modern/opus-silk/` (`TASK-E01-14`).
  O AAC da `TASK-E01-07` continua na seção AAC-LC de `generate.ps1`. Cada script é
  criado pela TASK dona e usa o FFmpeg de `$env:FFMPEG`.

## 10. Limitações conhecidas

| Limitação | Consequência | Quem cobre |
|---|---|---|
| Só PCM (WAV/RF64/Wave64/AIFF), FLAC nativo e MPEG Layer I/II/III estão implementados | AAC, ALAC, Vorbis, Opus e todo o resto do catálogo de `reference/audio/02-formats.md` não tocam | TASK-E01-07 (AAC), E01-11 (MP4/ALAC/M4B), E01-12 (Ogg/Vorbis), E01-13 e E01-14 (Opus), E01-08, E17-03..05, E17-13..17 |
| `channel_mask` é lido mas não vira mapa de canais | arquivo multicanal sai na ordem do arquivo | TASK-E01-09 |
| Sem DSD, MIDI, tracker nem CD | esses arquivos são rejeitados com `Unsupported` | TASK-E01-09, E12-05, E12-06 |
| Sem leitura de metadata, CUE ou capa (C12) | tags são puladas, não expostas | fora deste módulo (§7) |
| MD5 do FLAC não é verificado depois de uma busca | corrupção após busca só aparece como erro de quadro | por desenho (DECODE-AC-44) |
| FLAC só em contêiner nativo | FLAC em Ogg, MP4 ou Matroska é rejeitado | TASK-E01-08 |
