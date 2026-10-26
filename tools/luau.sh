#!/usr/bin/env bash
# Monta o luau-compile da MESMA release do Luau que a VM do cliente usa
# (release/735). Guarda o binário fora do repo, em ~/.cache/luaria/.
#
# Por que a release fixa: o bytecode carrega o opcode na ordem do enum
# `LuauOpcode`, e o enum muda entre releases (a master removeu LOP_LOADKX e
# acrescentou LOP_CONSTRUCT/FINCONSTRUCT). O mapa do motor é por nome de
# opcode, então o compilador tem que falar a mesma release da VM.
set -euo pipefail

VERSAO="${LUAU_TAG:-367f9d83}"
DEST="${1:-$HOME/.cache/luaria}"
FONTE="${LUAU_FONTE:-$HOME/.cache/luaria/luau-src}"

mkdir -p "$DEST"
if [ ! -d "$FONTE/.git" ]; then
    git clone https://github.com/luau-lang/luau.git "$FONTE"
fi
git -C "$FONTE" fetch -q origin "$VERSAO" 2>/dev/null || true
git -C "$FONTE" checkout -q "$VERSAO"
TMPDIR="$DEST" make -C "$FONTE" config=release luau-compile >/dev/null
install -m 0755 "$FONTE/build/release/luau-compile" "$DEST/luau-compile"
echo "$DEST/luau-compile"
