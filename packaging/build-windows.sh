#!/usr/bin/env bash
# ecu-werkbank für Windows bauen (Light-Version: ELM327, Demo, alles ohne Auto —
# kein SocketCAN). Wird unter Linux quer übersetzt.
#
#   ./packaging/build-windows.sh
#   → target/ecu-werkbank-<version>-windows-x86_64.zip
#
# Einmalig nötig:
#   rustup target add x86_64-pc-windows-gnu
#   sudo apt install mingw-w64
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
ZIEL=x86_64-pc-windows-gnu
NAME=ecu-werkbank-$VERSION-windows-x86_64
ORDNER=target/windows/$NAME
AUSGABE=target/$NAME.zip

echo "Baue ecu-werkbank $VERSION für Windows…"
cargo build --release --target "$ZIEL" --bin ecu-werkbank --bin ecu-werkbank-gui

echo "Stelle $ORDNER zusammen…"
rm -rf "$ORDNER"
mkdir -p "$ORDNER/schriften"
cp "target/$ZIEL/release/ecu-werkbank.exe" "target/$ZIEL/release/ecu-werkbank-gui.exe" "$ORDNER/"
# Textdateien mit Windows-Zeilenenden
for f in packaging/LIESMICH-Windows.txt LICENSE-MIT LICENSE-APACHE CREDITS.md; do
  sed 's/$/\r/' "$f" > "$ORDNER/$(basename "$f")"
done
[[ -f docs/handbuch/ecu-werkbank-Handbuch.pdf ]] && cp docs/handbuch/ecu-werkbank-Handbuch.pdf "$ORDNER/"
# Schriften, die die Oberfläche (egui/epaint) mitbringt — deren Lizenzen gehören dazu
SCHRIFTEN=$(ls -d "${CARGO_HOME:-$HOME/.cargo}"/registry/src/*/epaint-0.28*/fonts 2>/dev/null | head -1)
if [[ -n "$SCHRIFTEN" ]]; then
  for f in UFL.txt OFL.txt Hack-Regular.txt emoji-icon-font-mit-license.txt; do
    sed 's/$/\r/' "$SCHRIFTEN/$f" > "$ORDNER/schriften/$f"
  done
else
  echo "WARNUNG: Schrift-Lizenzen (epaint/fonts) nicht gefunden — bitte von Hand beilegen." >&2
fi

echo "Packe $AUSGABE…"
rm -f "$AUSGABE"
python3 -I - "$ORDNER" "$AUSGABE" <<'EOF'
import os, sys, zipfile
ordner, ausgabe = sys.argv[1], sys.argv[2]
basis = os.path.dirname(ordner)
with zipfile.ZipFile(ausgabe, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as z:
    for wurzel, _, dateien in os.walk(ordner):
        for d in sorted(dateien):
            p = os.path.join(wurzel, d)
            z.write(p, os.path.relpath(p, basis))
EOF
echo "Fertig: $AUSGABE"
