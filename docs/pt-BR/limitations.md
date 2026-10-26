# Limitações

English: [limitations.md](../limitations.md)

O que não funciona, funciona só em parte, ou depende de trabalho que ainda não foi feito. Medido por payload: o motor só conhece o binário do cliente em que foi validado, e o resto da lista vale para esse build.

## Estrutural

- **Payload fixo.** O motor valida os pontos de entrada antes de usá-los. Se a Roblox atualizar o cliente, o motor falha alto com mensagem em vez de executar código errado. Cada atualização exige nova medição.
- **Plataforma.** Só Linux com o cliente Android x86_64 no Mocktail. Não há suporte a Windows, macOS, iOS ou ao cliente desktop.
- **Scripts longos travam o jogo.** A chamada remota segura todas as threads até voltar. Abortar no meio não é seguro: o `luaD_call` foi tomado no meio da pilha do jogo e um retorno forçado corrompe o estado. Scripts devem ser curtos.
- **Thread principal.** O motor recusa a `Main`, porque a chamada remota segura o render enquanto espera. Ele espera uma thread de script (`RBX Worker`).
- **Sem `pcall` do lado C por padrão.** Erro de script sobe pelo `luaD_call` do jogo. `LUARIA_WRAP=1` embrulha em `pcall`.
- **Capability reduzida.** A thread capturada costuma ter menos permissões que a principal (sem `CreateInstances`, `Players`, `Network`). O chunk recebe uma máscara elevada por `LUARIA_IDENTIDADE`; sem isso `Instance.new` e `Drawing` falham.
- **Chunk grande.** Acima de um tamanho, o carregador do cliente aplica a verificação de assinatura do Roblox e recusa o bytecode. O motor ajusta essa checagem em memória; é um patch de um byte, idempotente.
- **Profundidade.** A descida em protos para em 64 níveis. Protos com tamanho divergente do esperado abortam o patch.
- **Retry.** Se a thread parada estiver num estado que o carregador não aceita, a tentativa falha e o motor tenta de novo, até 3 vezes.

## Diferenças de comportamento

- **`loadstring` dinâmico é lento.** O interpretador em Luau roda cerca de 10 vezes mais devagar que o nativo em laços e chamadas. `loadstring` com literal é compilado pelo host e roda no nativo.
- **`HttpGet` dinâmico não funciona.** O host só baixa URL literal. `request` e `http.request` passam pelo canal host→jogo.
- **Erros nativos sem linha.** Erro de `error`/`assert` mostra `nome:linha`; erro nativo (indexar nil, aritmética em nil) aponta para o interpretador, sem a linha do script.
- **Recursão não-tail** estoura em cerca de 3500 níveis (o nativo chega a cerca de 20000). Tail call é constante.
- **Números de linha** de erro só batem antes da primeira reescrita de `loadstring` inlinado.
- **Escrita em arquivo** só chega ao disco com a UI aberta: é a UI que lê o log do cliente e aplica as escritas. `luaria-cli` sozinho não grava.
- **`request`** morre com `host not responding (open the UI)` se nem a UI nem `luaria-cli --ficar` estiverem rodando.
- **`getsenv`** falha em alguns scripts do UNC ("script has no environment"): o ambiente já foi coletado do GC.

## Aproximadas

Estas funções existem e cobrem casos comuns, mas não reproduzem o comportamento real em tudo:

- `Drawing`: Frame, TextLabel e ImageLabel viram objetos numa ScreenGui. Triangle e Quad preenchidos saem só com contorno.
- `newcclosure`: closure via `coroutine.wrap`. Erro dentro mata a closure; sem recursão nem yield.
- `getrunningscripts`, `getloadedmodules`: heurística. Encontram envs de script pelo GC; uma tabela de usuário com campo `script` também conta. `getloadedmodules` lista módulo com env vivo, não o histórico de `require`.
- `getinstances`, `getscripts`, `getnilinstances`: árvore do DataModel e instâncias sem pai vindas do GC.
- `cloneref`, `compareinstances`: proxy de tabela, não a instância real.
- `checkcaller` sempre devolve `true`.
- `getthreadidentity`/`setthreadidentity` guardam um número; não mudam a capability real da thread.
- `hookmetamethod` só funciona em tabela, não em userdata.
- `debug.getconstant(s)`: a tabela de constantes sai com `-O0`. O teste da UNC espera o `-O1`, e voltar para `-O1` quebra a VM (FASTCALL). Por isso as duas falham no UNC.

## Pendentes de código nativo

Precisam de um stub novo escrito à mão e validado ao vivo, uma função por vez:

`hookfunction`, `hookmetamethod` em userdata e `__namecall`, `getnamecallmethod`, `filtergc`, `getconnections`, `firesignal`, `replicatesignal`, `fireclickdetector`, `firetouchinterest`, `getcallbackvalue`, `getscriptclosure`, `getscriptbytecode`, `getscripthash`, `decompile`, `debug.getproto(s)`, `debug.getstack`, `debug.setstack`, `gethiddenproperty`, `sethiddenproperty`, `setscriptable`, `getcustomasset`.

`getscriptbytecode` e `decompile` dependem do bytecode de scripts do jogo, que o cliente não expõe em texto plano; a estratégia ainda não está definida.

## Pendentes de canal host→jogo

`WebSocket`, `queue_on_teleport`, `rconsole*`, `messagebox`. Precisam de comunicação do host para o jogo em tempo de execução, que hoje só existe no sentido jogo→host (log) e no pedido HTTP.

## Não planejado

- Anti-cheat bypass. Ver [legal-notice.md](legal-notice.md).

## Experimental

`debug.setconstant` e `debug.setupvalue` existem como nativas com barreira do GC. Foram validadas só offline e ainda não foram confirmadas em cliente real. O `setconstant` exige que a constante nova tenha o mesmo tipo da antiga e dá erro caso contrário.
