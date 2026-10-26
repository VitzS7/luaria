# Adding a function to the executor

Português (pt-BR): [contributing-a-function.md](pt-BR/contributing-a-function.md)

Step by step to add or fix an API function. The API lives in plain Lua (`assets/prelude.lua`); only what needs client code goes to a native (see `limitations.md`).

## 1. Decide where the function goes

- **Plain Lua** (computation, tables, strings, files via snapshot): `assets/prelude.lua`.
- **Needs to read or call into the client**: it is a native. This is not a first-timer step; open an issue before writing the stub.
- **Needs host-to-game communication at runtime**: use the channel (`ponte*.rs`) and read `architecture.md` first.

## 2. Implement it in the prelude

The prelude runs inside the script's chunk. Follow the existing pattern:

```lua
publicar("appendfile", marcar(function(caminho, dados)
    validar(caminho)
    dados_validos(dados)
    -- ...
end))
```

- `publicar(nome, valor)` exposes the name in the script's environment. `marcar(f)` marks the closure as the executor's (`isexecutorclosure`).
- **No `pcall`.** The client's fast-pcall does not survive dynamic `loadstring`. To catch an error, use `coroutine.resume(coroutine.create(f))`, which returns `(false, msg)`.
- **No fake API.** If the function does not do what UNC expects, do not invent fields or return values. Leave it out, or mark it as approximate in `limitations.md`.
- Argument errors follow the UNC format, with `error(..., 3)` pointing at the caller.

## 3. Write the test

In `scripts/teste_prelude.lua`, use the `caso` helper:

```lua
-- example: `resultado` comes from calling the function under test
caso("appendfile accumulates", function()
    -- arrange, act, assert with an explicit error
    assert(resultado == "ab", "expected ab, got " .. tostring(resultado))
end)
```

The test runs outside the game, in the Luau REPL. It has no real `Instance` or `game`: use only plain Lua, or the minimal doubles already in the file.

## 4. Run

```bash
cargo test -p luaria --lib
cargo test -p luaria --bin luaria
sed -e '/^--@@PRELUDE@@/r assets/prelude.lua' -e '/^--@@PRELUDE@@/r assets/loadstring.lua' \
    scripts/teste_prelude.lua > /tmp/p.lua && luau /tmp/p.lua
```

The last `OK n/n` line must show `n` equal to the total. Any `FALHOU` (failed) line before that is a failure.

## 5. Check in the game and against UNC

With the client open inside an experience:

```bash
tools/test_exec.sh scripts/teste_print.lua LUARIA_OK_1
tools/unc.sh
```

The UNC score must not drop compared with the last measurement. If your function changed the number of passing subtests, say in the PR which subtest changed and why.

## 6. Comments

Comment only the why: the reason for a workaround, a client rule that is not obvious, a concurrency warning. Do not comment on what the code already says.
