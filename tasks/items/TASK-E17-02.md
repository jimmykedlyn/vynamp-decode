# TASK-E17-02 — Implementar trim, gain e timeline por codec

> Gerado pelo passo 37 a partir de `tasks/backlog.json`. Os campos de estado vêm do
> backlog e não devem ser editados aqui. A coluna `Then` é o requisito aprovado,
> copiado literal do backlog; `Given` e `When` foram autorados na execução.

| Campo | Valor |
|---|---|
| ID | `TASK-E17-02` |
| Módulo dono | `vynamp-decode` |
| Repo | `jimmykedlyn/vynamp-decode` |
| REQ coberto | `REQ-E17-02` |
| Critérios da spec | pendente — `specs/decode.md` §5 |
| Fase | M2 |
| Prioridade | P0 |
| Estado | planned |
| Depende de | `TASK-E01-07` |
| Bloqueia | `TASK-E17-20` |
| Modelo executor | Opus — áudio/RT, concorrência, segurança ou release |
| Esforço | 2–5 dias úteis |
| Risco | alto |

## Entrega

Contratos de priming, preskip, gain/DRC e samples por demux/decoder.

## Boundary

- **Não** implementa os decodificadores nem os demuxers que carregam esses números. Opus, Vorbis, AAC, ALAC e os containers MP4, Matroska e Ogg são `TASK-E01-07`; aqui só se define e certifica o contrato de trim, ganho e timeline que eles preenchem. Trazer o adapter de Opus para cá faria dois donos do mesmo parse.
- **Não** faz o predecode da próxima faixa nem o encadeamento sample-accurate do mix. O engine que consome esta timeline é `TASK-E01-06` (`Predecode next e trim validado`); o contrato entregue aqui alimenta aquele, e reimplementar o encaixe no decode esconderia o bug de gapless onde ele não está.
- **Não** decide política de ganho de reprodução, EQ, preamp, volume, pan nem ReplayGain track/album. Isso é `TASK-E04-01` em vynamp-engine. O delimitador entre as duas é exatamente o que esta TASK mede: o decoder informa o que já aplicou e o engine não reaplica; escolher o alvo de loudness aqui dobraria o ganho no consumidor.
- **Não** implementa o guard true-peak depois do processamento nem o estimador independente dele. É `TASK-E17-07` no engine, e o teto medido no PCM final é `TASK-E17-08`. Medir pico aqui, num contrato de trim, confundiria ganho intrínseco do codec com proteção de cadeia.
- **Não** publica `drc_control` e `neutral_path` no manifest por artefato. O manifest real de capacidades é `TASK-E17-12`; esta TASK produz o valor que o decode devolve, não o documento que o artefato publica.
- **Não** implementa o DRC normativo do xHE-AAC/USAC. O adapter de AOT42 é `TASK-E17-04`; aqui o contrato é genérico e o xHE é o consumidor que tem `drc_control` mais rico.

## Antes de começar

- [ ] Spec do módulo aprovada e o critério de aceite citado já existe nela
- [ ] Worktree própria criada (`cargo xtask repo worktree task-e17-02-claude-<slug>`)
- [ ] Fixtures/hardware necessários disponíveis ou limitação registrada
- [ ] Nenhuma TASK em execução tocando os mesmos arquivos

## Critérios de aceite

| ID | Given | When | Then |
|---|---|---|---|
| `TASK-E17-02-AC-1` | um corpus de trim com um Ogg Opus de 4800 frames a 48 kHz cujo OpusHead declara pre_skip 312, output_gain 0 e mapping family 0 e cuja última página tem granuleposition 5112, um MP3 com tag LAME válida de encoder delay 576 e padding 1152, um MP4 com dOps declarando pre_skip 312 e duration_ts 4800, um MKA com CodecDelay e DiscardPadding nos elementos 0x56AA e 0x75A2 e um Ogg Vorbis com granuleposition final 4800, todos com SHA256 em MANIFEST.txt | o Reader abre cada arquivo, puxa todos os pacotes até o EOF e o harness conta os frames entregues na timeline trimada e compara com a contagem declarada por arquivo, sem heurística de silêncio para o caso de metadado ausente | Opus/MP3/MP4/Matroska/Ogg fixtures não perdem ou duplicam frames |
| `TASK-E17-02-AC-2` | o mesmo corpus mais um Opus cujo OpusHead traz output_gain de +5 dB (1280 em Q8.8), um arquivo com ReplayGain track de -6 dB declarado na tag e um trilho cujo backend responde drc_control unknown, sabendo que hoje nenhum resultado de decode em crates/ tem campo de ganho | cada decodificador devolve por resultado intrinsic_gain_applied_db, drc_applied e o metadata pendente, e o harness soma os estágios que a camada de cima registra e mede o pico do PCM final para conferir aplicação única | ganho/DRC/RG aplicados uma vez e caminho neutro documentado. |

## Verificação

| Item | Comando / fixture | Evidência esperada |
|---|---|---|
| Teste automatizado | `cargo test` no repo `vynamp-decode` | verde no CI Gate |
| Oráculo independente | um teste novo em crates/vynamp-decode/tests/timeline.rs sobre tests/fixtures/trim/ (opus, ogg, mp4 e mka por binding, cada um com SHA256 em MANIFEST.txt, gerados por tools/fixtures/generate-trim-dev.ps1 com o FFmpeg fixado n8.1.3-23-g5c4f5744cd, que tem libopus e libvorbis), mais `cargo test --workspace --locked` em libs/decode. Hoje nada disso existe: tests/fixtures/ tem só flac, flac-conformance, mpeg e pcm, e `grep -rniE` por opushead, dops, 0x56aa e discardpadding em crates/*/src devolve 0 linhas | por fixture, os frames entregues têm de ser exatamente o número declarado: no Opus, granuleposition final 5112 menos pre_skip 312 dá 4800 frames a 48 kHz, e o fixture que o FFmpeg fixado gera mede pre_skip 312, output_gain 0 e mapping family 0; no MP3 com LAME de delay 576 e padding 1152 o Track tem de sair com delay 1105 e padding 623 (576 mais 529 e 1152 menos 529, a aritmética documentada em crates/vynamp-decode-mpa/src/demux.rs:766); no MP4 o dOps traz pre_skip 312 e duration_ts 4800. Para o ganho, a evidência é intrinsic_gain_applied_db valendo 5,0 uma única vez e drc_applied coerente com o drc_control declarado. O que ainda não existe: `grep -rni` por replaygain e por drc em crates/*.rs dá 0 linhas, nenhum resultado de decode carrega campo de ganho, os 508 testes verdes e 3 ignorados da suíte só exercitam delay e padding do Reader, e os MP3 do repo têm tag Info com LAME Lavf e delay 0, então nenhum fixture atual prova trim real de MP3 |
| Hardware | nenhum: o critério é contagem de frames e valor de ganho reportado sobre arquivos versionados, e nenhum teste abre dispositivo de áudio; a rota de saída é de vynamp-output, não deste repo | ausência é limitação registrada, não aprovação |

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
