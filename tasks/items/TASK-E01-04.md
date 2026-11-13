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
| Critérios da spec | `specs/decode.md` §5 (aprovada em 2026-11-13) — `DECODE-AC-01`, `-02`, `-03`, `-04`, `-10`, `-11`, `-13`, `-14`, `-15`, `-22`, `-23`, `-26`, `-32`, `-33`, conforme o mapa `DECODE-REQ-01/02/03/10/11/13/14/15/22/23` → `TASK-E01-04` da §4 |
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
| `TASK-E01-04-AC-3` | a build com as features de decoder fixadas no ADR-002 ativas | a API do registry enumera os containers e codecs que as features ligaram | decoder lista os formatos que abrem com nome, extensões e nível de verificação. |

> **AC-3 corrigido em 2026-11-13**, pelo mantenedor, na abertura da execução. O `Then`
> aprovado dizia "decoder lista perfis reais", e a Boundary desta mesma TASK proíbe perfis
> por formato: nem `ContainerDescriptor` nem `CodecDescriptor` têm campo de perfil ou de
> taxa. Perfil é tuple de runtime — `DECODE-REQ-70`, dona `TASK-E17-01`. O `Then` passou a
> ser **"decoder lista os formatos que abrem com nome, extensões e nível de verificação"**,
> provado por `registry_enumerates_names_extensions_and_verification` (`DECODE-AC-33`).
> O texto foi corrigido nos três lugares que o gate compara — `tasks/backlog.json`,
> `product/03-requisitos.md` e esta tabela — no PR de repin do umbrella, porque
> `cargo xtask check-tasks` exige que cada cláusula do `acceptance` apareça aqui.

## Verificação

| Item | Comando / fixture | Evidência esperada |
|---|---|---|
| Teste automatizado | `cargo test --workspace --locked` em `libs/decode`, mais `cargo test -p vynamp-decode --test facade --locked` e `cargo run --example support_matrix` | **medido em 2026-11-13**: 517 verdes, 3 ignorados, 0 falhas em 48 suítes (baseline da abertura: 513/48). A suíte `facade` dá 29 verdes, incluindo `renamed_files_probe_by_content_not_by_extension` e `registry_enumerates_names_extensions_and_verification`. O `support_matrix` imprime 4 containers (riff, aiff, flac, mpa) e 4 codecs (pcm, flac, mpeg-audio, mpeg-audio-layer1) |
| Oráculo independente | `cargo test -p vynamp-decode --test oracle_flac --test oracle_pcm --test oracle_mpeg --locked` sobre `tests/fixtures/{flac,pcm,mpeg}/MANIFEST.txt` (sha256 por arquivo), mais os testes de probe por conteúdo: `probe_rejects_others` em `crates/vynamp-decode-riff/tests/riff.rs:146` e `crates/vynamp-decode-aiff/tests/aiff.rs:313` | **medido em 2026-11-13, por teste que falha quando a implementação mente**: o FLAC gravado como `.mp3`, o MP2 gravado como `.wav` e o WAV gravado como `.flac` são agora arquivos reais em diretório temporário, e cada um abre com o codec do seu conteúdo e decodifica até o fim em `Mode::Strict`. FLAC cortado em 40 bytes dá `("unexpected eof", 40)`; `STREAMINFO` declarando 2³¹−1 frames com `max_frames: 1_000_000` dá `Limit(Frames)`, e o mesmo arquivo com 1 000 000 declarados abre. **Ausência medida**: nem `ContainerDescriptor` nem `CodecDescriptor` têm campo de perfis ou de taxas — a enumeração do AC-3 sai com nome, extensões e verificação, e a ausência está registrada na §10 da spec |
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

### 2026-11-13 — execução

**O que já existia.** Os adapters de WAV/AIFF/FLAC/MP3 já estavam registrados em
`crates/vynamp-decode/src/lib.rs` e o registry já estava implementado. O produto desta
TASK não foi código de decode novo: foi **prova**, mais a correção do AC-3. O baseline
medido na abertura foi 513 verdes em 48 suítes, com os três comportamentos dos ACs já
funcionando — mas **sem teste que os discriminasse** nos pontos que o AC pede.

**Por que a prova de vermelho foi por mutação.** Os testes cobrem comportamento que já
funcionava, então não há vermelho natural. Cada teste novo foi validado quebrando a
implementação de propósito e conferindo que ele falha, com a mutação revertida no mesmo
commit:

| Teste | Mutação aplicada | Resultado |
|---|---|---|
| `declared_length_beyond_max_frames_is_a_typed_error` | removido `opts.limits.check_frames(f)` em `flac/src/demux.rs` | falhou com `left: None, right: Some(Limit(Frames))` |
| `truncated_streaminfo_is_a_typed_error` | `ByteReader::eof` passou a reportar `offset: 0` | falhou com `left: ("unexpected eof", 0), right: ("unexpected eof", 40)` |
| `renamed_files_probe_by_content_not_by_extension` | `Registry::probe` passou a devolver o descritor pela extensão antes do score | falhou com `invalid mpeg sync at offset 0` no FLAC renomeado para `.mp3` |
| `registry_enumerates_names_extensions_and_verification` | removido o registro de `MPEG_AUDIO_LAYER1_CODEC` | falhou com a lista de codecs sem `mpeg-audio-layer1` |

**Decisão de fixture.** O teste do AC-1 precisa de arquivo renomeado no disco. O
workspace tem **0 pacotes de registry** no `Cargo.lock` (propriedade medida no ADR-002),
então `tempfile` não entrou como dev-dependency: o diretório é montado com
`std::env::temp_dir()` e o nome leva o PID, para duas execuções em paralelo não colidirem.

**Limitação reavaliada.** `Limits::max_depth` não é aplicado por nenhum demuxer, e a §10
apontava esta TASK como dona. Os quatro containers desta TASK são planos (RIFF, AIFF,
FLAC, MPEG audio não têm container dentro de container), então não há profundidade a
limitar e nada foi implementado. A linha da §10 foi reescrita para dizer isso e apontar
`TASK-E17-03`/`TASK-E01-07` como donas reais.

**Dependência.** O backlog ordena `TASK-E01-03` antes desta. O `registry.rs` de que esta
TASK dependia já existe e está verde; E01-03 é a state machine do `engine`, que não toca
este repositório. A ordem do backlog foi tratada como ordenação, não como bloqueio de
código, e isso está registrado no corpo do PR.
