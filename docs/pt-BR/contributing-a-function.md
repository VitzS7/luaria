# Como adicionar uma função ao executor

English: [contributing-a-function.md](../contributing-a-function.md)

Passo a passo para adicionar ou corrigir uma função da API. A API vive em Lua puro (`assets/prelude.lua`); só o que exige código do cliente vai para nativa (ver `limitations.md`).

## 1. Decidir onde a função vai

- **Lua puro** (cálculo, tabela, string, arquivo pelo instantâneo): `assets/prelude.lua`.
- **Precisa ler ou chamar o cliente**: é nativa. Não é um passo para quem está começando; abra uma issue antes de escrever o stub.
- **Precisa de comunicação em tempo de execução host→jogo**: use o canal (`ponte*.rs`) e leia `architecture.md` primeiro.

## 2. Implementar no prelude

O prelude roda dentro do chunk do script. Use o padrão já existente:

```lua
publicar("appendfile", marcar(function(caminho, dados)
    validar(caminho)
    dados_validos(dados)
    -- ...
end))
```

- `publicar(nome, valor)` expõe o nome no ambiente do script. `marcar(f)` marca a closure como do executor (`isexecutorclosure`).
- **Sem `pcall`.** O fast-pcall do cliente não sobrevive ao `loadstring` dinâmico. Para capturar erro, use `coroutine.resume(coroutine.create(f))`, que devolve `(false, msg)`.
- **Nada de API fingida.** Se a função não faz o que a UNC espera, não invente campos nem retorno. Deixe fora ou marque como aproximada em `limitations.md`.
- Erro de argumento segue o formato da UNC, com `error(..., 3)` apontando para quem chamou.

## 3. Escrever o teste

Em `scripts/teste_prelude.lua`, use o helper `caso`:

```lua
-- exemplo: `resultado` vem da chamada da função testada
caso("appendfile acumula", function()
    -- arrange, act, assert com erro explícito
    assert(resultado == "ab", "esperado ab, veio " .. tostring(resultado))
end)
```

O teste roda fora do jogo, no REPL do Luau. Ele não tem `Instance` nem `game` reais: use só o que for Lua puro, ou dublês mínimos já presentes no arquivo.

## 4. Rodar

```bash
cargo test -p luaria --lib
cargo test -p luaria --bin luaria
sed -e '/^--@@PRELUDE@@/r assets/prelude.lua' -e '/^--@@PRELUDE@@/r assets/loadstring.lua' \
    scripts/teste_prelude.lua > /tmp/p.lua && luau /tmp/p.lua
```

O último `OK n/n` tem que mostrar `n` igual ao total. Qualquer `FALHOU` antes disso é falha.

## 5. Conferir no jogo e na UNC

Com o cliente aberto dentro de uma experiência:

```bash
tools/test_exec.sh scripts/teste_print.lua LUARIA_OK_1
tools/unc.sh
```

O placar da UNC não pode cair em relação à última medição. Se a sua função mudou o número de passes, diga no PR qual subteste mudou e por quê.

## 6. Comentários

Comente só o porquê: a razão de um workaround, uma regra do cliente que não é óbvia, um aviso de concorrência. Não comente o que o código já diz.
