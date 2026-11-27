#!/usr/bin/env bash
# ecu-werkbank für macOS bauen (Light-Version: ELM327, Demo, alles ohne Auto —
# kein SocketCAN). Läuft nur auf einem Mac (Apple-SDK); ohne eigenen Mac
# über GitHub Actions: .github/workflows/mac.yml
#
#   ./packaging/build-mac.sh
#   → target/ecu-werkbank-<version>-macos.zip  (Universal: Intel + Apple-Chip)
#
# Einmalig nötig:
#   rustup target add aarch64-apple-darwin x86_64-apple-darwin
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
NAME=ecu-werkbank-$VERSION-macos
ORDNER=target/mac/$NAME
APP=$ORDNER/ecu-werkbank.app
AUSGABE=target/$NAME.zip

for ZIEL in aarch64-apple-darwin x86_64-apple-darwin; do
  echo "Baue ecu-werkbank $VERSION für ${ZIEL}…"
  cargo build --release --target "$ZIEL" --bin ecu-werkbank --bin ecu-werkbank-gui
done

echo "Stelle ${ORDNER} zusammen…"
rm -rf "$ORDNER"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" "$ORDNER/schriften"
# Universal-Programme (beide Chips in einer Datei)
lipo -create -output "$APP/Contents/MacOS/ecu-werkbank-gui" \
  target/aarch64-apple-darwin/release/ecu-werkbank-gui target/x86_64-apple-darwin/release/ecu-werkbank-gui
lipo -create -output "$ORDNER/ecu-werkbank" \
  target/aarch64-apple-darwin/release/ecu-werkbank target/x86_64-apple-darwin/release/ecu-werkbank

# Symbol aus dem PNG erzeugen
ICONSET=target/mac/ecu-werkbank.iconset
rm -rf "$ICONSET"
mkdir -p "$ICONSET"
for g in 16 32 128 256 512; do
  sips -z $g $g packaging/ecu-werkbank.png --out "$ICONSET/icon_${g}x${g}.png" >/dev/null
  sips -z $((g * 2)) $((g * 2)) packaging/ecu-werkbank.png --out "$ICONSET/icon_${g}x${g}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/ecu-werkbank.icns"

cat > "$APP/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>ECUWerkbank</string>
  <key>CFBundleDisplayName</key><string>ECUWerkbank</string>
  <key>CFBundleIdentifier</key><string>io.github.dflow81.ecu-werkbank</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleExecutable</key><string>ecu-werkbank-gui</string>
  <key>CFBundleIconFile</key><string>ecu-werkbank</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
EOF

# Ohne Apple-Entwicklerkonto: nur „ad hoc“ signieren (nötig für Apple-Chips)
codesign --force --deep --sign - "$APP"
codesign --force --sign - "$ORDNER/ecu-werkbank"

cp packaging/LIESMICH-Mac.txt LICENSE-MIT LICENSE-APACHE CREDITS.md "$ORDNER/"
[[ -f docs/handbuch/ecu-werkbank-Handbuch.pdf ]] && cp docs/handbuch/ecu-werkbank-Handbuch.pdf "$ORDNER/"
[[ -f docs/handbuch/ecu-werkbank-Manual.pdf ]] && cp docs/handbuch/ecu-werkbank-Manual.pdf "$ORDNER/"
SCHRIFTEN=$(ls -d "${CARGO_HOME:-$HOME/.cargo}"/registry/src/*/epaint-0.28*/fonts 2>/dev/null | head -1)
if [[ -n "$SCHRIFTEN" ]]; then
  for f in UFL.txt OFL.txt Hack-Regular.txt emoji-icon-font-mit-license.txt; do
    cp "$SCHRIFTEN/$f" "$ORDNER/schriften/$f"
  done
else
  echo "WARNUNG: Schrift-Lizenzen (epaint/fonts) nicht gefunden — bitte von Hand beilegen." >&2
fi

echo "Packe ${AUSGABE}…"
rm -f "$AUSGABE"
# ditto erhält die Mac-Dateirechte und Signaturen
ditto -c -k --keepParent "$ORDNER" "$AUSGABE"
echo "Fertig: $AUSGABE"
