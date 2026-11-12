# TASK-E17-13 — Expandir MPEG-H com renderer e licença

> Gerado pelo passo 37 a partir de `tasks/backlog.json`. Os campos de estado vêm do
> backlog e não devem ser editados aqui. A coluna `Then` é o requisito aprovado,
> copiado literal do backlog; `Given` e `When` foram autorados na execução.

| Campo | Valor |
|---|---|
| ID | `TASK-E17-13` |
| Módulo dono | `vynamp-decode` |
| Repo | `jimmykedlyn/vynamp-decode` |
| REQ coberto | `REQ-E17-13` |
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

MHAS/mha1/mhm1 com perfil/level, apresentações e renderer certificado.

## Boundary

- **Não** versiona o catálogo nem decide fase, política e viabilidade do MPEG-H: isso é `TASK-E17-01`, que fixa `config/format-catalog.json` rev 0.7.0 e o `schemas/codec-capability.schema.json`. A entrada `mpeg-h` está `state: planned` com `operations: [demux, decode, render]`; publicar essa linha como `available` aqui esvaziaria a autoridade do catálogo.
- **Não** resolve a licença de patente nem escolhe a fonte do backend MPEG-H: o inventário de codecs, fontes e políticas de build e distribuição é `TASK-E11-03`. Aqui a conclusão é que a licença de software do decodificador não concede patente, e a licença de patente aplicável por jurisdição é responsabilidade de quem distribui o build.
- **Não** implementa o downmix PCM básico nem o mapeamento de canais de saída: o mapeamento de canais e a conversão de DSD para PCM são `TASK-E01-09`. A confusão é plausível porque a Entrega fala em downmix e o codec entrega 6 canais; aqui o downmix do MPEG-H é o que o próprio stream declara, e o rotulado genérico de canais continua sendo trabalho de `TASK-E01-09`.
- **Não** entrega o demuxer de MP4 nem a seleção de faixa de áudio: os bindings de container e a seleção de faixa são `TASK-E17-03`. O MPEG-H mora em MP4 e em MHAS, e reescrever o parser ISOBMFF aqui daria dois donos do mesmo `ftyp` e `moov`.
- **Não** implementa a saída codificada, o passthrough ou o caminho HDMI do renderer: isso é `TASK-E17-16` para os codecs cinematográficos e `TASK-E17-19` para rota e headroom por dispositivo. Aqui a prova é sobre o PCM renderizado no host, sem dispositivo de saída.
- **Não** autentica AC-4, IAMF nem AC-3 junto com MPEG-H: os adapters condicionados e o renderer de apresentação são `TASK-E17-14` e `TASK-E17-15`, e os codecs de cinema são `TASK-E17-16`. A spec lista as três famílias na mesma frase de expansão M5, e o título diz só MPEG-H, então puxar AC-4 para cá criaria dois donos da mesma presentation e DRC.

## Antes de começar

- [ ] Spec do módulo aprovada e o critério de aceite citado já existe nela
- [ ] Worktree própria criada (`cargo xtask repo worktree task-e17-13-claude-<slug>`)
- [ ] Fixtures/hardware necessários disponíveis ou limitação registrada
- [ ] Nenhuma TASK em execução tocando os mesmos arquivos

## Critérios de aceite

| ID | Given | When | Then |
|---|---|---|---|
| `TASK-E17-13-AC-1` | um corpus MPEG-H com dois arquivos independentes: um `.mhas` elementar com uma apresentação de 2 canais e um `.mp4` com a faixa `mha1` e uma faixa `mhm1` com perfil e level declarados, cada um com o SHA-256 registrado em `tests/fixtures/mpeg-h/MANIFEST.txt` no mesmo formato de barra vertical de `tests/fixtures/mpeg/MANIFEST.txt`, mais uma pasta `tests/fixtures/mpeg-h/reference/` com o PCM de referência da apresentação de 2 canais renderizado pelo decodificador de referência do Fraunhofer, e o catálogo `config/format-catalog.json` rev 0.7.0 com a entrada `mpeg-h` em `state: planned`, `policy: conditional` e `operations: [demux, decode, render]` | o demuxer MHAS/MP4 é aberto e as apresentações são decodificadas até o fim em um teste, e o mesmo corpus é aberto em um segundo teste que só exercita o renderer de apresentação, com os dois resultados registrados separadamente em relatórios distintos de backend, versão e evidência | Decoder e renderer têm provas separadas |
| `TASK-E17-13-AC-2` | o mesmo corpus MPEG-H com a apresentação de 2 canais e uma apresentação de 6 canais com áudio de objeto, mais uma matriz de downmix para 2 canais declarada no próprio stream e um `Options` com `Limits::default()` (teto de 32 canais, 768 000 Hz e 64 GiB) | a apresentação de 6 canais é renderizada, a matriz de downmix é aplicada sobre o PCM e o resultado é comparado com o arquivo de referência da apresentação, e em seguida a mesma apresentação é submetida a um build onde a feature do renderer está desligada para que a ausência seja registrada como `unavailable` explícita e não como silêncio | objetos/downmix e saída testados ou unavailable explícito |
| `TASK-E17-13-AC-3` | o diretório `docs/sources/` do repo `vynamp-decode` com a ficha da fonte `https://github.com/Fraunhofer-IIS/mpeghdec`, que ainda não existe e será escrita no molde das quatro fichas atuais do diretório (`aiff.md`, `flac.md`, `mpeg-audio.md` e `riff.md`), e com a URL já referenciada em `sources` de `config/format-catalog.json` e o texto de `LEGAL.md` que classifica MPEG-H na feature `encumbered` desligada por padrão | a varredura do repositório procura, no código, nos textos de licença, no `README.md` e nos arquivos de fonte, afirmação de que a licença do software cobre as patentes do MPEG-H, e o resultado da varredura é anexado ao relatório de licença por alvo com a conclusão explícita de que a licença de software não implica licença de patente | licença de software não implica patentes. |

