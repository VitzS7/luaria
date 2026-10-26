#!/usr/bin/env bash
# Compila o executor. O GTK4 e o WebKitGTK 6.0 vêm de um sysroot local
# (~/tools/gtk4-sysroot), extraído dos RPMs de desenvolvimento — nada é
# instalado no sistema.
set -euo pipefail
cd "$(dirname "$0")/.."
SYS="${GTK4_SYSROOT:-$HOME/tools/gtk4-sysroot}"
export PKG_CONFIG_PATH="$SYS/usr/lib64/pkgconfig:$SYS/usr/share/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cache/luaria-target}"

# O bundle da UI é versionado; regerar aqui evita binário com app.js velho.
if [ -x assets/ui/node_modules/.bin/esbuild ]; then
    ( cd assets/ui && ./node_modules/.bin/esbuild src/app.js \
        --bundle --minify --format=iife --target=es2020 --outfile=app.js >/dev/null )
fi

cargo build --release "$@"
echo "binários em $CARGO_TARGET_DIR/release: luaria (webview), luaria-gtk (GTK4 puro), luaria-cli"
