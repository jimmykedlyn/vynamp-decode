# TASK-E01-01 — Provar decode, gapless e precisão de samples

> Gerado pelo passo 37 a partir de `tasks/backlog.json`. Os campos de estado vêm do
> backlog e não devem ser editados aqui. A coluna `Then` é o requisito aprovado,
> copiado literal do backlog; `Given` e `When` foram autorados na execução.

| Campo | Valor |
|---|---|
| ID | `TASK-E01-01` |
| Módulo dono | `vynamp-decode` |
| Repo | `jimmykedlyn/vynamp-decode` |
| REQ coberto | `REQ-E01-01` |
| Critérios da spec | pendente — `specs/decode.md` §5 |
| Fase | M0 |
| Prioridade | P0 |
| Estado | planned |
| Depende de | `TASK-E11-01` |
| Bloqueia | `TASK-E01-03`, `TASK-E04-02`, `TASK-E13-01` |
| Modelo executor | Opus — áudio/RT, concorrência, segurança ou release |
| Esforço | 2–5 dias úteis |
| Risco | alto |

## Entrega

Spike de decoder registry, corpus básico e ADR-002 com versões/features propostas.

## Boundary

- **Não** congela a ADR-002. As notas medem que ela se declara "especificação proposta,
  implementação não iniciada" e parcialmente substituída pelo ADR-011; congelar aqui
  criaria uma segunda fonte de verdade para a escolha de backend que a ADR posterior já
  derrubou. O congelamento é decisão do passo 43, não desta TASK.
- **Não** escolhe backend de terceiro. O `Cargo.toml` de `vynamp-decode` lista só crates
  `vynamp-*` do workspace: Symphonia e CPAL não estão lá, e os decoders são Rust próprio.
  Escolher um candidato aqui contradiria o ADR-011.
- **Não** prova WAV contra referência de terceiro. WAV é a referência que os outros
  comparam contra; o que existe para ele é o sha256 do `MANIFEST.txt` e o teste de
  adulteração, ambos fixados em `specs/decode.md`. Tratar "WAV decodifica" como
  comparação seria inventar o oráculo que falta.
- **Não** torna condicional a preservação de PCM24/32. O `Then` condiciona a um
  "perfil", e `specs/formatos.md` não tem regra de precisão por perfil: a
  preservação medida é incondicional.
- **Não** cobre AAC, ALAC, Vorbis, Opus, APE, WV ou TTA. O corpus desta TASK é WAV, FLAC e
  MPEG audio; os outros codecs são `TASK-E01-07` e `TASK-E01-08`.
- **Não** fixa contratos de priming e preskip por codec. O trim de delay e padding que os
  testes exercitam é o do `Reader`; o contrato por demux/decoder é `TASK-E17-02`.

## Antes de começar

- [ ] Spec do módulo aprovada e o critério de aceite citado já existe nela
- [ ] Worktree própria criada (`cargo xtask repo worktree task-e01-01-claude-<slug>`)
- [ ] Fixtures/hardware necessários disponíveis ou limitação registrada
- [ ] Nenhuma TASK em execução tocando os mesmos arquivos

## Critérios de aceite

| ID | Given | When | Then |
|---|---|---|---|
| `TASK-E01-01-AC-1` | três fixtures geradas pelo próprio spike — WAV PCM16, FLAC e MP3 do mesmo tom — com hash registrado no manifesto | cada arquivo é decodificado pelo backend candidato escolhido no ADR-002 e o PCM resultante é comparado sample a sample com o WAV de origem | FLAC/MP3/WAV decodificam contra referência |
| `TASK-E01-01-AC-2` | um FLAC com encoder delay e padding declarados no streaminfo, um WAV PCM24 e um WAV PCM32 | cada um é decodificado e a contagem de frames e os bits válidos são comparados antes e depois da remoção de delay e padding | delay/padding e PCM24/32 preservados quando perfil permite. |

## Verificação

