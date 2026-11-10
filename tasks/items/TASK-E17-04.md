# TASK-E17-04 — Avaliar e integrar xHE-AAC com capacidade honesta

> Gerado pelo passo 37 a partir de `tasks/backlog.json`. Os campos de estado vêm do
> backlog e não devem ser editados aqui. A coluna `Then` é o requisito aprovado,
> copiado literal do backlog; `Given` e `When` foram autorados na execução.

| Campo | Valor |
|---|---|
| ID | `TASK-E17-04` |
| Módulo dono | `vynamp-decode` |
| Repo | `jimmykedlyn/vynamp-decode` |
| REQ coberto | `REQ-E17-04` |
| Critérios da spec | pendente — `specs/decode.md` §5 |
| Fase | M3 |
| Prioridade | P0 |
| Estado | planned |
| Depende de | `TASK-E17-01`, `TASK-E01-07` |
| Bloqueia | `TASK-E17-20` |
| Modelo executor | Opus — áudio/RT, concorrência, segurança ou release |
| Esforço | 2–5 dias úteis |
| Risco | alto |

## Entrega

Adapter USAC/AOT42 quando viável, ou relatório de indisponibilidade por build/licença.

## Boundary

- **Não** implementa AAC-LC, HE-AAC, ADTS, LOAS/LATM nem ADIF. Os adapters de AAC e do framing dele são `TASK-E01-07`; AOT42 não é AAC-LC com extensão diferente e trazer o decoder de LC para cá é exatamente o atalho que o AC-2 proíbe.
- **Não** decide a política de licença nem a feature que agrupa os codecs encumbered. A revisão de licenças e o inventário de dependências são `TASK-E11-03`, e a política por perfil que já registra xHE como encumbered é `TASK-E17-01`; esta TASK aplica a decisão, não a reescreve.
- **Não** publica `available`, `licensed` e `state` no manifest por artefato. Isso é `TASK-E17-12`; aqui se produz o motivo unsupported auditado que aquele manifest cita.
- **Não** implementa AC-4, MPEG-H nem IAMF, que são NGA condicionais e parecem do mesmo pacote. `TASK-E17-13`, `TASK-E17-14` e `TASK-E17-15` são as donas, e todas dependem desta TASK-E17-03/E17-01 sem herdar o adapter.
- **Não** entrega AAC-LD, ELD, ELDv2 nem os codecs de fala. `TASK-E17-17` cobre AMR, 3GP, Speex, LD e ELD; são M5 opcionais e não entram no selo M3 desta TASK.
- **Não** define a aplicação única de ganho e DRC na cadeia. O contrato de ganho aplicado uma vez e caminho neutro é `TASK-E17-02`; sem ele, certificar DRC aqui certificaria só a metade do decoder.

## Antes de começar

- [ ] Spec do módulo aprovada e o critério de aceite citado já existe nela
- [ ] Worktree própria criada (`cargo xtask repo worktree task-e17-04-claude-<slug>`)
- [ ] Fixtures/hardware necessários disponíveis ou limitação registrada
- [ ] Nenhuma TASK em execução tocando os mesmos arquivos

## Critérios de aceite

| ID | Given | When | Then |
|---|---|---|---|
| `TASK-E17-04-AC-1` | um MP4 xHE-AAC com AOT42, 4800 frames a 48 kHz e DRC e loudness intrínsecos declarados no config, obtido de encoder licenciado com LICENSE e SOURCE no diretório do fixture, mais os cinco alvos windows-x64, linux-x64, macos-intel, macos-silicon e android-arm64 | o adapter é consultado e o fixture roda por alvo, medindo seek exato, trim de priming e se o DRC foi aplicado ou declarado inaplicável, com o motivo registrado por alvo | Fixtures DRC/seek/trim por alvo ou motivo unsupported auditado |
| `TASK-E17-04-AC-2` | um stream USAC cujo ASC declara audioObjectType 42 e sinaliza SBR e PS, um build sem nenhum backend xHE-AAC registrado e um segundo fixture AAC-LC para servir de controle | o registry resolve o codec do trilho e a capability é consultada antes de qualquer decode, e o resultado é comparado com o do controle AAC-LC | sem fallback LC nem declaração universal |
| `TASK-E17-04-AC-3` | o spike M0 desta avaliação com binário, versão, licença e resultado por alvo de cada backend candidato, e a feature encumbered que LEGAL.md:10 descreve e que o Cargo.toml do facade não declara (as features de lá são riff, aiff, pcm, flac, mpa e mp3) | o registro do spike é conferido contra o que o build realmente entrega e contra o estado da entrada aac-usac do catálogo, que é M3 conditional e planned | spike M0 registrado. |

## Verificação

| Item | Comando / fixture | Evidência esperada |
|---|---|---|
| Teste automatizado | `cargo test` no repo `vynamp-decode` | verde no CI Gate |
| Oráculo independente | um teste em crates/vynamp-decode/tests/xhe.rs sobre tests/fixtures/xhe/ com um MP4 AOT42 e SHA256, mais `cargo run -p vynamp-decode --example support_matrix` e a conferência da capability contra schemas/codec-capability.schema.json. O fixture não pode ser gerado nesta máquina: o FFmpeg fixado n8.1.3-23-g5c4f5744cd não tem decoder USAC nem xHE-AAC (`ffmpeg -decoders` filtrado por usac e xhe devolve 0), então ele vem de fonte externa licenciada | por alvo, o resultado esperado é ou decode com drc_control controllable e neutral_path declarado, ou erro tipado com motivo unsupported e o alvo nomeado; nunca available true sem backend, e o AOT42 não pode resolver para aac-lc nem para aac-he. O que ainda não existe: `grep -rniE` por usac, xhe e aot42 em crates/ dá 0 linhas; drc_control, neutral_path e intrinsic_gain não aparecem em nenhum .rs (0 ocorrências) embora o schema os exija e derrube o JSON sem eles; e a feature encumbered de LEGAL.md:10 não existe no Cargo.toml do facade |
| Hardware | nenhum: nenhum DAC é aberto, e o que falta é alvo de build e não dispositivo — este ambiente é Windows x64, sem máquina macOS, sem aparelho Android e sem iOS, então linux-x64, macos-intel, macos-silicon e android-arm64 não têm build aqui e os quatro entram no relatório de indisponibilidade por alvo | ausência é limitação registrada, não aprovação |

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
