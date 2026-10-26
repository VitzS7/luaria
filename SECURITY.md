# Security policy

Português (pt-BR): [SECURITY.pt-BR.md](SECURITY.pt-BR.md)

## Reporting a vulnerability

Do not open a public issue for a vulnerability. Use GitHub's **Private Vulnerability Reporting**: the **Security > Report a vulnerability** tab of this repository. The report is visible only to the maintainers.

Include: version or commit, steps to reproduce, expected impact, and a minimal case if possible. Do not send credentials, tokens or real account data in the report.

We reply within 7 days to confirm receipt. The fix and the disclosure are agreed with the reporter.

## Scope

Luaria runs on the user's Linux machine and has surfaces worth attention:

- **Local HTTP server of the UI** (`127.0.0.1`, free port). It refuses `Host` and `Origin` values that are not the UI's own, so a page open in the browser cannot send `POST /api/exec`.
- **Host-to-game bridge** (shared-memory ring). The game writes requests and the host serves them. Any code running in the client can write to the ring, so the host treats every field as untrusted data.
- **Host anti-SSRF.** Requests made by the host (`HttpGet`, `request`) only go to `http`/`https`, never to localhost or private IPs, including after DNS resolution and on every redirect. The checked IP is pinned with `--resolve`.
- **Local token** (`~/.config/luaria/.token`, mode 0600). Every `LUARIA_FS` line the game emits carries the token; without it, writes are refused.
- **UI file tree.** Every path goes through `resolver_caminho`: no `..`, no `\`, no leading slash, no symbolic link pointing outside the root.
- **ptrace injection.** The engine writes into the memory of a third-party process. A bug here can crash the client or corrupt the game state.
- **Third-party downloads** (`curl`) and the cache in `~/.cache/luaria/http/`.

Out of scope: user scripts that harm the user themselves, anti-cheat bypasses against Roblox, and problems in the Roblox client itself or in Mocktail (report those to their maintainers).
