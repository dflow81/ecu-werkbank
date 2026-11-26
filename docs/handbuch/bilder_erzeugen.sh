#!/usr/bin/env bash
# Erzeugt die Bildschirmfotos für das Handbuch neu — mit der Golf-6-Simulation und
# einem erfundenen Beispielfahrzeug (M-AB 123, WVWZZZ1KZAM000017). Es kommen keine
# echten Daten ins Bild: Datenordner, Labels und Einstellungen liegen in einem
# eigenen, leeren Arbeitsordner.
#
# Aufruf (aus dem Projektordner, nach `cargo build --release`):
#   docs/handbuch/bilder_erzeugen.sh [Arbeitsordner] [Seite …]
# Ohne Seiten werden alle Seiten fotografiert. Ergebnis: <Arbeitsordner>/bilder/*.png
# Erst ansehen, dann die guten Bilder nach docs/handbuch/bilder/ kopieren und
#   python3 docs/handbuch/build_handbuch.py
# aufrufen. Braucht ein laufendes X (DISPLAY) und python3-pil.
set -euo pipefail

PROJEKT="$(cd "$(dirname "$0")/../.." && pwd)"
ARBEIT="${1:-${TMPDIR:-/tmp}/ecu-werkbank-handbuch}"
shift || true
CLI="$PROJEKT/target/release/ecu-werkbank"
GUI="$PROJEKT/target/release/ecu-werkbank-gui"
VIN=WVWZZZ1KZAM000017

[ -x "$CLI" ] && [ -x "$GUI" ] || { echo "Erst bauen: cargo build --release" >&2; exit 1; }
[ -n "${DISPLAY:-}" ] || export DISPLAY=:0

export VAG_DATA_DIR="$ARBEIT/daten" VAG_LABELS="$ARBEIT/labels" HOME="$ARBEIT/home" \
       XDG_CONFIG_HOME="$ARBEIT/home" XDG_DATA_HOME="$ARBEIT/home"

# 1. Beispielfahrzeug mit Wartungsplan und drei Scans (nur beim ersten Mal)
if [ ! -e "$VAG_DATA_DIR/fahrzeugdaten.sqlite" ]; then
    mkdir -p "$VAG_DATA_DIR" "$VAG_LABELS" "$HOME"
    "$CLI" fahrzeug-neu --kennzeichen "M-AB 123" --vin "$VIN" --name "Golf 6 1.4 TSI" >/dev/null
    "$CLI" -f "$VIN" wartung-vorlage >/dev/null
    for _ in 1 2 3; do
        "$CLI" -t golf6 -f "$VIN" report --out "$ARBEIT/scan.txt" >/dev/null 2>&1
        sleep 1.1
    done
    # Scans auf drei Termine im Jahr verteilen, Ölwechsel und HU eintragen
    python3 -I - "$VAG_DATA_DIR/fahrzeugdaten.sqlite" "$VIN" <<'EOF'
import sqlite3, sys, re, datetime
db, vin = sys.argv[1], sys.argv[2]
c = sqlite3.connect(db)
basis = f"fahrzeuge/{vin}/"
termine = [("02.03.2026", 236100), ("01.07.2026", 239900), ("29.10.2026", 242480)]
scans = sorted(p for (p,) in c.execute("select pfad from ablage where pfad like ?", (basis + "historie/%",)))
alt_neu = {}
for pfad, (datum, km) in zip(scans, termine):
    (t,) = c.execute("select inhalt from ablage where pfad=?", (pfad,)).fetchone()
    ts = int(datetime.datetime.strptime(datum + " 10:00", "%d.%m.%Y %H:%M").timestamp())
    alt_neu[int(re.search(r"(\d+)\.toml$", pfad).group(1))] = (ts, datum, km)
    t = re.sub(r'^created = ".*"$', f'created = "{datum} 10:00"', t, count=1, flags=re.M)
    t = re.sub(r'^timestamp = \d+$', f"timestamp = {ts}", t, count=1, flags=re.M)
    t = re.sub(r'^km = \d+$', f"km = {km}", t, count=1, flags=re.M)
    c.execute("delete from ablage where pfad=?", (pfad,))
    c.execute("insert into ablage(pfad, inhalt, geaendert) values (?,?,?)",
              (f"{basis}historie/{ts}.toml", t, f"{datum} 10:00"))
# Trendpunkte passend umschreiben
row = c.execute("select inhalt from ablage where pfad=?", (basis + "trends.toml",)).fetchone()
if row:
    t = row[0]
    for alt, (ts, datum, km) in alt_neu.items():
        t = re.sub(rf'ts = {alt}\ndatum = ".*"\nkm = \d+',
                   f'ts = {ts}\ndatum = "{datum} 10:00"\nkm = {km}', t)
    c.execute("update ablage set inhalt=? where pfad=?", (t, basis + "trends.toml"))
(v,) = c.execute("select inhalt from ablage where pfad=?", (basis + "fahrzeug.toml",)).fetchone()
v = v.replace('name = "Ölwechsel + Ölfilter"\nkm_intervall = 15000\nmonate = 12\nletztes_datum = ""',
              'name = "Ölwechsel + Ölfilter"\nkm_intervall = 15000\nmonate = 12\nletzte_km = 229000\nletztes_datum = "10.11.2025"')
v = re.sub(r'(name = "HU / AU"\nmonate = 24\n)letztes_datum = ""', r'\1letztes_datum = "03.03.2025"', v)
v = re.sub(r'^km = \d+$', "km = 243100", v, count=1, flags=re.M)
c.execute("update ablage set inhalt=? where pfad=?", (v, basis + "fahrzeug.toml"))
c.commit()
EOF
fi

# 2. Fotos: die GUI fotografiert sich selbst (VAG_FOTO) und schließt sich danach
mkdir -p "$ARBEIT/bilder"
SEITEN=("$@")
[ ${#SEITEN[@]} -gt 0 ] || SEITEN=(start steuergeraete obd tp20 favoriten grundeinstellung batterie
    scope armaturen diagnose fahrzeuge service historie software sicherung testlauf sicherheit
    canmonitor skripte ident lexikon datenbank namen editor mitschnitt)
for seite in "${SEITEN[@]}"; do
    tab="$seite"; warten=6
    case "$seite" in
        service) tab=service-anzeige ;;
        tp20|favoriten|grundeinstellung) warten=16 ;;
        scope|armaturen|canmonitor) warten=10 ;;
    esac
    ppm="$ARBEIT/bilder/$seite.ppm"
    rm -f "$ppm"
    VAG_FAHRZEUG="$VIN" VAG_TRANSPORT=golf6 VAG_HELL=1 VAG_FENSTER=1000x880 VAG_TAB="$tab" \
        VAG_FOTO="$ppm" VAG_FOTO_WARTEN="$warten" timeout $((warten + 30)) "$GUI" >/dev/null 2>&1 </dev/null || true
    if [ -s "$ppm" ]; then
        python3 -I -c "import sys; from PIL import Image; Image.open(sys.argv[1]).save(sys.argv[2])" \
            "$ppm" "$ARBEIT/bilder/$seite.png" && rm -f "$ppm"
        echo "✓ $seite"
    else
        echo "✗ $seite (kein Bild)"
    fi
done
echo "Bilder: $ARBEIT/bilder"
