#!/usr/bin/env bash
# Roda o teste oficial da UNC no cliente e extrai o placar.
#
# O script de teste usa task.spawn/task.defer: ele agenda as verificações e
# retorna na hora. Por isso o harness executa, espera o "UNC Summary" aparecer
# no log e só então lê o resultado.
#
# Uso: tools/unc.sh [arquivo.lua]
set -uo pipefail
cd "$(dirname "$0")/.."

BIN="${CARGO_TARGET_DIR:-$HOME/.cache/luaria-target}/release/luaria-cli"
# cliente aberto pelo Mocktail não tem o stdout em /tmp: usa o log interno dele
LOG=${LUARIA_LOG:-$(ls -t /tmp/luaria-client.log ~/.var/app/space.bigrat.mocktail/data/mocktail/android/data/files/appData/logs/*.log 2>/dev/null | head -1)}
TESTE="${1:-scripts/unc_check.lua}"
URL="https://raw.githubusercontent.com/unified-naming-convention/NamingStandard/main/UNCCheckEnv.lua"

if [ ! -f "$TESTE" ]; then
    echo "[unc] baixando o teste oficial para $TESTE"
    curl -fsSL "$URL" -o "$TESTE" || { echo "[unc] falha no download"; exit 1; }
fi

PID=$(for p in /proc/[0-9]*; do grep -q libroblox "$p/maps" 2>/dev/null && { echo "${p#/proc/}"; break; }; done)
if [ -z "$PID" ]; then
    echo "[unc] nenhum cliente aberto — rode tools/launch.sh 189707 antes"
    exit 1
fi

echo "[unc] cliente pid=$PID, executando $TESTE (log: $LOG)"
# só as linhas desta rodada: o log guarda o placar das anteriores
INICIO=$(($(wc -l < "$LOG") + 1))
timeout 120 "$BIN" exec "$TESTE" --pid="$PID" 2>&1 | tail -3

echo "[unc] esperando o resumo (até 60s)"
for _ in $(seq 1 30); do
    sleep 2
    tail -n +"$INICIO" "$LOG" | grep -aq "UNC Summary" && break
done
RODADA=$(mktemp)
trap 'rm -f "$RODADA"' EXIT
tail -n +"$INICIO" "$LOG" > "$RODADA"
LOG=$RODADA

echo
echo "=== placar ==="
grep -ao "Tested with a [0-9]*% success rate ([0-9]* out of [0-9]*)" "$LOG" | tail -1
echo "passes: $(grep -c "CreatorOutput\] ✅" "$LOG")  falhas: $(grep -c "⛔" "$LOG")"
echo
echo "=== falhas ==="
grep -ao "⛔.*" "$LOG" | sed 's/.*⛔ //' | sort -u | head -80
echo
echo "=== aliases ausentes ==="
grep -ao "⚠️.*" "$LOG" | sed 's/.*⚠️ //' | sort -u | head -30
