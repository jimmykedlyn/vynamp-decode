# Spec — vynamp-decode · registro-capacidades

| Campo | Valor |
|---|---|
| Módulo | `vynamp-decode` |
| Crate(s) | `vynamp-decode-core` (módulo `registry`), `vynamp-decode` (fachada, `default_registry`, `open`, `open_path`) |
| Tipo | lib |
| Versão da spec | 0.2.0 |
| Data | 2026-11-11 |
| Estado | **rascunho autorado, não aprovado** — §3 a §10 escritos a partir do código e dos testes existentes; a aprovação é do mantenedor, por `GOVERNANCE.md` |
| Dono | mantenedor do `vynamp-decode` |
| Depende de | `vynamp-decode-core` (contratos); `vynamp-dsp` (opcional, feature `resample`) |
| É dependido por | engine; encode (para transcodificar sem segundo parser) |

> Esqueleto gerado pelo passo 37, preenchido em 2026-11-11. §1, §2 e §7 vêm de
> `01-arvore-final.md` §2 e valem como norma. Esta spec cobre **o registro que existe
> no código**: descritores estáticos de container e codec, seleção por probe e o nível
> de verificação declarado. O **modelo de capability de runtime** de
> `reference/audio/06-codec-capability-registry.md` (tuple
> container/codec/perfil/backend/plataforma, estado `available`) ainda não existe; ele
> entra em §4 como requisito `planned` (fase M3) e em §10, sem teste. Os contratos
> comuns (`Source`, `Demuxer`, `Decoder`, `Options`, erros) são de `specs/decode.md`.

## 1. Propósito

Transformar bytes de arquivo de midia em amostras PCM, com precisao de bit e duracao corretas.

**Não faz:** Nao escreve tag, nao monta biblioteca, nao toca audio, nao converte taxa.

## 2. Fronteira

| Permitido | Proibido |
|---|---|
| herda a fronteira de `specs/decode.md` | herda as proibicoes de `specs/decode.md` |

## 3. API pública

Só o que está aqui é contrato. Tudo é síncrono e sem estado global: o registro é um
valor que o chamador constrói e passa adiante.

### `vynamp-decode-core::registry`

| Item | Forma | Contrato |
|---|---|---|
| `Verification` | `enum { Conformance, Oracle, Experimental }` | nível de prova de um descritor: `Conformance` = bit-exato contra suíte de conformidade; `Oracle` = igual a uma implementação de referência; `Experimental` = sem prova externa |
| `OpenFn` | `fn(Box<dyn Source>, &Options) -> Result<Box<dyn Demuxer>>` | abre o container a partir da posição 0 da fonte |
| `ContainerDescriptor` | `{ name, extensions, probe: fn(&[u8]) -> u8, open: OpenFn, verification }` | `probe` recebe até 4096 bytes após as tags ID3v2 e devolve confiança 0..255; 0 = não é este container |
| `CodecDescriptor` | `{ name, verification, supports: fn(&CodecId) -> bool, make: fn(&Track, &Options) -> Result<Box<dyn Decoder>> }` | `supports` decide pelo `CodecId` da trilha, nunca pela extensão |
| `Registry::new()` | registro vazio | — |
| `Registry::add_container(&'static ContainerDescriptor)` | registra no fim | a ordem de registro é o critério final de desempate |
| `Registry::add_codec(&'static CodecDescriptor)` | registra no fim | o primeiro codec que aceita o `CodecId` vence |
| `Registry::probe(&mut dyn Source, hint_ext: Option<&str>, &Limits)` | `Result<&'static ContainerDescriptor>` | ver regra de seleção abaixo |
| `Registry::codec_for(&CodecId)` | `Option<&'static CodecDescriptor>` | primeiro registrado com `supports(id) == true` |
| `Registry::containers()` / `codecs()` | iteradores em ordem de registro | é a lista do que **este build** abre |

**Regra de seleção do probe.** O registro pula as tags ID3v2 do início (soma limitada
por `limits.max_metadata_bytes`, senão `Limit(Metadata)`; cabeçalho malformado não é
pulado), lê até 4096 bytes, chama todos os `probe` e escolhe a maior pontuação não
nula. Empate vai para o descritor cujas `extensions` contêm `hint_ext` (sem
diferenciar caixa); persistindo, para o primeiro registrado. A extensão **só
desempata**: nunca vence uma pontuação maior. A fonte volta à posição 0; o demuxer
trata as tags por conta própria. Nada pontuou: `Unsupported("container")`.