| Item | Comando / fixture | Evidência esperada |
|---|---|---|
| Teste automatizado | `cargo test --workspace --locked` em `libs/decode`, depois `cargo test -p vynamp-decode --test oracle_flac --test oracle_pcm --test oracle_mpeg --locked` | **medido em 2026-11-10**: 508 verdes, 3 ignorados, 0 falhas em 47 suítes; as três suítes de oráculo dão 9 verdes. Os ignorados são os dois `write`/`analyse` de `blackbox.rs` e um doc-test de `testkit/src/alloc.rs` |
| Oráculo independente | `cargo test -p vynamp-decode --test oracle_flac --test oracle_pcm --test oracle_mpeg --locked`, com as fixtures de `tests/fixtures/{flac,pcm,mpeg}/` e a referência que cada `MANIFEST.txt` nomeia. `oracle_flac.rs` confere sha256, o `STREAMINFO` bps contra `expected_bps(reference)` e compara frame a frame com o WAV ao lado; `oracle_mpeg.rs` confere sha256 do arquivo e da referência `.f32` e aplica o critério ISO/IEC 11172-4 (`rms < 2^-15/√12`, `max ≤ 2^-14`); `oracle_pcm.rs` compara contra o sinal documentado e o hash fixado | **medido**: 9 verdes. Adulterar um byte de `tests/fixtures/pcm/sine-s16le.wav` e repetir o comando dá **exit 101**: `manifest_files_decode_to_the_source` panica em `oracle_pcm.rs:144` com `left: "db4a9e0d…"` contra `right: "1592a474…"`; restaurado o byte, `git status` volta limpo — o hash é medido, não decorativo |
| Hardware | nenhum: o corpus é WAV/FLAC/MPEG versionado com hash e as comparações são numéricas; nenhum teste abre dispositivo de áudio | ausência é limitação registrada, não aprovação — nada aqui ouve um arquivo |

## DoD — pronto para review

- [ ] Todos os critérios de aceite com evidência
- [ ] `cargo fmt --check` e `cargo clippy --all-targets -- -D warnings` limpos
- [ ] `cargo test` verde; testes ignorados listados com motivo
- [ ] Spec do módulo atualizada se a implementação revelou algo novo
- [ ] `backlog.json` com o estado correto
- [ ] Revisão feita por contexto limpo, não pelo implementador
- [ ] PR aberto com template preenchido; CI Gate verde no SHA final
## Notas de execução

Medido em 2026-11-09. Provas em `evidence/PR/task-e01-01/`.

| AC | Verificação executada | Resultado |
|---|---|---|
| AC-1 | `cargo test --workspace --locked` em `vynamp-decode` (508 testes verdes, 3 ignorados, 0 falhas). Os três oráculos que importam: `tests/oracle_flac.rs:83` `every_manifest_file_has_its_hash_and_decodes_to_the_reference`, `tests/oracle_pcm.rs`, `tests/oracle_mpeg.rs`. Cada lê o MANIFEST, confere sha256 do arquivo, decodifica e compara contra o WAV/f32 de referência: rate, formato, samples, e `expected_bps(reference)` (`oracle_flac.rs:73`) contra o STREAMINFO | **Satisfeito para FLAC e MP3, não literal para WAV.** FLAC: referência WAV ao lado de cada `.flac` e comparação sample a sample. MP3: referência `.f32` de cada arquivo. **WAV é o que se compara *contra*, não se compara *com***: um WAV não tem WAV de referência, ele é a referência. O `Then` pede os três contra referência; para WAV o que existe é o sha256 no manifesto + as provas de verificações de `oracle_pcm.rs` (adulteração de um byte → `exit 101`). Registro como leitura: "WAV decodifica" está provado pelo hash e pela reação à adulteração, não por comparação contra terceiro |
| AC-1, delay/padding | `crates/vynamp-decode/tests/facade.rs`: `gapless_delay_and_padding_span_packets` (503), `frames_and_position_follow_the_trimmed_timeline` (560), `delay_plus_padding_beyond_frames_is_empty` (578), `unknown_length_still_trims_delay` (588); `crates/vynamp-decode-mpa/tests/demux.rs:223` `lame_tag_gives_exact_gapless_numbers` e `:1268` `a_tag_that_does_not_fit_the_frame_count_is_a_gapless_conflict`. Implementação em `crates/vynamp-decode/src/reader.rs:58-70` (`skip_until: delay`, `enforce_end: track.padding > 0`) e `:100-108` (trimmed frames) | **Satisfeito.** Delay e padding são cortados na facada e os testes cobrem o caso normal, o que passa de `frames`, o comprimento desconhecido e o conflito com tag |
| AC-2, PCM24/32 | `crates/vynamp-decode-pcm/tests/pcm.rs:96` `s24_both_endians_sign_extend` e `:113` `s32_both_endians`. Fixtures `tests/fixtures/pcm/sine-s24le.wav`, `sine-s32le.wav`, `sine-s24be.aif`, `sine-6ch-s24le.wav` — todos no MANIFEST. `SampleFormat::{I24, I32}` com armazenamento compartilhado em `crates/vynamp-decode-core/src/buffer.rs:10,85-101` | **Satisfeito.** Os dois formatos existem, têm fixture com hash no manifesto e teste dos dois endianness com extensão de sinal |
| AC-2, "quando perfil permite" | o `Then` condiciona a preservação a um **perfil**; o catálogo (`config/format-catalog.json`) tem `profiles` como lista de strings, sem regra de precisão por perfil | **A condição não tem objeto.** Não existe perfil que habilite ou desabilite PCM24/32 no produto hoje. Preservação medida e incondicional |

