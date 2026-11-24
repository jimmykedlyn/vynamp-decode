# TASK-E01-13 — Opus CELT-only (música fullband)

> Gerado pelo fatiamento de `TASK-E01-07` em 2026-11-22. Os campos de estado vêm do
> backlog e não devem ser editados aqui. A coluna `Then` é o requisito aprovado,
> copiado literal do backlog; `Given` e `When` foram autorados na execução.

| Campo | Valor |
|---|---|
| ID | `TASK-E01-13` |
| Módulo dono | `vynamp-decode` |
| Repo | `jimmykedlyn/vynamp-decode` |
| REQ coberto | `REQ-E01-13` (sub-requisito `DECODE-REQ-46-OPUS-CELT` em `specs/formatos.md` §4) |
| Critérios da spec | `specs/formatos.md` §5 — `DECODE-AC-56` (autorado, não aprovado) |
| Fase | M2 |
| Prioridade | P1 |
| Estado | planned |
| Depende de | `TASK-E01-04`, `TASK-E01-12` |
| Bloqueia | `TASK-E01-07`, `TASK-E01-14` |
| Modelo executor | Opus — áudio/RT, concorrência, segurança ou release |
| Esforço | 2–5 dias úteis |
| Risco | médio |

## Entrega

Adapter Opus CELT-only fullband em Ogg.

## Boundary

- **Não** cobre Opus SILK nem modo híbrido. Estes são `TASK-E01-14`, que depende desta TASK para compartilhar a infraestrutura de Opus, mas tem critério próprio (`DECODE-AC-57`).
- **Não** define política de pre-skip, end trim ou output gain. Estes são `TASK-E17-02`; aqui o adapter só reporta o que o container declara e a contagem de frames é bruta (antes de pre-skip).
- **Não** faz encoding nem prova a regra de não anunciá-lo. Essa regra é o `DECODE-REQ-59`, que `specs/formatos.md` §4 dá à `TASK-E01-07`; aqui o codec é registrado só para decode, e a prova fica com a dona.
- **Não** implementa o demuxer Ogg. O demuxer é entregue por `TASK-E01-12` (Vorbis), de que esta TASK depende, e compartilhado; esta TASK só registra o codec Opus CELT.

## Antes de começar

- [ ] Spec do módulo aprovada e o critério de aceite citado já existe nela
- [ ] Worktree própria criada (`cargo xtask repo worktree task-e01-13-claude-<slug>`)
- [ ] Fixtures/hardware necessários disponíveis ou limitação registrada
- [ ] Nenhuma TASK em execução tocando os mesmos arquivos

## Critérios de aceite

| ID | Given | When | Then |
|---|---|---|---|
| `TASK-E01-13-AC-1` | um Opus CELT-only fullband em Ogg em `tests/fixtures/modern/opus-celt/`, gerado pelo FFmpeg fixado (`-c:a libopus -application audio`), listado no `MANIFEST.txt` do diretório com sha256, taxa, canais e frames, mais o decode de referência produzido pelo mesmo FFmpeg | o arquivo é decodificado e o PCM é comparado contra o WAV de referência | SNR >= 90 dB contra a referência (tolerância explícita para lossy); `frames()` devolve a contagem bruta do manifesto (antes de pre-skip); seek para o frame da metade cai no slice esperado |

## Verificação

| Item | Comando / fixture | Evidência esperada |
|---|---|---|
| Teste automatizado | `cargo test -p vynamp-decode --test opus_celt --locked` (arquivo `crates/vynamp-decode/tests/opus_celt.rs`, só desta TASK) | verde no CI Gate |
| Geração de fixture | `pwsh tools/fixtures/generate-opus-celt.ps1` com o `$env:FFMPEG` (script próprio desta TASK, criado por ela; não toca `tools/fixtures/generate.ps1` nem o diretório de outra TASK) | escreve só `tests/fixtures/modern/opus-celt/` e o `MANIFEST.txt` dele, com o comando exato de cada arquivo; rodar duas vezes deixa os mesmos arquivos |
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
