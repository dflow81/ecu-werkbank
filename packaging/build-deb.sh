#!/usr/bin/env bash
# Baut ein .deb-Paket (Debian, Ubuntu, Linux Mint):  ./packaging/build-deb.sh
# Installieren:  sudo apt install ./target/ecu-werkbank_<version>_amd64.deb
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
ARCH=$(dpkg --print-architecture)
STAGE=target/deb/ecu-werkbank_${VERSION}_${ARCH}

cargo build --release --bins
rm -rf "$STAGE"
install -Dm755 target/release/ecu-werkbank          "$STAGE/usr/bin/ecu-werkbank"
install -Dm755 target/release/ecu-werkbank-gui      "$STAGE/usr/bin/ecu-werkbank-gui"
install -Dm755 target/release/mockecu           "$STAGE/usr/bin/ecu-werkbank-mockecu"
install -Dm644 packaging/ecu-werkbank.desktop       "$STAGE/usr/share/applications/ecu-werkbank.desktop"
install -Dm644 packaging/ecu-werkbank.svg           "$STAGE/usr/share/icons/hicolor/scalable/apps/ecu-werkbank.svg"
install -Dm644 packaging/80-ecu-werkbank-can.rules  "$STAGE/usr/lib/udev/rules.d/80-ecu-werkbank-can.rules"
install -Dm644 labels/beispiel.toml             "$STAGE/usr/share/doc/ecu-werkbank/beispiel-labels.toml"
install -Dm644 README.md                        "$STAGE/usr/share/doc/ecu-werkbank/README.md"
install -Dm644 packaging/ecu-werkbank.png           "$STAGE/usr/share/icons/hicolor/256x256/apps/ecu-werkbank.png"
# Lizenzen und Quellen (bei Weitergabe Pflicht)
DOC="$STAGE/usr/share/doc/ecu-werkbank"
install -Dm644 LICENSE-MIT    "$DOC/LICENSE-MIT"
install -Dm644 LICENSE-APACHE "$DOC/LICENSE-APACHE"
install -Dm644 CREDITS.md     "$DOC/CREDITS.md"
[[ -f docs/handbuch/ecu-werkbank-Handbuch.pdf ]] && install -Dm644 docs/handbuch/ecu-werkbank-Handbuch.pdf "$DOC/ecu-werkbank-Handbuch.pdf"
cat > "$DOC/copyright" <<'COPY'
ecu-werkbank
Copyright (c) 2026 dflow81
Lizenz: MIT oder Apache-2.0 (wahlweise), siehe LICENSE-MIT und LICENSE-APACHE.
Quellen der Login-Prüfwerte und deren Lizenzen: siehe CREDITS.md.
Mitgelieferte Schriften (egui/epaint): Lizenztexte im Unterordner schriften/.
COPY
SCHRIFTEN=$(ls -d "${CARGO_HOME:-$HOME/.cargo}"/registry/src/*/epaint-0.28*/fonts 2>/dev/null | head -1)
if [[ -n "$SCHRIFTEN" ]]; then
  for f in UFL.txt OFL.txt Hack-Regular.txt emoji-icon-font-mit-license.txt; do
    install -Dm644 "$SCHRIFTEN/$f" "$DOC/schriften/$f"
  done
else
  echo "WARNUNG: Schrift-Lizenzen (epaint/fonts) nicht gefunden — bitte von Hand beilegen." >&2
fi
strip "$STAGE/usr/bin/"* 2>/dev/null || true

SIZE=$(du -sk "$STAGE" | cut -f1)
mkdir -p "$STAGE/DEBIAN"
cat > "$STAGE/DEBIAN/control" <<CTRL
Package: ecu-werkbank
Version: $VERSION
Architecture: $ARCH
Maintainer: dflow81 <noreply@localhost>
Installed-Size: $SIZE
Depends: libc6, iproute2
Recommends: libgl1, libxkbcommon-x11-0, xdg-desktop-portal
Conflicts: vag-diag
Replaces: vag-diag
Section: utils
Priority: optional
Homepage: https://github.com/dflow81/ecu-werkbank
Description: ECUWerkbank — offene Fahrzeugdiagnose über SocketCAN
 Fehlerspeicher (UDS und TP 2.0/KWP2000), Messwerte, OBD-II, Codierungs-
 Sicherung, Mitschnitt-Auswertung und Fehlerspeicher-PDF. Mit eingebautem
 Demo-Fahrzeug zum Ausprobieren ohne Adapter. CAN-Adapter werden beim
 Einstecken automatisch mit 500 kbit/s eingerichtet.
CTRL
cat > "$STAGE/DEBIAN/postinst" <<'POST'
#!/bin/sh
set -e
command -v udevadm >/dev/null && udevadm control --reload || true
command -v update-desktop-database >/dev/null && update-desktop-database -q || true
POST
chmod 755 "$STAGE/DEBIAN/postinst"

dpkg-deb --root-owner-group --build "$STAGE" "target/ecu-werkbank_${VERSION}_${ARCH}.deb"
echo "Paket: target/ecu-werkbank_${VERSION}_${ARCH}.deb"
echo "Installieren:  sudo apt install ./target/ecu-werkbank_${VERSION}_${ARCH}.deb"
