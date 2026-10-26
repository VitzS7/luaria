#!/usr/bin/env bash
# Runs the sUNC-style suite on the open client and prints the score, same shape as unc.sh.
#
# Default: our own suite (scripts/sunc_own.lua) with the batches in scripts/sunc/.
# The official sUNC is obfuscated third-party code and only measures real behavior
# inside the official test game. It runs only with the exact hash you reviewed:
#
#   tools/sunc.sh                                        own suite
#   SUNC_OFFICIAL=1 tools/sunc.sh                        downloads the official file, prints sha256, stops
#   SUNC_OFFICIAL=1 SUNC_SHA256=<hash> tools/sunc.sh     runs it only if the downloaded hash matches
set -uo pipefail
cd "$(dirname "$0")/.."

BIN="${CARGO_TARGET_DIR:-$HOME/.cache/luaria-target}/release/luaria-cli"
# cliente aberto pelo Mocktail não tem o stdout em /tmp: usa o log interno dele
LOG=${LUARIA_LOG:-$(ls -t /tmp/luaria-client.log ~/.var/app/space.bigrat.mocktail/data/mocktail/android/data/files/appData/logs/*.log 2>/dev/null | head -1)}
PASTA="$HOME/.cache/luaria/sunc"
ARQ="$PASTA/sunc.lua"
LOADER="https://script.sunc.io/"

[ -x "$BIN" ] || { echo "[sunc] binário ausente: rode tools/build.sh --bin luaria-cli"; exit 1; }

PID=$(for p in /proc/[0-9]*; do grep -q libroblox "$p/maps" 2>/dev/null && { echo "${p#/proc/}"; break; }; done)
[ -n "$PID" ] || { echo "[sunc] nenhum cliente aberto — rode tools/launch.sh antes"; exit 1; }

if [ "${SUNC_OFFICIAL:-0}" != 1 ]; then
    for lote in a b c; do
        [ -f "scripts/sunc/batch_$lote.lua" ] || { echo "[sunc] falta scripts/sunc/batch_$lote.lua"; exit 1; }
    done
    TESTE=$(mktemp)
    RODADA=$(mktemp)
    trap 'rm -f "$TESTE" "$RODADA"' EXIT
    sed -e '/^--@@BATCH_A@@/r scripts/sunc/batch_a.lua' \
        -e '/^--@@BATCH_B@@/r scripts/sunc/batch_b.lua' \
        -e '/^--@@BATCH_C@@/r scripts/sunc/batch_c.lua' scripts/sunc_own.lua > "$TESTE"

    echo "[sunc] cliente pid=$PID, executando a suite própria (log: $LOG)"
    # só as linhas desta rodada: o log guarda o placar das anteriores
    INICIO=$(($(wc -l < "$LOG") + 1))
    timeout 120 "$BIN" exec "$TESTE" --pid="$PID" 2>&1 | tail -3

    echo "[sunc] esperando o fim da suite (até 90s)"
    for _ in $(seq 1 45); do
        sleep 2
        tail -n +"$INICIO" "$LOG" | grep -aq "sUNC_OWN_DONE" && break
    done
    tail -n +"$INICIO" "$LOG" > "$RODADA"

    echo
    echo "=== placar ==="
    grep -a "sUNC (own)" "$RODADA" | tail -1
    echo
    echo "=== falhas ==="
    sed -n 's/.*\(⛔ .*\)/\1/p' "$RODADA" | awk '!visto[$0]++' | head -80
    exit 0
fi

# Caminho oficial: o sUNC v2 só funciona dentro do jogo de testes oficial e usa loadstring/HttpGet,
# que este executor ainda não tem. Não é um placar confiável fora de lá.
mkdir -p "$PASTA"
destino=$(curl -fsSL "$LOADER" | grep -o 'https://[^"]*\.lua') || { echo "[sunc] falha ao ler o loader oficial"; exit 1; }
echo "[sunc] loader oficial aponta para: $destino"
curl -fsSL "$destino" -o "$ARQ" || { echo "[sunc] falha no download"; exit 1; }
HASH=$(sha256sum "$ARQ" | cut -d' ' -f1)
echo "[sunc] $(wc -c < "$ARQ") bytes, sha256=$HASH"

if [ "${SUNC_SHA256:-}" != "$HASH" ]; then
    echo "[sunc] não executado: confirme o hash com SUNC_SHA256=$HASH depois de revisar a origem"
    exit 3
fi

echo "[sunc] cliente pid=$PID, executando o sUNC oficial"
timeout 180 "$BIN" exec "$ARQ" --pid="$PID" 2>&1 | tail -3
sleep 20
echo "=== placar ==="
grep -ao "[0-9]*% success rate[^\"]*" "$LOG" | tail -1
echo "passes: $(grep -ac "✅" "$LOG")  falhas: $(grep -ac "⛔" "$LOG")"
