# TASK-E17-17 — Expandir speech, AAC low-delay e legados

> Gerado pelo passo 37 a partir de `tasks/backlog.json`. Os campos de estado vêm do
> backlog e não devem ser editados aqui. A coluna `Then` é o requisito aprovado,
> copiado literal do backlog; `Given` e `When` foram autorados na execução.

| Campo | Valor |
|---|---|
| ID | `TASK-E17-17` |
| Módulo dono | `vynamp-decode` |
| Repo | `jimmykedlyn/vynamp-decode` |
| REQ coberto | `REQ-E17-17` |
| Critérios da spec | pendente — `specs/decode.md` §5 |
| Fase | M5 |
| Prioridade | P1 |
| Estado | planned |
| Depende de | `TASK-E01-08`, `TASK-E12-05` |
| Bloqueia | nenhuma |
| Modelo executor | Opus — áudio/RT, concorrência, segurança ou release |
| Esforço | 2–5 dias úteis |
| Risco | alto |

## Entrega

Certificação AMR/3GP/Speex/LD/ELD/WMA Voice e bindings legacy opcionais.

## Boundary

- **Não** fixa o catálogo de formatos nem o modelo de capability e o probe por alvo: isso é `TASK-E17-01`, que versiona `config/format-catalog.json` e `schemas/codec-capability.schema.json`. Aqui as entradas `amr`, `aac-low-delay`, `speex` e `wma-voice` são consumidas como já declaradas na revisão 0.7.0 com `state` `planned` e `policy` `optional`; acrescentar campo ao schema de capability para caber o perfil LD/ELD duplicaria o dono do contrato que o manifest de release lê.
- **Não** resolve licença nem inventário de dependências: quem audita fontes, versões e política de build e distribuição de codec é `TASK-E11-03`. Esta TASK publica o helper e o texto de licença do binding opcional que ela certifica, apontando para esse inventário; escolher a licença do backend AMR ou do Speex, ou decidir se ele entra na build, é decisão de lá, e tomá-la aqui faria duas listas de licença discordarem no mesmo release.
- **Não** implementa trim, priming, preskip nem gain e DRC por codec: o contrato de `intrinsic_gain_applied_db`, `drc_applied` e timeline é `TASK-E17-02`. Codec de fala tem atraso intrínseco — o AMR trabalha em quadros de 20 ms e o Speex tem lookahead — e é tentador dobrar esse atraso no trim daqui; nesta TASK o `Track::delay` e o `Track::padding` só recebem o valor que o demuxer mediu, e a aplicação é do Reader entregue em `TASK-E17-02`.
- **Não** implementa USAC/AOT42, que é o xHE-AAC: o adapter e o relatório de indisponibilidade de xHE são `TASK-E17-04`. A confusão é plausível porque a spec trata AAC-LC, HE, USAC, LD e ELD na mesma frase e o catálogo lista as duas entradas lado a lado; aqui entram só os perfis de baixa latência LD, ELD e ELDv2 que a prova fechar, e xHE continua fora mesmo compartilhando o MP4.
- **Não** implementa WMA v1/v2/Pro/Lossless, APE, WavPack, TTA nem MPC: esses adapters são `TASK-E01-08` e a certificação de lossless e híbridos é `TASK-E17-05`. WMA Voice certifica-se aqui porque é codec de fala em ASF, mas herda o demuxer ASF de `TASK-E01-08`; escrever um segundo parser ASF para o perfil de voz daria dois donos do mesmo contêiner e dois lugares para corrigir a mesma falha de tamanho de objeto.
- **Não** entrega trackers, MIDI, SoundFont nem os codecs raros opcionais: isso é `TASK-E12-05`, que é dependência desta TASK. A palavra `legados` no título puxa MOD/XM/IT e SF2 para cá, e a palavra `helper` puxa o helper nativo isolado de `TASK-E07-05`; o helper publicado aqui é o do binding de codec opcional, um documento por perfil com backend, versão, licença e limites, sem RPC, sandbox nem memória compartilhada.

## Antes de começar

- [ ] Spec do módulo aprovada e o critério de aceite citado já existe nela
- [ ] Worktree própria criada (`cargo xtask repo worktree task-e17-17-claude-<slug>`)
- [ ] Fixtures/hardware necessários disponíveis ou limitação registrada
- [ ] Nenhuma TASK em execução tocando os mesmos arquivos

## Critérios de aceite

