# Contributing

Português (pt-BR): [CONTRIBUTING.pt-BR.md](CONTRIBUTING.pt-BR.md)

Thanks for wanting to help. Before you start, read the [legal notice](docs/legal-notice.md): this project does not accept anti-cheat bypasses, malicious scripts, or code copied from other executors.

## Workflow

1. For large changes, open an issue first, or comment on an existing one before coding.
2. Fork and create a branch from `main`: `git checkout -b fix/short-name`.
3. Keep changes small and focused. One PR, one goal.
4. Run the tests (below) before opening the PR.
5. Open the PR using the template. Describe what changed and how you tested it.

## Commits and PRs

Commit messages and PR titles are in **English**, in the format `type: short description`. Types: `feat`, `fix`, `refactor`, `chore`, `docs`, `test`. Examples:

- `feat: add appendfile to the prelude`
- `fix: handle writefile called without a path`

Add a commit body only when the cause is not obvious: explain the before and after.

## Required tests

```bash
cargo test -p luaria --lib
cargo test -p luaria --bin luaria
```

Changed the prelude? Also run the prelude harness (see [docs/contributing-a-function.md](docs/contributing-a-function.md)). Changed something that touches the client? Say in the PR what you tested live.

Tests that depend on the installed Mocktail client (`tools/test_exec.sh`, `tools/unc.sh`) are manual and do not run in CI.

## Code rules

- **No `pcall` in the prelude and no fake API.** If it does not do what UNC expects, do not expose it.
- **Comment only the why.** No comments that repeat the code.
- **No offsets, addresses or client binary bytes** in new code, comments, tests or documentation. Describe them as "measured per payload" and keep the measurements out of the repository.
- **No third-party material.** Do not copy code, dumps or snippets from other executors, or proprietary Roblox code. If a reference is needed, describe the behavior in your own words.
- **No premature abstraction.** One implementation, no interface. Wait for three concrete cases before generalizing.
- **No binaries** in PRs. We do not commit `.so`, binaries, `.apk`, `.dex`, logs or dumps.

Existing code and comments are in Portuguese (pt-BR) and stay that way for now. New comments are preferably written in English.

## Vulnerabilities

Do not open a public issue for a vulnerability. See [SECURITY.md](SECURITY.md).

## Code of Conduct

By taking part, you agree to the [Code of Conduct](CODE_OF_CONDUCT.md).
