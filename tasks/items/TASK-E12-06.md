# TASK-E12-06 — Entregar CD playback e ripping

> Gerado pelo passo 37 a partir de `tasks/backlog.json`. Os campos de estado vêm do
> backlog e não devem ser editados aqui. A coluna `Then` é o requisito aprovado,
> copiado literal do backlog; `Given` e `When` foram autorados na execução.

| Campo | Valor |
|---|---|
| ID | `TASK-E12-06` |
| Módulo dono | `vynamp-decode` |
| Repo | `jimmykedlyn/vynamp-decode` |
| REQ coberto | `REQ-E12-06` |
| Critérios da spec | pendente — `specs/decode.md` §5 |
| Fase | M5 |
| Prioridade | P2 |
| Estado | planned |
| Depende de | `TASK-E08-02`, `TASK-E03-08` |
| Bloqueia | nenhuma |
| Modelo executor | Opus — áudio/RT, concorrência, segurança ou release |
| Esforço | 10–25 dias úteis |
| Risco | alto |

## Entrega

Adapter de drive, secure mode/offset, metadados e relatório de checksums.

## Boundary

- **Não** encoda o PCM extraído. FLAC, WAV, Opus, Vorbis e MP3 são `TASK-E08-02`, dona do encoder registry; duplicar o registry aqui faria dois donos do mesmo mapeamento de parâmetros e de licença de encoder.
- **Não** grava as tags das faixas extraídas. Escrita em lote com preview atômico e backup é `TASK-E03-08`; aqui se entrega o metadado lido, não o arquivo reescrito.
- **Não** lê tags nem capas da biblioteca nem organiza as faixas em pastas. Leitura segura de tags é `TASK-E03-04` e organização com dedupe é `TASK-E08-04`.
- **Não** toca o CD. O adapter entrega PCM ao engine; callback, clock e dispositivo são `TASK-E01-02`, e um player dentro do decoder duplicaria a cadeia de saída.
- **Não** estende a extração para SACD ISO nem trata `.iso` de CD de dados como áudio. Extração legal de SACD é `TASK-E17-18`, e ISO genérico não é Red Book.

## Antes de começar

- [ ] Spec do módulo aprovada e o critério de aceite citado já existe nela
- [ ] Worktree própria criada (`cargo xtask repo worktree task-e12-06-claude-<slug>`)
- [ ] Fixtures/hardware necessários disponíveis ou limitação registrada
- [ ] Nenhuma TASK em execução tocando os mesmos arquivos

## Critérios de aceite

| ID | Given | When | Then |
|---|---|---|---|
| `TASK-E12-06-AC-1` | um CD-DA de 14 faixas cujo LBA 12345, dentro da faixa 3, tem dano não corrigível, e o adapter de teste que reproduz o mesmo setor a partir de uma imagem de setores de 2352 bytes | o rip completo roda em secure mode com 8 tentativas de releitura por setor e o relatório de leitura é emitido ao fim | Erros de leitura identificados |
| `TASK-E12-06-AC-2` | um rip em andamento na faixa 5 de 14, com arquivos temporários abertos, um job de encode de `TASK-E08-02` enfileirado e um `CancelSource` do `vynamp-decode-core` emitido antes do início | o token é cancelado com o rip em 37% e o laço de leitura chama `CancelToken::check` no próximo setor | cancelamento seguro |
| `TASK-E12-06-AC-3` | um CD cujo disc ID e offsets de faixa foram lidos do lead-in, com CRC32 calculado sobre o PCM de cada uma das 14 faixas, e dois cenários: nenhum provider de checksum externo configurado e um provider licenciado configurado | o rip termina nos dois cenários e o relatório de checksums é montado | rip verificado apenas com checksum real |
| `TASK-E12-06-AC-4` | o registro de providers com CD-Text lido do próprio disco, um provider de metadados com `license_id` e chave de API declarados e `metadata_opt_in` desligado no default, como pede `library/02-tags-letras.md` | o rip termina e o relatório registra origem, provider e licença de cada campo de metadado | licenças de providers atendidas. |

## Verificação

| Item | Comando / fixture | Evidência esperada |
|---|---|---|
| Teste automatizado | `cargo test` no repo `vynamp-decode` | verde no CI Gate |
| Oráculo independente | `cargo test -p vynamp-decode --features cdrom --test cd_rip --test cd_cancel --locked -- --nocapture` sobre `fixtures/cd/img14.iso` (imagem de setores com 14 faixas e buraco no LBA 12345) servida por um adapter de drive de teste, com o relatório impresso e comparado ao `MANIFEST.txt` de CRC32 por faixa | esperado: o relatório sai com 14 entradas, a faixa 3 marcada `ReadError { lba: 12345, retries: 8 }` e as outras 13 com CRC32 igual ao pinado; o cancelamento em 37% devolve `Error::Cancelled`, apaga os temporários (0 arquivos restantes) e deixa a fila sem item `running`; sem provider a faixa sai `unverified` e o texto `bit-perfect` não aparece em lugar nenhum; com opt-in desligado nenhuma requisição de metadado é emitida e o relatório traz `provider`, `license_id` e `origin`. **O que não existe hoje, medido em 2026-11-10:** os 9 crates do workspace não têm adapter de drive nem crate `cdrom`, `grep -rn -e cdda -e cdrom -e ripping -e scsi -e accuraterip crates/` devolve 0 linhas, e `Get-CimInstance Win32_CDROMDrive` não lista dispositivo nesta máquina. Existem `md5.rs` e `crc.rs` no `vynamp-decode-core` e `sha256` no `vynamp-decode-testkit`, mas nenhum caminho de CD os usa |
| Hardware | drive óptico com CD-DA Red Book e, para o secure mode físico, um drive que reporte C2: **ausente** neste ambiente — `Win32_CDROMDrive` não lista nenhum dispositivo e não há CD de áudio, então o caminho exercitável é a imagem de setores sob o adapter de teste | ausência é limitação registrada, não aprovação |

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
