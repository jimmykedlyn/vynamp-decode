# TASK-E01-08 — Completar codecs desktop e large files

> Gerado pelo passo 37 a partir de `tasks/backlog.json`. Os campos de estado vêm do
> backlog e não devem ser editados aqui. A coluna `Then` é o requisito aprovado,
> copiado literal do backlog; `Given` e `When` foram autorados na execução.

| Campo | Valor |
|---|---|
| ID | `TASK-E01-08` |
| Módulo dono | `vynamp-decode` |
| Repo | `jimmykedlyn/vynamp-decode` |
| REQ coberto | `REQ-E01-08` |
| Critérios da spec | pendente — `specs/decode.md` §5 |
| Fase | M3 |
| Prioridade | P1 |
| Estado | planned |
| Depende de | `TASK-E01-07` |
| Bloqueia | `TASK-E01-09`, `TASK-E12-05`, `TASK-E17-03`, `TASK-E17-05`, `TASK-E17-17` |
| Modelo executor | Opus — áudio/RT, concorrência, segurança ou release |
| Esforço | 5–15 dias úteis |
| Risco | alto |

## Entrega

WMA/APE/WV/TTA/MPC/MP1/MP2/WebM/MKA/RF64 adapters com isolation quando nativos.

## Boundary

- **Não** certifica o corpus dos codecs que entrega. Rodar WavPack/WVC/float, APE/TTA/MPC/WMA e PCM de containers adicionais contra referência e emitir a matriz de certificação é `TASK-E17-05`; aqui o adapter existe e tem fixture mínima, e a prova exaustiva tem dono próprio.
- **Não** reimplementa o decodificador MP1/MP2. `crates/vynamp-decode-mp3` já decodifica Layer I (`layer1.rs`) e Layer II (`layer2.rs`) e `crates/vynamp-decode-mpa` já faz o demux, com `mpeg-audio-layer1` marcado `Experimental` porque o FFmpeg não tem encoder de Layer I; o registry que expõe esses perfis é `TASK-E01-04`, e esta TASK garante que MP1 e MP2 aparecem na lista runtime, não que ganham decoder novo.
- **Não** entrega os bindings de container nem a seleção de faixa de áudio. WebM e Matroska/MKA aparecem aqui como perfil com fixture, mas CAF/AIFF-C/AU/Ogg FLAC, MP4 e Matroska/TS com escolha de faixa de áudio são `TASK-E17-03`; fazer seleção de faixa aqui duplicaria o dono do mesmo demuxer.
- **Não** implementa trackers, MIDI ou codecs raros de plugin. libopenmpt, synth com SF2 do usuário e o conjunto opcional testado são `TASK-E12-05`; a lista desta TASK termina em WMA, APE, WavPack, TTA, MPC, MP1, MP2, WebM, MKA e RF64.
- **Não** decide a política de tamanho máximo nem o valor de `Limits::max_bytes`. O default já é 64 GiB em `crates/vynamp-decode-core/src/limits.rs` e quem o configura é o chamador, cujo contrato de opções é `TASK-E01-04`; a TASK atravessa a fronteira de 4 GiB com `u64`, não redefine o teto.
- **Não** faz DSD nem DST. DSF/DFF para PCM é `TASK-E01-09` e WavPack DSD, DFF DST e raw DSD entram na certificação de `TASK-E17-18`; tratar `.wv` com DSD aqui confundiria perfil lossless PCM com container DSD.

## Antes de começar

- [ ] Spec do módulo aprovada e o critério de aceite citado já existe nela
- [ ] Worktree própria criada (`cargo xtask repo worktree task-e01-08-claude-<slug>`)
- [ ] Fixtures/hardware necessários disponíveis ou limitação registrada
- [ ] Nenhuma TASK em execução tocando os mesmos arquivos

## Critérios de aceite

