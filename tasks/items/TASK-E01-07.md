# TASK-E01-07 — Adicionar codecs modernos e capítulos

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
| Depende de | `TASK-E01-04` |
| Bloqueia | `TASK-E01-08`, `TASK-E03-03`, `TASK-E05-02`, `TASK-E05-06`, `TASK-E08-02`, `TASK-E17-02`, `TASK-E17-04`, `TASK-E18-01`, `TASK-E18-02` |
| Modelo executor | Opus — áudio/RT, concorrência, segurança ou release |
| Esforço | 5–12 dias úteis |
| Risco | alto |

## Entrega

Adapters AAC-LC/HE-AAC/ALAC/Vorbis/Opus e M4B conforme prova/licença.

## Boundary

- **Não** cobre os codecs desktop. WMA, APE, WavPack, TTA, MPC, MP1, MP2, WebM, MKA e RF64 são `TASK-E01-08`, que declara esta TASK como dependência; a lista daqui para em AAC-LC, HE-AAC, ALAC, Vorbis, Opus e M4B, e trazer os outros para cá faria a mesma TASK parecer duas vezes dona do mesmo `CodecId`.
- **Não** define a política de trim, pre-skip, output gain nem timeline por codec. Opus pre-skip/end trim/output_gain, MP4 edit lists e o `GaplessKind` de cada perfil são `TASK-E17-02`; aqui o adapter só reporta o que o container declara, e transformar isso em regra de gapless seria antecipar a TASK que esta bloqueia.
- **Não** entrega os bindings de container nem a seleção de faixa de áudio. CAF/AIFF-C/AU/Ogg FLAC, MP4 e Matroska/TS com escolha de faixa são `TASK-E17-03`; aqui cada adapter aceita o framing que o seu perfil exige, sem virar catálogo de containers.
- **Não** faz xHE-AAC/USAC. `TASK-E17-04` é dona de avaliar AOT42 e de relatar indisponibilidade por build/licença; anunciar USAC por ter AAC-LC do lado seria exatamente o erro que o AC-2 proíbe com HE-AAC.
- **Não** lê nem escreve tags e capítulos de arquivo. Gravar capítulo, preservar tag desconhecida e a matriz read/write de ID3/MP4/Matroska são `TASK-E17-06`; aqui o M4B só precisa entregar duração, seek e o capítulo que o container declara, sem escrita.
- **Não** entrega DSD nem o mapeamento de canal. DSF/DFF e os mapas mono/stereo/5.1/7.1 são `TASK-E01-09`, bloqueada por esta; misturar os dois faria a prova de perfil e a prova de downmix dependerem uma da outra.

## Antes de começar

- [ ] Spec do módulo aprovada e o critério de aceite citado já existe nela
- [ ] Worktree própria criada (`cargo xtask repo worktree task-e01-07-claude-<slug>`)
- [ ] Fixtures/hardware necessários disponíveis ou limitação registrada
- [ ] Nenhuma TASK em execução tocando os mesmos arquivos

## Critérios de aceite

| ID | Given | When | Then |
|---|---|---|---|
| `TASK-E01-07-AC-1` | um corpus por perfil em `tests/fixtures/modern/`, gerado com o FFmpeg fixado em `.cache/tools/ffmpeg/ffmpeg-n8.1.3-23-g5c4f5744cd-win64-gpl-8.1/bin/ffmpeg.exe` (`-c:a aac` em mp4 para AAC-LC, `-c:a alac` em mp4, `-c:a libvorbis` em Ogg, `-c:a libopus` em Ogg, e um M4B de `-f ipod -map_metadata` com um capítulo declarado em FFMETADATA), com um `MANIFEST.txt` dando sha256, taxa, canais, frames e o comando exato de cada arquivo | cada fixture é aberto por `vynamp_decode::open_path` e medido em três pontos: `frames()` contra os frames do manifesto, um `Reader::seek` para o frame da metade seguido de leitura até `Ok(false)`, e as informações que o perfil declara ter — taxa, canais e capítulos no caso do M4B | Fixture por perfil suporta duration/seek/metadata quando aplicável |
| `TASK-E01-07-AC-2` | um AAC-LC em mp4 cujo `esds` declara object type 2, e um HE-AAC v1 montado à mão a partir dele com o byte de AOT reescrito para 5 — o FFmpeg fixado é `--disable-libfdk-aac` e `-profile:a aac_he` aborta com `Profile not supported!`, então não há HE-AAC vindo de encoder — os dois a 48 kHz estéreo e com a mesma contagem de frames | o `default_registry()` é consultado sobre os dois — `Registry::probe` para o container, o `CodecId` que o demuxer devolve, `codec_for` para o descritor e a listagem de `codecs()` — e cada um é decodificado até o fim em `Mode::Strict` | HE-AAC não é anunciado por decode AAC-LC. |

## Verificação

| Item | Comando / fixture | Evidência esperada |
|---|---|---|
| Teste automatizado | `cargo test` no repo `vynamp-decode` | verde no CI Gate |
| Oráculo independente | `pwsh tools/fixtures/generate.ps1` com uma seção nova para AAC-LC/ALAC/Vorbis/Opus/M4B usando o `$env:FFMPEG` que o `AMBIENTE.ps1` define, depois `cargo test -p vynamp-decode --test modern_profiles --locked` e `cargo run -q --locked --example support_matrix` | a evidência tem que mostrar o `support_matrix` com as linhas dos perfis novos e o `frames()` de cada fixture igual ao do manifesto, com o seek da metade devolvendo o slice correto do decode completo. **Hoje nada disso existe**: `support_matrix` imprime exatamente 4 containers (riff, aiff, flac, mpa) e 4 codecs (pcm, flac, mpeg-audio, mpeg-audio-layer1); `crates/` tem 9 diretórios e nenhum de AAC, ALAC, Vorbis ou Opus — o facade `vynamp-decode` mais aiff, core, flac, mp3, mpa, pcm, riff e testkit; `grep -rni opus crates/` volta vazio; o `CodecId` em `crates/vynamp-decode-core/src/track.rs` tem só Pcm, Flac, Mp1, Mp2 e Mp3; e `tests/fixtures` soma 13 MB em flac, flac-conformance, mpeg e pcm, sem um único AAC, ALAC, Vorbis, Opus ou M4B. A linha de base medida hoje é 508 verdes, 3 ignorados e 0 falhas em 47 suítes em `cargo test --workspace --locked`. O HE-AAC do AC-2 não pode sair de encoder neste ambiente e a evidência tem que declarar que veio de `esds` reescrito |
| Hardware | nenhum: todos os perfis se decodificam de fixture em disco e nenhum caminho abre dispositivo de áudio. O FFmpeg fixado está presente e serve de gerador; a ausência de encoder HE-AAC na build (`--disable-libfdk-aac`) é limitação de ferramenta, declarada, não de hardware | ausência é limitação registrada, não aprovação |

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
