# TASK-E17-01 — Fixar catálogo por binding, capacidades e corpus

> Gerado pelo passo 37 a partir de `tasks/backlog.json`. Os campos de estado vêm do
> backlog e não devem ser editados aqui. A coluna `Then` é o requisito aprovado,
> copiado literal do backlog; `Given` e `When` foram autorados na execução.

| Campo | Valor |
|---|---|
| ID | `TASK-E17-01` |
| Módulo dono | `vynamp-decode` |
| Repo | `jimmykedlyn/vynamp-decode` |
| REQ coberto | `REQ-E17-01` |
| Critérios da spec | pendente — `specs/decode.md` §5 |
| Fase | M0 |
| Prioridade | P0 |
| Estado | planned |
| Depende de | `TASK-E11-01`, `TASK-E14-01` |
| Bloqueia | `TASK-E16-09`, `TASK-E17-04`, `TASK-E17-12`, `TASK-E17-13`, `TASK-E17-14`, `TASK-E17-15` |
| Modelo executor | Opus — áudio/RT, concorrência, segurança ou release |
| Esforço | 2–5 dias úteis |
| Risco | alto |

## Entrega

Catálogo versionado, spikes de backends/licenças e modelo de capability por alvo. Spike M0 fixa algoritmo/oversampling/latência do guard true-peak e estimador independente com fixtures.

## Boundary

- **Não** implementa o guard true-peak nem o estimador independente. As notas medem que o
  `grep` por `true.?peak|bs.1770|oversampl` em `config/`, `schemas/` e no decode não
  encontra código; o que existe é texto de spec e a linha do roadmap. Fixar algoritmo,
  oversampling e latência é `TASK-E17-07`, e medir o teto com oráculo é `TASK-E17-08`.
- **Não** entrega um spike de backend por alvo. `backend_candidates` tem 3 valores
  distintos em 68 entradas e 65 delas repetem o mesmo marcador; não há biblioteca de
  terceiro no repo para medir. A escolha de backend foi decidida pelo ADR-011.
- **Não** torna nenhum formato `available`. As 68 entradas do catálogo estão
  `state: planned`, e as 10 NGA — `aac-lc-adts`, `aac-he`, `aac-adif`, `ac3`,
  `eac3`, `truehd-mlp`, `dts`, `mpeg-h`, `ac4`, `iamf` — também. Viabilidade "sim"
  aqui seria afirmar implementação que `specs/formatos.md` ainda não tem.
- **Não** implementa o runtime de capability. O que se valida é a forma do JSON
  contra `schemas/format-catalog.schema.json` e as guardas de
  `xtask/src/catalog.rs`; inferir capability em runtime continua `M3`, como
  `specs/registro-capacidades.md` declara.
- **Não** integra xHE-AAC/USAC, AC-4, MPEG-H nem IAMF. `TASK-E17-04`, `TASK-E17-13`,
  `TASK-E17-14` e `TASK-E17-15` são donas desses adapters; aqui só se registra a decisão
  de fase e política.
- **Não** escreve as fixtures do corpus de codec. O catálogo referencia `sources`
  vazio em 56 das 68 entradas; popular isso é trabalho das TASKs de corpus, e a
  divisão está em `specs/formatos.md`, não nesta TASK.

## Antes de começar

- [ ] Spec do módulo aprovada e o critério de aceite citado já existe nela
- [ ] Worktree própria criada (`cargo xtask repo worktree task-e17-01-claude-<slug>`)
- [ ] Fixtures/hardware necessários disponíveis ou limitação registrada
- [ ] Nenhuma TASK em execução tocando os mesmos arquivos

## Critérios de aceite

| ID | Given | When | Then |
|---|---|---|---|
| `TASK-E17-01-AC-1` | o catálogo de perfis de codec da spec de capability registry | cada perfil é confrontado com o backend candidato e com o alvo | Cada perfil tem fase/política/operação |
| `TASK-E17-01-AC-2` | os codecs NGA listados no catálogo | a viabilidade de cada um é avaliada | codecs NGA têm decisão de viabilidade |
| `TASK-E17-01-AC-3` | os spikes de backend e o estimador true-peak independente | a inferência de capability é conferida contra o catálogo e o pin do oráculo | não inferir HE/USAC por AAC-LC nem renderer por decoder. Prova true-peak e pin do oráculo M0 rastreados |
| `TASK-E17-01-AC-4` | o escopo M0 desta TASK e a implementação do catálogo | o backlog é conferido depois da entrega | implementação permanece M3. |

## Verificação

| Item | Comando / fixture | Evidência esperada |
|---|---|---|
| Teste automatizado | `cargo xtask ci docs` no umbrella, que valida `config/format-catalog.json` contra `schemas/format-catalog.schema.json` e roda `catalog::validate`; mais `cargo test --workspace --locked` em `libs/decode` | **medido em 2026-11-10**: `cargo xtask ci docs` verde (33 testes do xtask, 0 falhas) e `check-docs` com `result: passed`, 193 tasks; a suíte do decode dá 508 verdes, 3 ignorados, 0 falhas. As guardas de `catalog.rs` que recusam `iamf` fora de `immersive-model`, FLAC em WebM e FLAC com perfil em WebM são exercitadas por esse gate |
| Oráculo independente | leitura campo a campo das 68 entradas de `config/format-catalog.json` contra os quatro critérios: fase/política/operação, decisão NGA, inferência de capability e estado | **medido**: `phase`, `policy` e `operations` faltando em 0 entradas; `target_platforms` vazio em 0; fases M1 4 / M2 10 / M3 30 / M5 24; políticas required 32 / optional 16 / conditional 19 / unsupported 1; **todas as 68 `planned`**. As 10 NGA estão presentes, com fase e política, todas `planned`. **Lacuna medida**: `backend_candidates` tem 3 valores distintos e 65 entradas repetem o marcador, então "confrontado com o backend candidato" não tem objeto em 65 casos. **Ausência medida**: `grep -rniE "true.?peak|bs\.1770|oversampl"` em `config/`, `schemas/`, `crates/`, `tests/` e `specs/` do decode não retorna nada — não há estimador nem pin de oráculo |
| Hardware | nenhum: o critério é sobre JSON versionado e validação de schema no host, não sobre áudio renderizado | ausência é limitação registrada, não aprovação — e a lacuna do true-peak é a mais séria desta TASK |

