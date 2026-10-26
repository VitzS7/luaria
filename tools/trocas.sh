#!/usr/bin/env bash
# Mede a troca de jogo: um Motor só (luaria-cli servir) executa em laço enquanto
# o cliente é fechado e reaberto em outro PlaceId. Conta quantas trocas o motor
# atravessou com a execução voltando a funcionar no processo novo.
#
# Uso: tools/trocas.sh [trocas] [place1 place2 ...]
set -uo pipefail
cd "$(dirname "$0")/.."

BIN="${CARGO_TARGET_DIR:-$HOME/.cache/luaria-target}/release/luaria-cli"
LOG=/tmp/luaria-client.log
SAIDA=/tmp/servir.out
TROCAS="${1:-5}"
shift || true
PLACES=("$@")
[ ${#PLACES[@]} -eq 0 ] && PLACES=(189707 142823291)

cat > /tmp/servir.lua <<'LUA'
print("SERVIR_{N} " .. tostring(game.PlaceId))
LUA

pid_do_jogo() {
    for p in /proc/[0-9]*; do
        grep -q libroblox "$p/maps" 2>/dev/null && { echo "${p#/proc/}"; return 0; }
    done
    return 1
}

esperar_jogo() {
    for _ in $(seq 1 60); do
        grep -aq "GameJoinLoadTime" "$LOG" 2>/dev/null && return 0
        sleep 2
    done
    return 1
}

: > "$SAIDA"
nohup "$BIN" servir /tmp/servir.lua --intervalo=6 --max=$((TROCAS * 30 + 30)) >> "$SAIDA" 2>&1 &
SERVIR=$!
trap 'kill $SERVIR 2>/dev/null' EXIT

certas=0
for i in $(seq 1 "$TROCAS"); do
    place=${PLACES[$(( (i - 1) % ${#PLACES[@]} ))]}
    echo "[trocas] $i/$TROCAS: abrindo placeId=$place"
    tools/launch.sh "$place" "$LOG" | tail -1
    novo=$(pid_do_jogo || echo 0)
    if ! esperar_jogo; then
        echo "[trocas] $i: o cliente não entrou no jogo em 120s"
        continue
    fi
    sleep 25
    marcador=$(grep -a "CreatorOutput\] SERVIR_" "$LOG" | tail -1 | sed 's/.*CreatorOutput\] //')
    if [ -n "$marcador" ] && [ "$(pid_do_jogo || echo 0)" = "$novo" ]; then
        certas=$((certas + 1))
        echo "[trocas] $i: ok pid=$novo ($marcador)"
    else
        echo "[trocas] $i: SEM execução no cliente novo (pid=$novo)"
    fi
done
echo "[trocas] resultado: $certas/$TROCAS trocas com execução funcionando"
echo "[trocas] últimas linhas do motor:"
tail -8 "$SAIDA"
