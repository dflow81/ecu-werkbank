#!/usr/bin/env bash
# ecu-werkbank als AppImage bauen (eine Datei, läuft ohne Installation).
#
#   ./packaging/build-appimage.sh
#   → target/ecu-werkbank-<version>-x86_64.AppImage
#   ./packaging/build-appimage.sh --nur-appdir   → nur target/appimage/AppDir, nicht packen
#
# Start:  ./ecu-werkbank-<version>-x86_64.AppImage                 → Oberfläche
#         ./ecu-werkbank-<version>-x86_64.AppImage ecu-werkbank scan    → Terminal-Programm
#         ./ecu-werkbank-<version>-x86_64.AppImage mockecu can0     → Test-Steuergerät
#
# Das Pack-Werkzeug appimagetool wird beim ersten Mal von der offiziellen
# AppImage-Seite geladen (github.com/AppImage/appimagetool) und unter
# target/werkzeuge/ abgelegt. Ohne FUSE läuft es über APPIMAGE_EXTRACT_AND_RUN.
#
# Hinweis: Das AppImage braucht mindestens die glibc des Bau-Rechners
# (hier 2.39: Debian 13, Ubuntu 24.04, Linux Mint 22 und neuer).
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
ARCH=x86_64
APPDIR=target/appimage/AppDir
AUSGABE=target/ecu-werkbank-$VERSION-$ARCH.AppImage
WERKZEUG=target/werkzeuge/appimagetool-$ARCH.AppImage

echo "Baue ecu-werkbank $VERSION (Release)…"
cargo build --release --bins

echo "Stelle AppDir zusammen…"
rm -rf "$APPDIR"
install -Dm755 target/release/ecu-werkbank     "$APPDIR/usr/bin/ecu-werkbank"
install -Dm755 target/release/ecu-werkbank-gui "$APPDIR/usr/bin/ecu-werkbank-gui"
install -Dm755 target/release/mockecu      "$APPDIR/usr/bin/mockecu"

# Startmenü-Datei und Icon (im Wurzelverzeichnis verlangt appimagetool beides)
sed "/^Exec=/a X-AppImage-Version=$VERSION" packaging/ecu-werkbank.desktop > "$APPDIR/ecu-werkbank.desktop"
install -Dm644 "$APPDIR/ecu-werkbank.desktop" "$APPDIR/usr/share/applications/ecu-werkbank.desktop"
install -Dm644 packaging/ecu-werkbank.png "$APPDIR/ecu-werkbank.png"
install -Dm644 packaging/ecu-werkbank.png "$APPDIR/usr/share/icons/hicolor/256x256/apps/ecu-werkbank.png"
install -Dm644 packaging/ecu-werkbank.svg "$APPDIR/usr/share/icons/hicolor/scalable/apps/ecu-werkbank.svg"
ln -sf ecu-werkbank.png "$APPDIR/.DirIcon"

# Lizenzen, Quellen, Handbuch
DOC="$APPDIR/usr/share/doc/ecu-werkbank"
install -Dm644 LICENSE-MIT    "$DOC/LICENSE-MIT"
install -Dm644 LICENSE-APACHE "$DOC/LICENSE-APACHE"
install -Dm644 CREDITS.md     "$DOC/CREDITS.md"
install -Dm644 README.md      "$DOC/README.md"
install -Dm644 README.en.md   "$DOC/README.en.md"
[[ -f docs/handbuch/ecu-werkbank-Handbuch.pdf ]] && install -Dm644 docs/handbuch/ecu-werkbank-Handbuch.pdf "$DOC/ecu-werkbank-Handbuch.pdf"
[[ -f docs/handbuch/ecu-werkbank-Manual.pdf ]] && install -Dm644 docs/handbuch/ecu-werkbank-Manual.pdf "$DOC/ecu-werkbank-Manual.pdf"
# Schriften, die die Oberfläche (egui/epaint) mitbringt — deren Lizenzen gehören dazu
SCHRIFTEN=$(ls -d "${CARGO_HOME:-$HOME/.cargo}"/registry/src/*/epaint-0.28*/fonts 2>/dev/null | head -1)
if [[ -n "$SCHRIFTEN" ]]; then
  for f in UFL.txt OFL.txt Hack-Regular.txt emoji-icon-font-mit-license.txt; do
    install -Dm644 "$SCHRIFTEN/$f" "$DOC/schriften/$f"
  done
else
  echo "WARNUNG: Schrift-Lizenzen (epaint/fonts) nicht gefunden — bitte von Hand beilegen." >&2
fi

cat > "$APPDIR/AppRun" <<'EOF'
#!/bin/sh
# ecu-werkbank AppImage: ohne Argument die Oberfläche,
# „ecu-werkbank …“ bzw. „mockecu …“ starten die Terminal-Programme.
HIER="$(dirname "$(readlink -f "$0")")"
case "$(basename "${ARGV0:-$0}")" in
  ecu-werkbank) exec "$HIER/usr/bin/ecu-werkbank" "$@" ;;
esac
case "${1:-}" in
  ecu-werkbank) shift; exec "$HIER/usr/bin/ecu-werkbank" "$@" ;;
  mockecu)  shift; exec "$HIER/usr/bin/mockecu" "$@" ;;
  *)        exec "$HIER/usr/bin/ecu-werkbank-gui" "$@" ;;
esac
EOF
chmod 755 "$APPDIR/AppRun"

if [[ "${1:-}" == "--nur-appdir" ]]; then
  echo "AppDir fertig (nicht gepackt): $APPDIR"
  echo "Ausprobieren ohne Packen:  $APPDIR/AppRun"
  exit 0
fi

if command -v appimagetool >/dev/null; then
  TOOL=appimagetool
else
  if [[ ! -x "$WERKZEUG" ]]; then
    echo "Lade appimagetool…"
    mkdir -p "$(dirname "$WERKZEUG")"
    curl -fL --retry 3 -o "$WERKZEUG.part" \
      "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-$ARCH.AppImage"
    mv "$WERKZEUG.part" "$WERKZEUG"
    chmod 755 "$WERKZEUG"
  fi
  TOOL="$WERKZEUG"
fi

# Laufzeit-Baustein: liegt er schon vor (z. B. aus einem früheren AppImage
# gezogen: head -c $(alt.AppImage --appimage-offset) alt.AppImage > …), wird
# er benutzt — sonst lädt appimagetool ihn selbst (scheitert, wenn GitHub hakt).
RUNTIME=target/werkzeuge/runtime-$ARCH
EXTRA=()
[[ -f "$RUNTIME" ]] && EXTRA=(--runtime-file "$RUNTIME")

echo "Packe $AUSGABE…"
rm -f "$AUSGABE"
ARCH=$ARCH APPIMAGE_EXTRACT_AND_RUN=1 "$TOOL" --no-appstream "${EXTRA[@]}" "$APPDIR" "$AUSGABE"
echo "Fertig: $AUSGABE"
