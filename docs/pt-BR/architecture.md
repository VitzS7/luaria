# Arquitetura

English: [architecture.md](../architecture.md)

Visão geral de como um script sai do editor e roda dentro do cliente. Detalhes de cada passo estão nos comentários de módulo dos arquivos citados em `crates/luaria/src/`.

## Pipeline

1. **Compilação.** `luau.rs` e `engine.rs` chamam o `luau-compile` da release 735 do Luau (`tools/luau.sh`). O bytecode tem que ser da mesma release da VM do cliente: o enum de opcodes muda entre releases, e um compilador de outra versão produz instruções que o cliente executa errado.
2. **Mapa de opcodes.** `opcodes.rs` lê a tabela de despacho do interpretador já carregado no processo e monta o mapa de opcodes **deste build**. O mapa é por nome, não por número, então um payload novo muda o mapa sem quebrar a tradução.
3. **Remapeamento.** O undump do cliente troca o byte de opcode de cada instrução. O motor reescreve o código de cada proto da árvore do chunk com o bytecode já traduzido.
4. **Âncoras e conexão.** `scan.rs`, `xref.rs`, `elf.rs`, `eh_frame.rs`, `plt.rs` e `vm.rs` localizam no payload ativo as funções de que o motor precisa (carga do bytecode, chamada de função, criação de string). `conexao.rs` confere os prólogos desses pontos antes de usá-los: se o payload mudou, o motor falha alto com mensagem. `estado_jogo.rs` decide quando reconectar, por exemplo quando o jogo troca de processo.
5. **Injeção.** `process.rs` e `remote.rs` usam `ptrace` para parar as threads e fazer chamadas remotas. `hook.rs` instala um stub x86-64 escrito à mão no ponto de chamada de `luaD_call`. O stub captura o `lua_State` de uma thread de script; threads do menu (LuaApp) e a `Main` são recusadas. `sonda.rs` lê `game.PlaceId` para distinguir jogo de menu, sem chamar função do jogo.
6. **Carga e chamada.** O chunk passa pelo `luau_load` real do cliente, a closure é chamada por `luaD_call`, e depois o motor devolve `L->top` ao valor anterior. Sem isso o cliente morre após algumas execuções. Folga de pilha é checada antes de cada chamada (`checar_folga` em `estado_jogo.rs`).

## Prelude e API do executor

- `assets/prelude.lua` define a API (`identifyexecutor`, `writefile`, `crypt`, `Drawing`, `debug.*`...) dentro de `(function() ... end)()`, para que os locais não vazem para o script do usuário. `prelude.rs` exporta cada nome e gera, por execução, a ponte que liga `getgenv()` aos globais reais do script (`ponte_globais`).
- `assets/loadstring.lua` é um interpretador de Luau escrito em Luau. Serve a `loadstring` com string dinâmica, porque o cliente não tem compilador em tempo de execução.
- `remoto.rs` resolve no host, antes de compilar, `game:HttpGet("url")` e `loadstring` com literal: o host baixa o texto e inlina a fonte. URL em variável não passa por aqui.
- `arquivos.rs` cuida de `writefile`/`readfile` sem código nativo: o host manda um instantâneo da pasta no início do chunk, e o jogo devolve as escritas por linhas de log que a UI aplica no disco.

## Nativas

Algumas funções precisam chamar código do próprio cliente (`getrawmetatable`, `setreadonly`, `getgc`, `getupvalue`...). `nativas.rs` grava essas funções x86-64 numa página do payload e preenche um slot por função com o endereço que o motor descobriu. Só funções que leem memória ou chamam o próprio cliente entram aqui; o que precisa de hook em runtime ainda não existe (ver `limitations.md`). `LUARIA_NATIVAS=0` desliga o bloco.

## Ponte host→jogo

O jogo não consegue esperar uma resposta do host durante `luaD_call` sem reentrar no interpretador. Por isso o canal é um anel em memória compartilhada (`ponte_layout.rs`): o jogo escreve um pedido num slot e espera o host marcar a resposta. `ponte.rs` é o poller do host; `ponte_codec.rs` codifica campos como `<len>:<bytes>`; `ponte_nativas.rs` tem as duas funções que o jogo chama (`enviar` e `receber`).

Quem atende o anel é a UI ou `luaria-cli exec --ficar`. Sem nenhum dos dois, `request` falha com `host not responding (open the UI)`. O host faz o `curl` com a mesma checagem anti-SSRF do `HttpGet` (IP privado recusado, IP fixado com `--resolve`).

## UI

- `bin/ui_web.rs`: janela GTK4 com WebKitGTK 6.0. Uma thread própria roda o motor; a janela nunca espera por ele.
- `bin/ui_web/servidor.rs`: servidor HTTP em `127.0.0.1` com porta livre. Só aceita `Host` e `Origin` da própria UI, para que uma página aberta no navegador não consiga mandar `POST /api/exec`.
- `bin/ui_web/estado.rs`: estado do motor e acompanhamento do log do cliente.
- `bin/ui_web/config.rs`, `pastas.rs`, `clientes_api.rs`: configuração, árvore de arquivos (todo caminho passa por `resolver_caminho`, sem `..`, `\` nem barra inicial) e lista de clientes.
- `clientes.rs`: lista os processos do cliente via `/proc`, sem `ptrace` e sem anexar.
- `assets/ui/`: HTML, CSS e JS. O JS é modular em `src/`; `app.js` é o bundle do CodeMirror gerado por esbuild e versionado.

## Estrutura de pastas

```
crates/luaria/src/
  engine.rs              motor: conexão, carga, patch, chamada
  opcodes.rs luau.rs     bytecode e tradução de opcodes
  scan.rs xref.rs elf.rs eh_frame.rs plt.rs vm.rs   âncoras no payload
  conexao.rs estado_jogo.rs sonda.rs                conexão e reconexão
  process.rs remote.rs hook.rs nativas.rs           ptrace, stub, nativas
  ponte*.rs                                         canal host<->jogo
  prelude.rs remoto.rs arquivos.rs                  API do executor
  clientes.rs                                       lista de clientes
  bin/cli.rs                                        luaria-cli
  bin/ui_web.rs bin/ui_web/                         UI webview
  bin/ui.rs bin/ui/                                 UI GTK4 pura (fallback)
assets/                  prelude.lua, loadstring.lua, ui/
scripts/                 testes Lua e teste da UNC
tools/                   build, instalação e testes de integração
docs/                    esta pasta
```
