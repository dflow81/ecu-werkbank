#!/usr/bin/env bash
# ecu-werkbank installieren (ohne Paketverwaltung).
#
#   ./packaging/install.sh            → für den eigenen Benutzer (~/.local), ohne sudo
#   ./packaging/install.sh --can      → zusätzlich CAN-Adapter automatisch einrichten (sudo)
#   ./packaging/install.sh --can --dfu SERIENNUMMER
#                                     → Adapter, der im Update-Modus (DFU) startet, automatisch
#                                       zurückholen (Seriennummer: lsusb -v -d 0483:df11 | grep iSerial)
#   ./packaging/install.sh --uninstall
set -euo pipefail
cd "$(dirname "$0")/.."

BIN="$HOME/.local/bin"
APPS="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
ICONS="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/scalable/apps"
RULE=/etc/udev/rules.d/80-ecu-werkbank-can.rules
DFU_RULE=/etc/udev/rules.d/81-ecu-werkbank-dfu.rules

CAN=0; DFU_SERIAL=""; UNINSTALL=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --can) CAN=1 ;;
    --dfu) DFU_SERIAL="${2:-}"; shift ;;
    --uninstall) UNINSTALL=1 ;;
    *) echo "Unbekannte Option: $1"; exit 1 ;;
  esac
  shift
done
if [[ -n "$DFU_SERIAL" && ! "$DFU_SERIAL" =~ ^[0-9A-Fa-f]+$ ]]; then
  echo "Seriennummer sieht falsch aus: $DFU_SERIAL"; exit 1
fi

if [[ $UNINSTALL == 1 ]]; then
  rm -f "$BIN/ecu-werkbank" "$BIN/ecu-werkbank-gui" "$BIN/mockecu" "$APPS/ecu-werkbank.desktop" "$ICONS/ecu-werkbank.svg"
  [[ -f $RULE || -f $DFU_RULE ]] && sudo rm -f "$RULE" "$DFU_RULE" && sudo udevadm control --reload
  echo "ecu-werkbank entfernt. (Eigene Labels in ~/.config/ecu-werkbank und Historie in ~/.local/share/ecu-werkbank bleiben erhalten.)"
  exit 0
fi

echo "Baue ecu-werkbank (Release)…"
cargo build --release --bins

install -Dm755 target/release/ecu-werkbank     "$BIN/ecu-werkbank"
install -Dm755 target/release/ecu-werkbank-gui "$BIN/ecu-werkbank-gui"
install -Dm755 target/release/mockecu      "$BIN/mockecu"
install -Dm644 packaging/ecu-werkbank.desktop  "$APPS/ecu-werkbank.desktop"
install -Dm644 packaging/ecu-werkbank.svg      "$ICONS/ecu-werkbank.svg"
command -v update-desktop-database >/dev/null && update-desktop-database "$APPS" 2>/dev/null || true

# Eigene Labels und Texte liegen in der Fahrzeug-Datenbank (Datenordner) — nichts zu kopieren.

if [[ $CAN == 1 ]]; then
  echo "Richte automatische CAN-Einrichtung ein (sudo)…"
  sudo install -Dm644 packaging/80-ecu-werkbank-can.rules "$RULE"
  if [[ -n "$DFU_SERIAL" ]]; then
    command -v dfu-util >/dev/null || { echo "dfu-util fehlt: sudo apt install dfu-util"; exit 1; }
    sed "s/@SERIAL@/$DFU_SERIAL/g" packaging/81-ecu-werkbank-dfu.rules.in | sudo tee "$DFU_RULE" >/dev/null
    echo "Adapter $DFU_SERIAL wird aus dem Update-Modus automatisch zurückgeholt."
  fi
  sudo udevadm control --reload
  echo "Ab jetzt wird ein eingesteckter CAN-Adapter automatisch mit 500 kbit/s aktiviert."
fi

echo
echo "Fertig. Start über das Menü („ecu-werkbank“) oder mit:  ecu-werkbank-gui"
case ":$PATH:" in *":$BIN:"*) ;; *) echo "Hinweis: $BIN ist nicht im PATH — im Terminal ggf. voll angeben.";; esac
