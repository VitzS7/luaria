# Environment variables

Português (pt-BR): [environment-variables.md](pt-BR/environment-variables.md)

Every variable the code reads. None is required.

## Usage

| Variable | Default | Effect |
| --- | --- | --- |
| `LUARIA_LOG` | most recent log between `/tmp/luaria-client.log` and the Mocktail internal log | Client log that the UI and `tools/unc.sh` follow |
| `LUARIA_SCRIPTS` | `~/.local/share/luaria/scripts` | Folder for the Scripts tab |
| `LUARIA_CHANGELOG` | `CHANGELOG.md` in the current directory | File read by the Home page. Without it, Home shows the empty state |
| `LUARIA_IDENTIDADE` | `8` | Capability mask of the chunk. `0` keeps the chunk sandboxed |
| `LUARIA_NATIVAS` | on | `0` turns the natives off (`getrawmetatable`, `setreadonly`, `getgc`...) |
| `LUARIA_WRAP` | off | `1` wraps the script in `pcall`. An error becomes a boolean return |
| `LUARIA_PERMITIR_MAIN` | off | `1` accepts the `Main` thread in the remote call. Freezes rendering; test only |
| `LUARIA_EST` | `1` | Remote call strategy |
| `LUARIA_LUAU_O` | `0` | Compiler `-O` level. Above 0 it emits FASTCALL and the client's VM runs the wrong builtin |
| `LUARIA_FFLAGS` | fixed compiler flags of the engine | Replaces the flags passed to the compiler |
| `LUARIA_LUAU_COMPILE` | `~/.cache/luaria/luau-compile` | Path to `luau-compile` |
| `LUARIA_CANTOS` | on | `0` uses an opaque rectangular window (for compositors without transparency support) |
| `LUARIA_DEV` | off | `1` reads `assets/ui/` from disk, relative to the current directory |
| `LUARIA_VISTA`, `LUARIA_ARQUIVO` | empty | Open the UI straight into a view or file (development use) |

## Diagnostics

| Variable | Effect |
| --- | --- |
| `LUARIA_DIAG` | Verbose log: `Proto` offsets, per-proto patching, registers |
| `LUARIA_VERBOSE` | Extra log from the state probe |
| `LUARIA_SEM_PATCH` | Loads without rewriting the code (isolates the undump) |
| `LUARIA_SEM_REMAP` | Sends the canonical bytecode and lets the client translate it |
| `LUARIA_SEM_EXEC` | Stops after loading, without calling the closure |
| `LUARIA_SEM_PRELUDE` | Runs without the prelude |
| `LUARIA_PROBE` | Stops after the state probe |
| `LUARIA_REJEITAR_VM` | Diagnostic for the separation of global tables between app and game |
| `LUARIA_FREIO` | Address (hex) of the call point used instead of the discovered one. Engine debugging only |

Log markers such as `LUARIA_FS`, `LUARIA_CLIP`, `LUARIA_ERRO` and `LUARIA_PLAYER` are lines the script prints for the UI, not environment variables.