### `vynamp-decode` (fachada)

| Item | Contrato |
|---|---|
| `default_registry() -> Registry` | registra os descritores habilitados por feature, nesta ordem: containers `riff`, `aiff`, `flac`, `mpa`; codecs `pcm`, `flac`, `mpeg-audio`, `mpeg-audio-layer1` |
| `open(Box<dyn Source>, Options) -> Result<Reader>` | `Reader::open_with(&default_registry(), src, opts, None)` — sem dica de extensão |
| `open_path(&Path, Options) -> Result<Reader>` | abre o arquivo e passa a extensão como `hint_ext` |
| `Reader::open_with(&Registry, src, opts, hint_ext)` | probe → `open` do container → `codec_for(track.codec)`; sem codec: `Unsupported("codec")` |

Descritores embutidos e o nível que cada um declara:

| Descritor | Tipo | Feature | Extensões / `CodecId` | `Verification` | Base da prova |
|---|---|---|---|---|---|
| `riff` | container | `riff` | wav, wave, w64, rf64, bw64 | `Oracle` | sem suíte oficial de PCM |
| `aiff` | container | `aiff` | aif, aiff, aifc | `Oracle` | idem |
| `flac` | container | `flac` | flac, fla | `Conformance` | IETF flac-test-files |
| `mpa` | container | `mpa` | mp3, mp2, mp1, mpa | `Oracle` | FFmpeg |
| `pcm` | codec | `pcm` | `Pcm(_)` | `Oracle` | sem suíte oficial |
| `flac` | codec | `flac` | `Flac` | `Conformance` | IETF flac-test-files |
| `mpeg-audio` | codec | `mp3` | `Mp2`, `Mp3` | `Oracle` | FFmpeg mp2float/mp3float, critério ISO/IEC 11172-4; os streams ISO não são redistribuíveis |
| `mpeg-audio-layer1` | codec | `mp3` | `Mp1` | `Experimental` | não há encoder Layer I no FFmpeg nem fixture real |

Features default: `riff`, `aiff`, `pcm`, `flac`, `mpa`, `mp3`.

## 4. Requisitos

`implemented` tem teste citado em "Onde é provado"; `planned` não tem e não pode ser
anunciado como disponível.

