#!/usr/bin/env bash
# Instala o executor no perfil do usuário: binários em ~/.local/bin, atalho no
# menu e ícone no tema hicolor. Não precisa de root e não mexe em nada do sistema.
#
# Uso: tools/instalar.sh            (usa os binários de tools/build.sh)
#      BIN=/outro/dir tools/instalar.sh
set -euo pipefail
cd "$(dirname "$0")/.."

ORIGEM="${CARGO_TARGET_DIR:-$HOME/.cache/luaria-target}/release"
BIN="${BIN:-$HOME/.local/bin}"
DADOS="${XDG_DATA_HOME:-$HOME/.local/share}"
APPS="$DADOS/applications"
ICONES="$DADOS/icons/hicolor/scalable/apps"

for b in luaria luaria-cli; do
    if [ ! -x "$ORIGEM/$b" ]; then
        echo "falta $ORIGEM/$b — rode tools/build.sh primeiro" >&2
        exit 1
    fi
done

install -d "$BIN" "$APPS" "$ICONES"
install -m 0755 "$ORIGEM/luaria" "$BIN/luaria"
install -m 0755 "$ORIGEM/luaria-cli" "$BIN/luaria-cli"
if [ -x "$ORIGEM/luaria-gtk" ]; then
    install -m 0755 "$ORIGEM/luaria-gtk" "$BIN/luaria-gtk"
else
    echo "aviso: $ORIGEM/luaria-gtk não existe; a UI GTK pura ficou de fora" >&2
fi
install -m 0644 assets/luaria.svg "$ICONES/luaria.svg"

# PNGs para menus/docks que não leem SVG (o hicolor espera bitmap nos tamanhos)
converter_svg() { # $1 origem, $2 destino, $3 lado em px
    local origem="$1" destino="$2" lado="$3"
    if command -v rsvg-convert >/dev/null 2>&1; then
        rsvg-convert -w "$lado" -h "$lado" "$origem" -o "$destino"
    elif command -v magick >/dev/null 2>&1; then
        magick -background none "$origem" -resize "${lado}x${lado}" "$destino"
    elif command -v inkscape >/dev/null 2>&1; then
        inkscape "$origem" -w "$lado" -h "$lado" -o "$destino"
    elif python3 -c 'import cairosvg' >/dev/null 2>&1; then
        python3 -c 'import cairosvg,sys; cairosvg.svg2png(url=sys.argv[1], write_to=sys.argv[2], output_width=int(sys.argv[3]), output_height=int(sys.argv[3]))' "$origem" "$destino" "$lado"
    else
        return 1
    fi
}

PNGS=0
for lado in 48 64 128 256; do
    destino="$DADOS/icons/hicolor/${lado}x${lado}/apps"
    install -d "$destino"
    if converter_svg assets/luaria.svg "$destino/luaria.png" "$lado"; then
        PNGS=$((PNGS + 1))
    else
        echo "aviso: nenhum conversor de SVG (rsvg-convert/magick/inkscape/cairosvg); só o SVG cobre" >&2
        break
    fi
done

# o nome do .desktop tem que ser o application-id: é assim que o compositor
# (app_id no Wayland, StartupWMClass no X11) associa a janela ao atalho e ao ícone
DESKTOP="$APPS/dev.luaria.executor.desktop"
rm -f "$APPS/luaria.desktop"
cat > "$DESKTOP" <<EOF
[Desktop Entry]
Type=Application
Name=Luaria
GenericName=Script executor
Comment=Runs Luau scripts in the open Roblox client
Exec=$BIN/luaria
TryExec=$BIN/luaria
Icon=luaria
Terminal=false
Categories=Development;
Keywords=roblox;luau;script;executor;
StartupNotify=true
StartupWMClass=dev.luaria.executor
EOF
chmod 0644 "$DESKTOP"

# índices são cacheados; sem isso o ícone só aparece depois de relogar
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database -q "$APPS" || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -q -t -f "$DADOS/icons/hicolor" || true
fi

echo "instalado:"
echo "  $BIN/luaria (webview)"
if [ -x "$BIN/luaria-gtk" ]; then echo "  $BIN/luaria-gtk (fallback GTK puro)"; fi
echo "  $BIN/luaria-cli"
echo "  $ICONES/luaria.svg"
if [ "$PNGS" -gt 0 ]; then
    echo "  $DADOS/icons/hicolor/(48, 64, 128, 256) apps/luaria.png ($PNGS tamanhos)"
fi
echo "  $DESKTOP"

case ":$PATH:" in
    *":$BIN:"*) ;;
    *) echo "aviso: $BIN não está no PATH; chame por caminho absoluto ou ajuste o PATH" >&2 ;;
esac
