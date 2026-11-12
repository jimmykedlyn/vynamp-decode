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
| Critérios da spec | `specs/decode.md` §5 — `DECODE-AC-05`, `-06`, `-07`, `-20`, `-21`, `-22`, `-28` a `-31` |
| Fase | M0 |
| Prioridade | P0 |
| Estado | implemented |
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

- [x] Spec do módulo aprovada e o critério de aceite citado já existe nela — a spec
  `specs/decode.md` está **autorada e não aprovada** (v0.3.0, estado no cabeçalho) e §5
  nomeia os testes de `triangulation.rs` em `DECODE-AC-28` a `-31`
- [x] Worktree própria criada (`cargo xtask repo worktree task-e01-01-claude-<slug>`) —
  medido: `cargo xtask repo worktree` existe só no umbrella e cria worktree de umbrella,
  que não isola o código do módulo; a branch de trabalho foi aberta no próprio submódulo,
  como no precedente `TASK-E04-02`. `git worktree list` e `.git/auralis/claims/` vazios
- [x] Fixtures/hardware necessários disponíveis ou limitação registrada — fixtures
  versionadas com hash; **hardware: nenhum**, e a ausência está registrada na linha
  `Hardware` de §Verificação
- [x] Nenhuma TASK em execução tocando os mesmos arquivos — nenhum outro worktree no
  umbrella nem no submódulo, e nenhuma claim ativa em 2026-11-12

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

- [x] Todos os critérios de aceite com evidência — a tabela da seção de notas de execução
  abaixo, uma linha por AC
- [x] `cargo fmt --check` e `cargo clippy --all-targets -- -D warnings` limpos — em
  2026-11-12, no workspace de `libs/decode`
- [x] `cargo test` verde; testes ignorados listados com motivo — 513 verdes, 3 ignorados,
  cada um com o motivo na própria anotação
- [x] Spec do módulo atualizada se a implementação revelou algo novo — `specs/decode.md`
  v0.3.0: §10 ganhou a limitação do STREAMINFO, §5 os critérios `DECODE-AC-28` a `-31` e
  §9 o corpus de gapless com manifesto
- [x] `backlog.json` com o estado correto — `implemented` no umbrella, no PR de repin que
  segue o merge deste
- [ ] Revisão feita por contexto limpo, não pelo implementador — **pendente:** quem
  escreveu não revisa; a revisão é do subagente `revisor` sobre o diff, em contexto limpo
