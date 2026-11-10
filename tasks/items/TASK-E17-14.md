# TASK-E17-14 — Expandir AC-4 e apresentações de áudio

> Gerado pelo passo 37 a partir de `tasks/backlog.json`. Os campos de estado vêm do
> backlog e não devem ser editados aqui. A coluna `Then` é o requisito aprovado,
> copiado literal do backlog; `Given` e `When` foram autorados na execução.

| Campo | Valor |
|---|---|
| ID | `TASK-E17-14` |
| Módulo dono | `vynamp-decode` |
| Repo | `jimmykedlyn/vynamp-decode` |
| REQ coberto | `REQ-E17-14` |
| Critérios da spec | pendente — `specs/decode.md` §5 |
| Fase | M5 |
| Prioridade | P1 |
| Estado | planned |
| Depende de | `TASK-E17-01`, `TASK-E17-03` |
| Bloqueia | nenhuma |
| Modelo executor | Opus — áudio/RT, concorrência, segurança ou release |
| Esforço | 2–5 dias úteis |
| Risco | alto |

## Entrega

Adapter AC-4 condicionado, seleção de apresentação/DRC e capacidades.

## Boundary

- **Não** implementa MPEG-H nem o renderer de objetos: MPEG-H com MHAS, mha1 e mhm1 é `TASK-E17-13`. A confusão é plausível porque a spec escreve `AC-4 (apresentações/DRC)` e `MPEG-H 3D Audio (MP4/mha1/mhm1/MHAS, baseline/level)` na mesma frase e as duas entradas do catálogo têm `operations` com `render` e `policy: conditional` lado a lado; aqui o escopo é a apresentação AC-4 com DRC, e o MPEG-H continua fora mesmo dividindo o container MP4.
- **Não** implementa o modelo IAMF, os codecs subjacentes nem as mix presentations: isso é `TASK-E17-15`. AC-4 e IAMF compartilham a palavra `apresentação` e o container MP4, e o catálogo classifica `iamf` como `kind: immersive-model` enquanto `ac4` é `kind: audio`; modelar as duas como a mesma coisa quebraria a guarda de `xtask/src/catalog.rs` que exige `immersive-model` para `codec_id` `iamf`.
- **Não** implementa AC-3, E-AC-3, TrueHD, MLP, DTS Core e DTS-HD nem o passthrough codificado: os codecs cinematográficos são `TASK-E17-16`. AC-4 é AC-4 e o título da TASK vizinha fala em `EAC3`; tratar os dois como a mesma família faria a política `conditional` de um codec valer para o outro.
- **Não** resolve licença nem escolhe a fonte do backend AC-4: o inventário de codecs, fontes e políticas de distribuição é `TASK-E11-03`. O catálogo já referencia `https://professional.dolby.com/technologies/ac-4/` em `sources` e o `LEGAL.md` do repo classifica AC-4 na feature `encumbered` desligada por padrão; a decisão de build e de redistribuir é de lá.
- **Não** fixa o catálogo nem decide a fase e a política da entrada `ac4`: isso é `TASK-E17-01`. Aqui a entrada é consumida como já declarada, com `state: planned`; trocá-la para `available` nesta TASK faria dois donos do mesmo estado.
- **Não** mede loudness no PCM final do buffer de saída nem protege contra true-peak: o guard e o teto no PCM final são `TASK-E17-07` e `TASK-E17-08`. O loudness de que a AC-1 fala é o da apresentação AC-4, medido no PCM decodificado e confrontado com a referência, e não uma leitura de loudness no nó de saída.

## Antes de começar

- [ ] Spec do módulo aprovada e o critério de aceite citado já existe nela
- [ ] Worktree própria criada (`cargo xtask repo worktree task-e17-14-claude-<slug>`)
- [ ] Fixtures/hardware necessários disponíveis ou limitação registrada
- [ ] Nenhuma TASK em execução tocando os mesmos arquivos

## Critérios de aceite

