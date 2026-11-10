# TASK-E17-05 — Certificar lossless, híbridos e PCM especiais

> Gerado pelo passo 37 a partir de `tasks/backlog.json`. Os campos de estado vêm do
> backlog e não devem ser editados aqui. A coluna `Then` é o requisito aprovado,
> copiado literal do backlog; `Given` e `When` foram autorados na execução.

| Campo | Valor |
|---|---|
| ID | `TASK-E17-05` |
| Módulo dono | `vynamp-decode` |
| Repo | `jimmykedlyn/vynamp-decode` |
| REQ coberto | `REQ-E17-05` |
| Critérios da spec | pendente — `specs/decode.md` §5 |
| Fase | M3 |
| Prioridade | P0 |
| Estado | planned |
| Depende de | `TASK-E01-08` |
| Bloqueia | `TASK-E17-20` |
| Modelo executor | Opus — áudio/RT, concorrência, segurança ou release |
| Esforço | 2–5 dias úteis |
| Risco | alto |

## Entrega

Corpus WavPack/WVC/float, APE/TTA/MPC/WMA e PCM de containers adicionais.

## Boundary

- **Não** implementa os decodificadores de WMA, APE, WavPack, TTA e Musepack. Os adapters são `TASK-E01-08`; esta TASK é dona do corpus, das fixtures e do selo. Escrever o decoder de APE aqui faria a certificação virar implementação sem revisão separada.
- **Não** cobre WavPack DSD, DFF DST nem native/DoP. `TASK-E17-18` certifica DSD especial e WavPack DSD, e `TASK-E01-09` faz DSF/DFF para PCM; o DSD do WavPack é entrada M5 opcional e não herda o selo lossless desta TASK.
- **Não** decide se o APE fica em default ou em codec reverso. A revisão de licença é `TASK-E11-03`, e LEGAL.md condiciona o APE a ela; certificar áudio aqui não autoriza distribuir o decoder.
- **Não** garante que .wvc, cue, capítulos e ratings não se percam em rename, nem a matriz de tags por container. Isso é `TASK-E17-06`; aqui o .wvc só é validado como correção de um par, não como arquivo com metadados preservados.
- **Não** implementa o caminho de large file nos containers novos nem decide o limite de tamanho. Oferecer offsets64 é `TASK-E01-08` (AC-1); esta TASK verifica o u64 e o ds64 que já existem em riff e w64 e o que os adapters novos precisam para passar.
- **Não** publica o selo lossless no manifest por artefato. `TASK-E17-12` gera o manifest com backend, features e hashes; o veredito lossless daqui é insumo dele.

## Antes de começar

- [ ] Spec do módulo aprovada e o critério de aceite citado já existe nela
- [ ] Worktree própria criada (`cargo xtask repo worktree task-e17-05-claude-<slug>`)
- [ ] Fixtures/hardware necessários disponíveis ou limitação registrada
- [ ] Nenhuma TASK em execução tocando os mesmos arquivos

## Critérios de aceite

| ID | Given | When | Then |
|---|---|---|---|
| `TASK-E17-05-AC-1` | um WavPack inteiro s32 de 4800 frames e um WavPack float fltp dos mesmos 4800 frames, ambos gerados de tools/fixtures/source-sine.wav, mais um TTA de 4800 frames com 24 bits por amostra e um ALAC em M4A, todos com SHA256 em MANIFEST.txt | cada arquivo é decodificado até o EOF e o PCM resultante é comparado amostra por amostra com o WAV de origem estreitado na profundidade do fixture, com tolerância zero | Lossless inteiro sample-exact |
| `TASK-E17-05-AC-2` | um WavPack híbrido sem o sidecar .wvc, o mesmo par com .wvc válido, o mesmo par com .wvc cujo hash não casa e com o nome em caixa diferente, e o .wvc aberto sozinho como se fosse mídia | a capability e o selo lossless são resolvidos para os quatro casos e o decode é tentado em cada um, registrando o motivo quando recusa | WV sem correção não recebe selo lossless |
| `TASK-E17-05-AC-3` | um corpus com APE em níveis de compressão distintos e nas versões 3.99 e 4.x, MPC SV7 e SV8, WMA v1, v2 e Pro, AIFF-C sowt e owt com valid bits declarados diferentes da largura do container, e um RF64 e um Wave64 com ds64 u64 acima de 4 GiB | o corpus roda por versão, nível, endianness e valid bits e o arquivo gigante roda atrás de #[ignore] com a variável de ambiente documentada, registrando o esperado por linha | níveis/versões/endianness/validbits/u64 são verificados. |

## Verificação

| Item | Comando / fixture | Evidência esperada |
|---|---|---|
| Teste automatizado | `cargo test` no repo `vynamp-decode` | verde no CI Gate |
| Oráculo independente | tests/fixtures/wv, tests/fixtures/tta, tests/fixtures/ape, tests/fixtures/mpc e tests/fixtures/asf com SHA256 em MANIFEST.txt, gerados com o FFmpeg fixado onde ele tem encoder (wavpack e tta existem e respondem em `ffmpeg -encoders`) e importados com LICENSE e SOURCE onde não tem (APE e Musepack não têm encoder nesse FFmpeg e não há CLI wavpack nesta máquina), mais um teste de comparação amostra a amostra contra tools/fixtures/source-sine.wav no molde de crates/vynamp-decode/tests/oracle_flac.rs. Hoje crates/ tem 9 crates e nenhuma cobre WavPack, APE, TTA, Musepack ou ASF | cada fixture lossless tem de decodificar idêntico ao fonte: o WavPack inteiro que o FFmpeg fixado grava é s32p com 4800 frames a 48 kHz, o float é fltp com os mesmos 4800 frames e o TTA reporta 24 bits por amostra e 4800 frames, então a evidência é igualdade de amostra e não tolerância. O lado híbrido não tem como ser produzido aqui: `ffmpeg -muxers` filtrado por wvc dá 0 e não existe CLI wavpack, então o par .wv com .wvc vem de fora com SHA, e `grep -rniE` por wvc e sidecar em crates/*/src dá 0 linhas hoje. A parte u64 já existe para RIFF (ds64 em crates/vynamp-decode-riff/src/wav.rs:24-95) e Wave64 (u64 em w64.rs:47) e não existe para nenhum container novo; endianness tem um precedente em AIFF-C sowt (crates/vynamp-decode-aiff/src/comm.rs:98) |
| Hardware | nenhum: a prova é igualdade numérica de amostras e leitura de campos de container sobre arquivos, com o arquivo gigante de AC-3 atrás de #[ignore] e variável de ambiente; nenhum dispositivo de áudio é aberto | ausência é limitação registrada, não aprovação |

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