- [x] PR aberto com template preenchido; CI Gate verde no SHA final — PR
  [#27](https://github.com/jimmykedlyn/vynamp-decode/pull/27), base `develop`, template
  preenchido; `CI Gate` verde em `292897d` com `fmt`, `clippy`, `test` e `machete` todos
  `SUCCESS` e `mergeStateStatus` `CLEAN`. Este commit muda o SHA, e o Gate roda de novo
  sobre ele; o que está registrado é o veredito medido, não uma promessa

## Notas de execução

Fechada em 2026-11-12. A evidência da suíte desta execução entra em
`evidence/PR/task-e01-01/` no umbrella, pelo PR de repin que segue o merge deste — antes
do merge não há SHA de módulo para a evidência apontar.

A nota anterior desta seção dava o AC-1 por "satisfeito para FLAC e MP3, não literal para
WAV". O erro estava no meio da frase: **para MP3 o `Then` também não é literal**, porque
um codec com perda não decodifica sample a sample para o PCM de origem. Ela também não
mencionava que o corpus de MP3 gapless estava versionado **sem manifesto** — quinze
arquivos sem hash, contra a regra de `engineering/05-testes.md`. As duas coisas foram
corrigidas nesta execução, e a nota antiga não é apagada do histórico: o `git log` guarda
as duas versões.

| AC | O que foi executado | Resultado |
|---|---|---|
| AC-1, um tom só | `crates/vynamp-decode/tests/triangulation.rs:233` `wav_flac_and_mp3_of_the_same_tone_decode_against_one_reference`. Afirma primeiro que `tests/fixtures/pcm/sine-s16le.wav` e `tests/fixtures/flac/ref/st16-48k.wav` têm o **mesmo sha256** (`1592a474…`): a referência do FLAC é o próprio fixture PCM. Depois compara cada perna: WAV e FLAC, ambos `I16`, 4 800 frames, iguais ao tom de origem deslocado (`>>16`), e iguais **entre si**; MP3 (`g-cbr128-48k-stereo`), `F32`, 14 400 frames, acima de 20 dB de SNR contra o mesmo tom | **Satisfeito, com o critério que cada perna permite.** WAV e FLAC: igualdade exata. MP3: perda impede igualdade, então o critério é SNR contra a origem — **medido 25,9 dB** — mais o critério ISO/IEC 11172-4 contra o decode do FFmpeg, medido em `tests/mpeg.rs` (`rms 3,1e-7`, `max 1,6e-6`, ambos abaixo de `2^-15/√12` e `2^-14`). O `Then` literal ("sample a sample com o WAV de origem") é satisfeito para os dois lossless e não tem objeto para o MP3 |
| AC-1, hash do corpus | `triangulation.rs:142` `every_gapless_fixture_has_its_hash_in_the_manifest` sobre o novo `crates/vynamp-decode/tests/fixtures/mpeg/triangulation.sha256`: 15 entradas, sha256 conferido arquivo a arquivo, e a lista do manifesto tem de ser exatamente a do diretório (subset reprova). `triangulation.rs:174` `a_flipped_byte_leaves_the_pinned_hash` prova que o hash morde: um bit trocado no `.src.wav` sai do valor fixado | **Satisfeito.** O manifesto é gerado por medição, não digitado: `tools/fixtures/generate-mpeg-dev.ps1` passou a escrevê-lo, e as 15 fixtures foram reproduzidas **byte a byte** com ffmpeg `n8.1.3-35`. Adulterar um hash no manifesto faz o teste falhar (`exit 101`), verificado |
| AC-2, delay/padding | `triangulation.rs:292` `the_gapless_trim_is_declared_by_mpeg_and_absent_in_flac`. FLAC (`st16-level0`): `delay == 0`, `padding == 0`, `gapless == Exact`, saída = 4 800 = `total_samples`. MP3 com tag LAME: `delay == 1105`, `padding == 623`, `gapless == FromMetadata`, e `14 400 + 1105 + 623 == 16 128` frames declarados | **Satisfeito no codec que declara os campos, e a condição do AC não existe no FLAC.** O `Then` pede "delay e padding declarados no streaminfo"; o STREAMINFO do RFC 9639 §8.2 tem 34 bytes e nenhum campo para isso — o codec é lossless, sem priming. Registrado como limitação em `specs/decode.md` §10, que é onde o contrato manda registrar divergência de referência |
| AC-2, PCM24/32 | `triangulation.rs:323` `pcm24_and_pcm32_keep_every_valid_bit`: `sine-s32le.wav` (`I32`) é igual ao tom de origem, e `sine-s24le.wav` (`I24`) é igual ao tom `>>8`, amostra a amostra. O teste também afirma que o tom de origem **tem** bits abaixo do corte de 16 — sem isso o fixture `s24` seria um `s16` esticado e passaria sem provar nada sobre os 8 bits baixos | **Satisfeito.** Complementa, sem substituir, `vynamp-decode-pcm/tests/pcm.rs:96` e `:113`, que cobrem os dois endianness e a extensão de sinal |
| AC-2, "quando perfil permite" | o catálogo `config/format-catalog.json` tem `profiles` como lista de strings, sem regra de precisão por perfil | **A condição continua sem objeto.** Nenhum perfil habilita ou desabilita PCM24/32 no produto; a preservação medida é incondicional |

**Estado da suíte.** `cargo test --workspace --locked` em 2026-11-12: **513 verdes, 3
ignorados, 0 falhas**, em 48 suítes. Os cinco novos são os de `triangulation.rs`. Os três
ignorados são os mesmos de antes, cada um com motivo escrito: `blackbox::intensity_probe_gains`
e `blackbox::write_blackbox_streams` ("writes and analyses scratch files for the black-box
comparison") e o doc-test de `alloc.rs`. `cargo fmt --all --check` e `cargo clippy
--workspace --all-targets -- -D warnings` limpos. `cargo machete` não está instalado neste
ambiente: ausência registrada, não pulada em silêncio.

**O que continua de fora, e por quê.**

- **HARDWARE: nenhum.** Nada aqui ouve um arquivo. Todas as comparações são numéricas
  contra referência versionada, e nenhum teste abre dispositivo de áudio.
- **O `deliverable` pedia "Spike de decoder registry".** O registry existe e é de
  **produto** (`crates/vynamp-decode-core/src/registry.rs`, desde `7f47c0a`), não um
  spike; a distinção não tem objeto. As versões e features que o `deliverable` também
  pedia estão escritas na §"Backend, versões e features" da ADR-002.
- **A ADR-002 não foi congelada aqui**, e a Boundary desta TASK o proíbe: o congelamento
  é do passo 43, e a ADR-011 substituiu parte dela.
- **`cargo machete` ausente** neste ambiente; o `ci-lib.yml` do módulo trata ausência como
  registro, não como portão.

**Limitação achada de passagem, não corrigida.** Rodar
`tools/fixtures/generate-mpeg-dev.ps1` inteiro regera quatro fixtures de
`crates/vynamp-decode-mpa/tests/fixtures/` com **bytes diferentes e o mesmo tamanho**
(`cbr128-44k-stereo.mp3`, `cbr128-48k-stereo.mp3`, `mpeg2-22k-stereo.mp3`,
`nolame-cbr128-48k.mp3`). As fifteen do gapless são reproduzíveis byte a byte; essas
quatro não são. O defeito é do gerador ou da versão do libmp3lame, é anterior a esta TASK
e não pertence a ela — fica registrado para a TASK que for dona do corpus MPEG.
