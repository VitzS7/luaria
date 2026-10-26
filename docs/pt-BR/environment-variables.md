# Variáveis de ambiente

English: [environment-variables.md](../environment-variables.md)

Todas as variáveis lidas pelo código. Nenhuma é obrigatória.

## Uso

| Variável | Padrão | Efeito |
| --- | --- | --- |
| `LUARIA_LOG` | log mais recente entre `/tmp/luaria-client.log` e o log interno do Mocktail | Log do cliente que a UI e `tools/unc.sh` acompanham |
| `LUARIA_SCRIPTS` | `~/.local/share/luaria/scripts` | Pasta da aba Scripts |
| `LUARIA_CHANGELOG` | `CHANGELOG.md` no diretório atual | Arquivo lido pela Home. Sem ele, a Home mostra o estado vazio |
| `LUARIA_IDENTIDADE` | `8` | Capability do chunk. `0` mantém o chunk sandboxed |
| `LUARIA_NATIVAS` | ligado | `0` desliga as nativas (`getrawmetatable`, `setreadonly`, `getgc`...) |
| `LUARIA_WRAP` | desligado | `1` embrulha o script em `pcall`. Erro vira retorno booleano |
| `LUARIA_PERMITIR_MAIN` | desligado | `1` aceita a thread `Main` na chamada remota. Congela o render; só para teste |
| `LUARIA_EST` | `1` | Estratégia de chamada remota |
| `LUARIA_LUAU_O` | `0` | Nível `-O` do compilador. Acima de 0 emite FASTCALL e a VM do cliente executa o builtin errado |
| `LUARIA_FFLAGS` | flags de compilação fixas do motor | Substitui as flags passadas ao compilador |
| `LUARIA_LUAU_COMPILE` | `~/.cache/luaria/luau-compile` | Caminho do `luau-compile` |
| `LUARIA_CANTOS` | ligado | `0` usa janela retangular e opaca (para compositor sem suporte a transparência) |
| `LUARIA_DEV` | desligado | `1` lê `assets/ui/` do disco, a partir do diretório atual |
| `LUARIA_VISTA`, `LUARIA_ARQUIVO` | vazio | Abrem a UI direto numa vista ou arquivo (uso de desenvolvimento) |

## Diagnóstico

| Variável | Efeito |
| --- | --- |
| `LUARIA_DIAG` | Log detalhado: offsets do `Proto`, patch por proto, registradores |
| `LUARIA_VERBOSE` | Log extra da sonda de estado |
| `LUARIA_SEM_PATCH` | Carrega sem reescrever o código (isola o undump) |
| `LUARIA_SEM_REMAP` | Manda o bytecode canônico e deixa o cliente traduzir |
| `LUARIA_SEM_EXEC` | Para depois da carga, sem chamar a closure |
| `LUARIA_SEM_PRELUDE` | Executa sem o prelude |
| `LUARIA_PROBE` | Para depois da sonda de estado |
| `LUARIA_REJEITAR_VM` | Diagnóstico da separação das tabelas globais entre app e jogo |
| `LUARIA_FREIO` | Endereço (hex) do ponto de chamada usado no lugar do descoberto. Só para depurar o motor |

Marcadores de log do tipo `LUARIA_FS`, `LUARIA_CLIP`, `LUARIA_ERRO` e `LUARIA_PLAYER` são linhas que o script imprime para a UI, não variáveis de ambiente.