| ID | Given | When | Then |
|---|---|---|---|
| `TASK-E17-14-AC-1` | um corpus AC-4 com um `.ac4` elementar e um `.mp4` com a faixa AC-4, cada um com o SHA-256 em `tests/fixtures/ac-4/MANIFEST.txt` e com a apresentação, a contagem de canais e o loudness declarados em um arquivo de referência por apresentação; e a entrada `ac4` de `config/format-catalog.json` rev 0.7.0, que está `state: planned`, `policy: conditional`, `operations: [demux, decode, render]`, `containers: [MP4, elementary]` e `profiles: [AC-4 presentations]` | para cada build de backend, o arquivo é aberto, a apresentação é escolhida, o número de canais e o loudness medido da apresentação decodificada são confrontados com o arquivo de referência e o resultado é publicado por build com o backend e a versão que o produziram | Apresentação/canais/loudness batem referência por build |
| `TASK-E17-14-AC-2` | dois arquivos com o mesmo codec em containers diferentes: um `.mp4` com faixa AC-4 e um `.ac4` elementar, mais um `.mp4` sem faixa de áudio reconhecível e um `.mp4` cuja faixa declara um codec que o build não tem, e um build com a feature `ac4` desligada | a disponibilidade de AC-4 é lida do tuple de capability do próprio build e confrontada com o container de cada arquivo, e o teste afirma que um `.mp4` não produz capacidade AC-4 por ser MP4, inclusive no build em que a feature está desligada, onde o resultado esperado é `unavailable` com motivo | não inferir disponibilidade por container MP4. |

## Verificação

| Item | Comando / fixture | Evidência esperada |
|---|---|---|
| Teste automatizado | `cargo test` no repo `vynamp-decode` | verde no CI Gate |
| Oráculo independente | um teste de integração novo em `libs/decode/crates/vynamp-decode/tests/ac4.rs`, rodado por `cargo test -p vynamp-decode --features ac4 --test ac4`, sobre um corpus novo em `tests/fixtures/ac-4/` com `MANIFEST.txt` por barra vertical no mesmo formato de `tests/fixtures/mpeg/MANIFEST.txt` e um `reference/` com o loudness e o layout de referência por apresentação, mais `cargo xtask check-docs` no umbrella | esperado: por build, a apresentacao escolhida tem a contagem de canais e o loudness dentro da tolerancia declarada contra a referencia, e a diferenca numerica e impressa no relatorio; o `.mp4` e o `.ac4` elementar produzem a mesma capability de codec e a mesma apresentacao; o `.mp4` sem faixa de audio e o `.mp4` com codec ausente no build sao recusados com motivo, e o build sem a feature `ac4` devolve `unavailable` mesmo para o `.mp4`; nenhum teste afirma disponibilidade de AC-4 derivada do container. O que ainda nao existe hoje: um `grep -rniE` por `ac4` e por `ac-4` nos `.rs` de `libs/decode` devolve 1 linha, e ela e a string de hash `57edf4a22be3c955ac49da2e2107b67a` em `crates/vynamp-decode-core/tests/md5.rs`, nao codigo de AC-4; `CodecId` so tem `Pcm`, `Flac`, `Mp1`, `Mp2` e `Mp3`; nao ha crate de MP4, e um `grep -rniE` por `ftyp` e `moov` devolve 0; um `grep -rniE` por `drc_control`, `neutral_path`, `loudness` e `lufs` nos `.rs` devolve 0; e `tests/fixtures/` tem 152 arquivos, nenhum de AC-4. O FFmpeg empacotado em `D:/Projects/vynamp-media-player/.cache/tools/ffmpeg/ffmpeg-n8.1.3-23-g5c4f5744cd-win64-gpl-8.1/bin/ffmpeg.exe` tem o demuxer `ac4` listado em `-demuxers` mas nenhum encoder `ac4`, que recusa com `Unknown encoder 'ac4'` e fecha com `Error opening output files: Encoder not found`, e nenhum decoder `ac4` em `-decoders`, entao o corpus tem de vir de fonte externa licenciada e o script de geracao falha alto quando o arquivo falta |
| Hardware | nenhum: a apresentação, os canais e o loudness são medidos sobre o PCM decodificado no host, a partir de `SliceSource` e `FileSource`, sem abrir rota de áudio; o que falta é corpus licenciado com referência por apresentação, não hardware. Esta máquina é Windows 11 x64 com apenas áudio integrado Intel e Conexant ISST Audio, sem placa de áudio profissional, sem aparelho Android, sem aparelho iOS e sem máquina macOS | ausência é limitação registrada, não aprovação |

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
