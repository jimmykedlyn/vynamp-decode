# TASK-E01-07 — AAC-LC + HE-AAC (não anunciado como AAC-LC)

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
| Depende de | `TASK-E01-04`, `TASK-E01-11`, `TASK-E01-12`, `TASK-E01-13`, `TASK-E01-14` |
| Bloqueia | `TASK-E01-08`, `TASK-E03-03`, `TASK-E05-02`, `TASK-E05-06`, `TASK-E08-02`, `TASK-E17-02`, `TASK-E17-04`, `TASK-E18-01`, `TASK-E18-02` |
| Modelo executor | Opus — áudio/RT, concorrência, segurança ou release |
| Esforço | 5–12 dias úteis |

## Objetivo

Adapter AAC-LC/HE-AAC conforme prova/licença; HE-AAC não é anunciado como AAC-LC.

## Requisitos cobertos

| Req | Resultado | Restrições |
|---|---|---|
| REQ-E01-07 | Suportar AAC-LC com decoder de software; HE-AAC v1 **não** pode ser anunciado como AAC-LC; decidir backend FFmpeg nativo vs. symlink para libavcodec; não aceitar DRM. | ffmpeg ou caps negociados por capabilities; HE-AAC deve ser reportado como codec separado (não é AAC-LC) |

## Plano técnico

- Passos numerados, arquivos novos/modificados e serviços que sobem.
- **Edge cases**: HE-AAC v1 com 960 ou 1024 samples/frame, AAC-LD/ELD (não suportado), DRM.
- **Estratégia de teste**: unit tests + fixtures sintéticas; cross-check com ffmpeg.
- **Riscos**: dependência de feature FFmpeg; licença; HE-AAC não anunciado como AAC-LC.

## Critérios de aceite da spec (pendente)

Nenhum `DECODE-AC-*` foi autorado para esta TASK.

| ID | Given (contexto) | When (ação) | Then (resultado observável) |
|---|---|---|---|
| `TASK-E01-07-AC-1` | Fixture com AAC-LC de perfil compatível | Decode até o fim em `Mode::Strict` | PCM decodificado com SNR ≥ 90 dB contra referência; HE-AAC não é anunciado por decode AAC-LC. |
| `TASK-E01-07-AC-2` | `default_registry()` consultado sobre AAC-LC e HE-AAC v1 | `probe`, `codec_for`, `codecs()` | HE-AAC v1 tem codec separado (não é AAC-LC). |

## Verificação

Comandos, hardware, fixtures e como observou cada critério. "Testes passaram" não é evidência.

| Comando / evidência | Resultado |
|---|---|
| `cargo test -p vynamp-decode` | pendente |
| `cargo clippy --all-targets -- -D warnings` | pendente |

##兼容性 / Riscos

Nenhum declarado.

## Dependências

- Depende de: `TASK-E01-04`, `TASK-E01-11`, `TASK-E01-12`, `TASK-E01-13`, `TASK-E01-14`
- Bloqueia: `TASK-E01-08`, `TASK-E03-03`, `TASK-E05-02`, `TASK-E05-06`, `TASK-E08-02`, `TASK-E17-02`, `TASK-E17-04`, `TASK-E18-01`, `TASK-E18-02`

## Definition of Done

- [ ] Objetivo implementado.
- [ ] Testes e validações com evidência.
- [ ] Edge cases tratados ou explicitamente fora de escopo.
- [ ] Riscos documentados e sem surpresas.
- [ ] PR revisado e mergeado.

## Plano de teste (TDD)

| ID | Given | When | Then |
|---|---|---|---|
| `TASK-E01-07-AT-1` | Fixture AAC-LC válida | Decode até o fim | PCM decodificado |
| `TASK-E01-07-AT-2` | Registry com codecs | `probe`, `codec_for`, `codecs()` | HE-AAC reportado separado de AAC-LC |

## Riscos

Pendente.

## Critérios de aceite — rascunho (não conta como pronto)

Nenhum critério ainda. Se precisar, use rascunho com status `draft` e link para spec/issue; não copie para a matriz final.

## Plano de execução

1. Descreva o passo.
2. Use checklists para cada dia ou story.
3. Atualize `tasks/backlog.json` quando terminar (status, branch, PR).

## Dependências

- Depende de: `TASK-E01-04`, `TASK-E01-11`, `TASK-E01-12`, `TASK-E01-13`, `TASK-E01-14`
- Bloqueia: `TASK-E01-08`, `TASK-E03-03`, `TASK-E05-02`, `TASK-E05-06`, `TASK-E08-02`, `TASK-E17-02`, `TASK-E17-04`, `TASK-E18-01`, `TASK-E18-02`

## Definition of Done

- [ ] Objetivo implementado.
- [ ] Testes e validações com evidência.
- [ ] Edge cases tratados ou explicitamente fora de escopo.
- [ ] Riscos documentados e sem surpresas.
- [ ] PR revisado e mergeado.

## Plano de teste (TDD)

| ID | Given | When | Then |
|---|---|---|---|
| `TASK-E01-07-AT-1` | Fixture AAC-LC válida | Decode até o fim | PCM decodificado |
| `TASK-E01-07-AT-2` | Registry com codecs | `probe`, `codec_for`, `codecs()` | HE-AAC reportado separado de AAC-LC |

## Riscos

Pendente.

## Critérios de aceite — rascunho (não conta como pronto)

Nenhum critério ainda. Se precisar, use rascunho com status `draft` e link para spec/issue; não copie para a matriz final.

## Plano de execução

1. Descreva o passo.
2. Use checklists para cada dia ou story.
3. Atualize `tasks/backlog.json` quando terminar (status, branch, PR).