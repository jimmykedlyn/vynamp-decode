# TASK-E17-15 — Expandir IAMF como modelo e renderer

> Gerado pelo passo 37 a partir de `tasks/backlog.json`. Os campos de estado vêm do
> backlog e não devem ser editados aqui. A coluna `Then` é o requisito aprovado,
> copiado literal do backlog; `Given` e `When` foram autorados na execução.

| Campo | Valor |
|---|---|
| ID | `TASK-E17-15` |
| Módulo dono | `vynamp-decode` |
| Repo | `jimmykedlyn/vynamp-decode` |
| REQ coberto | `REQ-E17-15` |
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

Demux IAMF, codecs subjacentes e mix presentations com rendering por layout.

## Boundary

- **Não** implementa o decoder Opus, AAC-LC nem FLAC em IAMF: os adapters Opus, AAC e Vorbis são `TASK-E01-07` e a certificação de lossless e híbridos é `TASK-E17-05`. A confusão é plausível porque o catálogo lista `Opus`, `AAC-LC`, `FLAC` e `LPCM` como `profiles` da entrada `iamf` e o texto da AC-1 alerta exatamente para isso; aqui o codec subjacente é consumido como componente, e o que se prova é o modelo de apresentação e o renderer.
- **Não** implementa MPEG-H nem AC-4: MPEG-H com mha1 e mhm1 é `TASK-E17-13` e AC-4 com DRC é `TASK-E17-14`. As três famílias compartilham container MP4, `operations` com `render` e `policy: conditional` no catálogo, e as três são M5; modelar IAMF como MPEG-H daria dois donos do mesmo renderer espacial.
- **Não** entrega codec de saída nem encoder de IAMF: os encoders e o registry de encode são `TASK-E08-02` e `TASK-E18-02`, no `vynamp-converter`. A AC-2 fala em licenças de codec subjacente, e essa leitura é de licença de uso do decoder, não de um encoder novo.
- **Não** implementa o mixer PCM nem o grafo de saída que combina canais: o mixer, o drain e as transições são `TASK-E01-05` no `vynamp-engine`. O IAMF entrega uma mix presentation e o Vynamp tem um mixer próprio; reimplementar a combinação aqui daria dois donos do mesmo sinal mixado.
- **Não** fixa o catálogo nem a classificação `immersive-model` da entrada `iamf`: isso é `TASK-E17-01`, e a guarda de `xtask/src/catalog.rs` que exige `kind: immersive-model` para `codec_id: iamf` já é exercitada pelo gate de docs. Esta TASK consome a entrada como declarada, em `state: planned`.
- **Não** implementa o suporte de codec raro nem de trackers e MIDI que também vivem em M5: isso é `TASK-E12-05` e `TASK-E17-18`. O rótulo `M5` do catálogo reúne 24 das 68 entradas, de `aac-low-delay` a `wma-voice`, e tratá-las como um bloco só misturaria o modelo de apresentação do IAMF com codec de fala, MIDI e tracker.

## Antes de começar

- [ ] Spec do módulo aprovada e o critério de aceite citado já existe nela
- [ ] Worktree própria criada (`cargo xtask repo worktree task-e17-15-claude-<slug>`)
- [ ] Fixtures/hardware necessários disponíveis ou limitação registrada
- [ ] Nenhuma TASK em execução tocando os mesmos arquivos

## Critérios de aceite

| ID | Given | When | Then |
|---|---|---|---|
| `TASK-E17-15-AC-1` | quatro arquivos isolados que não são IAMF — um `.opus` puro, um `.aac` ADTS, um `.flac` e um `.wav` LPCM — cada um com SHA-256 em `tests/fixtures/iamf/MANIFEST.txt`, mais um `.iamf` de verdade com `codec_id` `iamf`, `kind: immersive-model` e `operations: [demux, decode, render]` em `config/format-catalog.json` rev 0.7.0 | os cinco arquivos passam pelo probe de container e pela tabela de codec da fachada, e o teste exige que nenhum dos quatro primeiros produza a capability de IAMF nem a de mix presentation, e que o quinto só produza `render` quando a mix presentation e o renderer estiverem presentes no arquivo | Opus/AAC-LC/FLAC/LPCM isolados não aprovam modelo |
| `TASK-E17-15-AC-2` | um `.iamf` com duas mix presentations — uma de 2 canais e uma de 6 canais com layout de objeto — cada uma com loudness e DRC declarados, o PCM de referência da apresentação renderizada por layout, e um diretório `tests/fixtures/iamf/reference/` com o SHA-256 de cada referência | a mix presentation é escolhida, o loudness, o layout e o DRC da apresentação renderizada são confrontados com a referência e com a tolerância declarada por apresentação, e o resultado é publicado junto da licença do codec subjacente usado em cada elemento de áudio e do renderer, com o id de licença de cada um | loudness/layout/DRC e renderer por referência com licenças. |

