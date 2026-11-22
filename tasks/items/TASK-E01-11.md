# TASK-E01-11 — MP4 demux + ALAC + M4B (capítulos)

> Gerado pelo passo 37 a partir de `tasks/backlog.json`. Os campos de estado vêm do
> backlog e não devem ser editados aqui. A coluna `Then` é o requisito aprovado,
> copiado literal do backlog; `Given` e `When` foram autorados na execução.

| Campo | Valor |
|---|---|
| ID | `TASK-E01-11` |
| Módulo dono | `vynamp-decode` |
| Repo | `jimmykedlyn/vynamp-decode` |
| REQ coberto | `REQ-E01-11` |
| Critérios da spec | pendente — `specs/decode.md` §5 |
| Fase | M2 |
| Prioridade | P1 |
| Estado | planned |
| Depende de | `TASK-E01-04` |
| Bloqueia | `TASK-E01-06`, `TASK-E01-07` |
| Modelo executor | Opus — áudio/RT, concorrência, segurança ou release |
| Esforço | 5–12 dias úteis |
| Risco | alto |
| Tamanho | L |

## Then — critério de aceite (copiado do backlog)

o PCM decodificado é bit-exato ao da origem; `frames()` devolve a contagem bruta do manifesto (antes de trim); um seek para o frame da metade seguido de leitura até `Ok(false)` cai no mesmo slice do decode linear; a lista de capítulos (título + timestamp) é igual à do FFMETADATA de origem; `frames()` devolve a contagem bruta do manifesto; seek para o frame da metade cai no slice esperado

## Given — contexto de execução (autorado na execução)

Fixtures: arquivo MP4 contendo ALAC (lossless) com capítulossos (FFMETADATA + tag `©too`) e M4B com capítulos. O decoder ALAC lê o ALAC Lossless Frame; o demuxer MP4 expõe o manifesto de frames (offset, tamanho, duração, timestamp do capítulo).

## When — passos de execução (autorados na execução)

1. Demuxer MP4 lê `moov`/`mdia`/`minf`/`stbl`/`stts`, `stsc`, `stsz`, `stco`, `stts` e constrói o manifesto de frames.
2. `frames()` retorna o número de frames do manifesto (antes de qualquer trim de encoder delay/padding).
3. Seek para o frame da metade: `Reader::seek(frame_half)` → leitura até `Ok(false)` deve cair no mesmo slice que o decode linear do início até `frame_half`.
4. Lista de capítulos (título + timestamp) é igual à do FFMETADATA de origem.

## Verificação

| Item | Esperado | Evidência |
|---|---|---|
| PCM bit-exato ao da origem | igual ao decodificador ALAC de referência | comparação byte-a-byte |
| `frames()` contagem bruta | igual ao manifesto, antes de trim | log do teste |
| Seek frame metade → slice igual | slice do seek == slice do decode linear | log do teste |
| Capítulos M4B | título + timestamp == FFMETADATA | log do teste |

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