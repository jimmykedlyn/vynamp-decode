# TASK-E01-14 — Opus SILK + modo híbrido

> Gerado pelo fatiamento de `TASK-E01-07` em 2026-11-22. Os campos de estado vêm do
> backlog e não devem ser editados aqui. A coluna `Then` é o requisito aprovado,
> copiado literal do backlog; `Given` e `When` foram autorados na execução.

| Campo | Valor |
|---|---|
| ID | `TASK-E01-14` |
| Módulo dono | `vynamp-decode` |
| Repo | `jimmykedlyn/vynamp-decode` |
| REQ coberto | `REQ-E01-14` (sub-requisito `DECODE-REQ-46-OPUS-SILK` em `specs/formatos.md` §4) |
| Critérios da spec | `specs/formatos.md` §5 — `DECODE-AC-57` (autorado, não aprovado) |
| Fase | M2 |
| Prioridade | P1 |
| Estado | planned |
| Depende de | `TASK-E01-04`, `TASK-E01-12`, `TASK-E01-13` |
| Bloqueia | `TASK-E01-07` |
| Modelo executor | Opus — áudio/RT, concorrência, segurança ou release |
| Esforço | 2–5 dias úteis |
| Risco | médio |

## Entrega

Adapter Opus SILK e modo híbrido em Ogg.

## Boundary

- **Não** cobre Opus CELT-only fullband. Este é `TASK-E01-13`, de que esta TASK depende para compartilhar a infraestrutura de Opus.
- **Não** define política de pre-skip, end trim ou output gain. Estes são `TASK-E17-02`; aqui o adapter só reporta o que o container declara e a contagem de frames é bruta (antes de pre-skip).
- **Não** faz encoding nem prova a regra de não anunciá-lo. Essa regra é o `DECODE-REQ-59`, que `specs/formatos.md` §4 dá à `TASK-E01-07`; aqui o codec é registrado só para decode, e a prova fica com a dona.
- **Não** implementa o demuxer Ogg. O demuxer é entregue por `TASK-E01-12` (Vorbis), de que esta TASK depende, e compartilhado; esta TASK só registra os modos SILK e híbrido do codec Opus.

## Antes de começar

- [ ] Spec do módulo aprovada e o critério de aceite citado já existe nela
- [ ] Worktree própria criada (`cargo xtask repo worktree task-e01-14-claude-<slug>`)
- [ ] Fixtures/hardware necessários disponíveis ou limitação registrada
- [ ] Nenhuma TASK em execução tocando os mesmos arquivos

## Critérios de aceite

| ID | Given | When | Then |
|---|---|---|---|
| `TASK-E01-14-AC-1` | um Opus SILK-only e um Opus híbrido em Ogg em `tests/fixtures/modern/opus-silk/`, gerados pelo FFmpeg fixado a partir da mesma origem mono de banda larga com semente fixa (`-f lavfi -i "anoisesrc=color=pink:amplitude=0.3:duration=5:sample_rate=48000:seed=42,volume='0.5+0.5*sin(2*PI*4*t)':eval=frame"`, ruído rosa de 5 s modulado em amplitude a 4 Hz; um tom puro não serve, porque o libopus troca para CELT) e com `-fflags +bitexact -serial_offset 0`, para que cada geração saia byte a byte igual: o SILK-only com `-ac 1 -ar 16000 -c:a libopus -application voip -b:a 16k`, o híbrido com `-ac 1 -c:a libopus -application voip -b:a 18k` (48 kHz fullband, bitrate baixo; 18k fica no meio da faixa de 14k a 22k em que todo pacote saiu híbrido na medição, e a 24k já apareceram pacotes CELT config 31 misturados); cada um listado no `MANIFEST.txt` do diretório com sha256, taxa, canais e frames, e com decode de referência do mesmo FFmpeg | o byte de TOC de cada pacote de áudio dos dois arquivos é lido do Ogg (RFC 6716 §3.1, os 5 bits altos dão o config), e depois cada arquivo é decodificado e o PCM é comparado contra o WAV de referência correspondente | todo pacote do arquivo SILK-only tem config de TOC entre 0 e 11 e todo pacote do arquivo híbrido tem config de TOC entre 12 e 15; SNR >= 80 dB contra a referência (tolerância explícita, menor que CELT pelo codec); `frames()` devolve a contagem bruta do manifesto; seek para o frame da metade cai no slice esperado |

## Verificação

| Item | Comando / fixture | Evidência esperada |
|---|---|---|
| Teste automatizado | `cargo test -p vynamp-decode --test opus_silk_hybrid --locked` (arquivo `crates/vynamp-decode/tests/opus_silk_hybrid.rs`, só desta TASK) | verde no CI Gate |
| Geração de fixture | `pwsh tools/fixtures/generate-opus-silk.ps1` com o `$env:FFMPEG` (script próprio desta TASK, criado por ela; não toca `tools/fixtures/generate.ps1` nem o diretório de outra TASK) | escreve só `tests/fixtures/modern/opus-silk/` e o `MANIFEST.txt` dele, com o comando exato de cada arquivo; rodar duas vezes deixa os mesmos arquivos |
| Oráculo independente | WAV de referência gerado pelo mesmo FFmpeg fixado, não pela implementação sob teste; o modo de cada pacote é lido do TOC (RFC 6716 §3.1), não do que o adapter diz ter decodificado | comparação SNR contra fonte externa e contagem de config de TOC por arquivo; sem o TOC, um híbrido que saiu SILK-only passaria pelo mesmo SNR |

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
