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
| Depende de | `TASK-E01-04`, `TASK-E01-11`, `TASK-E01-12`, `TASK-E01-13` |
| Bloqueia | `TASK-E01-06` |
| Modelo executor | Opus — áudio/RT, concorrência, segurança ou release |
| Esforço | 5–12 dias úteis |
| Risco | alto |
| Tamanho | L |

## Then — critério de aceite (copiado do backlog)

Fixture por perfil suporta duration/seek/metadata quando aplicável; HE-AAC não é anunciado por decode AAC-LC.

## Given — contexto de execução (autorado na execução)

Decoders FFmpeg: `libfdk_aac` (AAC-LC/HE-AAC v1/v2) — licença *Fractional Licensing* (FRAC); fallback `native` (AAC-LC) sem HE-AAC. A prova usa corpus sintético (AAC-LC 128 kbit/s CBR 44.1 kHz stereo + HE-AAC v1 64 kbit/s 48 kHz stereo) gerado com FFmpeg fixado. O fallback `native` abre o HE-AAC mas devolve erro `ENOTSUP` no `codec_for` → HE-AAC não aparece na lista de codecs suportados.

## When — passos de execução (autorados na execução)

1. `default_registry()` é consultado sobre os dois perfis — `Registry::probe` para o container, o `CodecId` que o demuxer devolve, `codec_for` para o descritor e a listagem de `codecs()` — e cada um é decodificado até o fim em `Mode::Strict`.
2. AAC-LC: duração, seek frame-a-frame e metadados (sample rate, canais, bitrate) conferem com o FFprobe de referência.
3. HE-AAC: `codec_for` devolve `None` / `ENOTSUP`; `codecs()` não lista `aac_he` / `aac_he_v2`.

## Verificação

| Item | Esperado | Evidência |
|---|---|---|
| AAC-LC duration/seek/metadata | igual ao FFprobe | log do teste |
| HE-AAC não anunciado | `codec_for` = `None`, ausente em `codecs()` | log do teste |
| Licença FRAC registrada | `ADR-002` / `NOTICE` | commit da TASK |

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