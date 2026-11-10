# TASK-E01-09 — Entregar channel mapping e DSD para PCM

> Gerado pelo passo 37 a partir de `tasks/backlog.json`. Os campos de estado vêm do
> backlog e não devem ser editados aqui. A coluna `Then` é o requisito aprovado,
> copiado literal do backlog; `Given` e `When` foram autorados na execução.

| Campo | Valor |
|---|---|
| ID | `TASK-E01-09` |
| Módulo dono | `vynamp-decode` |
| Repo | `jimmykedlyn/vynamp-decode` |
| REQ coberto | `REQ-E01-09` |
| Critérios da spec | pendente — `specs/decode.md` §5 |
| Fase | M3 |
| Prioridade | P1 |
| Estado | planned |
| Depende de | `TASK-E01-08`, `TASK-E04-02` |
| Bloqueia | `TASK-E01-10`, `TASK-E12-04` |
| Modelo executor | Opus — áudio/RT, concorrência, segurança ou release |
| Esforço | 5–15 dias úteis |
| Risco | alto |

## Entrega

Mapas mono/stereo/5.1/7.1 e DSF/DFF decimator com diagnóstico.

## Boundary

- **Não** entrega DSD nativo nem DoP. Backend condicionado ao DAC, packing DoP e capabilities de taxa/DSD são `TASK-E12-04`; está fora de escopo de um decoder PCM, e `config/format-catalog.json` separa `dsd-pcm` (M3, required, demux+decode) de `dsd-native` (M5, conditional, demux+passthrough) exatamente por isso.
- **Não** faz resampling para trocar a taxa de saída do DSD nem trata a taxa PCM resultante como arbitrária. O resampler próprio e a política de qualidade são `TASK-E04-02`, que esta TASK declara como dependência; aqui o decimador fixa uma taxa de saída documentada e nada além disso.
- **Não** implementa os controles de canal da engine — mono, routing, upmix, bass management e os meters de cada estágio. São `TASK-E04-07` e `TASK-E12-10`; o downmix daqui é a matriz explícita do decoder, provada por fixture, e não o nó de grafo que a UI manipula.
- **Não** tem dono do surround físico nem do hardware multicanal. Bass management, delays por canal e saída em driver multicanal exigem hardware real e ficam em `TASK-E12-10`; aqui o mapa termina em amostras PCM num buffer, sem negociar com dispositivo nenhum.
- **Não** cobre WavPack DSD, DFF DST nem extração de SACD ISO. `TASK-E17-18` é dona dessa certificação e `TASK-E17-05` do corpus lossless e híbrido; tratar `.wv` com DSD ou DST aqui criaria dois parsers para o mesmo framing.
- **Não** decide o downmix de formatos cinematográficos licenciados. AC3/EAC3/TrueHD/MLP/DTS com downmix declarado é `TASK-E17-16`; a matriz provada aqui é a de PCM de mapa conhecido, e apresentá-la como downmix Dolby seria alegar formato licenciado inexistente.

## Antes de começar

- [ ] Spec do módulo aprovada e o critério de aceite citado já existe nela
- [ ] Worktree própria criada (`cargo xtask repo worktree task-e01-09-claude-<slug>`)
- [ ] Fixtures/hardware necessários disponíveis ou limitação registrada
- [ ] Nenhuma TASK em execução tocando os mesmos arquivos

## Critérios de aceite

