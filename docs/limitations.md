# Limitations

Português (pt-BR): [limitations.md](pt-BR/limitations.md)

What does not work, works only partly, or depends on work not yet done. Measured per payload: the engine only knows the client binary it was validated against, and this list applies to that build.

## Structural

- **Fixed payload.** The engine validates the entry points before using them. If Roblox updates the client, the engine fails loudly with a message instead of executing wrong code. Every update needs a new measurement.
- **Platform.** Linux only, with the Android x86_64 client running in Mocktail. There is no support for Windows, macOS, iOS or the desktop client.
- **Long scripts freeze the game.** The remote call holds all threads until it returns. Aborting in the middle is not safe: `luaD_call` was taken in the middle of the game's stack, and a forced return corrupts the state. Keep scripts short.
- **Main thread.** The engine refuses `Main`, because the remote call holds the renderer while it waits. It waits for a script thread (`RBX Worker`).
- **No C-side `pcall` by default.** Script errors propagate through the game's `luaD_call`. `LUARIA_WRAP=1` wraps the script in `pcall`.
- **Reduced capability.** The captured thread usually has fewer permissions than the main one (no `CreateInstances`, `Players`, `Network`). The chunk receives an elevated mask through `LUARIA_IDENTIDADE`; without it `Instance.new` and `Drawing` fail.
- **Large chunks.** Above a certain size, the client's loader applies the Roblox signature check and refuses the bytecode. The engine adjusts that check in memory; it is a one-byte patch and idempotent.
- **Depth.** The descent through protos stops at 64 levels. Protos whose size diverges from what is expected abort the patch.
- **Retry.** If the stopped thread is in a state the loader does not accept, the attempt fails and the engine tries again, up to 3 times.

## Behavior differences

- **Dynamic `loadstring` is slow.** The Luau interpreter runs about 10 times slower than native code in loops and calls. `loadstring` with a literal is compiled by the host and runs natively.
- **Dynamic `HttpGet` does not work.** The host only downloads literal URLs. `request` and `http.request` go through the host-to-game bridge.
- **Native errors have no line.** Errors from `error`/`assert` show `name:line`; native errors (indexing nil, arithmetic on nil) point at the interpreter, without the script's line.
- **Non-tail recursion** overflows at about 3500 levels (native reaches about 20000). Tail calls are constant.
- **Error line numbers** only match before the first rewrite of an inlined `loadstring`.
- **File writes** only reach disk while the UI is open: the UI reads the client log and applies the writes. `luaria-cli` alone does not write.
- **`request`** fails with `host not responding (open the UI)` if neither the UI nor `luaria-cli --ficar` is running.
- **`getsenv`** fails in some UNC scripts ("script has no environment"): the environment was already collected from the GC.

## Approximate

These functions exist and cover common cases, but do not reproduce the real behavior in every detail:

- `Drawing`: Frame, TextLabel and ImageLabel become objects inside a ScreenGui. Filled Triangle and Quad come out as outlines only.
- `newcclosure`: a closure built on `coroutine.wrap`. An error inside kills the closure; no recursion or yield.
- `getrunningscripts`, `getloadedmodules`: heuristics. They find script envs through the GC; a user table with a `script` field also counts. `getloadedmodules` lists modules with a live env, not the `require` history.
- `getinstances`, `getscripts`, `getnilinstances`: DataModel tree and parentless instances taken from the GC.
- `cloneref`, `compareinstances`: table proxy, not the real instance.
- `checkcaller` always returns `true`.
- `getthreadidentity`/`setthreadidentity` store a number; they do not change the thread's real capability.
- `hookmetamethod` only works on tables, not on userdata.
- `debug.getconstant(s)`: the constant table comes out with `-O0`. The UNC test expects `-O1`, and going back to `-O1` breaks the VM (FASTCALL). That is why both fail in UNC.

## Pending native code

These need a new stub written by hand and validated live, one function at a time:

`hookfunction`, `hookmetamethod` on userdata and `__namecall`, `getnamecallmethod`, `filtergc`, `getconnections`, `firesignal`, `replicatesignal`, `fireclickdetector`, `firetouchinterest`, `getcallbackvalue`, `getscriptclosure`, `getscriptbytecode`, `getscripthash`, `decompile`, `debug.getproto(s)`, `debug.getstack`, `debug.setstack`, `gethiddenproperty`, `sethiddenproperty`, `setscriptable`, `getcustomasset`.

`getscriptbytecode` and `decompile` depend on the bytecode of game scripts, which the client does not expose in plain text; the strategy is not defined yet.

## Pending host-to-game channel

`WebSocket`, `queue_on_teleport`, `rconsole*`, `messagebox`. They need host-to-game communication at runtime, which today only exists in the game-to-host direction (log) and in HTTP requests.

## Not planned

- Anti-cheat bypass. See [legal-notice.md](legal-notice.md).

## Experimental

`debug.setconstant` and `debug.setupvalue` are implemented as natives with a GC write barrier. They were validated offline only and have not been confirmed on a live client yet. `setconstant` requires the new constant to have the same type as the old one and errors otherwise.
