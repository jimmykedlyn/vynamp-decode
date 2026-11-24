# TASK-E01-11 — MP4 demux + ALAC + M4B (capítulos)

> Gerado pelo fatiamento de `TASK-E01-07` em 2026-11-21. Os campos de estado vêm do
> backlog e não devem ser editados aqui. A coluna `Then` é o requisito aprovado,
> copiado literal do backlog; `Given` e `When` foram autorados na execução.

| Campo | Valor |
|---|---|
| ID | `TASK-E01-11` |
| Módulo dono | `vynamp-decode` |
| Repo | `jimmykedlyn/vynamp-decode` |
| REQ coberto | `REQ-E01-11` (sub-requisito `DECODE-REQ-46-MP4` em `specs/formatos.md` §4) |
| Critérios da spec | `specs/formatos.md` §5 — `DECODE-AC-53`, `DECODE-AC-54` (autorados, não aprovados) |
| Fase | M2 |
| Prioridade | P1 |
| Estado | planned |
| Depende de | `TASK-E01-04` |
| Bloqueia | `TASK-E01-07` |
| Modelo executor | Opus — áudio/RT, concorrência, segurança ou release |
| Esforço | 3–6 dias úteis |
| Risco | médio |

## Entrega

Demuxer MP4, adapter ALAC lossless e leitura de capítulos M4B.

## Boundary

- **Não** cobre AAC-LC nem HE-AAC. Estes são `TASK-E01-07`, que depende desta TASK para o demuxer MP4 mas tem critério próprio (`DECODE-AC-34`, `DECODE-AC-35`).
- **Não** define política de trim ou gapless. Pre-skip, end trim e output gain são `TASK-E17-02`; aqui o adapter só reporta o que o container declara.
- **Não** escreve capítulos nem tags. Escrita é `TASK-E17-06`; aqui só leitura.
- **Não** faz seek por timestamp de capítulo. Seek é por frame; a verificação de capítulo é enumerativa. Nenhuma TASK do backlog é dona disso: a autoridade é a spec do módulo em `specs/decode.md`, que fixa seek por frame como unidade mínima.

## Antes de começar

- [ ] Spec do módulo aprovada e o critério de aceite citado já existe nela
- [ ] Worktree própria criada (`cargo xtask repo worktree task-e01-11-claude-<slug>`)
- [ ] Fixtures/hardware necessários disponíveis ou limitação registrada
- [ ] Nenhuma TASK em execução tocando os mesmos arquivos

## Critérios de aceite

| ID | Given | When | Then |
|---|---|---|---|
| `TASK-E01-11-AC-1` | um ALAC em MP4 em `tests/fixtures/modern/alac/`, gerado pelo FFmpeg fixado (`-c:a alac`), e o PCM de origem usado para produzi-lo, ambos no `MANIFEST.txt` do diretório com sha256 e frame count | o arquivo é aberto por `vynamp_decode::open_path` e decodificado até o fim; o PCM resultante é comparado amostra a amostra contra o PCM de origem | o PCM decodificado é bit-exato ao da origem; `frames()` devolve a contagem bruta do manifesto (antes de trim); um seek para o frame da metade seguido de leitura até `Ok(false)` cai no mesmo slice do decode linear |
| `TASK-E01-11-AC-2` | um M4B em `tests/fixtures/modern/alac/`, gerado pelo FFmpeg fixado (`-f ipod -map_metadata`) com um capítulo declarado via FFMETADATA, listado no `MANIFEST.txt` do diretório | o arquivo é aberto e os capítulos são enumerados pela API pública | a lista de capítulos (título + timestamp) é igual à do FFMETADATA de origem; `frames()` devolve a contagem bruta do manifesto; seek para o frame da metade cai no slice esperado |

## Verificação

| Item | Comando / fixture | Evidência esperada |
|---|---|---|
| Teste automatizado | `cargo test -p vynamp-decode --test mp4_alac --locked` (arquivo `crates/vynamp-decode/tests/mp4_alac.rs`, só desta TASK) | verde no CI Gate |
| Oráculo independente | PCM de origem gerado antes do ALAC; FFMETADATA de origem preservado como fixture | comparação bit-exata e enumeração de capítulos contra fonte, não contra espelho da implementação |

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