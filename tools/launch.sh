#!/usr/bin/env bash
# Abre o cliente Mocktail direto numa experiência, com stdout capturado.
# Uso: tools/launch.sh [placeId] [log]
# Derruba instâncias anteriores pelo PID real (nunca pkill -f: mata o próprio shell).
set -uo pipefail

PLACE="${1:-189707}"
LOG="${2:-/tmp/luaria-client.log}"
APP="/app/bin/mocktail"

matar() {
    for p in /proc/[0-9]*; do
        pid=${p#/proc/}
        exe=$(readlink "$p/exe" 2>/dev/null) || continue
        case "$exe" in
            "$APP"|*/mocktail_failure_dialog) kill -TERM "$pid" 2>/dev/null ;;
        esac
    done
    sleep 1
    for p in /proc/[0-9]*; do
        pid=${p#/proc/}
        exe=$(readlink "$p/exe" 2>/dev/null) || continue
        case "$exe" in "$APP"|*/mocktail_failure_dialog) kill -KILL "$pid" 2>/dev/null ;; esac
    done
}

pid_do_jogo() {
    for p in /proc/[0-9]*; do
        grep -q libroblox "$p/maps" 2>/dev/null && { echo "${p#/proc/}"; return 0; }
    done
    return 1
}

matar
: > "$LOG"
nohup flatpak run space.bigrat.mocktail --launch-uri "roblox://experiences/start?placeId=$PLACE" >> "$LOG" 2>&1 &
echo "[launch] aberto placeId=$PLACE log=$LOG"

for i in $(seq 1 120); do
    sleep 2
    if pid=$(pid_do_jogo); then
        echo "[launch] cliente carregou libroblox pid=$pid (${i}x2s)"
        exit 0
    fi
    if grep -q "authentication preflight failed" "$LOG"; then
        echo "[launch] auth preflight falhou (rate-limit do Mocktail), tentando de novo em 45s"
        matar
        sleep 45
        nohup flatpak run space.bigrat.mocktail --launch-uri "roblox://experiences/start?placeId=$PLACE" >> "$LOG" 2>&1 &
    fi
done
echo "[launch] timeout: cliente não carregou libroblox"
tail -5 "$LOG"
exit 1
