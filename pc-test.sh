#!/usr/bin/env bash
# PC-Test ohne Hardware: virtuelles CAN + Mock-ECU + Diagnose-Befehle.
# Aufruf:  ./pc-test.sh
set -e
cd "$(dirname "$0")"
export PATH="$HOME/.cargo/bin:$PATH"

# 1) Virtuelles CAN einrichten (einmalig; braucht sudo)
if ! ip link show vcan0 >/dev/null 2>&1; then
  echo "Richte vcan0 ein (sudo)…"
  sudo modprobe vcan
  sudo ip link add dev vcan0 type vcan
  sudo ip link set up vcan0
fi

# 2) Bauen
cargo build --bin ecu-werkbank --bin mockecu >/dev/null 2>&1

# 3) Mock-ECU im Hintergrund starten
./target/debug/mockecu vcan0 >/dev/null 2>&1 &
MOCK=$!
trap "kill $MOCK 2>/dev/null" EXIT
sleep 1

run() { echo; echo "\$ ecu-werkbank $*"; ./target/debug/ecu-werkbank -i vcan0 "$@"; }

# 4) Durchspielen
run detect
run scan
run rdid --tx 0x7E0 --rx 0x7E8 --did 0xF190     # VIN
run rdid --tx 0x7E0 --rx 0x7E8 --did 0xF187     # Teilenummer
run rdid --tx 0x7E0 --rx 0x7E8 --did 0xF40C     # Messwert (Label-skaliert)
run dtc  --tx 0x7E0 --rx 0x7E8                   # Fehlercodes mit Klartext
run coding --tx 0x7E0 --rx 0x7E8 --did 0x0600 --ecu 0x09
run clear --tx 0x7E0 --rx 0x7E8                  # Fehler löschen
run kwp-ident --address 0x01                     # TP 2.0: Identifikation
run kwp-dtc   --address 0x01                     # TP 2.0: Fehlerspeicher
run kwp-block --address 0x01 --block 1 --count 3 # TP 2.0: Messwertblock
run autoscan                                     # UDS + TP 2.0
run obd-info                                     # OBD-II: VIN, Readiness
run obd-live --count 2                           # OBD-II: Live-Daten
run record --out /tmp/vag-mitschnitt.log --seconds 2
run decode-log /tmp/vag-mitschnitt.log --report-only

echo; echo "Fertig. (Mock-ECU wird beendet.)"