**O que esta TASK não provou, e o que ela produziria se fosse feita.**

O `deliverable` é "Spike de decoder registry, corpus básico e **ADR-002** com
versões/features propostas". Medido:

- **O spike de registry existe, mas não é o que o `deliverable` pede.** A frase
  anterior desta nota dizia que `find . -name "*.rs" | grep -i registry` não retorna
  nada. **Está errada**: `crates/vynamp-decode-core/src/registry.rs` existe desde
  `7f47c0a` (2026-10-23), duas semanas antes desta medição, e é código de runtime —
  `Registry`, `ContainerDescriptor`, `CodecDescriptor`, `Verification`, `probe()`
  com skip de ID3v2 e desempate por extensão, `codec_for()`. `crates/vynamp-decode/src/lib.rs:106`
  registra quatro containers (RIFF, AIFF, FLAC, MPA) e quatro codecs. A conclusão que
  sobrou é mais estreita e continua de pé: **esse registry é o do produto, não um
  spike**, e o que o `deliverable` pede em separado é o ADR-002 com versões/features.
  O outro catálogo, `config/format-catalog.json` + `xtask/src/catalog.rs` (68
  entradas, validação de `required`/alvos/`association_allowed`), é **planejamento**,
  não runtime: marca as 68 entradas `state: planned`, e
  `reference/audio/06-codec-capability-registry.md:5` diz "Sem decoder implementado
  neste pacote, nenhum perfil é anunciado disponível".
  O backend candidato do ADR-002 (Symphonia/CPAL) **não existe no `Cargo.toml`** do
  decode: os codecs são Rust próprio, sem candidato nomeado.
- **ADR-002 está proposto, não congelado.** `adr/ADR-002-audio-core.md:3`:
  "Estado: especificação proposta, implementação não iniciada… congelar após
  TASK-E01-01/TASK-E01-02". E o rodapé diz: "Parcialmente substituída por
  [ADR-011](ADR-011-pilha-propria.md)". Congelar ADR-002 neste estado seria
  congelar uma decisão que o próprio arquivo reconhece como parcialmente
  substituída — o passo 43 (congelar ADRs) precisa decidir isso antes.

**Custo se essa leitura estiver errada:** se o mantenedor considerar que a suíte de
oráculos mais o `registry.rs` de runtime já são o "spike de registry", a TASK fecha
com AC-1 e AC-2 satisfeitos, com a ressalva de WAV, e o que sobra é só o ADR-002. Se
a leitura correta for que o registry spike e o ADR-002 congelado são entregáveis
separáveis, a TASK fica em dois blocos: a prova de decodificação está feita, o ADR
não — e o ADR não pode ser congelado enquanto ADR-011 o substituir parcialmente.

**Limitação honesta:** nenhuma dessas medições foi feita em hardware de áudio. Nada
aqui ouve um arquivo; tudo é comparação numérica contra referência versionada.
