# Política de segurança

English: [SECURITY.md](SECURITY.md)

## Como reportar

Não abra issue pública para vulnerabilidade. Use o **Private Vulnerability Reporting** do GitHub: aba **Security > Report a vulnerability** deste repositório. O relatório fica visível só para os mantenedores.

Inclua: versão ou commit, passos para reproduzir, impacto esperado e, se possível, um caso mínimo. Não envie credenciais, tokens ou dados de contas reais no relatório.

Respondemos em até 7 dias para confirmar o recebimento. Correção e divulgação são combinadas com quem reportou.

## Escopo

O Luaria roda no Linux do usuário e tem superfícies que valem atenção:

- **Servidor HTTP local da UI** (`127.0.0.1`, porta livre). Recusa `Host` e `Origin` que não sejam da própria UI, para que uma página aberta no navegador não mande `POST /api/exec`.
- **Ponte host↔jogo** (anel em memória compartilhada). O jogo escreve pedidos e o host os atende. Qualquer código que roda no cliente pode escrever no anel: o host trata todo campo como dado não confiável.
- **Anti-SSRF do host.** Pedidos feitos pelo host (`HttpGet`, `request`) só vão para `http`/`https`, nunca para localhost ou IP privado, inclusive após resolver DNS e em cada redirect. O IP checado é fixado com `--resolve`.
- **Token local** (`~/.config/luaria/.token`, modo 0600). Cada linha `LUARIA_FS` que o jogo emite leva o token; sem ele a escrita é recusada.
- **Árvore de arquivos da UI.** Todo caminho passa por `resolver_caminho`: sem `..`, sem `\`, sem barra inicial, sem link simbólico para fora da raiz.
- **Injeção por ptrace.** O motor escreve na memória de um processo de terceiros. Um bug aqui pode derrubar o cliente ou corromper o estado do jogo.
- **Downloads de terceiros** (`curl`) e cache em `~/.cache/luaria/http/`.

Fora de escopo: scripts de usuário que fazem mal ao próprio usuário, bypass de anti-cheat da Roblox e problemas no próprio cliente da Roblox ou no Mocktail (reporte a eles).
