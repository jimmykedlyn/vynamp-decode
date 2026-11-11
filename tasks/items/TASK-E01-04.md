# TASK-E01-04 — Entregar fonte local e decoder registry

> Gerado pelo passo 37 a partir de `tasks/backlog.json`. Os campos de estado vêm do
> backlog e não devem ser editados aqui. A coluna `Then` é o requisito aprovado,
> copiado literal do backlog; `Given` e `When` foram autorados na execução.

| Campo | Valor |
|---|---|
| ID | `TASK-E01-04` |
| Módulo dono | `vynamp-decode` |
| Repo | `jimmykedlyn/vynamp-decode` |
| REQ coberto | `REQ-E01-04` |
| Critérios da spec | pendente — `specs/decode.md` §5 |
| Fase | M1 |
| Prioridade | P1 |
| Estado | planned |
| Depende de | `TASK-E01-03` |
| Bloqueia | `TASK-E01-05`, `TASK-E01-07` |
| Modelo executor | Opus — áudio/RT, concorrência, segurança ou release |
| Esforço | 3–8 dias úteis |
| Risco | alto |

## Entrega

Adapters local source/probe/decode para WAV/AIFF/FLAC/MP3.

## Boundary

- **Não** cria o registry. Ele já existe em `vynamp-decode-core/src/registry.rs`
  (`Registry::probe`, `add_container`, `add_codec`, `containers()`, `codecs()`); esta TASK
  entrega os adapters de WAV/AIFF/FLAC/MP3 que se registram nele. Reescrever o registry
  seria trabalho de `TASK-E01-03`, não desta.
- **Não** lista perfis nem taxas por formato. A API do registry, em
  `specs/registro-capacidades.md`, enumera container, nome, extensões e nível de
  verificação — não há `profiles` nem lista de sample rates no descritor. "Perfis
  reais" do AC-3 teria de sair do catálogo de planejamento do umbrella, que é
  dado, não runtime.
- **Não** lê o catálogo `config/format-catalog.json`. Ele é do umbrella, tem 68
  entradas todas `state: planned` e é validado por `cargo xtask ci docs`, e
  `specs/registro-capacidades.md` fixa que o decoder não o consulta em tempo de
  execução.
- **Não** cobre AAC, ALAC, Vorbis, Opus, APE, WV, TTA, MPC nem WMA. A lista desta TASK é
  WAV, AIFF, FLAC e MP3; o resto é `TASK-E01-07`, `TASK-E01-08` e `TASK-E17-05`.
- **Não** implementa probing de DSF/DFF, CAF, AU ou Ogg. `TASK-E17-03` e `TASK-E01-09`
  são donas dos containers e do DSD para PCM.
- **Não** decide política de tamanho máximo por arquivo. Os limites são os de
  `Limits` (`max_bytes`, `max_frames`, `max_channels`, `max_sample_rate`),
  declarados em `specs/decode.md`, e quem os configura é o chamador; o adapter só
  devolve o erro tipado.

## Antes de começar

- [ ] Spec do módulo aprovada e o critério de aceite citado já existe nela
- [ ] Worktree própria criada (`cargo xtask repo worktree task-e01-04-claude-<slug>`)
- [ ] Fixtures/hardware necessários disponíveis ou limitação registrada
- [ ] Nenhuma TASK em execução tocando os mesmos arquivos

## Critérios de aceite

| ID | Given | When | Then |
|---|---|---|---|
| `TASK-E01-04-AC-1` | um FLAC renomeado para .mp3 e um MP3 renomeado para .wav | o probe lê o cabeçalho sem consultar a extensão e o registry escolhe o decoder pelo conteúdo | Extensão incorreta permite sniff |
| `TASK-E01-04-AC-2` | um arquivo truncado no meio do streaminfo e um cabeçalho que declara 2^31 frames, com o limite de tamanho configurado | o probe e o decode rodam sobre os dois arquivos | corrupt/oversized falham com erro tipado |
| `TASK-E01-04-AC-3` | a build com as features de decoder fixadas no ADR-002 ativas | a API do registry enumera formatos, perfis e taxas suportados | decoder lista perfis reais. |

## Verificação

| Item | Comando / fixture | Evidência esperada |
|---|---|---|
| Teste automatizado | `cargo test --workspace --locked` em `libs/decode`, mais `cargo test -p vynamp-decode --test facade --locked` e `cargo run --example support_matrix` | **medido em 2026-11-10**: 508 verdes, 3 ignorados, 0 falhas em 47 suítes; a suíte `facade` dá 27 verdes, incluindo `probe_tie_resolved_by_extension`, `probe_nothing_matches` e `random_bytes_are_unsupported`. O `support_matrix` imprime 4 containers (riff, aiff, flac, mpa) e 4 codecs (pcm, flac, mpeg-audio, mpeg-audio-layer1) |
| Oráculo independente | `cargo test -p vynamp-decode --test oracle_flac --test oracle_pcm --test oracle_mpeg --locked` sobre `tests/fixtures/{flac,pcm,mpeg}/MANIFEST.txt` (sha256 por arquivo), mais os testes de probe por conteúdo: `probe_rejects_others` em `crates/vynamp-decode-riff/tests/riff.rs:146` e `crates/vynamp-decode-aiff/tests/aiff.rs:313` | **medido**: um FLAC gravado como `.mp3` abre com `codec=Flac`, um MP2 gravado como `.wav` abre com `codec=Mp2` e um WAV gravado como `.flac` abre com `Pcm(Signed, 16 bits)`, os três em `Mode::Strict` e decodificando até o fim — a extensão não decide nada. FLAC truncado em 40 bytes dá `Invalid { what: "unexpected eof", offset: 40 }`; `STREAMINFO` declarando 2^31-1 frames com `max_frames: 1_000_000` dá `Limit(Frames)`. **Ausência medida**: nem `ContainerDescriptor` nem `CodecDescriptor` têm campo de perfis ou de taxas, então a enumeração do AC-3 sai com nome, extensões e nível de verificação — não com perfis |
| Hardware | nenhum: o probe lê cabeçalho e o decode compara contra fixtures versionadas; não há dispositivo de áudio em nenhum caminho | ausência é limitação registrada, não aprovação |

## DoD — pronto para review

- [ ] Todos os critérios de aceite com evidência
- [ ] `cargo fmt --check` e `cargo clippy --all-targets -- -D warnings` limpos
- [ ] `cargo test` verde; testes ignorados listados com motivo
- [ ] Spec do módulo atualizada se a implementação revelou algo novo
- [ ] `backlog.json` com o estado correto
- [ ] Revisão feita por contexto limpo, não pelo implementador
- [ ] PR aberto com template preenchido; CI Gate verde no SHA final
## Notas de execução

Preenchido durante a execução, não antes.