| ID | Requisito | REQ de produto | TASK | Onde é provado |
|---|---|---|---|---|
| `DECODE-REQ-60` | O container é escolhido pela maior pontuação de `probe`, nunca pela extensão. | `REQ-E01-04` | `TASK-E01-04` | `facade.rs::probe_tie_resolved_by_extension` — implemented |
| `DECODE-REQ-61` | A extensão só desempata pontuações iguais; sem desempate, vence a ordem de registro. | `REQ-E01-04` | `TASK-E01-04` | `facade.rs::probe_tie_resolved_by_extension` — implemented |
| `DECODE-REQ-62` | Nenhum container reconhece os bytes: `Unsupported("container")`, e a fonte volta à posição 0. | `REQ-E01-04` | `TASK-E01-04` | `facade.rs::probe_nothing_matches`, `facade.rs::random_bytes_are_unsupported` — implemented |
| `DECODE-REQ-63` | O probe pula tags ID3v2 até o limite de metadados; acima dele é `Limit(Metadata)`; cabeçalho malformado não é pulado. | `REQ-E01-04` | `TASK-E01-04` | `facade.rs::probe_skips_large_id3v2_tag`, `probe_malformed_id3v2_does_not_skip`, `probe_id3v2_beyond_metadata_limit_is_a_limit_error` — implemented |
| `DECODE-REQ-64` | O codec é escolhido pelo `CodecId` da trilha; sem codec registrado, `Unsupported("codec")`. | `REQ-E01-04` | `TASK-E01-04` | `facade.rs::codec_lookup` — implemented |
| `DECODE-REQ-65` | Todo descritor declara `Verification`, e o nível embutido corresponde à prova que existe. | `REQ-E17-01` | `TASK-E17-01` | `facade.rs::builtin_formats_have_the_expected_verification_level` — implemented |
| `DECODE-REQ-66` | `default_registry()` contém exatamente os descritores das features ligadas; a lista de formatos que a UI mostra sai de `containers()`/`codecs()` do build real, não de catálogo. | `REQ-E01-04` | `TASK-E01-04` | `facade.rs::codec_lookup` — implemented |
| `DECODE-REQ-67` | Codec `Experimental` continua marcado como tal até ganhar prova externa; não é promovido por passar nos próprios testes. | `REQ-E17-01` | `TASK-E17-01` | `facade.rs::builtin_formats_have_the_expected_verification_level` — implemented |
| `DECODE-REQ-68` | O `Reader` é `Send`, para o engine abrir num fio e decodificar noutro. | `REQ-E01-04` | `TASK-E01-04` | `facade.rs::reader_is_send` — implemented |
| `DECODE-REQ-69` | Registrar um container ou codec novo exige spec em `specs/formatos.md`, fixture e nível de `Verification` justificado na tabela de §3. | `REQ-E17-01` | `TASK-E17-01` | revisão de PR — implemented (processo) |
| `DECODE-REQ-70` | Capability de runtime por tuple (operação, container, codec, perfil, backend, versão, plataforma, limites de taxa/canais, seek/gapless/DRC, evidência e licença), no formato de `schemas/codec-capability.schema.json`. | `REQ-E17-01` | `TASK-E17-01` | — planned (M3) |
| `DECODE-REQ-71` | Rejeição de probe devolve o motivo, não só `Unsupported`. | `REQ-E17-01` | `TASK-E17-01` | — planned (M3) |
| `DECODE-REQ-72` | Só capability `verified` e `available` gera associação de arquivo ou promessa de playback; `planned`/`experimental`/`unavailable` aparecem com rótulo. | `REQ-E17-01` | `TASK-E17-01` | — planned (M3); o catálogo de planejamento já impede `available` em `planned`: `xtask/src/catalog.rs::planned_capability_cannot_advertise_availability` (umbrella) |
| `DECODE-REQ-73` | Suporte a demux não implica decode, e decode não implica encode, passthrough ou escrita de tag. | `REQ-E17-01` | `TASK-E17-01` | — planned (M3) |

## 5. Critérios de aceite

| ID | Given | When | Then | Teste |
|---|---|---|---|---|
| `DECODE-AC-60` | um registro com dois containers que pontuam igual e um que pontua menos | `probe` com `hint_ext` de um dos empatados | vence o empatado cuja extensão bate; o de menor pontuação nunca vence | `facade.rs::probe_tie_resolved_by_extension` |
| `DECODE-AC-61` | um registro em que nenhum `probe` pontua | `probe` | `Err(Unsupported("container"))` e `position() == 0` | `facade.rs::probe_nothing_matches` |
| `DECODE-AC-62` | bytes aleatórios | `open` | `Err(Unsupported("container"))` | `facade.rs::random_bytes_are_unsupported` |
| `DECODE-AC-63` | um WAV precedido de ID3v2 grande dentro do limite | `probe` | identifica `riff` | `facade.rs::probe_skips_large_id3v2_tag` |
| `DECODE-AC-64` | um cabeçalho ID3v2 malformado antes do áudio | `probe` | a tag não é pulada e o resultado é `Unsupported("container")` | `facade.rs::probe_malformed_id3v2_does_not_skip` |
| `DECODE-AC-65` | tags ID3v2 cuja soma passa `max_metadata_bytes` | `probe` | `Err(Limit(Metadata))` | `facade.rs::probe_id3v2_beyond_metadata_limit_is_a_limit_error` |
| `DECODE-AC-66` | `default_registry()` de um build com qualquer combinação de features | `codec_for` em `Pcm`, `Flac`, `Mp1`, `Mp2`, `Mp3` e contagem de `containers()` | cada codec existe se e só se sua feature está ligada, e a contagem de containers é a das features ligadas | `facade.rs::codec_lookup` |
| `DECODE-AC-67` | `default_registry()` | ler `verification` de todos os descritores | `flac` = `Conformance`, `mpeg-audio-layer1` = `Experimental`, os demais = `Oracle` | `facade.rs::builtin_formats_have_the_expected_verification_level` |
| `DECODE-AC-68` | o tipo `Reader` | compilar `assert_send::<Reader>()` | compila | `facade.rs::reader_is_send` |

