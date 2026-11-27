#!/usr/bin/env bash
# Erzeugt die Bildschirmfotos für das Handbuch neu — mit der Golf-6-Simulation und
# einem erfundenen Beispielfahrzeug (M-AB 123, WVWZZZ1KZAM000017). Es kommen keine
# echten Daten ins Bild: Datenordner, Labels und Einstellungen liegen in einem
# eigenen, leeren Arbeitsordner.
#
# Aufruf (aus dem Projektordner, nach `cargo build --release`):
#   docs/handbuch/bilder_erzeugen.sh [Arbeitsordner] [Seite …]
# Ohne Seiten werden alle Seiten fotografiert. Ergebnis: <Arbeitsordner>/bilder/*.png
# Englische Bilder: VAG_SPRACHE=en davorsetzen (eigener Arbeitsordner, Ziel bilder-en/).
# Erst ansehen, dann die guten Bilder nach docs/handbuch/bilder/ kopieren und
#   python3 docs/handbuch/build_handbuch.py
# aufrufen. Braucht ein laufendes X (DISPLAY), python3-pil, pdftoppm (poppler-utils)
# und für das Handy-Bild chromium. Seite „pdf“ = PDF-Bilder und Handy-Bild, „handy“ = nur Handy-Bild.
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
    # Beispieldaten immer deutsch anlegen (gespeicherte Namen bleiben deutsch, siehe README)
    "$CLI" --sprache de fahrzeug-neu --kennzeichen "M-AB 123" --vin "$VIN" --name "Golf 6 1.4 TSI" >/dev/null
    "$CLI" --sprache de -f "$VIN" wartung-vorlage >/dev/null
    for _ in 1 2 3; do
        "$CLI" --sprache de -t golf6 -f "$VIN" report --out "$ARBEIT/scan.txt" >/dev/null 2>&1
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

# 1b. Eigene Feldnamen und zwei Favoriten für das Bild „favoriten“/„namen“ (je Sprache)
if [ "${VAG_SPRACHE:-de}" = en ]; then
    N=("Load and ignition" "Engine speed" "Engine load" "Basic values" "Load and ignition")
else
    N=("Last und Zündung" "Drehzahl" "Motorlast" "Grundwerte" "Last und Zündung")
fi
"$CLI" messwerttext --adresse 01 --block 2 --text "${N[0]}" >/dev/null
"$CLI" messwerttext --adresse 01 --block 2 --feld 1 --text "${N[1]}" >/dev/null
"$CLI" messwerttext --adresse 01 --block 2 --feld 2 --text "${N[2]}" >/dev/null
python3 -I - "$VAG_DATA_DIR/fahrzeugdaten.sqlite" "${N[3]}" "${N[4]}" <<'EOF2'
import sqlite3, sys, datetime
c = sqlite3.connect(sys.argv[1])
t = "".join(f'[[favorit]]\nadresse = 1\nblock = {b}\nname = "{n}"\n\n' for b, n in ((1, sys.argv[2]), (2, sys.argv[3])))
c.execute("delete from ablage where pfad = 'messwert_favoriten.toml'")
c.execute("insert into ablage(pfad, inhalt, geaendert) values ('messwert_favoriten.toml', ?, ?)",
          (t, datetime.datetime.now().strftime("%d.%m.%Y %H:%M")))
c.commit()
EOF2

# 2. Fotos: die GUI fotografiert sich selbst (VAG_FOTO) und schließt sich danach
mkdir -p "$ARBEIT/bilder"
SEITEN=("$@")
[ ${#SEITEN[@]} -gt 0 ] || SEITEN=(start steuergeraete obd tp20 favoriten grundeinstellung batterie
    scope armaturen handy-qr diagnose fahrzeuge service historie software sicherung testlauf sicherheit
    canmonitor skripte ident lexikon datenbank datenbank-vorschau datenbank-inhalt namen editor
    mitschnitt rueckfrage pdf)
for seite in "${SEITEN[@]}"; do
    case "$seite" in pdf|handy) continue ;; esac
    tab="$seite"; warten=6
    case "$seite" in
        service) tab=service-anzeige ;;
        rueckfrage) tab=service-frage ;;
        handy-qr) tab=handy; warten=10 ;;
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

# 3. PDF-Berichte (erste Seite als Bild) und die Handy-Ansicht im Browser
pdf_bild() {  # <pdf> <name>
    if [ -s "$1" ] && pdftoppm -r 110 -f 1 -l 1 -png -singlefile "$1" "$ARBEIT/bilder/$2"; then
        echo "✓ $2"
    else
        echo "✗ $2 (kein PDF)"
    fi
}
neuestes_pdf() { ls -t "$VAG_DATA_DIR/fahrzeuge/$VIN/berichte/"*.pdf 2>/dev/null | head -1; }
if printf '%s\n' "${SEITEN[@]}" | grep -qx pdf; then
    export VAG_FAHRZEUG="$VIN"
    # Firmenkopf mit erfundener Werkstatt und einfachem Logo (roter Kreis, blauer Balken)
    python3 -I -c "
