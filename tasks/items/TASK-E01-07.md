# TASK-E01-07 — AAC-LC + HE-AAC (não anunciado como AAC-LC)

> Gerado pelo passo 37 a partir de `tasks/backlog.json`. Os campos de estado vêm do
> backlog e não devem ser editados aqui. A coluna `Then` é o requisito aprovado,
> copiado literal do backlog; `Given` e `When` foram autorados na execução.

| Campo | Valor |
|---|---|
| ID | `TASK-E01-07` |
| Módulo dono | `vynamp-decode` |
| Repo | `jimmykedlyn/vynamp-decode` |
| REQ coberto | `REQ-E01-07` |
| Critérios da spec | pendente — `specs/decode.md` §5 |
| Fase | M2 |
| Prioridade | P1 |
| Estado | planned |
| Depende de | `TASK-E01-04`, `TASK-E01-11`, `TASK-E01-12`, `TASK-E01-13`, `TASK-E01-14` |
| Bloqueia | `TASK-E01-08`, `TASK-E03-03`, `TASK-E05-02`, `TASK-E05-06`, `TASK-E08-02`, `TASK-E17-02`, `TASK-E17-04`, `TASK-E18-01`, `TASK-E18-02` |
| Modelo executor | Opus — áudio/RT, concorrência, segurança ou release |
| Esforço | 5–12 dias úteis |
| Deliverable | Adapter AAC-LC/HE-AAC conforme prova/licença; HE-AAC não é anunciado como AAC-LC. |
| Acceptance | Fixture por perfil suporta duration/seek/metadata quando aplicável; HE-AAC não é anunciado por decode AAC-LC. |

## Given — contexto de execução (autorado na execução)

Licença válida do AAC-LC/HE-AAC; fixtures SBR com níveis conhecidos; containers ADTS e MP4.

## When — ação (autorado na execução)

Adapter compila com feature flag; no decode AAC-LC o HE-AAC **não** é anunciado; M0/M1 decodifica AAC-LC até o fim; HE-AAC SBR detectado e decodificado; seek/frame-accurate em todos os perfis.

## Then — resultado esperado (REQ literal)

Fixture por perfil suporta duration/seek/metadata quando aplicável; HE-AAC não é anunciado por decode AAC-LC.

## Verificação

Comandos, hardware, fixtures e como observou cada critério. "Testes passaram" não é evidência.

| Comando / evidência | Resultado |
|---|---|
| `cargo test -p vynamp-decode` | pendente |
| `cargo clippy --all-targets -- -D warnings` | pendente |

## Compatibilidade / Riscos

Nenhum declarado.

## DoD — pronto para review

- [ ] `cargo test` + `cargo clippy -D warnings` verdes no SHA final
- [ ] Critérios da spec (`Then`) observados em fixtures/ambientes listados com motivo
- [ ] Spec do módulo atualizada se a implementação revelou algo novo
- [ ] `backlog.json` com o estado correto
- [ ] Revisão feita por contexto limpo, não pelo implementador
- [ ] PR aberto com template preenchido; CI Gate verde no SHA final

## Notas de execução

Preenchido durante a execução, não antes.