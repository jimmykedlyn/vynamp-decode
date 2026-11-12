# TASK-E17-16 — Certificar áudio cinematográfico e passthrough

> Gerado pelo passo 37 a partir de `tasks/backlog.json`. Os campos de estado vêm do
> backlog e não devem ser editados aqui. A coluna `Then` é o requisito aprovado,
> copiado literal do backlog; `Given` e `When` foram autorados na execução.

| Campo | Valor |
|---|---|
| ID | `TASK-E17-16` |
| Módulo dono | `vynamp-decode` |
| Repo | `jimmykedlyn/vynamp-decode` |
| REQ coberto | `REQ-E17-16` |
| Critérios da spec | pendente — `specs/decode.md` §5 |
| Fase | M5 |
| Prioridade | P1 |
| Estado | planned |
| Depende de | `TASK-E17-03`, `TASK-E04-10` |
| Bloqueia | nenhuma |
| Modelo executor | Opus — áudio/RT, concorrência, segurança ou release |
| Esforço | 2–5 dias úteis |
| Risco | alto |

## Entrega

AC3/EAC3/TrueHD/MLP/DTS Core/HD, downmix e saída codificada condicionada.

## Boundary

- **Não** implementa o caminho de saída codificada no host nem o exclusive do backend de áudio: a saída exclusiva e os device profiles são `TASK-E04-05`, o caminho integer direct e as combinações validadas são `TASK-E04-06`, e a rota, o codec negociado e o headroom por dispositivo são `TASK-E17-19`, todos no `vynamp-output`. Esta TASK decide e prova se o sinal codificado pode seguir por aquele caminho; implementá-lo aqui daria dois donos do mesmo exclusive.
- **Não** implementa AC-4, MPEG-H nem IAMF: AC-4 com DRC é `TASK-E17-14`, MPEG-H com mha1 e mhm1 é `TASK-E17-13` e o modelo IAMF é `TASK-E17-15`. A spec lista `TrueHD/MLP/E-AC-3/DTS core/HD-HR/HD-MA` como entradas separadas ao lado de Atmos, e o catálogo traz `ac3`, `eac3`, `truehd-mlp` e `dts` com `operations` idêntico, de modo que a confusão entre os quatro é fácil e cada codec precisa de dono único.
- **Não** implementa o downmix PCM básico nem o mapeamento de canais: o mapeamento de canais e a conversão de DSD para PCM são `TASK-E01-09`, no mesmo repo. O downmix que a Entrega cita é o que esta TASK mede contra a referência do codec, e a matriz de canais do lado Vynamp continua sendo de `TASK-E01-09`.
- **Não** fecha a matriz numérica de qualidade do DSP nem o callback budget e o soak de 8 horas: isso é `TASK-E04-10`, dependência declarada desta TASK, e a evidência aqui é a incompatibilidade pontual de DSP por codec, não o teto medido.
- **Não** resolve a licença de patente nem o inventário de fontes e versões: o inventário de codecs e a política de distribuição é `TASK-E11-03`. O `LEGAL.md` do repo já classifica E-AC-3, TrueHD/MLP e DTS na feature `encumbered` desligada por padrão, e a conclusão de licença de software versus licença de patente por codec cabe aqui; escolher backend e decidir o que entra na build não cabe.
- **Não** anuncia Dolby Atmos nem DTS:X como renderer genérico nem implementa renderer imersivo Dolby: renderer de codec de objeto é `TASK-E17-13` para MPEG-H e `TASK-E17-15` para IAMF, e esta TASK é a que precisa provar a **ausência** do anúncio. A spec diz que Atmos não é formato genérico autônomo, e o catálogo registra a restrição de que metadata de Atmos e DTS:X não concede renderer imersivo; implementar ou anunciar o renderer aqui produziria exatamente a promessa que a AC-1 proíbe.

## Antes de começar

- [ ] Spec do módulo aprovada e o critério de aceite citado já existe nela
- [ ] Worktree própria criada (`cargo xtask repo worktree task-e17-16-claude-<slug>`)
- [ ] Fixtures/hardware necessários disponíveis ou limitação registrada
- [ ] Nenhuma TASK em execução tocando os mesmos arquivos

## Critérios de aceite

| ID | Given | When | Then |
|---|---|---|---|
| `TASK-E17-16-AC-1` | um corpus cinematográfico com um `.ac3` 5.1, um `.ec3` E-AC-3 5.1, um `.mka` com TrueHD dentro de Matroska, um `.mlp`, um `.dts` DTS Core e um `.dtshd` DTS-HD, mais um arquivo com metadata Atmos e outro com metadata DTS:X, e a entrada `dts` de `config/format-catalog.json` rev 0.7.0, que está `state: planned`, `policy: conditional` e `operations: [demux, decode, passthrough]` | cada arquivo é decodificado, a capability resultante é lida e o relatório por codec é confrontado com a lista de renderers disponíveis no build, e o teste exige que nenhum arquivo com metadata Atmos ou DTS:X resulte em `render` disponível nem em capability de renderer genérico, com a recusa explícita e o motivo registrados | Atmos/DTS:X não anunciados como renderer genérico |
| `TASK-E17-16-AC-2` | o mesmo corpus cinematográfico decodificado em PCM, mais a saída codificada condicionada do host, o registro de rota de dispositivo disponível neste build e o texto de licença do backend, e a matriz numérica de qualidade do DSP (`TASK-E04-10`) como referência de incompatibilidade | o teste percorre codec por codec, executa o downmix PCM básico quando o codec entrega mais canais que a rota, tenta a saída codificada contra a rota do host, e para cada caso registra o que foi provado, o que foi recusado e o motivo; quando o DSP do host não pode reprocessar o sinal, o caminho é marcado incompatível em vez de cair para PCM silencioso | receiver/rota/licença e DSP incompatível comprovados ou recusados. |