## Verificação

| Item | Comando / fixture | Evidência esperada |
|---|---|---|
| Teste automatizado | `cargo test` no repo `vynamp-decode` | verde no CI Gate |
| Oráculo independente | um teste de integração novo em `libs/decode/crates/vynamp-decode/tests/mpeg_h.rs`, rodado por `cargo test -p vynamp-decode --features mpegh --test mpeg_h`, sobre um corpus novo em `tests/fixtures/mpeg-h/` gerado por um `tools/fixtures/generate-mpegh-dev.ps1` no molde de `tools/fixtures/generate-mpeg-dev.ps1`, mais o gate `cargo xtask check-docs` do umbrella, que valida `config/format-catalog.json` contra `schemas/format-catalog.schema.json` e roda `catalog::validate` | esperado: o relatorio de decoder e o relatorio de renderer ficam em arquivos distintos, cada um com backend, versao, hash da fixture e evidencia; a apresentacao de 2 canais decodificada bate com o PCM de referencia dentro da tolerancia declarada e a apresentacao de 6 canais renderizada bate com a referencia de 6 canais; o downmix para 2 canais fica perto da referencia dentro de tolerancia explicita por apresentacao; com a feature do renderer desligada o mesmo arquivo devolve `unavailable` com motivo, e nao PCM de 6 canais nem silencio; a varredura de patentes nao encontra nenhuma afirmacao de que a licenca de software cobre patente. O que ainda nao existe hoje: `libs/decode` tem 106 arquivos `.rs` em 9 crates e um `grep -rniE` por cada um dos termos `mpegh`, `mhas`, `mha1` e `mhm1` nos `.rs` devolve 0 linhas; `CodecId` em `crates/vynamp-decode-core/src/track.rs` so enumera `Pcm`, `Flac`, `Mp1`, `Mp2` e `Mp3`, cinco variantes; nao existe crate de MP4 nem de MHAS, e um `grep -rniE` por `ftyp`, `moov`, `matroska` e `ebml` nos `.rs` devolve 0; `traits.rs` declara so `Demuxer` e `Decoder`, sem operacao de render; um `grep -rniE` por `render`, `drc`, `loudness`, `lufs` e `presentation` nos `.rs` devolve 1 linha, o comentario `Presentation timestamp in frames.` em `crates/vynamp-decode-core/src/packet.rs:8`, sem relacao com render; um `grep -rniE` por `drc_control`, `neutral_path` e `intrinsic_gain` devolve 0; e `tests/fixtures/` tem 152 arquivos em flac, flac-conformance, mpeg e pcm, nenhum MPEG-H; a feature `mpegh` tambem nao existe no `Cargo.toml` da fachada, de modo que `cargo test -p vynamp-decode --features mpegh` hoje para com `the package 'vynamp-decode' does not contain this feature`. O FFmpeg `n8.1.3-23-g5c4f5744cd` usado na medição (a cópia ficava na pasta apagada; hoje o oráculo de dev é `$env:FFMPEG`, ver `engineering/08-reinicio-windows.md` do umbrella) nao tem decoder `mpegh` nem muxer MHAS, entao o corpus MPEG-H tem de vir de fonte externa licenciada, com SHA-256 e licenca por fixture, e o script de geracao falha alto quando o arquivo falta |
| Hardware | nenhum: a prova é sobre PCM decodificado e renderizado no host a partir de `SliceSource` e `FileSource`, sem rota de saída e sem device de áudio aberto; o que falta aqui é corpus licenciado com referência de renderer, não placa. Esta máquina é Windows 11 x64 com apenas o áudio integrado Intel e o Conexant ISST Audio (`Get-PnpDevice -Class AudioEndpoint` devolve microfone interno e alto-falante), sem placa de áudio profissional, sem aparelho Android, sem aparelho iOS e sem máquina macOS | ausência é limitação registrada, não aprovação |

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
