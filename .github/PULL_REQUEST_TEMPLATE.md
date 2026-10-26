<!-- PT-BR: título e commits em inglês (`feat: short description`). Descreva o porquê, não as linhas. Vulnerabilidade: não abra PR público, veja SECURITY.md. -->

## What changes

<!-- One paragraph. Why the change exists, not what the lines do. -->

## How I tested it

- [ ] `cargo test -p luaria --lib`
- [ ] `cargo test -p luaria --bin luaria`
- [ ] Prelude harness (if you changed `assets/prelude.lua` or `loadstring.lua`)
- [ ] Live on the client (`tools/test_exec.sh` or `tools/unc.sh`), with the score before and after

## Checklist

- [ ] Commits and PR title follow `type: short description` in English
- [ ] No offset, address or client binary byte in code, tests or docs
- [ ] No code copied from other executors and no third-party dump
- [ ] Prelude has no `pcall` and no fake API (unimplemented functions stay out or go in `docs/limitations.md`)
- [ ] No anti-cheat bypass