## Verificação

| Item | Comando / fixture | Evidência esperada |
|---|---|---|
| Teste automatizado | `cargo test` no repo `vynamp-decode` | verde no CI Gate |
| Oráculo independente | um teste de integração novo em `libs/decode/crates/vynamp-decode/tests/cinematic.rs`, rodado por `cargo test -p vynamp-decode --features encumbered --test cinematic`, sobre um corpus novo em `tests/fixtures/cinematic/` com `MANIFEST.txt` por barra vertical no mesmo formato de `tests/fixtures/mpeg/MANIFEST.txt` e PCM de referência por codec, mais `cargo test --workspace --locked` no `libs/decode` e `cargo xtask check-docs` no umbrella | esperado: por codec, o relatorio traz o numero de canais, o PCM decodificado comparado com a referencia dentro da tolerancia declarada e o resultado do downmix com o desvio numerico impresso; nenhum dos dois arquivos com metadata Atmos ou DTS:X produz `render` disponivel, e a recusa vem com o motivo; o caminho de saida codificada e marcado como provado ou recusado por codec contra a rota do host, e o DSP incompativel e nomeado em vez de cair para PCM silencioso; a linha de licenca por codec separa licenca de software de licenca de patente. O que ainda nao existe hoje: um `grep -rniE` por cada um dos termos `ac3`, `eac3`, `truehd`, `mlp`, `dts` e `dtshd` nos `.rs` de `libs/decode` devolve 0; `CodecId` em `crates/vynamp-decode-core/src/track.rs` so enumera `Pcm`, `Flac`, `Mp1`, `Mp2` e `Mp3`; nao ha crate de MP4 nem de Matroska, e um `grep -rniE` por `ftyp`, `moov`, `matroska` e `ebml` devolve 0; a feature `encumbered` nao aparece em nenhum `Cargo.toml` de `libs/decode`, 0 ocorrencias contra as de `LEGAL.md` e dos documentos de projeto; um `grep -rniE` por `drc_control`, `neutral_path` e `intrinsic_gain` devolve 0 e um `grep -rniE` por `render`, `drc`, `loudness` e `lufs` devolve 0; a suite atual do repo e `cargo test --workspace --locked` com 508 verdes, 3 ignorados e 0 falhas em 38 binarios de teste, e `tests/fixtures/` tem 152 arquivos, nenhum cinematografico. Com o FFmpeg `n8.1.3-23-g5c4f5744cd` usado na medição (a cópia ficava na pasta apagada; hoje o oráculo de dev é `$env:FFMPEG`, ver `engineering/08-reinicio-windows.md` do umbrella) da para gerar e decodificar por round-trip AC-3 5.1 a 384 kb/s (15 360 bytes em 0,32 s a 48 kHz), E-AC-3 (17 920 bytes), DTS Core mono a 1 411 kb/s (54 636 bytes em 0,3 s) e TrueHD 5.1 dentro de Matroska (17 265 bytes em 0,3 s, com PCM s32le cru de 345 600 bytes, ou 345 702 bytes em WAV por causa do cabecalho), e o TrueHD e o MLP elementares so voltam com o demuxer declarado: `-f truehd` e `-f mlp` leem o arquivo inteiro sem erro, enquanto a deteccao automatica de um `.mlp` cru falha com `Invalid data found when processing input`, o que mostra que o corpus precisa de container ou de dica de formato e de referencia propria |
| Hardware | ausente: a prova de passthrough e de saída codificada exige um receiver ou uma rota de saída que aceite AC-3, E-AC-3, TrueHD ou DTS codificado, e um dispositivo de áudio que aceite passthrough, além do codec Dolby/DTS para os casos com metadata Atmos e DTS:X — nada disso existe aqui, este ambiente é Windows 11 x64 com apenas o áudio integrado Intel e o Conexant ISST Audio (`Get-PnpDevice -Class AudioEndpoint` devolve microfone interno e alto-falante), sem placa de áudio profissional, sem receiver, sem aparelho Android, sem aparelho iOS e sem máquina macOS. O que roda sem hardware é o decode, o downmix e a medição de incompatibilidade de DSP, sempre no host e com o caminho de saída codificada marcado como recusa registrada | ausência é limitação registrada, não aprovação |

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