## Verificação

| Item | Comando / fixture | Evidência esperada |
|---|---|---|
| Teste automatizado | `cargo test` no repo `vynamp-decode` | verde no CI Gate |
| Oráculo independente | um teste de integração novo em `libs/decode/crates/vynamp-decode/tests/iamf.rs`, rodado por `cargo test -p vynamp-decode --features iamf --test iamf`, sobre um corpus novo em `tests/fixtures/iamf/` com `MANIFEST.txt` por barra vertical no mesmo formato de `tests/fixtures/mpeg/MANIFEST.txt` e `reference/` com o PCM por layout, mais `cargo xtask check-docs` no umbrella, que roda a guarda de `catalog::validate` que recusa `iamf` fora de `immersive-model` | esperado: os quatro arquivos isolados falham no probe de IAMF e nenhum produz a capability de IAMF, enquanto o `.iamf` produz `demux`, `decode` e `render`; o loudness, o layout e o DRC de cada mix presentation batem com a referencia dentro da tolerancia declarada e a diferenca numerica e impressa por apresentacao; o relatorio lista o id de licenca de cada codec subjacente e do renderer, e o `CodecCapability` sai com `state` conforme a prova e `license_ids` nao vazio quando `available` e verdadeiro. O que ainda nao existe hoje: um `grep -rniE` por `iamf` nos `.rs` de `libs/decode` devolve 0; `CodecId` em `crates/vynamp-decode-core/src/track.rs` so enumera `Pcm`, `Flac`, `Mp1`, `Mp2` e `Mp3`; `traits.rs` declara `Demuxer` e `Decoder` e nada de render; um `grep -rniE` por `drc_control`, `neutral_path`, `loudness`, `lufs` e `presentation` nos `.rs` devolve 1 linha, o comentario `Presentation timestamp in frames.` em `crates/vynamp-decode-core/src/packet.rs:8`, nenhuma de codigo de IAMF; nao ha crate de MP4 nem de IAMF; e `tests/fixtures/` tem 152 arquivos, nenhum de IAMF; a feature `iamf` tambem nao existe, e `cargo test -p vynamp-decode --features iamf` hoje para com `the package 'vynamp-decode' does not contain this feature`. O FFmpeg empacotado em `D:/Projects/vynamp-media-player/.cache/tools/ffmpeg/ffmpeg-n8.1.3-23-g5c4f5744cd-win64-gpl-8.1/bin/ffmpeg.exe` tem muxer e demuxer `iamf`, mas sem decoder `iamf` e sem encoder `iamf`; a tentativa de gravar um `.iamf` com `-c:a libopus` falha com `Could not write header (incorrect codec parameters ?)` e a variante com `-stream_group` de audio element falha com `No layer in audio element specification`, de modo que o corpus IAMF com mix presentation tem de vir de fonte externa licenciada e o script de geracao falha alto quando o arquivo falta |
| Hardware | nenhum: a mix presentation é renderizada e conferida no host sobre o PCM, a partir de `SliceSource` e `FileSource`, sem abrir rota de áudio; o que falta é corpus IAMF licenciado com referência por layout, não hardware. Esta máquina é Windows 11 x64 com apenas áudio integrado Intel e Conexant ISST Audio, sem placa de áudio profissional, sem aparelho Android, sem aparelho iOS e sem máquina macOS | ausência é limitação registrada, não aprovação |

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