| ID | Given | When | Then |
|---|---|---|---|
| `TASK-E17-17-AC-1` | um corpus de fala com um arquivo por perfil — AMR-NB 12,2 kbit/s em `.amr` (8000 Hz mono, 0,32 s, como o que o FFmpeg empacotado gera em 518 bytes), AMR-WB em `.3gp`, Speex NB, WB e UWB em `.spx`, AAC-LD e AAC-ELD em `.m4a`, WMA Voice em `.asf` — cada um com o SHA-256 no mesmo manifest de fixtures que `tests/fixtures/mpeg/MANIFEST.txt` usa, mais um `CancelSource` armado e um `Options` com `Limits::default()` (teto de 32 canais e 768 kHz), e uma pasta de entrega ainda vazia onde moram o helper do binding opcional e o texto de licença por perfil | o harness abre cada um dos 8 arquivos pela fachada `vynamp_decode::open` com a feature de fala ligada, puxa pacotes até disparar o cancelamento no meio do segundo pacote, depois reabre e faz `seek` para o frame 160 do último pacote com `SeekTo` medindo `preroll`, e por fim lista e lê os documentos de licença e de helper publicados por perfil | Fixture por perfil, cancel/seek limites e helper/licença publicados |
| `TASK-E17-17-AC-2` | um diretório de varredura com o corpus de fala completo mais três arquivos que têm faixa de vídeo — um `.3gp` de 640x480 com H.264 e AMR-NB, um `.mp4` com H.264 e AAC-LC e um `.avi` com MP3 — um build em que as features `amr`, `speex`, `aac-ld` e `wma-voice` estão parcialmente desligadas, e o manifest de capacidades e os textos de interface gerados para esse build | o harness roda a associação em lote sobre o diretório inteiro com a política de abertura do produto, coleta a lista de extensões que o sistema registra e varre manifest, README, catálogo e textos de interface procurando promessa de suporte universal a codecs ou de cobertura de todas as extensões | não associar vídeo em lote nem prometer todos codecs. |

## Verificação

| Item | Comando / fixture | Evidência esperada |
|---|---|---|
| Teste automatizado | `cargo test` no repo `vynamp-decode` | verde no CI Gate |
| Oráculo independente | um teste de integração novo em `libs/decode/crates/vynamp-decode/tests/speech.rs`, atrás de uma feature `speech` a criar no `Cargo.toml` da fachada, rodado com `cargo test -p vynamp-decode --features speech --test speech`, sobre 8 fixtures gerados ou coletados por `tools/fixtures/generate-speech-dev.ps1` no molde de `tools/fixtures/generate-mpeg-dev.ps1` — `.amr` AMR-NB 12,2 kbit/s 8000 Hz mono, `.3gp` AMR-NB, `.spx` Speex NB, WB e UWB, `.m4a` AAC-LD e AAC-ELD, `.asf` WMA Voice — com `tests/fixtures/speech/MANIFEST.txt` separado por barra vertical no mesmo formato de `tests/fixtures/mpeg/MANIFEST.txt`, mais um `CancelSource` disparado no segundo pacote e um `seek` para o frame 160 | por perfil: o `Track` aberto declara taxa e canais do próprio arquivo (8000 Hz mono no AMR-NB, 16000 Hz mono no Speex WB, 48000 Hz mono no AAC-ELD), `Reader::frames()` bate com a duração do manifest, o cancelamento devolve `Err(Error::Cancelled)` nas 8 fixtures e o `seek` ao frame 160 nao falha e devolve `SeekTo` cujo `preroll` nao passa de um quadro de codec mais o lookahead do perfil (320 amostras a 16 kHz no quadro do Speex WB), e o manifest e o helper por perfil listam backend, versão, licença e limites de taxa e canais; no lote, nenhuma das três extensões com vídeo entra na associação e nenhum texto afirma cobertura universal. O que ainda não existe hoje: `libs/decode` tem 106 arquivos `.rs` em 9 crates e `grep -i` por `amr`, `speex` e `aac` nos `.rs` devolve 0 ocorrências; `CodecId` em `crates/vynamp-decode-core/src/track.rs` só enumera `Pcm`, `Flac`, `Mp1`, `Mp2` e `Mp3`; não há crate de MP4, ASF ou Ogg; `grep -c capabilit` nos `.rs` também dá 0, então o registry de capability não existe; e `tests/fixtures/` tem 151 arquivos em flac, flac-conformance, mpeg e pcm, nenhum de fala, de modo que o teste nem compila. Dos 8 perfis só o AMR-NB é gerável com o FFmpeg empacotado em `.cache/tools/ffmpeg/ffmpeg-n8.1.3-23-g5c4f5744cd-win64-gpl-8.1/bin/ffmpeg.exe`, que traz `libopencore_amrnb` como encoder mas só tem `amrwb`, `speex` e `wmavoice` como decoder e recusa `-profile:a aac_ld` com `Profile not supported!`; os outros 7 perfis têm de vir de corpus externo licenciado e o script de geração falha alto quando o arquivo falta, em vez de pular o perfil |
| Hardware | nenhum: os 8 perfis entram por arquivo e nenhum teste abre dispositivo de áudio, porque fala é decodificada em CPU a partir de `SliceSource` e `FileSource` e o cancelamento e o `seek` são exercidos no `Reader`, sem rota de saída. O que falta é corpus, não hardware: esta máquina é Windows x64 com o FFmpeg n8.1.3 empacotado em `D:/Projects/vynamp-media-player/.cache/tools/ffmpeg/`, e nenhum perfil de fala exige placa pro, aparelho Android, aparelho iOS ou máquina macOS | ausência é limitação registrada, não aprovação |

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
