# TASK-E01-14 — Opus SILK/híbrido em Ogg

> Gerado pelo passo 37 a partir de `tasks/backlog.json`. Os campos de estado vêm do
> backlog e não devem ser editados aqui. A coluna `Then` é o requisito aprovado,
> copiado literal do backlog; `Given` e `When` foram autorados na execução.

| Campo | Valor |
|---|---|
| ID | `TASK-E01-14` |
| Módulo dono | `vynamp-decode` |
| Repo | `jimmykedlyn/vynamp-decode` |
| REQ coberto | `REQ-E01-14` |
| Critérios da spec | pendente — `specs/decode.md` §5 |
| Fase | M2 |
| Prioridade | P1 |
| Estado | planned |
| Depende de | `TASK-E01-04`, `TASK-E01-13` |
| Bloqueia | `TASK-E01-06`, `TASK-E01-07` |
| Modelo executor | Opus — áudio/RT, concorrência, segurança ou release |
| Esforço | 5–12 dias úteis |
| Risco | alto |
| Tamanho | L |

## Then — critério de aceite (copiado do backlog)

SNR >= 80 dB contra a referência (tolerância explícita, menor que CELT pelo codec); `frames()` devolve a contagem bruta do manifesto; seek para o frame da metade cai no slice esperado

## Given — contexto de execução (autorado na execução)

Fixtures: arquivo Ogg contendo Opus SILK e modo híbrido a 16 kHz/24 kHz/48 kHz (configurações típicas SILK/híbrido). O demuxer Ogg lê as páginas Ogg, constrói o manifesto de frames. O adapter Opus (SILK/híbrido) decodifica cada quadro.

## When — passos de execução (autorados na execução)

1. Demuxer Ogg constrói o manifesto de frames.
2. `frames()` retorna o número de frames do manifesto.
3. Decodifica linearmente do início ao fim, medindo SNR contra o decodificador Opus de referência (FFmpeg) com tolerância menor que CELT.
4. Seek para o frame da metade: `Reader::seek(frame_half)` → leitura até `Ok(false)` cai no mesmo slice que o decode linear.

## Verificação

| Item | Esperado | Evidência |
|---|---|---|
| SNR >= 80 dB | contra referência FFmpeg, tolerância menor que CELT | relatório de SNR |
| `frames()` contagem bruta | igual ao manifesto | log do teste |
| Seek frame metade → slice igual | slice do seek == slice do decode linear | log do teste |

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