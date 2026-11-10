# TASK-E12-05 — Entregar trackers, MIDI e codecs raros

> Gerado pelo passo 37 a partir de `tasks/backlog.json`. Os campos de estado vêm do
> backlog e não devem ser editados aqui. A coluna `Then` é o requisito aprovado,
> copiado literal do backlog; `Given` e `When` foram autorados na execução.

| Campo | Valor |
|---|---|
| ID | `TASK-E12-05` |
| Módulo dono | `vynamp-decode` |
| Repo | `jimmykedlyn/vynamp-decode` |
| REQ coberto | `REQ-E12-05` |
| Critérios da spec | pendente — `specs/decode.md` §5 |
| Fase | M5 |
| Prioridade | P2 |
| Estado | planned |
| Depende de | `TASK-E01-08`, `TASK-E07-05` |
| Bloqueia | `TASK-E17-17`, `TASK-E17-18` |
| Modelo executor | Opus — áudio/RT, concorrência, segurança ou release |
| Esforço | 10–25 dias úteis |
| Risco | alto |

## Entrega

Adapters libopenmpt, synth com SF2 do usuário e codecs opcionais testados.

## Boundary

- **Não** escreve as fixtures de certificação de subsongs, loops e SF2. Elas são `TASK-E17-18`, que existe para certificar a expansão depois das implementações E12; trazer o corpus para cá faria duas TASKs donas do mesmo critério.
- **Não** cobre AMR, 3GP, Speex, AAC-LD/ELD nem WMA Voice. Legados e fala são `TASK-E17-17`, e a confusão é plausível porque "codecs raros" está no título desta TASK.
- **Não** implementa o sandbox, o RPC autenticado nem a memória compartilhada do helper nativo. Isso é `TASK-E07-05`; aqui o adapter apenas consome um helper que já existe isolado.
- **Não** entrega WMA, APE, WV, TTA, MPC, MP1/MP2, WebM, MKA nem RF64. São `TASK-E01-08`, e o AC-4 desta TASK fala de isolation de nativo, não de codecs desktop.
- **Não** encoda nem grava o render em arquivo. O render devolve PCM ao decodificador; exportar módulo ou MIDI para WAV/FLAC é `TASK-E08-02`, dona do encoder registry.
- **Não** extrai áudio de SACD ISO nem converte DSD para PCM. A extração legal sem proteção é `TASK-E17-18` e o decimator DSF/DFF é `TASK-E01-09`; a fase M5 comum é o que induz o erro.

## Antes de começar

- [ ] Spec do módulo aprovada e o critério de aceite citado já existe nela
- [ ] Worktree própria criada (`cargo xtask repo worktree task-e12-05-claude-<slug>`)
- [ ] Fixtures/hardware necessários disponíveis ou limitação registrada
- [ ] Nenhuma TASK em execução tocando os mesmos arquivos

## Critérios de aceite

| ID | Given | When | Then |
|---|---|---|---|
| `TASK-E12-05-AC-1` | as 11 entradas de perfil do catálogo — `tracker` com MOD, XM, IT, S3M, MO3, MTM e UMX, e `midi` com MIDI, RMI, XMF e KAR — todas `state: planned` e com `sources: []` em `config/format-catalog.json`, mais o inventário de licenças de `TASK-E11-03` | cada perfil é aberto pelo build que o implementa — adapter libopenmpt para tracker e synth com SF2 para MIDI — e a matriz é emitida com backend, versão, licença e nível de verificação | Matriz por perfil e licença |
| `TASK-E12-05-AC-2` | um `.it` cuja lista de ordens volta ao padrão 0 por comando `Bxx` sem fim, um `.mod` com loop de padrão e um `.mid` com o ponto de loop do instrumento do SF2 habilitado, com o limite de loops fixado em 1 na configuração do adapter | o render puxa amostras até o fim e o contador de loops do adapter é lido no relatório da sessão | loops limitados |
| `TASK-E12-05-AC-3` | um `.mid` de 3 minutos e um SF2 fornecido pelo usuário, de licença declarada no inventário de `TASK-E11-03` e sha256 pinado no `MANIFEST.txt` (a fixture ainda não existe no repo), num repo sem nenhum SoundFont versionado e sem banco embutido: `grep -rn -e soundfont -e sf2 crates/` devolve 0 linhas, medido em 2026-11-10 | o `.mid` é aberto primeiro sem SF2 declarado e depois com o caminho e o sha256 do SF2 passados nas opções do adapter | fonte SF2 explicitada |
| `TASK-E12-05-AC-4` | o helper nativo de `TASK-E07-05` rodando fora do processo do decoder, com RPC autenticado e memória compartilhada limitada, e um `.xm` cujo sample corrompido derruba o helper | o helper aborta no meio do render e o adapter recebe o encerramento pelo canal de RPC | helpers nativos isolados |
| `TASK-E12-05-AC-5` | um build com as features default (`riff`, `aiff`, `pcm`, `flac`, `mpa`, `mp3`, sem `tracker` e sem `midi`) e três arquivos sem backend — `.mo3` (módulo comprimido cujo conteúdo interno é MP3 ou Ogg), `.umx` e `.xmf` | `open_path` é chamado nos três e o `support_matrix` é impresso no mesmo build | sem suporte fictício. |

## Verificação

| Item | Comando / fixture | Evidência esperada |
|---|---|---|
| Teste automatizado | `cargo test` no repo `vynamp-decode` | verde no CI Gate |
| Oráculo independente | `cargo run --example support_matrix` mais `cargo test -p vynamp-decode --features tracker,midi --test trackers --test midi --locked` sobre `fixtures/tracker/` (`loops.it`, `pattern-loop.mod`, `subsong.xm`) e `fixtures/midi/` (`loop.mid`, `tone.kar`) com `MANIFEST.txt` de sha256, e `cargo test -p vynamp-decode --features midi --test sf2_source` com o SF2 de teste pinado | esperado: a matriz sai com 11 linhas de perfil — 7 de tracker e 4 de MIDI — cada uma com backend, versão, licença e nível de verificação; o `loops.it` para em exatamente 1 volta e o total de frames bate com o da lista de ordens pinada; abrir `.mid` sem SF2 devolve `Unsupported("no soundfont")` e nunca silêncio, e com SF2 a sessão expõe caminho e sha256 do arquivo; `.mo3`, `.umx` e `.xmf` no build default devolvem `Unsupported`, e o `support_matrix` não ganha linha para eles. **O que não existe hoje, medido em 2026-11-10:** os 9 crates do workspace são `vynamp-decode`, `-aiff`, `-core`, `-flac`, `-mp3`, `-mpa`, `-pcm`, `-riff` e `-testkit`; não há crate de tracker nem de synth, `grep -rn -e openmpt -e soundfont -e midi -e tracker crates/` devolve 0 linhas, `fixtures/` tem só `flac`, `flac-conformance`, `mpeg` e `pcm`, e o `support_matrix` imprime 4 containers e 4 codecs. A suíte mede 508 verdes e 3 ignorados, nenhum sobre tracker ou MIDI, então nenhum desses testes roda antes de o código existir |
| Hardware | nenhum: o synth e o adapter renderizam em memória e o helper roda em processo separado no próprio host; nenhum teste deste escopo abre dispositivo de áudio, placa ou DAC, e os insumos são módulos, `.mid` e SF2 versionados | ausência é limitação registrada, não aprovação |

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