### Critérios que esta spec **não** fixa

- Os valores numéricos de pontuação de cada `probe`. São detalhe de cada crate de
  formato; o contrato é só a ordem relativa.
- A ordem de registro além da de `default_registry()`. Um registro montado pelo
  chamador desempata pela ordem que ele escolheu.
- Qualquer critério de `DECODE-REQ-70` a `-73`: são `planned` e ganham critério quando
  a TASK sair de `planned`.

## 6. Erros e cancelamento

Erro é dado, não `panic!`. O registro não cancela nada: probe e lookup são limitados
(4096 bytes, `max_metadata_bytes`) e terminam sem esperar.

| Situação | Erro |
|---|---|
| nenhum container pontua | `Error::Unsupported("container")` |
| container aberto, nenhum codec aceita o `CodecId` | `Error::Unsupported("codec")` |
| tags ID3v2 acima do limite | `Error::Limit(LimitKind::Metadata)` |
| erro de I/O da fonte durante o probe | o erro da fonte, propagado |
| erro do `open` do container ou do `make` do codec | o erro do crate de formato (`specs/formatos.md` §6) |

O cancelamento da leitura é o de `specs/decode.md` (`Options` com sinal explícito),
verificado pelo `Reader` antes de cada pacote.

## 7. Fora de escopo

metadata, CUE e capas (C12: a spec anunciou, o crate nao existe — fica registrado como limitacao); encoders; saida de audio

## 8. Dependências

Depende de: `vynamp-dsp` (opcional, feature `resample`)

É dependido por: engine; encode (para transcodificar sem segundo parser)

## 9. Testes e fixtures

- Testes do registro: `crates/vynamp-decode/tests/facade.rs`. Usam descritores
  sintéticos (`LOW`, `TIE_A`, `TIE_B`, `NONE`) com `probe` de pontuação fixa, sem
  arquivo externo.
- O nível `Verification` é provado pelos testes de cada formato:
  `conformance_flac.rs` (suíte IETF), `oracle_flac.rs`, `oracle_pcm.rs`,
  `oracle_mpeg.rs` (FFmpeg). Os que dependem de corpus grande ficam atrás de
  `#[ignore]` com motivo escrito, como manda `specs/formatos.md` §9.
- O catálogo de planejamento (`config/format-catalog.json`) e suas guardas vivem no
  umbrella e são testados em `xtask/src/catalog.rs`; não são o registro de runtime.
- Comando: `cargo test -p vynamp-decode --test facade`, e com
  `--no-default-features --features <lista>` para conferir `DECODE-AC-66` por
  combinação.

## 10. Limitações conhecidas

| Limitação | Consequência | Quem cobre |
|---|---|---|
| O registro tem 4 containers e 4 codecs: RIFF/AIFF/FLAC/MPEG audio. | Os outros 60+ formatos do catálogo seguem `planned`; nenhum é anunciado. | `specs/formatos.md` §4, `TASK-E01-07/08/09`, `TASK-E17-03/05` |
| Não há capability de runtime por tuple, só `Verification` por descritor. | A UI só sabe "abre ou não abre", sem perfil, backend ou limites. | `DECODE-REQ-70`, `TASK-E17-01` (M3) |
| Rejeição de probe não diz o motivo. | O usuário vê só "formato não suportado". | `DECODE-REQ-71` |
| MPEG Layer I é `Experimental`. | Não pode ser vendido como suporte verificado. | `TASK-E17-17` (legados) |
| MPEG Layer II/III é `Oracle` contra FFmpeg, não `Conformance`. | Correção é medida pelo critério ISO/IEC 11172-4 contra referência, não por vetores oficiais, que não são redistribuíveis. | aceito; reavaliar se os vetores forem licenciados |
| `Verification` é declarado à mão no descritor. | Um descritor novo pode declarar nível errado. | `DECODE-AC-67` trava os embutidos; `DECODE-REQ-69` exige justificativa em PR |
| O registro é estático (`&'static`); não há registro de plugin em runtime. | Formato novo exige recompilar. | decisão de projeto; plugins de codec são `TASK-E17-17` (helper de binding) |
