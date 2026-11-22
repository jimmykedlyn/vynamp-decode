# TASK-E01-14 — Opus SILK + modo híbrido

> Gerado pelo fatiamento de `TASK-E01-07` em 2026-11-21. Os campos de estado vêm do
> backlog e não devem ser editados aqui. A coluna `Then` é o requisito aprovado,
> copiado literal do backlog; `Given` e `When` foram autorados na execução.

| Campo | Valor |
|---|---|
| ID | `TASK-E01-14` |
| Módulo dono | `vynamp-decode` |
| Repo | `jimmykedlyn/vynamp-decode` |
| REQ coberto | `REQ-E01-07` (sub-requisito `DECODE-REQ-46d`) |
| Critérios da spec | pendente — `specs/decode.md` §5 (`DECODE-AC-40`) |
| Fase | M2 |
| Prioridade | P1 |
| Estado | planned |
| Depende de | `TASK-E01-04`, `TASK-E01-13` |
| Bloqueia | `TASK-E01-07` |
| Modelo executor | Opus — áudio/RT, concorrência, segurança ou release |
| Esforço | 2–5 dias úteis |
| Risco | médio |

## Entrega

Adapter Opus SILK e modo híbrido em Ogg.

## Boundary

- **Não** cobre Opus CELT-only fullband. Este é `TASK-E01-13`, que esta TASK depende para compartilhar a infraestrutura de Opus e o demuxer Ogg.
- **Não** define política de pre-skip, end trim ou output gain. Estes são `TASK-E17-02`; aqui o adapter só reporta o que o container declara e a contagem de frames é bruta (antes de pre-skip).
- **Não** faz encoding. A regra de não anunciar encoding é `DECODE-REQ-59`. Nenhuma TASK do backlog é dona disso: a autoridade é a spec do módulo em `specs/formatos.md`, que fixa a regra do `DECODE-REQ-59`.
- **Não** implementa o demuxer Ogg. O demuxer é entregue por `TASK-E01-12` (Vorbis) e compartilhado; esta TASK só registra os modos SILK e híbrido do codec Opus.

## Antes de começar

- [ ] Spec do módulo aprovada e o critério de aceite citado já existe nela
- [ ] Worktree própria criada (`cargo xtask repo worktree task-e01-14-claude-<slug>`)
- [ ] Fixtures/hardware necessários disponíveis ou limitação registrada
- [ ] Nenhuma TASK em execução tocando os mesmos arquivos

## Critérios de aceite

| ID | Given | When | Then |
|---|---|---|---|
| `TASK-E01-14-AC-1` | um Opus SILK e um Opus híbrido em Ogg gerados pelo FFmpeg fixado (`-c:a libopus -application voip` para SILK, `-application audio` com taxa <= 16 kHz para híbrido), cada um com decode de referência do mesmo FFmpeg | cada arquivo é decodificado e o PCM é comparado contra o WAV de referência correspondente | SNR >= 80 dB contra a referência (tolerância explícita, menor que CELT pelo codec); `frames()` devolve a contagem bruta do manifesto; seek para o frame da metade cai no slice esperado |

## Verificação

| Item | Comando / fixture | Evidência esperada |
|---|---|---|
| Teste automatizado | `cargo test -p vynamp-decode --test modern_profiles --locked` | verde no CI Gate |
| Oráculo independente | WAV de referência gerado pelo mesmo FFmpeg fixado, não pela implementação sob teste | comparação SNR contra fonte externa |

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