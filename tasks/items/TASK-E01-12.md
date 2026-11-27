# TASK-E01-12 — Ogg demux + Vorbis

> Gerado pelo fatiamento de `TASK-E01-07` em 2026-11-22. Os campos de estado vêm do
> backlog e não devem ser editados aqui. A coluna `Then` é o requisito aprovado,
> copiado literal do backlog; `Given` e `When` foram autorados na execução.

| Campo | Valor |
|---|---|
| ID | `TASK-E01-12` |
| Módulo dono | `vynamp-decode` |
| Repo | `jimmykedlyn/vynamp-decode` |
| REQ coberto | `REQ-E01-12` (sub-requisito `DECODE-REQ-46-VORBIS` em `specs/formatos.md` §4) |
| Critérios da spec | `specs/formatos.md` §5 — `DECODE-AC-55` (aprovado pelo mantenedor no merge do PR de spec de 2026-11-27) |
| Fase | M2 |
| Prioridade | P0 |
| Estado | planned |
| Depende de | `TASK-E01-04` |
| Bloqueia | `TASK-E01-07`, `TASK-E01-13`, `TASK-E01-14` |
| Modelo executor | Opus — áudio/RT, concorrência, segurança ou release |
| Esforço | 2–5 dias úteis |
| Risco | médio |

## Entrega

Demuxer Ogg e adapter Vorbis.

## Boundary

- **Não** cobre Opus. Opus em Ogg é `TASK-E01-13` (CELT) e `TASK-E01-14` (SILK/híbrido), que compartilham o demuxer Ogg mas têm critérios próprios.
- **Não** define política de trim ou gapless. Pre-skip e end trim são `TASK-E17-02`; aqui o adapter só reporta o que o container declara.
- **Não** faz encoding nem prova a regra de não anunciá-lo. Essa regra é o `DECODE-REQ-59`, que `specs/formatos.md` §4 dá à `TASK-E01-07`; aqui o codec é registrado só para decode, e a prova fica com a dona.

## Antes de começar

- [ ] Spec do módulo aprovada e o critério de aceite citado já existe nela
- [ ] Worktree própria criada (`cargo xtask repo worktree task-e01-12-claude-<slug>`)
- [ ] Fixtures/hardware necessários disponíveis ou limitação registrada
- [ ] Nenhuma TASK em execução tocando os mesmos arquivos

## Critérios de aceite

| ID | Given | When | Then |
|---|---|---|---|
| `TASK-E01-12-AC-1` | um Vorbis em Ogg em `tests/fixtures/modern/vorbis/`, gerado pelo FFmpeg fixado (`-c:a libvorbis`), listado no `MANIFEST.txt` do diretório com sha256, taxa, canais e frames, mais o decode de referência produzido pelo mesmo FFmpeg (`ffmpeg -i in.ogg -f wav ref.wav`) | o arquivo é decodificado e o PCM é comparado contra o WAV de referência | SNR >= 90 dB contra a referência (tolerância explícita para lossy); `frames()` devolve a contagem bruta do manifesto; seek para o frame da metade cai no slice esperado |

## Verificação

| Item | Comando / fixture | Evidência esperada |
|---|---|---|
| Teste automatizado | `cargo test -p vynamp-decode --test ogg_vorbis --locked` (arquivo `crates/vynamp-decode/tests/ogg_vorbis.rs`, só desta TASK) | verde no CI Gate |
| Geração de fixture | `pwsh tools/fixtures/generate-vorbis.ps1` com o `$env:FFMPEG` (script próprio desta TASK, criado por ela; não toca `tools/fixtures/generate.ps1` nem o diretório de outra TASK) | escreve só `tests/fixtures/modern/vorbis/` e o `MANIFEST.txt` dele, com o comando exato de cada arquivo; rodar duas vezes deixa os mesmos arquivos |
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
