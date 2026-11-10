# TASK-E17-03 — Completar bindings de containers e seleção de áudio

> Gerado pelo passo 37 a partir de `tasks/backlog.json`. Os campos de estado vêm do
> backlog e não devem ser editados aqui. A coluna `Then` é o requisito aprovado,
> copiado literal do backlog; `Given` e `When` foram autorados na execução.

| Campo | Valor |
|---|---|
| ID | `TASK-E17-03` |
| Módulo dono | `vynamp-decode` |
| Repo | `jimmykedlyn/vynamp-decode` |
| REQ coberto | `REQ-E17-03` |
| Critérios da spec | pendente — `specs/decode.md` §5 |
| Fase | M3 |
| Prioridade | P0 |
| Estado | planned |
| Depende de | `TASK-E01-08` |
| Bloqueia | `TASK-E17-13`, `TASK-E17-14`, `TASK-E17-15`, `TASK-E17-16`, `TASK-E17-20` |
| Modelo executor | Opus — áudio/RT, concorrência, segurança ou release |
| Esforço | 2–5 dias úteis |
| Risco | alto |

## Entrega

CAF/AIFF-C/AU/Ogg FLAC, MP4 e Matroska/TS com seleção de faixas de áudio.

## Boundary

- **Não** implementa os codecs que esses containers carregam. AAC-LC, HE-AAC, ALAC, Vorbis e Opus são `TASK-E01-07`; esta TASK é dona do binding demux e da escolha de faixa. Implementar o decoder de Opus aqui faria a certificação de container depender do codec errado.
- **Não** define o trim, ganho e timeline dos bindings novos. O contrato de priming, preskip e ganho é `TASK-E17-02`, que depende desta para existir; duplicar a aritmética nos dois lados é como o delay acaba aplicado duas vezes.
- **Não** decodifica vídeo nem sincroniza A/V. O catálogo mantém vídeo fora e a associação de extensões de vídeo é `TASK-E16-07`, que certifica ativação e associações nos cinco alvos; abrir MKV aqui não cria player de vídeo nem justifica default de `video/webm`.
- **Não** escreve nem lê tag, capítulo ou sidecar desses containers. A matriz read/write de ID3, MP4, Matroska, CAF, AIFF e BWF é `TASK-E17-06`; parsers de capa e capítulo no demux de áudio ampliariam a superfície de metadata sem dono.
- **Não** faz o binding condicional de AC-4, MPEG-H ou IAMF em MP4 e TS. `TASK-E17-13`, `TASK-E17-14` e `TASK-E17-15` são as donas, e todas dependem desta; aqui eles não entram nem como recusa especial.
- **Não** publica a capability por artefato. O manifest real de capacidades é `TASK-E17-12`; esta TASK entrega o que o probe e o binding respondem, não o documento assinado do build.

## Antes de começar

- [ ] Spec do módulo aprovada e o critério de aceite citado já existe nela
- [ ] Worktree própria criada (`cargo xtask repo worktree task-e17-03-claude-<slug>`)
- [ ] Fixtures/hardware necessários disponíveis ou limitação registrada
- [ ] Nenhuma TASK em execução tocando os mesmos arquivos

## Critérios de aceite

| ID | Given | When | Then |
|---|---|---|---|
| `TASK-E17-03-AC-1` | um CAF PCM s16le e um CAF f32le de 4800 frames a 48 kHz, um AIFF-C sowt e um AIFC float, um AU linear PCM de 4800 frames, um Ogg FLAC, um M4A com ALAC, um M4A com Opus dOps, um MKA Opus com CodecDelay e um MPEG-TS com AAC ADTS, cada um com SHA256 em MANIFEST.txt e gerado pelo FFmpeg fixado | cada arquivo passa por Registry::probe com o hint_ext do nome, a duração da faixa é lida, o seek roda em 0, 1, 576, 1152, metade e último frame e a leitura é cancelada no meio com CancelSource::cancel | Bindings exigidos passam duration/seek/cancel |
| `TASK-E17-03-AC-2` | um WebM craftado que declara CodecID A_FLAC, mais o stub de 264 bytes que o FFmpeg n8.1.3 deixa ao recusar FLAC em WebM (mensagem Only VP8 or VP9 or AV1 video and Vorbis or Opus audio) e a entrada webm-audio do catálogo consultada em runtime | o probe e a consulta de capability respondem sobre os dois arquivos e o resultado é comparado com a lista de perfis que WebM aceita, sem consultar a extensão | WebM não anuncia FLAC |
| `TASK-E17-03-AC-3` | um MKA com duas faixas Opus, language eng e por, disposition default na faixa 0 e nenhuma na faixa 1, mais um MKV com uma faixa de vídeo e duas de áudio em línguas diferentes | a política de seleção roda sobre a lista de Track devolvida pelo demuxer, sem abrir decodificador de vídeo e sem decodificar áudio, e a mesma consulta é repetida dez vezes para conferir determinismo | seleção language/default é determinística sem decodificar vídeo. |

## Verificação

| Item | Comando / fixture | Evidência esperada |
|---|---|---|
| Teste automatizado | `cargo test` no repo `vynamp-decode` | verde no CI Gate |
| Oráculo independente | seis crates novas (caf, au, ogg, mp4, matroska, ts) registradas no facade, tests/fixtures/caf, tests/fixtures/au, tests/fixtures/ogg, tests/fixtures/mp4, tests/fixtures/mka e tests/fixtures/ts com SHA256 em MANIFEST.txt gerados por tools/fixtures/generate-containers-dev.ps1, e um teste de seek por binding no molde de assert_seek_equivalence em crates/vynamp-decode/tests/mpeg.rs:293. Hoje `cargo run -p vynamp-decode --example support_matrix` imprime exatamente 4 containers (riff, aiff, flac, mpa) e 4 codecs (pcm, flac, mpeg-audio, mpeg-audio-layer1), e crates/ tem 9 crates sem nenhuma de CAF, AU, Ogg, MP4, Matroska ou TS | por binding novo, duração igual à do fixture (CAF s16le e f32le de 4800 frames, AU de 4800, MKA Opus com CodecDelay e DiscardPadding achados nos bytes 346 e 5442 do par de duas faixas que o FFmpeg fixado gerou) e seek que devolve o mesmo slice do decode completo nas 12 posições do molde, com Error::Cancelled na leitura cancelada. WebM com A_FLAC tem de sair Unsupported e não decodificar: o FFmpeg n8.1.3 não escreve FLAC em WebM e deixa um stub de 264 bytes (medido), e a única guarda existente hoje é estática, sobre o JSON, em xtask/src/catalog.rs:75-89. A seleção de idioma não tem como passar hoje: `grep -rniE` por language, lang_code e disposition em crates/*/src dá 0 linhas, Track não tem campo de língua nem de default, e cada demuxer devolve uma única faixa (tracks: vec![track] em aiff lib.rs:132, flac demux.rs:787, riff lib.rs:158) |
| Hardware | nenhum: os critérios são sobre bytes e metadados de container (probe, duração, seek, cancel e escolha de faixa) e nenhum dispositivo é aberto; o próprio AC-3 exige não decodificar vídeo, o que é verificação de código e não de hardware | ausência é limitação registrada, não aprovação |

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