| ID | Given | When | Then |
|---|---|---|---|
| `TASK-E01-09-AC-1` | um WAV 5.1 extensible de 48 kHz 24 bits com `channel_mask` 0x3f em que cada canal carrega um sinal distinguível (pan com ganhos 1, 0,5, 0,25, 0,125, 0,0625 e 0,03125 sobre o mesmo tom, dando picos de 4.194.304, 2.097.152, 1.048.576, 524.288, 262.144 e 131.072 — medido com o FFmpeg fixado e `-c:a pcm_s24le`, porque o `tools/fixtures/source-sine.wav` tem pico 1.073.741.824 em cada canal e o corte para 24 bits divide por 256) — o fixture versionado `tests/fixtures/pcm/sine-6ch-s24le.wav` tem `channel_mask` 0x3f e 4.800 frames, mas repete o mesmo plano em 0/2/4 e em 1/3/5 com pico 4.194.304 em todos, então não distingue canal errado de canal certo —, mais os mapas mono, stereo e 7.1 e a matriz de downmix 5.1 para stereo com o headroom declarado | o arquivo é demuxado e decodificado, o plano de cada canal é comparado com o sinal que foi gravado nele, e o downmix é calculado e comparado com o mesmo downmix feito pelo FFmpeg fixado com `-af` com `pan=stereo` e a matriz de coeficientes 0,5/0,25/0,125/0,25 por lado, gravando f32 bruto com `-f f32le`, em f32 bruto | Canal mapeado corretamente, headroom aplicado em downmix |
| `TASK-E01-09-AC-2` | três DSF montados byte a byte no formato do container (bloco `DSD ` de 28 bytes, `fmt ` de 52 bytes com taxa 2.822.400, 5.644.800 e 11.289.600 Hz, bloco `data` com 4.096 bytes por canal e padrão `0xAA 0x55` alternado), mais os DFF equivalentes, cada um com uma linha de `MANIFEST.txt` declarando taxa DSD, taxa PCM de saída e o `divisor` de decimação escolhido | cada fixture é aberto, decodificado até o fim e a capacidade reportada é inspecionada — o rótulo do descritor, o `CodecId` e o `SampleFormat`/taxa de saída — e a taxa de saída é confrontada com a taxa DSD dividida pelo decimador declarado, não com a taxa DSD | DSD64/128/256 fixture não é rotulado native. |

## Verificação

| Item | Comando / fixture | Evidência esperada |
|---|---|---|
| Teste automatizado | `cargo test` no repo `vynamp-decode` | verde no CI Gate |
| Oráculo independente | um gerador de fixture em `tools/fixtures/` que escreve o DSF/DFF byte a byte (o FFmpeg fixado tem o demuxer `dsf` e os decoders `dsd_lsbf_planar`/`dsd_msbf_planar`, mas não tem encoder nem muxer de DSD, então o fixture não pode sair dele), mais `pwsh tools/fixtures/generate.ps1` para o WAV 5.1 distinguível; prova por `cargo test -p vynamp-decode --test channel_map --test dsd --locked` e comparação numérica do downmix contra o f32 bruto do FFmpeg | a evidência do AC-1 tem que mostrar seis planos distintos com os picos declarados e o downmix batendo com o do FFmpeg dentro de tolerância declarada, com a diferença de headroom medida em dB. **Medido hoje**: `crates/vynamp-decode-riff/src/fmt.rs` já lê `channel_mask` do `fmt ` extensible (o fixture versionado traz 0x3f) e o expõe em `Track::channel_mask`, mas `crates/vynamp-decode-core` não tem `ChannelLayout` — `grep -rn ChannelLayout crates/` volta vazio — e `grep -rni downmix crates/` devolve 1 linha só, o nome do teste `mono_fixture_is_the_downmix` em crates/vynamp-decode-flac/tests/oracle.rs:68, nenhuma implementação. O AC-2 não tem nada: `grep -rni -e dsf -e dff -e dsd crates/ specs/` volta vazio, `CodecId` não tem variante DSD e não existe crate de DSD; hoje um `.dsf` cai em `Unsupported("container")`. A linha de base é 508 verdes, 3 ignorados e 0 falhas em 47 suítes |
| Hardware | nenhum: o mapa de canal e o downmix são aritmética sobre buffer e o DSD é decimado para PCM em memória, então nenhum DAC, placa multicanal, aparelho Android, iOS ou macOS é tocado. A ausência de hardware multicanal real é limitação declarada da prova, não aprovação — a evidência para na matriz de amostras | ausência é limitação registrada, não aprovação |

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