import sys
from PIL import Image, ImageDraw
im = Image.new('RGB', (360, 120), 'white'); d = ImageDraw.Draw(im)
d.ellipse((20, 20, 100, 100), fill=(204, 34, 34)); d.rectangle((130, 45, 340, 75), fill=(31, 58, 120))
im.save(sys.argv[1])" "$ARBEIT/logo.png"
    if [ "${VAG_SPRACHE:-de}" = en ]; then
        "$CLI" firmenkopf --name "Example Garage Ltd" --adresse "1 Example Street, Exampletown" \
            --telefon "0123 456789" --email "info@example.org" --logo "$ARBEIT/logo.png" >/dev/null
    else
        "$CLI" firmenkopf --name "Kfz-Werkstatt Muster" --adresse "Musterstraße 1, 12345 Musterstadt" \
            --telefon "0123 456789" --email "info@example.org" --logo "$ARBEIT/logo.png" >/dev/null
    fi
    T=(-t golf6)
    "$CLI" "${T[@]}" report --out "$ARBEIT/bericht.pdf" --no-history >/dev/null 2>&1
    pdf_bild "$ARBEIT/bericht.pdf" bericht
    "$CLI" "${T[@]}" kaufcheck --out "$ARBEIT/kaufcheck.pdf" >/dev/null 2>&1
    pdf_bild "$ARBEIT/kaufcheck.pdf" pdf-kaufcheck
    "$CLI" akte --out "$ARBEIT/akte.pdf" >/dev/null 2>&1
    pdf_bild "$ARBEIT/akte.pdf" pdf-fahrzeugakte
    "$CLI" "${T[@]}" --demo-muster "drehzahl=schwanken:800:3500:20;geschwindigkeit=rampe:0:80:8" \
        obd-live --pids 0C,0D,05,0B,11 --count 100 --interval 250 --csv "$ARBEIT/fahrt.csv" >/dev/null 2>&1
    "$CLI" csv-pdf "$ARBEIT/fahrt.csv" --out "$ARBEIT/messwerte.pdf" >/dev/null 2>&1
    pdf_bild "$ARBEIT/messwerte.pdf" pdf-messwerte
    "$CLI" "${T[@]}" reparaturbericht --arbeiten "$( [ "${VAG_SPRACHE:-de}" = en ] && echo "Ignition coil cyl. 3 replaced" || echo "Zündspule Zyl. 3 erneuert")" >/dev/null 2>&1
    pdf_bild "$(neuestes_pdf)" pdf-reparaturbericht
    "$CLI" "${T[@]}" nachruestung --name AHK --phase vorher >/dev/null 2>&1
    "$CLI" "${T[@]}" nachruestung --name AHK --phase nachher >/dev/null 2>&1
    pdf_bild "$(neuestes_pdf)" pdf-nachruestung
    "$CLI" "${T[@]}" notfall-sicherung >/dev/null 2>&1
    pdf_bild "$(neuestes_pdf)" pdf-ersatzteilpass
fi
if printf '%s\n' "${SEITEN[@]}" | grep -qxE 'pdf|handy'; then
    # Handy-Ansicht: Terminal-Server starten, Seite mit chromium fotografieren
    export VAG_FAHRZEUG="$VIN"; T=(-t golf6)
    "$CLI" "${T[@]}" handy --port 8099 --seconds 40 > "$ARBEIT/handy.txt" 2>&1 &
    HPID=$!
    for _ in $(seq 1 20); do grep -qo 'http://[^ ]*' "$ARBEIT/handy.txt" && break; sleep 0.5; done
    URL=$(grep -o 'http://[^ ]*:8099/[^ ]*' "$ARBEIT/handy.txt" | head -1 | sed 's#http://[^/]*#http://127.0.0.1:8099#')
    if [ -n "$URL" ]; then
        sleep 3
        chromium --headless=new --disable-gpu --hide-scrollbars --no-first-run --user-data-dir="$ARBEIT/chromium" \
            --force-device-scale-factor=3 --window-size=360,400 --virtual-time-budget=4000 \
            --screenshot="$ARBEIT/bilder/handy.png" "$URL" >/dev/null 2>&1 && echo "✓ handy" || echo "✗ handy"
    else
        echo "✗ handy (keine Adresse)"
    fi
    kill "$HPID" 2>/dev/null || true
fi
echo "Bilder: $ARBEIT/bilder"
