#!/usr/bin/env bash
# Ciclo completo de teste: abre o cliente, espera entrar no jogo, executa um script
# e procura o marcador no log. Uso: tools/test_exec.sh scripts/teste_print.lua LUARIA_OK_1
set -uo pipefail
cd "$(dirname "$0")/.."
BIN="${CARGO_TARGET_DIR:-$HOME/.cache/luaria-target}/release/luaria-cli"
SCRIPT="${1:-scripts/teste_print.lua}"
MARCA="${2:-LUARIA_OK_1}"
LOG=/tmp/luaria-client.log

timeout 420 tools/launch.sh 189707 "$LOG" || { echo "cliente não subiu"; exit 1; }
PID=$(for p in /proc/[0-9]*; do grep -q libroblox "$p/maps" 2>/dev/null && { echo "${p#/proc/}"; break; }; done)
[ -z "$PID" ] && { echo "sem pid"; exit 1; }
echo "[teste] pid=$PID — esperando entrar no jogo"
for _ in $(seq 1 60); do
    sleep 3
    grep -q "AvatarJoinMetrics" "$LOG" && break
done

echo "[teste] executando $SCRIPT"
timeout 120 "$BIN" exec "$SCRIPT" --pid="$PID" 2>&1 | tail -12
echo "[teste] marcador '$MARCA' no log: $(grep -c "$MARCA" "$LOG")"
grep -n "$MARCA" "$LOG" | head -3
kill -0 "$PID" 2>/dev/null && echo "[teste] cliente vivo" || echo "[teste] cliente MORTO"