| ID | Given | When | Then |
|---|---|---|---|
| `TASK-E01-08-AC-1` | um RF64 esparso de 5.368.709.200 bytes (5 GiB de `data`, 2 canais 16 bits a 48 kHz, portanto 1.342.177.280 frames) e o Wave64 equivalente, mais o primeiro fixture de cada perfil novo que declara posição acima de 2^32 bytes — o maior arquivo versionado hoje é `tests/fixtures/flac-conformance/subset/27 - old format variable blocksize file created with Flake 0.11.flac`, com 2.215.845 bytes — o RF64 é criado com `fsutil sparse setflag` antes de escrever o cabeçalho e o último byte, e a alocação real medida por `GetCompressedFileSizeW` é 131.072 bytes contra 5.368.709.200 aparentes, enquanto o mesmo arquivo sem a flag aloca os 5.368.709.120 bytes inteiros; `df -k /d` do Git Bash não serve como prova porque também não se move num arquivo não esparso do mesmo tamanho | os três são abertos em `Mode::Strict` e a posição atravessa a fronteira de 2^32 bytes: `frames()`, `Reader::seek` para o frame 1.000.000.000 e leitura de um pacote, e um seek para o último frame do arquivo | Arquivos >4GiB usam offsets64 |
| `TASK-E01-08-AC-2` | cada adapter novo (WMA, APE, WavPack, TTA, MPC, MP1, MP2, WebM, MKA, RF64) registrado atrás da sua feature, com pelo menos um fixture por perfil em `tests/fixtures/desktop/` e uma linha de `MANIFEST.txt` com sha256, taxa, canais, frames e o gerador; o FFmpeg fixado gera `-c:a wavpack`, `-c:a tta`, `-c:a wmav1`, `-c:a wmav2`, `-c:a mp2` e `-c:a libtwolame`, mas não tem encoder de APE, de MPC nem de MP1 (`ffmpeg -encoders` não mostra `ape`, `mpc` nem layer 1) | o build roda `cargo run -q --locked --example support_matrix` e a lista de perfis que ele imprime é confrontada com os arquivos presentes em `tests/fixtures/desktop/` e com as entradas de `config/format-catalog.json` | cada perfil tem fixture e lista runtime |
| `TASK-E01-08-AC-3` | seis arquivos de 40 bytes sem container conhecido gravados como `.wma`, `.ape`, `.mpc`, `.opus`, `.m4a` e `.mka`, um WAV com `format tag` não suportado no `fmt `, um arquivo truncado no meio do cabeçalho e a suíte de byte-flip e truncamento que já existe em `crates/vynamp-decode-riff/tests/riff.rs` (`byte_flip_and_truncation_smoke`, 128 flips e 129 truncamentos por variante de container) | cada um é aberto por `open_path` em `Mode::Strict` e de novo em `Mode::Resilient`, e o resultado é confrontado com a regra de que entrada desconhecida vira dado tipado e não `panic!` nem `unwrap` | unsupported sem crash. |

## Verificação

| Item | Comando / fixture | Evidência esperada |
|---|---|---|
| Teste automatizado | `cargo test` no repo `vynamp-decode` | verde no CI Gate |
| Oráculo independente | `pwsh tools/fixtures/generate.ps1` com a seção nova de perfis desktop usando o `$env:FFMPEG` (ver `engineering/08-reinicio-windows.md` do umbrella), `cargo test -p vynamp-decode --test desktop_profiles --locked`, `cargo run -q --locked --example support_matrix` e o teste de arquivo grande atrás de `VYNAMP_LARGE_FIXTURES` com `-- --ignored`, seguindo o padrão que `crates/vynamp-decode/tests/conformance_flac.rs` já usa com `VYNAMP_FLAC_TEST_FILES` | a evidência do AC-1 tem que mostrar `frames()` de 1.342.177.280 e o seek além de 2^32 devolvendo o pacote certo. **Medido hoje** em um RF64 esparso de 5.368.709.200 bytes montado fora do repo (cabeçalho de 80 bytes mais 5 GiB de `data`): abre em 0,29–0,58 ms no build release e em 11,8–22,4 ms no debug, com `frames Some(1342177280)`; `seek` ao frame 1.000.000.000 devolve 4096 frames a partir do byte 4.000.000.080; `seek` ao último frame custa 5,5–11 µs e entrega 1 frame a partir do byte 5.368.709.196 — os dois offsets só cabem em u64; e o arquivo ocupa 131.072 bytes de alocação real, medidos por `GetCompressedFileSizeW`, embora `df -k /d` não se mova. Ou seja, o caminho `ds64` de `crates/vynamp-decode-riff/src/wav.rs` já usa u64 e o que falta é o fixture versionado e os perfis novos. **O que não existe**: nenhum dos adapters WMA, APE, WavPack, TTA, MPC, WebM, MKA está em `crates/`; `cargo test --workspace --locked` dá 508 verdes, 3 ignorados e 0 falhas em 47 suítes, e nenhum teste abre arquivo maior que 2,2 MB. No AC-3 o comportamento medido hoje é `Unsupported("container")` para os seis arquivos de 40 bytes, `Unsupported("format tag")` para o WAV com tag 0x0161, `Invalid { what: "missing data chunk", offset: 12 }` para o cabeçalho cortado em 12 bytes e `Invalid { what: "unexpected eof", offset: 20 }` para o corte em 20 bytes; um WAV com cabeçalho inteiro e 4096 bytes aleatórios de corpo **não** falha, abre com `frames Some(2048)`, e o `unexpected eof` no offset 4096 só aparece quando o RIFF declara tamanho 0xFFFFFFFF, então a evidência tem que usar os cortes que de fato falham e mostrar as mesmas variantes de erro com os perfis novos ligados, não só "não crashou". O FFmpeg fixado gera wavpack, tta, wmav1, wmav2 e mp2, e não tem encoder de APE, MPC ou MP1 — a evidência tem que dizer qual fixture veio de outra fonte e declarar a origem |
| Hardware | nenhum: os perfis se decodificam de fixture em disco e nenhum caminho abre dispositivo de áudio. O arquivo acima de 4 GiB é esparso e não custa espaço nem I/O real; a ausência de encoder de APE, MPC e MP1 no FFmpeg fixado é limitação de ferramenta, declarada, não de hardware | ausência é limitação registrada, não aprovação |

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
