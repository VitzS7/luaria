# Architecture

Português (pt-BR): [architecture.md](pt-BR/architecture.md)

High-level view of how a script goes from the editor to running inside the client. Details for each step are in the module comments of the files under `crates/luaria/src/`.

## Pipeline

1. **Compilation.** `luau.rs` and `engine.rs` call the `luau-compile` from Luau release 735 (`tools/luau.sh`). The bytecode must come from the same release as the client's VM: the opcode enum changes between releases, and a compiler from another version produces instructions the client executes wrongly.
2. **Opcode map.** `opcodes.rs` reads the dispatch table of the interpreter already loaded in the process and builds the opcode map for **this build**. The map is keyed by name, not by number, so a new payload changes the map without breaking the translation.
3. **Remapping.** The client's undump swaps the opcode byte of each instruction. The engine rewrites the code of every proto in the chunk tree with the already-translated bytecode.
4. **Anchors and connection.** `scan.rs`, `xref.rs`, `elf.rs`, `eh_frame.rs`, `plt.rs` and `vm.rs` locate, in the active payload, the functions the engine needs (bytecode loading, function call, string creation). `conexao.rs` checks the prologues of those points before using them: if the payload changed, the engine fails loudly with a message. `estado_jogo.rs` decides when to reconnect, for example when the game switches process.
5. **Injection.** `process.rs` and `remote.rs` use `ptrace` to stop threads and make remote calls. `hook.rs` installs a hand-written x86-64 stub at the call site of `luaD_call`. The stub captures the `lua_State` of a script thread; threads from the menu (LuaApp) and the `Main` thread are refused. `sonda.rs` reads `game.PlaceId` to tell a game from the menu, without calling any game function.
6. **Loading and calling.** The chunk goes through the client's real `luau_load`, the closure is called through `luaD_call`, and afterwards the engine restores `L->top` to its previous value. Without this the client dies after a few executions. Stack headroom is checked before every call (`checar_folga` in `estado_jogo.rs`).

## Prelude and executor API

- `assets/prelude.lua` defines the API (`identifyexecutor`, `writefile`, `crypt`, `Drawing`, `debug.*`...) inside `(function() ... end)()`, so its locals do not leak into the user's script. `prelude.rs` exports each name and generates, per execution, the bridge that connects `getgenv()` to the script's real globals (`ponte_globais`).
- `assets/loadstring.lua` is a Luau interpreter written in Luau. It serves `loadstring` with a dynamic string, because the client has no compiler at runtime.
- `remoto.rs` resolves on the host, before compiling, `game:HttpGet("url")` and `loadstring` with a literal: the host downloads the text and inlines the source. A URL held in a variable does not go through here.
- `arquivos.rs` handles `writefile`/`readfile` without native code: the host sends a snapshot of the folder at the start of the chunk, and the game sends writes back through log lines that the UI applies to disk.

## Natives

Some functions need to call code inside the client itself (`getrawmetatable`, `setreadonly`, `getgc`, `getupvalue`...). `nativas.rs` writes these x86-64 functions into a page of the payload and fills one slot per function with the address the engine found. Only functions that read memory or call into the client itself live here; anything that needs a runtime hook does not exist yet (see `limitations.md`). `LUARIA_NATIVAS=0` turns the block off.

## Host-to-game bridge

The game cannot wait for a host response during `luaD_call` without re-entering the interpreter. So the channel is a ring in shared memory (`ponte_layout.rs`): the game writes a request into a slot and waits for the host to mark the response. `ponte.rs` is the host-side poller; `ponte_codec.rs` encodes fields as `<len>:<bytes>`; `ponte_nativas.rs` holds the two functions the game calls (`enviar` and `receber`).

The UI or `luaria-cli exec --ficar` serves the ring. Without either of them, `request` fails with `host not responding (open the UI)`. The host performs the `curl` with the same anti-SSRF check as `HttpGet` (private IPs refused, IP pinned with `--resolve`).

## UI

- `bin/ui_web.rs`: GTK4 window with WebKitGTK 6.0. A dedicated thread runs the engine; the window never waits for it.
- `bin/ui_web/servidor.rs`: HTTP server on `127.0.0.1` on a free port. It only accepts `Host` and `Origin` headers from the UI itself, so a page open in the browser cannot send `POST /api/exec`.
- `bin/ui_web/estado.rs`: engine state and client log tailing.
- `bin/ui_web/config.rs`, `pastas.rs`, `clientes_api.rs`: configuration, file tree (every path goes through `resolver_caminho`, which rejects `..`, `\` and a leading slash), and client list.
- `clientes.rs`: lists client processes through `/proc`, without `ptrace` and without attaching.
- `assets/ui/`: HTML, CSS and JS. The JS is modular under `src/`; `app.js` is the CodeMirror bundle generated by esbuild and committed to the repository.

## Folder layout

```
crates/luaria/src/
  engine.rs              engine: connection, loading, patching, calls
  opcodes.rs luau.rs     bytecode and opcode translation
  scan.rs xref.rs elf.rs eh_frame.rs plt.rs vm.rs   anchors in the payload
  conexao.rs estado_jogo.rs sonda.rs                connection and reconnection
  process.rs remote.rs hook.rs nativas.rs           ptrace, stub, natives
  ponte*.rs                                         host<->game bridge
  prelude.rs remoto.rs arquivos.rs                  executor API
  clientes.rs                                       client list
  bin/cli.rs                                        luaria-cli
  bin/ui_web.rs bin/ui_web/                         webview UI
  bin/ui.rs bin/ui/                                 pure GTK4 UI (fallback)
assets/                  prelude.lua, loadstring.lua, ui/
scripts/                 Lua test scripts and the UNC test
tools/                   build, installation and integration tests
docs/                    this folder
```
