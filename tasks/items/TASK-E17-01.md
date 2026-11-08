# TASK-E17-01 — Fixar catálogo por binding, capacidades e corpus

> Gerado pelo passo 37 a partir de `tasks/backlog.json`. Os campos de estado vêm do
> backlog e não devem ser editados aqui. Critério de aceite: **PENDENTE** de autoria —
> a coluna `Then` é o requisito aprovado, copiado literal; `Given` e `When` ainda
> não existem.

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

<!-- PENDENTE: o que esta TASK não faz, mesmo parecendo relacionado. -->

## Antes de começar

- [ ] Spec do módulo aprovada e o critério de aceite citado já existe nela
- [ ] Worktree própria criada (`cargo xtask repo worktree task-e17-01-claude-<slug>`)
- [ ] Fixtures/hardware necessários disponíveis ou limitação registrada
- [ ] Nenhuma TASK em execução tocando os mesmos arquivos

## Critérios de aceite

| ID | Given | When | Then |
|---|---|---|---|
| `TASK-E17-01-AC-1` | PENDENTE | PENDENTE | Cada perfil tem fase/política/operação |
| `TASK-E17-01-AC-2` | PENDENTE | PENDENTE | codecs NGA têm decisão de viabilidade |
| `TASK-E17-01-AC-3` | PENDENTE | PENDENTE | não inferir HE/USAC por AAC-LC nem renderer por decoder. Prova true-peak e pin do oráculo M0 rastreados |
| `TASK-E17-01-AC-4` | PENDENTE | PENDENTE | implementação permanece M3. |

## Verificação

| Item | Comando / fixture | Evidência esperada |
|---|---|---|
| Teste automatizado | `cargo test` no repo `vynamp-decode` | verde no CI Gate |
| Oráculo independente | PENDENTE | comparação numérica, não "parece certo" |
| Hardware | PENDENTE | ausência é limitação registrada, não aprovação |

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
