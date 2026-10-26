# Changelog

## 0.1.0

First public release.

- Luau executor for the Roblox Android x86_64 client in Mocktail: ptrace injection, bytecode translation, calls into the client's real `luau_load` and `luaD_call`.
- Webview UI (editor, scripts library, settings and themes), GTK4 fallback UI and `luaria-cli`.
- Executor API prelude: files, crypt, `request` through a host-to-game channel, metatables, `getgc`, `debug.*` introspection. UNC 58/83.
- Experimental: `debug.setconstant` and `debug.setupvalue` (natives with a GC write barrier, validated offline only).
- Own sUNC suite written from the public contract (`scripts/sunc_own.lua`, 79 tests).
- English documentation with pt-BR translations, issue and PR templates, CI.