## DoD — pronto para review

- [ ] Todos os critérios de aceite com evidência
- [ ] `cargo fmt --check` e `cargo clippy --all-targets -- -D warnings` limpos
- [ ] `cargo test` verde; testes ignorados listados com motivo
- [ ] Spec do módulo atualizada se a implementação revelou algo novo
- [ ] `backlog.json` com o estado correto
- [ ] Revisão feita por contexto limpo, não pelo implementador
- [ ] PR aberto com template preenchido; CI Gate verde no SHA final
## Notas de execução

Medido em 2026-11-09. Provas em `evidence/PR/task-e17-01/`.

| AC | Verificação executada | Resultado |
|---|---|---|
| AC-1 | `config/format-catalog.json` lido campo a campo nas 68 entradas; `xtask/src/catalog.rs:5-69` lido para as guardas; `cargo xtask ci docs` verde | **Satisfeito, com uma lacuna.** As 68 entradas têm `phase`, `policy`, `operations` e `target_platforms` preenchidos (`faltando phase/policy/operations: nenhum`; `target_platforms` vazio: 0). Fase distribuída M1 4 / M2 10 / M3 30 / M5 24; política required 32 / optional 16 / conditional 19 / unsupported 1. **Lacuna medida:** `backend_candidates` assume só **3 valores** em 68 entradas — 65 repetem o marcador "Adapter auditado; escolher e fixar configuração em M0", **2** nomeiam backend real (`flac-native` e `mp3`, ambos "vynamp-decode (ADR-011)") e 1 é helper isolado (`sacd-iso`). `sources` está vazio em **56 de 68**. O AC pede "confrontado com o backend candidato"; para 65 das 68 entradas a coluna é marcador, não candidato |
| AC-2 | as entradas NGA no catálogo: `aac-lc-adts`, `aac-he`, `aac-adif`, `ac3`, `eac3`, `truehd-mlp`, `dts`, `mpeg-h`, `ac4`, `iamf` | **Satisfeito no que o AC pede, e o resultado é uniforme:** as 10 têm fase e política atribuídas e **todas** estão `state: planned` — nenhuma está `available`, `verified` ou `experimental`. A decisão de viabilidade registrada no catálogo é, portanto, "ainda não". A distinção que o `Then` exige está guardada em código, não em prosa: `aac-lc-adts` e `aac-he` são entradas **separadas** com `codec_id: aac` e fases diferentes, e `xtask/src/catalog.rs` recusa `iamf` com `kind` diferente de `immersive-model` ("IAMF must be an immersive model") |
| AC-3 | procura por estimador true-peak independente e por pin de oráculo em `config/`, `schemas/`, `libs/`, `tests/` | **Não satisfeito.** `grep -rniE "true.?peak|bs\.1770|oversampl"` em `config/`+`schemas/`+`libs/`+`tests/` **não retorna nada**. Não há estimador, não há pin de oráculo. O que existe é texto de spec: `design/06-painel-dsp.md:74` ("oversampling ≥ 4× conforme ITU-R BS.1770… algoritmo pinado na fixture") descreve o que deveria existir, e `delivery/01-roadmap.md:41` põe "Proteção final true-peak" em **M3 · E17-07/08**. O pin que o `Then` exige não existe em lugar nenhum. A parte "não inferir HE/USAC por AAC-LC nem renderer por decoder" **está** satisfeita nas entradas separadas e na guarda de IAMF |
| AC-4 | `tasks/backlog.json`: `TASK-E17-01` com `status: planned`, `phase: M0`; catálogo com as 68 entradas `planned` | **Satisfeito.** Nenhuma implementação de catálogo foi feita nesta TASK: o que existe é o JSON de planejamento mais a validação do `xtask`, ambos já presentes antes. Implementar runtime de capability continua M3 |

**O que esta TASK mediu, e o que ela não mediu.**

Mediu: a forma do catálogo contra o schema (o gate `ci docs` valida contra
`schemas/format-catalog.schema.json` e roda `catalog::validate`), a completude de
fase/política/operação/alvo nas 68 entradas, a separação AAC-LC × HE × USAC, e as
guardas de IAMF/Bluetooth/FLAC-em-WebM que o `catalog.rs` impõe.

**Não mediu:** um único spike de backend. O `deliverable` pede "spikes de
backends/licenças"; `backend_candidates` não nomeia backend nenhum e não há código de
backend no repositório para medir. E não mediu true-peak: AC-3 exige "prova
true-peak e pin do oráculo M0 rastreados", e **os dois estão ausentes**, não só não
documentados.

**Custo se essa leitura estiver errada:** se "spikes de backend" for satisfeito pela
lista de candidatos nomeados na spec (Symphonia/CPAL em ADR-002), o AC-1 fecha. Se a
exigência for mais que isso — uma medição por backend e por alvo —, o AC-1 está
aberto e o AC-3 está aberto em definitivo. A TASK fecha três dos quatro critérios e
registra o quarto como ausência medida.
