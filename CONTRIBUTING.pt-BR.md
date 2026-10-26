# Como contribuir

English: [CONTRIBUTING.md](CONTRIBUTING.md)

Obrigado por querer ajudar. Antes de começar, leia o [aviso legal](docs/pt-BR/legal-notice.md): este projeto não aceita bypass de anti-cheat, scripts maliciosos ou código copiado de outros executores.

## Fluxo

1. Abra uma issue para mudanças grandes, ou comente numa issue existente antes de codar.
2. Faça um fork e crie uma branch a partir de `main`: `git checkout -b fix/nome-curto`.
3. Mudanças pequenas e focadas. Um PR, um objetivo.
4. Rode os testes (abaixo) antes de abrir o PR.
5. Abra o PR preenchendo o template. Descreva o que mudou e como você testou.

## Commits e PRs

Commits e títulos de PR são em **inglês**, no formato `tipo: descrição curta`, tipos `feat`, `fix`, `refactor`, `chore`, `docs`, `test`. Exemplos:

- `feat: add appendfile to the prelude`
- `fix: handle writefile called without a path`

Corpo do commit só quando a causa não for óbvia: explique o antes e o depois.

## Testes obrigatórios

```bash
cargo test -p luaria --lib
cargo test -p luaria --bin luaria
```

Mudou o prelude? Rode também o harness do prelude (ver [docs/pt-BR/contributing-a-function.md](docs/pt-BR/contributing-a-function.md)). Mudou algo que toca o cliente? Diga no PR o que você testou ao vivo.

Testes que dependem do cliente Mocktail instalado (`tools/test_exec.sh`, `tools/unc.sh`) são manuais e não rodam no CI.

## Regras do código

- **Prelude sem `pcall`** e **sem API fingida**. Se não faz o que a UNC espera, não exponha.
- **Comentário só do porquê.** Nada de comentário que repete o código.
- **Nada de offset, endereço ou byte de binário do cliente** em código novo, em comentário, em teste ou em documentação. Fale em termos de "medido por payload" e deixe a medição fora do repositório.
- **Sem material de terceiros.** Não copie código, dumps ou trechos de outros executores, nem código proprietário da Roblox. Se uma referência for necessária, descreva o comportamento com suas palavras.
- **Sem abstração prematura.** Uma implementação, sem interface. Três casos concretos antes de generalizar.
- **Sem binários** no PR. Não commitamos `.so`, binários, `.apk`, `.dex`, logs ou dumps.

Código e comentários existentes estão em português e continuam assim por ora. Comentários novos, de preferência, em inglês.

## Vulnerabilidades

Não abra issue pública para vulnerabilidade. Veja [SECURITY.pt-BR.md](SECURITY.pt-BR.md).

## Código de conduta

Ao participar, você concorda com o [Código de Conduta](CODE_OF_CONDUCT.pt-BR.md).
