# ECUWerkbank

**Diagnose & Codierung für deutsche Autos** — offenes Werkzeug über **SocketCAN**, in Rust.
Geeignet für Fahrzeuge von VW, Audi, Seat und Škoda. Eigenständige
Implementierung der veröffentlichten Standard-Protokolle:

- **ISO 15765-2** (ISO-TP) – Transport, selbst implementiert
- **ISO 14229** (UDS) – Diagnose-Services
- **ISO 15765-4** – CAN-Adressierung (OBD)

> **Neuer Name:** Das Projekt hieß früher **vag-diag**. Vorhandene Daten (Fahrzeuge, Scans,
> Sicherungen, eigene Labels) übernimmt ECUWerkbank beim ersten Start automatisch,
> alte Datensicherungen lassen sich weiter einspielen.

Kein proprietärer Code, keine lizenzgebundene Hardware. Funktioniert mit jedem
SocketCAN-Adapter (CANable/candleLight, PCAN, MCP2515-HAT, …).

> **⚠ Benutzung auf eigene Gefahr.** ECUWerkbank kann in Steuergeräte schreiben
> (Codierung, Anpassungen, Service-Rückstellung, Grundeinstellung, Stellglieder).
> Falsche Werte können Funktionen am Fahrzeug verstellen oder abschalten. Vor jedem
> Schreiben kommt eine Rückfrage (im Terminal: ohne `--ja` nur Vorschau), Codierung
> und Anpassungen werden vorher gesichert. Trotzdem gilt: nur am stehenden Auto, nur
> wenn du weißt, was der Wert bewirkt. Es gibt keine Gewährleistung (siehe Lizenz).
>
> **Kilometerstand:** ECUWerkbank liest den Kilometerstand, schreibt ihn aber **nie** —
> jeder Schreibweg lehnt die km-Kennungen ab (Tachomanipulation ist strafbar,
> § 22b StVG).

## Handbuch

Ausführliche Anleitung mit Bildern (Deutsch, einfache Sprache):
[docs/handbuch/ecu-werkbank-Handbuch.pdf](docs/handbuch/ecu-werkbank-Handbuch.pdf).
Neu bauen: `python3 docs/handbuch/build_handbuch.py`. Die Bilder sind Fotos der GUI
mit der Golf-6-Simulation; die GUI fotografiert sich dabei selbst und schließt sich:
`VAG_TRANSPORT=golf6 VAG_HELL=1 VAG_FENSTER=1000x880 VAG_TAB=<seite> VAG_FOTO=bild.ppm
VAG_FOTO_WARTEN=<sekunden> ecu-werkbank-gui` (`VAG_HELL` = helles Design, `VAG_FENSTER` =
Fenstergröße; Liste der Seiten im Kopf von `build_handbuch.py`). Mit eigenem
`VAG_DATA_DIR`/`VAG_LABELS`, damit keine echten Daten ins Bild kommen.

## Sprachen (Deutsch, English, eigene)

Oben rechts in der GUI: Auswahl **🌐 Sprache** (wird gemerkt). Im Terminal:
`ecu-werkbank --sprache en …` für einen Aufruf oder `ecu-werkbank sprache --setzen en` dauerhaft.
Ohne Wahl gilt die Systemsprache (`LANG`), sonst Deutsch. `VAG_SPRACHE=en` überschreibt alles.

**Neue Sprache hinzufügen (ohne Programmieren, ohne neu zu bauen):**

```
ecu-werkbank sprache --vorlage fr          # schreibt <Datenordner>/sprachen/fr.toml mit allen Texten
#   rechts übersetzen:  "Speichern" = "Enregistrer"   — leer = bleibt deutsch
#   oben  _name = "Français"  eintragen
ecu-werkbank sprache                       # zeigt alle Sprachen und wie viel übersetzt ist
```

Platzhalter wie `{n}` oder `{a}` müssen in der Übersetzung stehen bleiben. Dieselbe
Vorlage ergänzt auch eine vorhandene Sprache, wenn neue Texte dazugekommen sind (die
bisherige Datei wird als `.toml.bak` gesichert). Fest einbauen: Datei nach
`lang/<code>.toml` legen und in `src/i18n.rs` bei `EINGEBAUT` eintragen.

Für Entwickler: Texte stehen deutsch im Code. `tr!("…")` übersetzt (mit Platzhaltern:
`tr!("{n} Fehler", n = anzahl)`), `trs!("…")` liefert `&'static str`, `trn!("…")` markiert
Texte in festen Tabellen (angezeigt mit `i18n::t`), `platzhalter!` ist dasselbe ohne
Übersetzung (für Pfade und Dateiformate). `build.rs` sammelt alle markierten Texte für die
Vorlage; ein Test prüft, dass die Platzhalter in `lang/en.toml` passen.

Wird ein deutscher Text geändert oder eine Funktion entfernt, bleibt die alte
Übersetzung als „verwaister“ Eintrag liegen. Anzeigen bzw. entfernen (mit Sicherung
`.toml.bak`; Reihenfolge und Kommentare der Datei bleiben):

```
ecu-werkbank sprache --verwaist   --datei lang/en.toml
ecu-werkbank sprache --aufraeumen --datei lang/en.toml
```

Noch deutsch (auch auf Englisch): Klartexte der Fehlercodes, Namen der Steuergeräte,
Readiness- und Abgas-Testnamen, der Fehlerstatus in gespeicherten Scans, die Ratgeber-
und Lexikon-Inhalte (geführte Fehlersuche, Grundeinstellungs-Lexikon) sowie die Beschreibungen in `--help`.

Gespeicherte Daten (Scans, Wartungsposten, Trend-Namen, Ordnernamen) bleiben bewusst
deutsch, damit Vergleiche und alte Dateien in jeder Sprache funktionieren. Erkennt der
Code bestimmte Fehler (TP-2.0-Kanal kurz gesperrt, Sicherheitsabbruch des Reglers,
fehlende Umgebungsdaten), dann an einer festen Fehlerart — nie am übersetzten Text.

## Bedienung der GUI

Die GUI startet mit einer **Startseite**: Übersicht zum aktiven Fahrzeug
(km-Stand, letzter Scan, fällige Wartung, Trend-Hinweise) und Kacheln für die
häufigsten Aufgaben (Schnell-Check, Fehlerspeicher-PDF, Verbindung prüfen,
Fehlersuche, Live-Werte, Batterietest, Gemisch-Check, Gebrauchtwagen-Check,
Fahrzeugakte). Alles Weitere ist in vier Gruppen sortiert:

| Gruppe | Unterseiten |
|---|---|
| 🔧 Diagnose | Steuergeräte · Ältere Steuergeräte (TP 2.0) · OBD-II · Codierung · Erweiterte Identifikation · Prüfen & Fehlersuche · Sicherheit & Assistenz · Andere Marken |
| 📈 Messen | Live-Grafik · Zielbereich · Batterie · Partikelfilter · Motor-Checks · Fahrt-Rekorder · Rohwerte umrechnen |
| 🚘 Fahrzeug | Fahrzeuge & Wartung · Service-Anzeige · Historie & Trends · Softwarestände · Codier-Sicherung · Nachrüstung |
| 🛠 Werkzeuge | Testlauf · Mitschnitt & Lernhilfe · Eigene Namen · Label-Editor · Fehlerbilder · Grundeinstellungen (Referenz) · Experten: Routine & Stellglied (UDS) |

Oben immer erreichbar: Verbindungsart, aktives Fahrzeug, 🔌 Verbindung,
✅ Schnell-Check, 📄 Fehler-PDF, Hell/Dunkel.

## Ohne Adapter ausprobieren (Demo-Fahrzeug)

Ein simuliertes Fahrzeug ist eingebaut (UDS, TP 2.0/KWP2000 und OBD-II; Werte
ändern sich wie bei einer Fahrt, Fehlerspeicher lassen sich löschen):

```bash
./target/release/ecu-werkbank -t demo autoscan
./target/release/ecu-werkbank -t demo obd-info
./target/release/ecu-werkbank -t demo kwp-block --address 0x01 --block 1,2 --count 0
VAG_TRANSPORT=demo ./target/release/ecu-werkbank-gui      # oder in der GUI oben „Demo-Fahrzeug“
```

## Installieren

```bash
./packaging/install.sh          # für deinen Benutzer (~/.local), ohne sudo; Eintrag im Startmenü
./packaging/install.sh --can    # zusätzlich: CAN-Adapter beim Einstecken automatisch auf 500 kbit/s (sudo)
./packaging/build-deb.sh        # oder ein .deb-Paket bauen → sudo apt install ./target/ecu-werkbank_*.deb
./packaging/build-appimage.sh   # oder ein AppImage bauen → target/ecu-werkbank-<version>-x86_64.AppImage
```

**AppImage:** eine einzelne Datei, läuft ohne Installation. Ohne Argument startet die
Oberfläche, `… .AppImage ecu-werkbank scan` das Terminal-Programm, `… .AppImage mockecu can0`
das Test-Steuergerät. Lizenzen, Credits und Handbuch liegen im AppImage unter
`usr/share/doc/ecu-werkbank/`. Es braucht mindestens die glibc des Bau-Rechners (gebaut
unter Debian 13: glibc 2.39 → Ubuntu 24.04, Linux Mint 22 und neuer). Das Pack-Werkzeug
`appimagetool` lädt das Skript beim ersten Mal von github.com/AppImage/appimagetool;
`--nur-appdir` stellt nur den Ordner zusammen, ohne zu packen.

Nach `--can` bzw. dem .deb entfallen die `sudo ip link …`-Befehle: Adapter
einstecken, fertig. Labels liegen dann in `~/.config/ecu-werkbank/labels`
(Projektordner `./labels` hat Vorrang, `VAG_LABELS` überschreibt beides),
Historie und Codierungs-Sicherungen in `~/.local/share/ecu-werkbank/`.

### Windows (Light-Version)

```bash
./packaging/build-windows.sh    # unter Linux quer übersetzt → target/ecu-werkbank-<version>-windows-x86_64.zip
```

ZIP entpacken, `ecu-werkbank-gui.exe` starten — keine Installation. Unter Windows gibt
es kein SocketCAN, daher: **ELM327** (COM-Port oder WLAN), **Demo-Fahrzeug** und alles
ohne Auto (Fahrzeuge, Wartung, Teile, PDFs, Mitschnitte, Label-Editor). **Nicht**:
CAN-Adapter und damit TP 2.0 (ältere Steuergeräte). Daten liegen in
`%APPDATA%\ecu-werkbank`. Einmalig zum Bauen nötig: `rustup target add x86_64-pc-windows-gnu`
und `sudo apt install mingw-w64`. Der CAN-Zugang ist in `src/can.rs` gekapselt — ein
Windows-Treiber (z. B. slcan oder gs_usb) ließe sich dort später einhängen.

### macOS (Light-Version, ungetestet)

Gleicher Umfang wie unter Windows (ELM327, Demo, alles ohne Auto; kein CAN-Adapter).
Gebaut wird auf einem Mac mit `./packaging/build-mac.sh` → `target/ecu-werkbank-<version>-macos.zip`
(`ecu-werkbank.app` für Intel und Apple-Chip). Ohne eigenen Mac baut GitHub sie: Reiter
„Actions“ → „Mac-Version“ → „Run workflow“. Die App ist nicht bei Apple registriert:
beim ersten Start Rechtsklick → „Öffnen“. Daten liegen in `~/.local/share/ecu-werkbank/`.

## Bauen

```bash
cargo build --release
```

## Testen ohne Auto (virtuelles CAN)

```bash
sudo modprobe vcan
sudo ip link add dev vcan0 type vcan
sudo ip link set up vcan0

./target/release/ecu-werkbank -i vcan0 scan
```

## Mit echtem Adapter

```bash
# Bitrate meist 500 kbit/s (Antriebs-CAN); bei manchen Bussen 100 kbit/s
sudo ip link set can0 type can bitrate 500000
sudo ip link set up can0

./target/release/ecu-werkbank -i can0 scan
./target/release/ecu-werkbank -i can0 discover --seconds 5
./target/release/ecu-werkbank -i can0 dtc   --tx 0x7E0 --rx 0x7E8
./target/release/ecu-werkbank -i can0 rdid  --tx 0x7E0 --rx 0x7E8 --did 0xF190   # VIN
./target/release/ecu-werkbank -i can0 clear --tx 0x7E0 --rx 0x7E8
```

### Bedienung in der Oberfläche

Wie beim Werkstatt-Diagnosegerät: **Diagnose → Steuergeräte**, einmal alle Steuergeräte
auslesen, dann eines anklicken. Darunter erscheinen alle Funktionen dieses Steuergeräts:
Fehler lesen/löschen, Identifikation, Messwerte, Codierung, Anpassung, Grundeinstellung
und Stellglieddiagnose (je nachdem, was das Steuergerät kann). Ein Klick springt mit
vorbelegter Adresse auf die passende Seite.

### Welcher Adapter kann was? (CAN-Interface vs. ELM327)

**CAN-Interface** = SocketCAN-Adapter (z. B. USB-CAN, `-i can0`). **ELM327** =
günstiger OBD-Adapter über USB, Bluetooth oder WLAN (`-t elm327`).

| Funktion | CAN-Interface | ELM327 | Hinweis |
|---|:---:|:---:|---|
| **OBD-II** (Fehler, Readiness/AU, Live-Werte, VIN, Freeze Frame) | ✅ | ✅ | herstellerunabhängig |
| **UDS-Steuergeräte** (Fehlerspeicher lesen/löschen, Identifikation, Messwerte per Kennung) | ✅ | ⚠️ | ELM327: nur Geräte mit bekannter CAN-ID (OBD-Adressen, Label-Datei, Fahrzeugprofil); Lesen am Golf 6 geprüft |
| Codierung, Anpassungen, Login (UDS) | ✅ | ⚠️ | ELM327: Codierung lesen am Golf 6 geprüft, Schreiben noch ungetestet |
| Service-Reset, Partikelfilter, Kilometerstand, Softwarestände (UDS) | ✅ | ⚠️ | ELM327: Lesen geht, Schreiben ungetestet |
| Schnell-Check, Batterietest, Gebrauchtwagen-Check | ✅ | ✅ | über OBD-II |
| **Auto-Scan aller Steuergeräte** | ✅ | ⚠️ | ELM327: nur OBD-Adressen + bekannte Geräte, kein Hersteller-Suchlauf; findet auch reine OBD-Geräte |
| Andere Marken: Steuergeräte suchen | ✅ | ⚠️ | ELM327: nur OBD-Adressen |
| **TP 2.0 / KWP2000** (ältere Fahrzeuge, z. B. Golf 6 Komfort-Geräte) | ✅ | ❌ | ELM327 kann TP 2.0 nicht |
| Messwertblöcke, ⭐ Favoriten, Blockscan | ✅ | ❌ | brauchen TP 2.0 |
| **Gateway-Verbauliste und -Codierung** (direkt aus dem Gateway) | ✅ | ❌ | geht mit dem CAN-Interface, am Golf 6 geprüft; der ELM327 kann kein TP 2.0 |
| Motor-Checks (Steuerkette, Leerlauf …), Fahrt-Rekorder | ✅ | ❌ | lesen Messwertblöcke (TP 2.0) |
| Grundeinstellung an TP-2.0-Geräten (Funktion 04) | ✅ | ❌ | erst nach einem eigenen Mitschnitt freigeschaltet |
| Live-Grafik / Scope | ✅ | ⚠️ | ELM327: nur OBD- und UDS-Kanäle, keine Messwertblöcke |
| **CAN-Mitschnitt** aufnehmen, Lernhilfe aus Mitschnitt | ✅ | ❌ | braucht direkten Buszugriff |
| Auto-Erkennung (Auto an/aus, Fahrzeug automatisch wählen) | ✅ | ❌ | liest die Adapter-Zähler |
| Adapter-Prüfmodus, CAN-Zustand (BUS-OFF …) | ✅ | ❌ | nur SocketCAN |
| Mitschnitt auswerten, Rohwerte umrechnen, Label-Editor, Eigene Namen, Codier-Helfer, Wartung, PDFs | ➖ | ➖ | offline, kein Adapter nötig |

✅ geht · ⚠️ geht eingeschränkt · ❌ geht nicht · ➖ braucht keinen Adapter

> **Stand der Prüfung:** Beide Adapter sind am Golf 6 getestet. Der ELM327 (USB, v1.5,
> FTDI-Chip) seit 14.11.2026 **lesend**: OBD-II (VIN, Readiness, Fehler, Live-Werte),
> Steuergeräte-Suche und die UDS-Steuergeräte mit Herstelleradresse (Kombi, Airbag,
> Climatronic, Radio: Fehlerspeicher, Codierung und erweiterte Identifikation). Am
> CAN-Interface zusätzlich per Mitschnitt mit einem Werkstatt-Diagnosegerät verglichen
> (15.11.2026): Codierungen, Fehlerarten, Messwert-Formeln und die Gateway-Verbauliste. Schreiben über
> ELM327 ist noch ungeprüft. Für ältere Fahrzeuge (TP 2.0) führt am CAN-Interface kein
> Weg vorbei. Alles lässt sich vorher ohne Auto mit `-t demo` ausprobieren.

### Erfahrungen am echten Fahrzeug (Golf 6, 2010)

- Adapter DSD TECH SH-C31x (candleLight/gs_usb): startet nach dem Einstecken
  manchmal im Update-Modus (`lsusb` zeigt „STM Device in DFU Mode“).
  Zurückholen: `sudo dfu-util -a 0 -s 0x08000000:leave`. `restart-ms` kennt
  diese Firmware nicht.
- Bus-Off sofort nach dem ersten Senden → CAN-High/-Low vertauscht.
  OBD: Pin 6 = High, Pin 14 = Low, Pin 5 = Masse.
- TP-2.0-Logikadressen sind NICHT die gewohnten Diagnoseadressen (Golf 6:
  0x09 Servolenkung, 0x1D Einparkhilfe, 0x1F Gateway, 0x20 Bordnetz,
  0x22–0x25 Türen, 0x2A Lenksäule, 0x43 Anhänger). Die Namen kommen darum aus
  der Antwort des Steuergeräts.
- Kombi, Airbag, Klima und Radio sprechen schon UDS (0x714, 0x715, 0x746,
  0x76C; Antwort = Anfrage + 0x6A). Der Scan sucht 0x700–0x7DF ab (≈ 3 s).
- Steuergeräte verlangen 10 ms Abstand zwischen Frames (T3), melden
  „beschäftigt“ (0x21) oder kennen nur 0x22 statt 0x1A — alles eingebaut.

### Schnell-Scan, Fehlertexte, Adapter automatisch

- **Schnell-Scan:** Nach dem ersten vollen Scan steht in `fahrzeuge/<VIN>/profil.toml`,
  welche Steuergeräte das Auto hat. Danach liest der Scan zuerst die VIN und fragt
  nur diese ab (Golf 6: ~10 s statt ~40 s). Voller Suchlauf: `report --voll` bzw.
  Haken „Voll-Scan“ in der GUI. Kaufcheck und Nachrüstung suchen immer alles ab.
- **Status alter Fehlercodes** (TP 2.0/KWP) im Klartext nach ISO 14230:
  sporadisch / aktuell / wird geprüft, Warnlampe, Fehlerart-Nummer.
- **Eigene Fehlertexte:** `ecu-werkbank fehlertext --code 01627 --text "…"` oder GUI
  Werkzeuge → Fehlerbilder → „Eigene Fehlertexte“. Gilt in allen Anzeigen und PDFs.
  Viele auf einmal: `ecu-werkbank fehler-geruest` liest aus allen UDS-Steuergeräten
  **alle möglichen** Fehlercodes (nur lesen) und schreibt eine Datei zum Ausfüllen;
  danach `ecu-werkbank fehlertexte-uebernehmen <datei>`. TP 2.0 kennt diese Abfrage nicht.
- **Auto-Scan-Protokolle einlesen:** Fahrzeug → Historie & Trends → „📥 Auto-Scan-Protokolle importieren“
  (oder `ecu-werkbank autoscan-import <dateien…>`, Probelauf mit `--probe`). Auto-Scan-Protokolle
  eines Werkstatt-Diagnosegeräts als Textdatei
  (deutsch/englisch, auch alte Latin-1-Dateien) kommen mit km-Stand, Datum und allen
  Umgebungsbedingungen je Fehler in den Verlauf des Fahrzeugs (erkannt an der
  Fahrgestellnummer); ihre Fehlertexte werden eigene Fehlertexte — nur für Codes ohne
  eigenen Text. Die Protokolle und Texte bleiben in deinem Datenordner.
- **Readiness-Assistent (OBD-II):** je offenem Abgas-Test eine Fahranleitung, Fortschritt
  seit dem Löschen (km, Warmläufe, Motorlaufzeit) und — falls das Auto es meldet — welcher
  Test in der laufenden Fahrt schon geprüft wurde.
- **Namen für Messwertblöcke und Anpassungen in einem Rutsch:** Diagnose → Ältere Steuergeräte (TP 2.0) →
  „Alle Blöcke lesen (1–255)“ → „✏ Namen für alle Blöcke und Felder eintragen“ zeigt jedes
  Feld mit aktuellem Wert und Einheit zum Benennen. Fahrzeug → Codier-Sicherung →
  Anpassungen → „✏ Kennungen benennen“ listet alle Kennungen der neuesten
  Anpassungs-Sicherung (ohne Auto); die Namen erscheinen danach im Vergleich
  (`<Datenordner>/kennungstexte.toml`).
- **Eigene Messwert-Texte (TP 2.0):** Name je Block und je Feld, z. B.
  `ecu-werkbank messwerttext --adresse 01 --block 1 --feld 2 --text "Kühlmitteltemperatur"`
  oder in der GUI beim Messwertblock auf ✏. Ohne eigenen Text zeigt ECUWerkbank die Art aus
  der Umrechnungsformel mit „~“ (z. B. „~Temperatur“). Gespeichert in
  `<Datenordner>/messwerttexte.toml`; gilt in GUI, `kwp-block`, Testlauf und Kurven.
- **Blockscan:** `ecu-werkbank -i can0 kwp-blockscan --address 01` liest alle Messwertblöcke
  (1–255, nur lesen) und zeigt je Feld Wert und Art — so findest du, welche Blöcke
  dein Steuergerät überhaupt hat.
- **Namensvorschläge (Motor 01):** `ecu-werkbank -i can0 messwert-vorschlaege` prüft die
  allgemein bekannte Belegung (Block 1–4 Grundwerte, 115 Ladedruck) gegen dein
  Steuergerät: ✓ nur, wenn die Umrechnungsformel zur erwarteten Wertart passt.
  Mit `--ja` werden die passenden Namen übernommen; eigene Texte bleiben.
- **Fehlertexte ohne Label-Dateien:** Reihenfolge eigene Texte → Label-Dateien (freiwillig)
  → eingebaute OBD-Liste. Motor-Codes werden in OBD-Codes umgerechnet: 16384–17383 =
  P0000–P0999, 17408–18407 = P1000–P1999 (ältere Motoren), und bei neueren Motor-/Getriebe-
  steuergeräten (MED17) ist die Zahl der OBD-Code selbst (Golf 6: **08851 = 0x2293 = P2293**).
  Herstellerspezifische Bauteilcodes anderer Steuergeräte (01627, 00097 …) haben keine offene Liste — dafür
  eigene Texte (GUI: ✏-Knopf direkt neben Codes ohne Text). Bei UDS-Fehlern steht die genormte Fehlerart als Klartext dabei
  (z. B. „Fehlerart 13: Unterbrechung“).
- **Adapter aus dem Update-Modus automatisch zurückholen:**
  `./packaging/install.sh --can --dfu <Seriennummer>` (Seriennummer zeigt der
  Verbindungs-Assistent, solange der Adapter im Update-Modus steckt).

### Startseite, Steuergeräte-Liste, Verbindungshinweise

- **Fehler-Übersicht** auf der Startseite: neu / weiterhin / weg seit dem
  vorletzten Scan, „war früher schon einmal da“ und „kommt immer wieder“ (≥ 3 Scans).
- **Steuergeräte dieses Fahrzeugs** (Diagnose → Steuergeräte): alle Geräte aus dem
  letzten Scan mit Namen und Teilenummer; Klick → Fehler oder Identifikation live
  lesen, egal ob UDS oder TP 2.0.
- **Verbindungs-Assistent** wertet den CAN-Zustand aus: Bus-Off (Adern vertauscht?),
  Error-Passive (keine Verbindung / Zündung aus), Error-Active ohne Antwort (Auto schläft).

### Testlauf am Fahrzeug

`ecu-werkbank testlauf --km <Tachostand>` (GUI: Werkzeuge → Testlauf) liest in einem
Durchgang — nur lesend — Verbindung, VIN, alle Steuergeräte mit Fehlerspeicher,
Messwertblöcke 1–30 des Motors (mit Rohdaten) und sucht den Kilometerstand in den
UDS-Kennungen des Kombis (0x2200–22FF, 0x0200–02FF, 0x1000–10FF). Das Protokoll
landet im Fahrzeugordner unter `berichte/testlauf-….txt`.

### GUI

- Ampel oben neben dem Interface: bereit / keine Verbindung / Bus-Off / Update-Modus
  / kein Adapter (Klick öffnet den Verbindungs-Assistenten).
- Fehlende Zeichen (→ ✓ ≤ …) kommen aus den Systemschriften DejaVu Sans bzw. Noto Symbols.
- Die Steuergeräte-Seitenleiste erscheint nur noch bei Messwerten und Codierung.

### Adapter ohne Auto testen (Prüfmodus)

Der Adapter hört im Loopback-Modus seine eigenen Nachrichten; das
Demo-Steuergerät läuft dann direkt auf dem echten Adapter. ECUWerkbank erkennt
den Modus und schaltet das doppelte Linux-Echo selbst ab.

```bash
sudo ip link set can0 down
sudo ip link set can0 type can bitrate 500000 loopback on
sudo ip link set can0 up
./target/release/mockecu can0 &
./target/release/ecu-werkbank -i can0 autoscan

# zurück fürs Auto
sudo ip link set can0 down
sudo ip link set can0 type can bitrate 500000 loopback off
sudo ip link set can0 up
```

## TP 2.0 / KWP2000 (ältere CAN-Fahrzeuge ~2004–2010)

```bash
./target/release/ecu-werkbank -i can0 detect                       # UDS oder TP 2.0?
./target/release/ecu-werkbank -i can0 kwp-ident --address 0x01     # Teilenummer, Bezeichnung
./target/release/ecu-werkbank -i can0 kwp-dtc   --address 0x01     # Fehlerspeicher (5-stellig + P-Code)
./target/release/ecu-werkbank -i can0 kwp-clear --address 0x01
./target/release/ecu-werkbank -i can0 kwp-block --address 0x01 --block 1 --count 0   # live, Strg-C
./target/release/ecu-werkbank -i can0 tp20 --address 0x01 --request 1a9b             # roher KWP-Request
```

In der GUI (`./target/release/ecu-werkbank-gui`) gibt es dafür den Reiter **„Ältere Steuergeräte (TP 2.0)“**:
Steuergeräte suchen (läuft im Hintergrund), Identifikation, Fehlerspeicher
lesen/löschen und Messwertblöcke live. Start-Interface vorgeben mit
`VAG_IFACE=can0`.

**⭐ Favoriten-Messwerte:** Oft gebrauchte Blöcke merkst du beim geöffneten
Messwertblock über den **Stern (☆ → ★)**. Sie erscheinen dann als Leiste über
dem Block — ein Klick öffnet und liest den Block sofort, ohne jedes Mal Adresse
und Nummer einzutippen. Entfernen über das **✖** an der Leiste oder nochmal den
Stern. Die Liste gilt geräteübergreifend (bei mehreren Autos stehen die Adressen
des jeweiligen Modells drin) und liegt in `messwert_favoriten.toml` im Datenordner.

**⚙ Grundeinstellung (Funktion 04) an TP-2.0-Steuergeräten:** Welche Anfragen der Hersteller
dafür über TP 2.0 schickt, ist nicht verlässlich veröffentlicht. ECUWerkbank rät das
nicht, sondern **lernt den Ablauf aus deinem eigenen Mitschnitt** — bis dahin ist
das Senden für dieses Steuergerät **gesperrt**:

1. Reiter „Mitschnitt & Lernhilfe“: Aufnahme starten (CAN-Interface und das Kabel
   eines Werkstatt-Diagnosegeräts über ein OBD-Y-Kabel gleichzeitig am Auto).
2. Mit dem Diagnosegerät am Steuergerät die Grundeinstellung (Funktion 04) für eine
   Gruppe einschalten, ein paar Sekunden laufen lassen, ausschalten.
3. Aufnahme stoppen. Im Reiter „Ältere Steuergeräte (TP 2.0)“ unter „⚙ Grundeinstellung“ dieselbe Gruppe
   eintragen, „📂 Mitschnitt auswerten…“, das Ergebnis prüfen und übernehmen.

Danach lässt sich jede Gruppe dieses Steuergeräts starten (mit Rückfrage), die
Werte laufen live mit, „⏹ Aus“ beendet sie. Übernommen wird nur, was das
Steuergerät im Mitschnitt bestätigt hat; beim Starten wird nur die Gruppennummer
ausgetauscht. Der Ablauf liegt im Fahrzeugordner (`grundeinstellung.toml`) — ein
am Golf gelernter Ablauf gilt nie für ein anderes Auto.

Im Reiter **Werkzeuge → „Experten: Routine & Stellglied (UDS)“** (UDS-Steuergeräte, Routine 0x31 und
Stellglied 0x2F) kommt vor jedem Senden eine Rückfrage; im Terminal zeigen
`routine` und `iocontrol` ohne `--ja` nur an, was gesendet würde.

`autoscan` und `report` suchen jetzt **beide Welten** ab: UDS (Labels +
OBD-Bereich) und TP 2.0 (Adressen 0x01..0x7F, ca. 8 s). Mit `--no-tp20`
überspringen.

> **Wichtig:** TP 2.0 ist vom Hersteller nicht offiziell veröffentlicht. Die
> Implementierung in [`src/tp20.rs`](src/tp20.rs) stützt sich auf öffentliche
> Community-Reverse-Engineering-Doku. Gesichert sind Setup-ID 0x200, Opcodes
> 0xC0/0xD0 und das Keep-Alive < 1 s. Byte-Offsets, die mit `TODO(verify)`
> markiert sind (ID-Kodierung der Setup-Response, Timing-Parameter, Längen-
> Präfix), **müssen gegen einen echten Mitschnitt deines Fahrzeugs geprüft**
> werden, bevor man sich darauf verlässt:
> ```bash
> candump -L can0 > handshake.log   # während ein Kanal geöffnet wird
> ```

### Quellen zur TP-2.0-Struktur
- Autosport Labs Forum – TP 2.0 Reverse Engineering:
  https://forum.autosportlabs.com/viewtopic.php?p=27858
- OVMS-Entwicklerliste – getestete TP-2.0-Engine (SEAT Mii):
  https://lists.openvehicles.com/archives/list/ovmsdev@lists.openvehicles.com/thread/FDVRIIZNUR765CESHVMECH73E4DF4EOC/
- NI Automotive Diagnostic Command Set – Keep-Alive/Timing:
  https://www.ni.com/docs/en-US/bundle/automotive-diagnostic-command-set-toolkit-c-api-ref/page/ndvwtpconnectiontest.html

## Fehlerspeicher als PDF

```bash
./target/release/ecu-werkbank -i can0 report --out fehlerspeicher.pdf --note "Golf 6, B-XY 123, 154.300 km"
./target/release/ecu-werkbank -t demo report --out beispiel.pdf      # Beispiel ohne Auto
```

Fragt alle Steuergeräte ab (UDS und TP 2.0) und erstellt ein A4-Protokoll:
Datum, VIN, Notiz, Übersicht aller Steuergeräte und je Steuergerät die
Fehlercodes mit deinen Klartexten und dem Status (aktuell / gespeichert /
sporadisch). Mit `--out datei.txt` entsteht stattdessen eine Textdatei.
In der GUI: **„📄 Fehler-PDF“** oben (Notiz im Reiter „Steuergeräte“), danach
„PDF öffnen“ in der Statuszeile.

## Verbauliste (verbaute Steuergeräte)

```bash
./target/release/ecu-werkbank -i can0 verbauliste                       # live vom Auto (voller Suchlauf)
./target/release/ecu-werkbank -i can0 verbauliste --no-tp20 --out liste.txt
./target/release/ecu-werkbank -f "B XY 123" verbauliste --gespeichert   # ohne Auto, aus dem letzten Scan
```

Zeigt alle verbauten Steuergeräte, nach System geordnet (Motor & Antrieb,
Fahrwerk & Assistenz, Karosserie & Komfort …), je Gerät mit Teilenummer und
Status (i.O. / Fehler / Speicher nicht lesbar), dazu eine Zusammenfassung.
Entspricht der Verbau-/Installationsliste gängiger Werkstattdiagnose — die
Namen stammen aus der Bauteil-Kennung des Steuergeräts, nicht aus einer
geratenen Nummern-Übersetzung. In der GUI im Reiter **„🔧 Steuergeräte“** unter
**„📋 Verbauliste nach System“** (mit „💾 als Textdatei speichern“).

**Direkt aus dem Gateway** (TP 2.0, nur lesen): welche Steuergeräte das Gateway
als verbaut kennt und bei welchen ein Fehler gespeichert ist — schnell, ohne
jedes Steuergerät einzeln abzufragen:

```bash
./target/release/ecu-werkbank -i can0 gateway-verbauliste
```

In der GUI unter **„📋 Verbauliste nach System“ → „🔌 Verbauliste aus dem
Gateway lesen“**. Die Gateway-Codierung liest `kwp-coding --address 1f`.
Am Golf 6 per Mitschnitt mit einem Werkstatt-Diagnosegerät verglichen
(15.11.2026): dieselben 20 Steuergeräte, dieselben 4 mit Fehler, dieselbe
Codierung.

## Schnell-Check, Batterietest, Wartung

```bash
./target/release/ecu-werkbank -f "B XY 123" -i can0 check       # Ampel: Fehler, neu/behoben, Readiness, Spannung, Wartung → PDF
./target/release/ecu-werkbank -f "B XY 123" -i can0 batterie    # Ruhe-, Start- und Ladespannung bewerten → PDF
./target/release/ecu-werkbank -f "B XY 123" wartung-vorlage     # Öl, Filter, Kerzen, Bremsflüssigkeit, HU …
./target/release/ecu-werkbank -f "B XY 123" wartung-erledigt --name öl --km 154300
./target/release/ecu-werkbank -f "B XY 123" wartung             # was ist fällig?
./target/release/ecu-werkbank -f "B XY 123" km 154300
./target/release/ecu-werkbank -f "B XY 123" teil "Bremsscheiben + Beläge vorn" --km 154300 --teilenummer 5K0698151 --notiz "ATE" --preis 189,90
./target/release/ecu-werkbank -f "B XY 123" teile               # gewechselte Teile mit Datum, km und Preis
./target/release/ecu-werkbank -f "B XY 123" tanken 42,5 --km 154800 --preis 74,30   # --teil = nicht vollgetankt
./target/release/ecu-werkbank -f "B XY 123" tankbuch            # Tankfüllungen mit Verbrauch (l/100 km, voll zu voll)
./target/release/ecu-werkbank -f "B XY 123" kosten              # gesamt, pro Monat, pro 1000 km, je Jahr
./target/release/ecu-werkbank -f "B XY 123" kalender            # Wartungstermine als .ics (Erinnerung 1 Woche vorher)
./target/release/ecu-werkbank -f "B XY 123" reparatur "Zündspule Zyl. 3 getauscht, 45 €"
./target/release/ecu-werkbank -f "B XY 123" akte                # Fahrzeugakte als PDF
./target/release/ecu-werkbank -f "B XY 123" akte-zip --out /media/usb/golf.zip
./target/release/ecu-werkbank datensicherung --ziel /media/usb/ecu-werkbank --tage 7 --behalten 8   # einrichten
./target/release/ecu-werkbank datensicherung                    # jetzt sichern (alle Fahrzeuge + eigene Labels)
./target/release/ecu-werkbank datensicherung-zurueck /media/usb/ecu-werkbank/ecu-werkbank-sicherung-….zip
```

GUI: **„✅ Schnell-Check“** oben (Ergebnisfenster mit Ampel), Reiter
**„Batterie“** (Live-Kurve beim Starten), im Reiter **„Fahrzeuge“** Wartungsplan
(„✔ heute erledigt“), km-Stand, **gewechselte Teile** (Teil, Teilenummer, km,
Datum, Preis, Notiz), **Tankbuch** (Verbrauch nach der Voll-zu-voll-Methode),
**Kosten** (gesamt, pro Monat, pro 1000 km, je Jahr), **„📅 Termine in den Kalender“**
(.ics-Datei fürs Handy; überfällige Termine landen auf heute, Posten nur nach km werden
aus der bisherigen Fahrleistung geschätzt), **Fahrzeugakte (PDF)** mit Kosten und
Verbrauch und **Als ZIP sichern**. Heißt ein Teil wie ein
Wartungsposten (z. B. „Luftfilter“, auswählbar über ▼), wird der Posten mit eingetragen.
Beim Programmstart erscheint ein Hinweis, wenn beim aktiven Fahrzeug etwas fällig ist.
**Datensicherung:** oben unter „Fahrzeuge & Wartung“ → „💾 Datensicherung“ einen Ordner wählen
(USB-Stick, Cloud-Ordner). Die GUI sichert dann beim Start von selbst, wenn die letzte
Sicherung älter als eingestellt ist und der Ordner erreichbar ist; alte Sicherungen werden bis
auf die neuesten aufgeräumt. „Sicherung einspielen“ holt fehlende Dateien zurück, ohne
vorhandene zu überschreiben.

Weitere Hilfen in der GUI:

- **🔎 neben jedem Fehlercode**: Websuche nach Code + Fahrzeugmodell.
- **Umgebungsbedingungen** zu Fehlern (UDS-Snapshot: z. B. km-Stand, Drehzahl) und
  **OBD-Freeze-Frame** stehen im Fehler-PDF; im Reiter OBD-II per Knopf.
- **Readiness-Assistent** (OBD-II): verfolgt live, welche Abgas-Tests noch offen
  sind, mit Fahrhinweisen, und meldet, wenn alles fertig ist.
- **Sollwerte / Ampel**: Werte außerhalb des Bereichs werden rot. Eigene Grenzen
  in den Labels (für einige OBD-Werte sind großzügige Grundwerte eingebaut):

  ```toml
  [[sollwert]]
  quelle = "kwp"   # kwp (Messwertblock) | uds (DID) | obd (PID)
  id = 1           # Block / DID / PID
  feld = 2         # nur KWP: Feld 1–4
  min = 80.0
  max = 105.0
  ```

- **Scope**: ⚠ **Warnen bei Grenzwert** (Ton + Meldung, zeichnet die 30 s davor
  und danach auf), 🚗 **Fahrtenschreiber** (startet bei laufendem Motor, speichert
  beim Abstellen CSV + PDF), 🕓 **Armaturen** (Rundinstrumente),
  📱 **Handy-Anzeige** (Adresse wird angezeigt, im Handy-Browser öffnen — gleiches
  WLAN; nur lesend), **CSV dazuladen** (Aufzeichnungen übereinanderlegen).

### Handy-Anzeige — ohne App, im Browser

```bash
./target/release/ecu-werkbank -i can0 handy                 # oder in der GUI: Scope → 📱 Handy-Anzeige
```

Die angezeigte Adresse (z. B. `http://192.168.1.50:8088/3f9a…/`) im Handy-Browser
öffnen — mit dem Schlüssel am Ende, ohne ihn zeigt der Server nichts; Handy und Laptop im gleichen WLAN — im Auto am einfachsten den
**Handy-Hotspot** einschalten und den Laptop damit verbinden. Ist die Firewall
`ufw` aktiv (Standard bei Linux Mint), einmalig freigeben:

```bash
sudo ufw allow from 192.168.0.0/16 to any port 8088 proto tcp   # nur lokales Netz
sudo ufw allow from 172.16.0.0/12 to any port 8088 proto tcp    # iPhone-Hotspot (172.20.10.x)
```

## Diagnose: Prüfergebnisse, Gemisch, Trends, frühere Reparaturen

ECUWerkbank ist ein Diagnose-Werkzeug — es steckt nicht ständig am Auto. Alles
wird pro Diagnose-Termin im Fahrzeugordner auf dem Laptop gespeichert; aus
mehreren Terminen entstehen die Trends.

```bash
./target/release/ecu-werkbank -i can0 obd-tests                 # Mode 06: wie knapp sind Kat, Lambdasonden … an der Grenze? Aussetzer je Zylinder
./target/release/ecu-werkbank -f "B XY 123" -i can0 gemisch     # Leerlauf vs. 2500 1/min → Falschluft / Kraftstoffdruck / zu fett
./target/release/ecu-werkbank -f "B XY 123" trends              # Verläufe + Hinweise auf schleichende Verschlechterung
./target/release/ecu-werkbank -t demo --demo-muster "fehler=falschluft" gemisch --seconds 5   # Demo
```

- **Prüfergebnisse (Mode 06):** Wert, Grenzen und Reserve je Test; „knapp“ unter
  25 % Reserve; ein Zylinder mit deutlich mehr Aussetzern wird gemeldet.
  Genormt, funktioniert an jedem Benziner mit OBD-II. Die Grenzen liefert das
  Motorsteuergerät selbst. **Herstellertests** (Test-Nummer ab 0x80, z. B. „Lambdasonde
  B1 S2 · Test 0x81“) sind nicht veröffentlicht — oft Zähler oder Verhältnisse, die normal
  nah an einer Grenze liegen. Sie zählen darum nur als bestanden/nicht bestanden, nie als
  „knapp“. Jeder Schnell-Check und Kaufcheck speichert alle Prüfwerte roh im Scan
  (`[[pruefwerte]]`: mid, tid, wert, min, max), damit man sie später nachprüfen kann.
- **Gemisch-Assistent:** vergleicht die Gemischkorrektur im Leerlauf und bei
  erhöhter Drehzahl. Mager nur im Leerlauf ⇒ Falschluft; mager überall ⇒
  Kraftstoffversorgung/Luftmassenmesser; fett im Leerlauf ⇒ Tankentlüftung/
  Einspritzventil. Mit Hubraum (Reiter Fahrzeuge) zusätzlich Luftmassen-Check.
  Richtwerte, keine Bauteilprüfung. Diesel melden meist keine Gemischwerte.
- **Trends:** Schnell-Check, Batterietest und Gemisch-Assistent speichern ihre
  Werte (`trends.toml` im Fahrzeugordner). Ab 3 Terminen erkennt ECUWerkbank z. B.
  sinkende Batteriespannung, wachsende Gemischkorrektur, steigende Aussetzer
  oder schrumpfende Reserve des Katalysators. Steht im Schnell-Check, in der
  Historie (Abschnitt Trends, mit Verlaufskurve) und in der Fahrzeugakte.
- **Frühere Reparaturen:** Kommt ein Fehlercode wieder, zeigt ECUWerkbank deine
  Reparatur-Notiz von damals — auch von anderen deiner Fahrzeuge — unter dem
  Code, im Fehler-PDF und als Warnung im Schnell-Check.

In der GUI: Reiter **„Diagnose“**.

## Motor-Checks, Fahrt-Rekorder, Anpassungen

GUI: **Messen → Motor-Checks** und **Messen → Fahrt-Rekorder**; Anpassungen auf
der Seite **Fahrzeug → Codier-Sicherung**. Alles nur lesend, über die
Messwertblöcke des Motors (TP 2.0, Adresse 01). Welcher Block/Feld es bei deinem
Motor ist, legst du einmal je Fahrzeug fest (`motorcheck.toml` im Fahrzeugordner);
„Kandidaten suchen“ bzw. `kwp-blockscan` zeigt passende Felder.

- **Steuerkette** (gerade beim 1.4 TSI CAXA ein Thema): Phasenlage der Nockenwelle
  betriebswarm im Leerlauf, als Trend. Die erste Messung ist der Bezugswert;
  wandert der Wert um 2° bzw. 4° weg, kommt ein Hinweis bzw. eine Warnung. Dazu
  die genormten Fehler P0016/P0017/P0011/P0014.
- **Ladedruck / Kraftstoffdruck:** im 3. Gang voll beschleunigen, ECUWerkbank schreibt
  Soll und Ist mit und bewertet die Lücke (Ladedruck 100/200 mbar, Kraftstoff
  7 %/15 %; die erste Sekunde zählt als Ansprechzeit). Standard für Ladedruck:
  Block 115 Feld 3/4. Grenzen sind eigene, vorsichtige Schätzungen.
- **Fahrt-Rekorder:** Blöcke laufen mit, Ringpuffer; bei neuem Motorfehler, einer
  Grenze (`003/4>40`) oder „Merken“ werden Vorlauf + Nachlauf als CSV gespeichert
  (`rekorder/` im Fahrzeugordner).
- **Anpassungen sichern:** alle lesbaren Kennungen 0x0100–0x04FF und 0x2000–0x24FF
  jedes UDS-Steuergeräts; Vergleich vorher/nachher zeigt geänderte Kennungen.

```bash
./target/release/ecu-werkbank -i can0 kette --feld 093/2          # Feld merken + messen
./target/release/ecu-werkbank -i can0 kette                       # später wiederholen
./target/release/ecu-werkbank -i can0 druckpruefung --art ladedruck --sekunden 20
./target/release/ecu-werkbank -i can0 rekorder --bloecke 1,2,3,115 --grenze "001/2>110"
./target/release/ecu-werkbank -i can0 anpassung-sichern
./target/release/ecu-werkbank anpassung-vergleich
```

## Gebrauchtwagen-Check, Fehlersuche, Reparatur, Verbindung, Lernhilfe

```bash
./target/release/ecu-werkbank -i can0 kaufcheck --note "Inserat XY"   # gelöschte Fehler? Tacho? Fahrgestellnummern? → PDF Kaufberatung
./target/release/ecu-werkbank -f WVWZZZ1KZAM000017 kaufcheck --aus-scan   # ohne Auto: aus dem letzten gespeicherten Scan (GUI: „📄 Aus letztem Scan“)
./target/release/ecu-werkbank pruefplan P0301                         # geführte Fehlersuche
./target/release/ecu-werkbank -f "B XY 123" -i can0 report            # vorher-Stand …
./target/release/ecu-werkbank -i can0 fehler-loeschen --ja            # … reparieren, löschen, Probefahrt …
./target/release/ecu-werkbank -f "B XY 123" -i can0 reparaturbericht --arbeiten "Zündspule Zyl. 3 erneuert"
./target/release/ecu-werkbank -i can0 verbindung                      # Adapter/Bitrate/Steuergeräte prüfen
./target/release/ecu-werkbank lernen mit-aktion.log --vergleich ohne-aktion.log --out labels/gelernt.toml
./target/release/ecu-werkbank -t demo --demo-muster "kauf=manipuliert" kaufcheck   # Demo: manipuliertes Auto
```

- **Gebrauchtwagen-Check:** „Strecke/Warmläufe seit Fehlerlöschen“ (kürzlich
  gelöscht?), Kilometerstand aus Motor, Kombiinstrument und Fehlerspeicher
  (Fehler bei höherem km-Stand als der Tacho ⇒ Manipulation?), gleiche
  Fahrgestellnummer in allen Steuergeräten (sonst getauscht), Readiness,
  Abgas-Tests, Batterie. Plausibilitätsprüfung, kein Gutachten.
- **Geführte Fehlersuche:** Prüfpläne für Gemisch, Aussetzer, Katalysator,
  Lambdasonden, Luftmassenmesser, Thermostat, Ladedruck, AGR, Tankentlüftung,
  Geber, Bordspannung, Datenbus; Schritte öffnen das passende Werkzeug,
  Ergebnis als Reparatur-Notiz.
- **Reparatur-Modus:** vorher sichern → reparieren → alle Fehler löschen →
  Probefahrt (Readiness) → nachher prüfen → **Reparaturbericht** (PDF).
- **Verbindungs-Assistent:** findet CAN-Interfaces, prüft „aktiv“, Bitrate,
  ELM327-Port und Rechte, und ob Steuergeräte antworten — mit Befehl zur Abhilfe.
- **Lernhilfe:** schlägt aus einem Werkstatt-Mitschnitt Label-Einträge vor
  (Stellfunktionen, Codier-Bits, Anpassungen, Messwerte) — damit werden
  „Steuergerät stellt selbst“ und Codier-Labels für dein Auto möglich.

In der GUI: Reiter **„Diagnose“** (Gebrauchtwagen-Check, Fehlersuche,
Reparatur-Modus), **„🔌 Verbindung“** oben, Lernhilfe im Reiter **„Mitschnitt“**.

## Softwarestände, Nachrüstung, Codier-Helfer, Partikelfilter, Fehlerbilder, Sicherheit

```bash
./target/release/ecu-werkbank -f "B XY 123" -i can0 software                              # Teile-/SW-/HW-Stände, Vergleich zum letzten Mal
./target/release/ecu-werkbank -f "B XY 123" -i can0 nachruestung --name AHK --phase vorher
./target/release/ecu-werkbank -f "B XY 123" -i can0 nachruestung --name AHK --phase nachher --notiz "Westfalia, codiert"
./target/release/ecu-werkbank -i can0 codier-bit --tx 0x7E0 --rx 0x7E8 --byte 1 --bit 2 --wert ein --ja   # mit Sicherung vorher
./target/release/ecu-werkbank -f "B XY 123" -i can0 dpf                                   # Partikelfilter (Kennungen im Fahrzeug, [dpf])
./target/release/ecu-werkbank -f "B XY 123" wissen --code P0300 --neu "meist Zündspule"   # eigenes Fehlerbild
./target/release/ecu-werkbank -f "B XY 123" wissen --code P0300                           # Treffer
./target/release/ecu-werkbank -i can0 sicherheit                                          # Airbag, ESP, Assistenz (nur lesen)
```

- **Softwarestände:** Teilenummer, Software-/Hardwarestand, Systemname und
  Seriennummer je Steuergerät; der Vergleich zeigt getauschte, neu programmierte,
  neue oder fehlende Steuergeräte. Auch in der Fahrzeugakte.
- **Nachrüst-Assistent:** vorher Scan + Codierung sichern, einbauen, nachher
  vergleichen (neue Steuergeräte, neue Fehler, Codierungsänderungen) → PDF.
- **Codier-Helfer** (Seite „Codierung“): Steuergerät aus der letzten
  Codier-Sicherung wählen oder vom Steuergerät lesen, alle Bytes als Bits
  sehen und schalten, **jedem Bit und Byte selbst einen Namen geben** (✏),
  Liste der geänderten Bits, Schreiben mit automatischer Sicherung und Kontrolle.
  Siehe „Eigene Codiertexte“.
- **Partikelfilter:** Rußbeladung, km seit Regeneration, Differenzdruck,
  Abgastemperatur mit Ampel und Fahrhinweisen. Die Kennungen sind
  motorspezifisch (aus Mitschnitt/Lernhilfe) und werden je Fahrzeug gespeichert.
- **Fehlerbilder:** eigene Erfahrungen je Modell und Code; erscheinen in der
  Fehlersuche, zusätzlich Reparaturen gleicher Modelle.
- **Sicherheit & Assistenz:** Airbag, Bremsen/ESP, Lenkung, Parkbremse und
  Assistenzsysteme gruppiert, mit Fehlern und Umgebungsdaten — nur lesend.

## Zielbereich: Drehzahl (oder andere Werte) halten oder schwanken lassen

Muster: `halten:2500`, `schwanken:2000:3000:10` (zwischen 2000 und 3000, alle 10 s),
`rampe:800:3000:20` (in 20 s von 800 auf 3000).

**1. Fahrer-Assistent** — du hältst den Wert selbst mit dem Gaspedal; große
Anzeige „MEHR GAS / WENIGER GAS / IM BEREICH“, Ton wenn länger daneben, Anteil
„im Bereich“, auch auf dem Handy. Funktioniert an jedem Auto:

```bash
./target/release/ecu-werkbank -i can0 ziel --muster halten:2500 --toleranz 150
./target/release/ecu-werkbank -i can0 ziel --pid 0D --muster schwanken:50:80:30 --toleranz 5   # Geschwindigkeit
```

**2. Steuergerät stellt selbst** (Werkstattfunktion, z. B. Leerlaufdrehzahl
anheben). **Nur im Stand.** Abbruch automatisch bei Fahrt, Überschreitung der
Grenze, Kommunikationsfehlern, Zeitlimit, Strg-C bzw. NOT-AUS — danach bekommt
das Steuergerät die Kontrolle zurück. Die Kennungen sind je Motorsteuergerät
verschieden und kommen aus einem **Mitschnitt** (ein Werkstattgerät hebt die
Drehzahl an, `ecu-werkbank record` zeichnet auf, `decode-log` zeigt den
Stellglied-Befehl). Dann in eine Label-Datei eintragen:

```toml
[[stellfunktion]]
name = "Leerlaufdrehzahl anheben"
art = "uds-io"        # uds-io (0x2F Stellglied) | uds-routine (0x31)
tx = 0x7E0
rx = 0x7E8
id = 0x....           # aus dem Mitschnitt
bytes = 2
faktor = 1.0          # Rohwert = (Soll - offset) / faktor
offset = 0.0
min = 800.0           # Sicherheitsgrenzen
max = 1500.0
rueckmeldung_pid = 0x0C
max_dauer_s = 300
einheit = "1/min"
```

```bash
./target/release/ecu-werkbank -i can0 stellen --funktion leerlauf --muster schwanken:900:1400:8 --seconds 60       # zeigt nur den Plan
./target/release/ecu-werkbank -i can0 stellen --funktion leerlauf --muster schwanken:900:1400:8 --seconds 60 --ja  # führt aus
```

**3. Simulation** — das Demo-Fahrzeug folgt einstellbaren Verläufen:

```bash
./target/release/ecu-werkbank -t demo --demo-muster "drehzahl=schwanken:2000:3000:10" ziel --muster halten:2500
./target/release/ecu-werkbank -t demo --demo-muster "geschwindigkeit=halten:0" stellen --funktion demo --muster halten:1500 --ja
```

In der GUI: Reiter **„Zielbereich“** (Assistent, Steuergerät-Funktion mit
NOT-AUS, Simulation).

## Fahrzeugakte (ein Ordner pro Fahrzeug)

Alles zu einem Auto landet automatisch in einem Ordner, benannt nach der
**Fahrgestellnummer**:

```text
~/.local/share/ecu-werkbank/fahrzeuge/WVWZZZ1KZAW000001/
  fahrzeug.toml   berichte/   historie/   codierung/   aufzeichnungen/   scope/   mitschnitte/
```

Das **Kennzeichen** (und eine Bezeichnung wie „Golf 6“) ordnest du der VIN zu —
damit findest du das Fahrzeug leichter: in der Liste, im PDF und per
`--fahrzeug "B XY 123"`. Ältere Autos ohne lesbare VIN bekommen vorläufig einen
Ordner nach Kennzeichen; sobald die VIN bekannt ist, wird er umbenannt.

```bash
./target/release/ecu-werkbank fahrzeug-neu --kennzeichen "B XY 123" --name "Golf 6 1.4 TSI"
./target/release/ecu-werkbank -f "B XY 123" -i can0 report          # PDF → berichte/, Scan → historie/
./target/release/ecu-werkbank -f "B XY 123" -i can0 kwp-block --address 0x01 --block 1,2 --count 0 --csv fahrt.csv
./target/release/ecu-werkbank -f "B XY 123" csv-pdf ~/.local/share/ecu-werkbank/fahrzeuge/*/aufzeichnungen/fahrt.csv
./target/release/ecu-werkbank fahrzeuge                               # Übersicht aller Fahrzeuge
```

In der GUI: oben das **aktive Fahrzeug** wählen (wird gemerkt), Reiter
**„Fahrzeuge“** zum Anlegen/Bearbeiten und „Ordner öffnen“. Fehler-PDF,
CSV-Aufzeichnungen, Codierungs-Sicherungen und **Scope-PDF** („📄 Als PDF“ im
Reiter Scope: Übersicht + je Messwert ein Diagramm mit min/max/Mittel) werden
ohne Nachfrage dort gespeichert.

## Fahrzeug-Historie und Vergleich

Jeder `report` (und jedes Fehler-PDF in der GUI) wird automatisch unter der
Fahrgestellnummer gespeichert und mit dem letzten Scan verglichen: **neu**,
**behoben**, **weiterhin vorhanden**, und ob ein neuer Fehler früher schon
einmal da war (**wiederkehrend**). Das steht auch im PDF.

```bash
./target/release/ecu-werkbank history                       # alle Fahrzeuge
./target/release/ecu-werkbank history --vin WVWZZZ1KZAW000001
./target/release/ecu-werkbank compare --vin WVWZZZ1KZAW000001   # neueste zwei Scans
```

In der GUI: **Fahrzeug → Historie & Trends** — oben Kilometer- und Fehlerverlauf, darunter
zwei Scans wählen, Unterschiede sehen, als PDF speichern.

## Komplett-Sicherung für den Notfall (Ersatzteil-Pass)

Für den Fall, dass ein Steuergerät kaputtgeht: **ein Klick sichert alles, was
man für den Tausch braucht** — Teilenummer, Software- und Hardwarestand,
Codierung und Anpassungen aller Steuergeräte (UDS und TP 2.0) — und erstellt
das PDF **„Ersatzteil-Pass“** (pro Steuergerät: welches Teil kaufen, Codierung,
was nach dem Einbau zu tun ist, z. B. Wegfahrsperre online anlernen).
Nur lesen, Dauer etwa 10–20 Minuten. GUI: **Sicherung → „🛟 Komplett-Sicherung starten“**.

```bash
ecu-werkbank notfall-sicherung
```

Der Programmspeicher (die Software) selbst ist über die Diagnose nicht lesbar
und wird nicht gebraucht — ein Ersatzteil mit gleicher Teilenummer bringt sie mit.
Anpassungen der TP-2.0-Steuergeräte: gesichert wird alles, was das Gerät über
Kennungen (0x22) herausgibt; ganz alte Geräte ohne diesen Dienst geben so nichts her.

### Erweiterte Identifikation, Grundeinstellungen (Referenz)

```bash
ecu-werkbank -i can0 ident --geraet 714        # alle Standard-Kennfelder eines UDS-Geräts (Teilenr., SW/HW, Seriennr., Datum …)
ecu-werkbank grundeinstellung --system bremse  # Katalog üblicher Werkstattfunktionen (nur Nachschlagen)
```

`ident` liest nur (funktioniert auf jedem UDS-Gerät, auch beim Golf).
`grundeinstellung` ist eine reine Nachschlage-Hilfe und greift nie ins Auto ein.

In der Oberfläche gibt es beide als eigene Seiten: **Erweiterte Identifikation**
(Gruppe „Diagnose“) und **Grundeinstellungen (Referenz)** (Gruppe „Werkzeuge“).

### Nach dem Tausch: Anpassungen zurückschreiben

GUI: **Sicherung → Anpassungen → „↩ Anpassungen zurückschreiben“**: Sicherung
und Steuergerät wählen, **vergleichen** (nur lesen), gewünschte Werte anhaken,
bestätigen, schreiben. Im Terminal:

```bash
ecu-werkbank -i can0 anpassung-zurueck                                   # Steuergeräte der neuesten Sicherung
ecu-werkbank -i can0 anpassung-zurueck --geraet 714                      # vergleichen (nur lesen)
ecu-werkbank -i can0 anpassung-zurueck --geraet 714 --kennung 2243       # zeigt, was geschrieben würde
ecu-werkbank -i can0 anpassung-zurueck --geraet 714 --kennung 2243 --ja  # schreibt + prüft
```

`--geraet` ist hex: UDS-Request-ID (z. B. `714` Kombi) oder TP-2.0-Adresse (z. B. `20`).
Schutz: geschrieben wird nur, was heute **anders** ist, **gleich lang** ist
(sonst passt der Aufbau nicht — anderes Teil) und sich beim zweiten Lesen
**nicht von selbst ändert** (Messwert). Der **Kilometerstand wird nie
geschrieben.** Jeder Wert wird danach zur Kontrolle gelesen; die Werte davor
werden als eigene Sicherung abgelegt (damit lässt sich alles rückgängig machen).
Fahrgestellnummer von Sicherung und Auto müssen zusammenpassen.
Lehnt das Steuergerät ab (Zugriffsberechtigung/Login nötig), wird nichts erzwungen.
Bei TP-2.0-Steuergeräten ist das Schreiben über 0x2E am Auto noch nicht geprüft.

### Login (SecurityAccess), wenn ein Gerät das Schreiben verweigert

Manche UDS-Steuergeräte verlangen vor dem Schreiben eine Anmeldung: Sie geben
erst eine Zufallszahl heraus (den „Seed"), und das Werkzeug muss die passende
Antwort (den „Key") schicken. Bei diesen Fahrzeugen wird der Key meist über **SA2** berechnet
— ein kleines Programm (Bytecode), das in den **Flash-Daten deines Steuergeräts**
steckt und pro Gerät und Softwarestand verschieden ist. **ECUWerkbank liefert
keinen Bytecode mit**; die Rechenmaschine (SA2) ist eingebaut und geprüft, den
Bytecode für dein Gerät gibst du selbst vor.

```bash
ecu-werkbank -i can0 login --geraet 714 --sa2 "68 02 81 49 …" --level 3   # Bytecode hinterlegen
ecu-werkbank -i can0 login --geraet 714 --test                           # anmelden (schreibt nichts)
ecu-werkbank login                                                       # hinterlegte Logins zeigen
ecu-werkbank login --geraet 714 --loeschen                               # wieder entfernen
```

In der Oberfläche steht das Feld unter **Anpassungen zurückschreiben → „🔑 Login"**.
Ist ein Login hinterlegt, meldet sich das Zurückschreiben bei „Zugriff
verweigert" automatisch an und schreibt dann. Der Login öffnet nur das Tor zum
**Codieren und Anpassen** — ECUWerkbank kann kein Steuergerät flashen. Für die
älteren TP-2.0-Geräte ist der Login noch nicht eingebaut (dort läuft er anders).

Die SA2-Rechenmaschine ist eigener Code nach öffentlich dokumentierter
Beschreibung, gegengeprüft an bekannten Werten (Seed `1a1b1c1d` → Key
`6a37f02e`). Dank an die offene Dokumentation von
[bri3d/sa2_seed_key](https://github.com/bri3d/sa2_seed_key).

### Sichern und wieder einspielen (ganzes Fahrzeug)

```bash
ecu-werkbank --fahrzeug <VIN> akte-zip --out golf.zip   # alle Daten des Fahrzeugs als ZIP
ecu-werkbank fahrzeug-import golf.zip                    # wieder einspielen (z. B. nach Neuinstallation)
```

Spielt Scans, Codierungen, Software-, Anpassungs- und Notfall-Sicherungen samt
Berichten zurück. Vorhandene Dateien bleiben, außer mit `--ueberschreiben`.

## Codierungs-Sicherung

```bash
./target/release/ecu-werkbank -i can0 coding-backup         # alle Steuergeräte sichern
./target/release/ecu-werkbank coding-list
./target/release/ecu-werkbank coding-compare                # neueste zwei, bis aufs Bit (mit Label-Namen)
./target/release/ecu-werkbank -i can0 coding-restore --file <sicherung.toml> --tx 0x7E0        # zeigt nur an
./target/release/ecu-werkbank -i can0 coding-restore --file <sicherung.toml> --tx 0x7E0 --ja   # schreibt + prüft
```

UDS-Steuergeräte: Codierung über DID 0x0600 (oder `coding_did` aus dem Label),
Zurückschreiben mit anschließender Kontrolle. Manche Steuergeräte verlangen
dafür eine Zugriffsberechtigung — dann bricht es mit einer klaren Meldung ab.
TP-2.0-Steuergeräte:
- **Neuere** (Golf 6: Bordnetz 20, ABS 03, Einparkhilfe 1D, Anhänger 43) verstehen
  Kennungen wie UDS — die lange Codierung kommt über Kennung 0x0600 und lässt sich
  im Codier-Helfer Bit für Bit ansehen und benennen. Am Golf 6 per Mitschnitt mit
  einem Werkstatt-Diagnosegerät bestätigt (14.11.2026): Bordnetz (30 Byte) und
  ABS (18 Byte) liefern Byte für Byte dieselbe Codierung.
- **Ältere** (Motor, Türen, Lenksäule) haben eine Kurzcodierung in der
  Identifikation (Byte 16–19 = `03 xx xx xx`); ECUWerkbank zeigt sie als Zahl
  (am Golf 6 mit einem Werkstatt-Diagnosegerät verglichen: stimmt). Andere (Gateway, Lenkung):
  Roh-Identifikation.
- **Lange Codierung schreiben** (`kwp-langcodierung --neu … --ja`, GUI Seite „Codierung“):
  Bordnetz/ABS über Kennung 0x0600, Gateway über die Identifikation 0x9A, danach
  wird zur Kontrolle neu gelesen. **Noch nicht am Auto bestätigt:** am echten Auto
  läuft deshalb erst ein Probelauf — er zeigt die Anfrage, sendet aber nichts. Das
  wird freigeschaltet, sobald ein Mitschnitt die Anfrage bestätigt. Gleiches gilt für
  `kwp-anpassung-schreiben` (Anpassungskanal ändern).
- TP-2.0-Adressen gibt man immer hexadezimal an, mit oder ohne `0x` (`20` = `0x20` = Bordnetz).

```bash
./target/release/ecu-werkbank -i can0 kwp-coding --address 20     # Bordnetz lesen
./target/release/ecu-werkbank -i can0 kwp-langcodierung --address 20 --neu <hex>   # Vorschau: welche Bits sich ändern
./target/release/ecu-werkbank codiertext --tx 220 --byte 0 --bit 2 --text "…"   # TP 2.0: 200 + Adresse
```

In der GUI: Reiter **„Codier-Sicherung“** bzw. Seite „Codierung“. Beide zeigen nur die
Sicherungen des oben gewählten Fahrzeugs — so kann keine fremde Codierung aufs Auto
geschrieben werden. Eine neue Sicherung landet beim angesteckten Auto (nach VIN) und
wählt es aus.

### Eigene Codiertexte

Was ein Codier-Bit bewirkt, lernst du selbst und trägst es einmal ein — es gilt
dann im Codier-Helfer, beim Aufschlüsseln und im Sicherungsvergleich. Die Texte
hängen an der CAN-ID des Steuergeräts (z. B. 714 = Kombi beim Golf 6) und
liegen in `codiertexte.toml` im Datenordner. Ohne eigenen Text wird der Name aus
deinen `[[coding]]`-Labels genommen.

```bash
./target/release/ecu-werkbank codiertext --tx 714 --byte 0 --bit 3 --text "Tagfahrlicht"
./target/release/ecu-werkbank codiertext --tx 714 --byte 2 --text "Ländervariante"   # ganzes Byte
./target/release/ecu-werkbank codiertext --tx 714                                      # Texte zeigen
./target/release/ecu-werkbank decode-coding --tx 714 --bytes 0901A3                    # aufschlüsseln
```

So lernst du ein Bit: Bit ändern und schreiben (oder mit einem anderen Gerät
codieren und vorher/nachher sichern), Wirkung am Auto ansehen, dann im
Codier-Helfer bei „Geändert gegenüber …“ auf ✏ und den Namen eintragen.
`--tx` ist immer hexadezimal (714 = 0x714).

## Live-Grafik

GUI-Reiter **„Scope“**: mehrere Kurven gleichzeitig aus OBD-II-Werten,
TP-2.0-Messwertblöcken und UDS-DIDs; Zeitfenster, „normiert“ zum Vergleichen
verschiedener Einheiten, aktuell/min/max-Tabelle, CSV aufzeichnen und alte
CSV-Aufzeichnungen wieder laden.

## OBD-II (herstellerunabhängig)

```bash
./target/release/ecu-werkbank -i can0 obd-info                 # VIN, MIL, Readiness
./target/release/ecu-werkbank -i can0 obd-dtc                  # gespeichert / sporadisch / permanent
./target/release/ecu-werkbank -i can0 obd-live --pids 0C,0D,05 --count 0
./target/release/ecu-werkbank -i can0 obd-clear                # löscht auch die Readiness!
```

Funktioniert auch mit ELM327 (`-t elm327 --port /dev/ttyUSB0`). In der GUI: Reiter **„OBD-II“**.

## Andere Marken (z. B. Mercedes, BMW, Opel)

ECUWerkbank ist für VW/Audi/Skoda/Seat gebaut. Für andere Marken geht, was genormt ist:
**OBD-II** (Seite OBD-II: Motor-/Getriebefehler, Readiness, Abgas-Prüfwerte, Live-Werte) und
die Seite **Diagnose → Andere Marken**: Sie sucht die Steuergeräte an der OBD-Buchse und liest
Kennung (UDS-Kennungen F187, F18C, F189, F191, F197 … bzw. KWP2000 0x1A) und Fehlerspeicher
(UDS 0x19 bzw. KWP2000 0x18). Ergebnis als Fehlerspeicher-PDF im Fahrzeugordner (nach VIN).

```bash
./target/release/ecu-werkbank -i can0 fremd          # suchen, lesen, PDF (nur lesen)
```

So wird gesucht (nur SocketCAN): 1,5 s zuhören; CAN-IDs mit Verkehr werden nie benutzt. Bei
mehr als 20 belegten IDs (Buchse hängt an einem Fahrzeug-Bus) nur 0x7E0–0x7E7, sonst alle freien
IDs 0x400–0x7FF, je 25 ms, nur „Tester Present“ (0x3E). Treffer werden danach einzeln
nachgeprüft. Mit ELM327 und Demo nur die OBD-Adressen. Fehler löschen geht je Steuergerät mit
Rückfrage (UDS 0x14 bzw. KWP2000 0x14).

Nicht möglich: Herstellertexte zu Fehlern (nur genormte P-Codes haben Text), Codierung,
Anpassungen, Service-Rückstellung, 29-Bit-Adressen, DoIP (Ethernet) und Autos mit K-Leitung.
Am echten Mercedes noch nicht ausprobiert.

## Messwerte aufzeichnen (CSV)

```bash
./target/release/ecu-werkbank -i can0 kwp-block --address 0x01 --block 1,2,3 --count 0 --csv fahrt.csv
./target/release/ecu-werkbank -i can0 obd-live --count 0 --interval 250 --csv obd-fahrt.csv
```

Format für deutsche Tabellenprogramme: `;` als Trenner, Dezimalkomma, erste
Spalte Zeit in Sekunden. Jede Zeile wird sofort geschrieben (Abbruch mit
Strg-C verliert nichts). In der GUI: „⏺ CSV aufzeichnen“ neben „Live“.

## Mitschnitte auswerten

```bash
./target/release/ecu-werkbank -i can0 record --out mitschnitt.log --seconds 30   # oder: candump -L can0 > mitschnitt.log
./target/release/ecu-werkbank decode-log mitschnitt.log
./target/release/ecu-werkbank -t demo record --out beispiel.log                  # Beispiel ohne Auto
```

`decode-log` übersetzt TP 2.0/KWP2000, UDS und OBD-II in Klartext und gibt
einen **Prüfbericht** aus: Stimmt die Reihenfolge der Kanal-IDs? Passt das
Längenfeld? Läuft die Sequenznummer über den Kanal weiter? Welche
Messwert-Formeln kommen vor? Damit lassen sich die `TODO(verify)`-Stellen in
`src/tp20.rs` mit einem einzigen Mitschnitt deines Autos klären — am besten
während ein anderes, funktionierendes Diagnosegerät einen Kanal öffnet. In der
GUI: Reiter **„Mitschnitt“**.

### Mitschnitte in andere Programme, DBC-Signale

```bash
./target/release/ecu-werkbank log-export --log mitschnitt.log --out mitschnitt.pcap   # Wireshark
./target/release/ecu-werkbank log-export --log mitschnitt.log --out mitschnitt.asc    # Vector/SavvyCAN
./target/release/ecu-werkbank dbc-decode --dbc eigene.dbc --log mitschnitt.log [--nur Drehzahl]
```

`dbc-decode` liest eigene oder frei lizenzierte DBC-Dateien (Intel/Motorola,
Vorzeichen, Faktor/Offset, Wertetexte); ECUWerkbank bringt keine Hersteller-DBC mit.

### Fahrzeug-Datenbank (nur lokal)

Fahrzeugwissen kann in einer eigenen Datenbank gesammelt werden (SQLite-Datei
im Datenordner, keine Installation nötig). Jede Quelle wird mit Art, Lizenz und
Herkunft gespeichert und lässt sich als Ganzes wieder entfernen.

```bash
./target/release/ecu-werkbank db                       # Übersicht
./target/release/ecu-werkbank db eingebaut             # Normdaten + mitgelieferte Community-Daten
./target/release/ecu-werkbank db labels                # eigene Label-Dateien übernehmen
./target/release/ecu-werkbank db dbc --datei vw_pq.dbc --lizenz MIT --hersteller Volkswagen
./target/release/ecu-werkbank db paket --datei meine-daten.toml   # Datenpaket (TOML/JSON)
./target/release/ecu-werkbank db csv --datei fehler.csv           # code;text  oder  did;name;einheit;faktor;offset
./target/release/ecu-werkbank db export --ziel beitrag.toml --lizenz CC0-1.0
./target/release/ecu-werkbank db quellen               # was ist drin, woher, welche Lizenz
./target/release/ecu-werkbank db entfernen --id 3      # Vorschau; mit --ja wirklich
./target/release/ecu-werkbank db sichern --ziel kopie.sqlite
```

Daten ohne Lizenzangabe und selbst gekaufte Herstellerdaten bleiben immer nur
bei dir; `db export` lässt sie weg und sagt, was fehlt. Das Format der
Datenpakete steht in `src/db/paket.rs`, ein Beispiel in
`daten/community/vw/adressliste.toml`. Tippfehler im Paket werden gemeldet,
nicht still übergangen. Plan und Aufbau: [ROADMAP.md](ROADMAP.md), „Plan: Fahrzeug-Datenbank“.

### Live-CAN-Monitor (GUI)

**Werkzeuge → CAN-Monitor** zeigt live jede CAN-ID mit Daten, Anzahl und Rate.
Bytes, die sich gerade ändern, sind farbig; ein Klick auf eine ID zeigt die
einzelnen Bits. Der Monitor **hört nur zu** und sendet nichts.

Signal finden: **„Rauschen merken“** drücken. Alles, was sich bis dahin von
selbst geändert hat (Zähler, Prüfsummen), wird danach ignoriert. Dann am Auto
etwas betätigen, etwa den Blinker: Hervorgehoben werden nur noch die Bits, die
sich dadurch ändern, und neue IDs (★). Mit **„Mitschnitt speichern“** landet
alles als candump-Datei im Fahrzeugordner. Eine eigene DBC-Datei zeigt
Botschaftsnamen und Signale; mit 📈 kommt ein Signal als Kurve in die Live-Grafik
(der Monitor muss dafür weiterlaufen). Geht mit SocketCAN-Adapter und im Demo-Fahrzeug
(dort mit erfundenem Verkehr und einem Blinker zum Ausprobieren).

Hinweis Golf 6: Am OBD-Stecker kommt durch das Gateway meist nur Diagnose-Verkehr an.

**Dateien ins Fenster ziehen:** Mitschnitt (`.log`, `.asc`), DBC, Aufzeichnung
(`.csv`) und Label-Datei (`.toml`) einfach auf das Fenster ziehen. Sie öffnen
sich auf der passenden Seite. Eine Datensicherung (`.zip`) wird dabei nicht
zurückgespielt, das geht nur bewusst über „Fahrzeuge & Wartung“.

## Eigene Label-Dateien (dein Inhalt)

Lege `*.toml`-Dateien in den Ordner `labels/` (eine pro Steuergerät). Format und
Beispiel siehe [`labels/beispiel.toml`](labels/beispiel.toml) und das Modul
[`src/labels.rs`](src/labels.rs). Für über 300 genormte Codes (P0xxx, P2xxx, U0xxx) sind **deutsche Grundtexte
eingebaut** (eigene Formulierungen, `src/dtc_db.rs`); deine Label-Texte haben
immer Vorrang. Bequemer im GUI-Reiter **„Label-Editor“** (Steuergerät, Fehlertexte, Messwerte;
legt beim Speichern eine `.toml.bak`-Sicherung an). Damit bekommen DTCs Klartextnamen:

```bash
./target/release/ecu-werkbank labels                      # zeigt geladene Labels
./target/release/ecu-werkbank -l meine_labels dtc --tx 0x7E0 --rx 0x7E8
```

> Die Texte formulierst du selbst (oder nimmst sie aus offenen, genormten
> Quellen). Fremde Label-Dateien anderer Diagnoseprogramme liest ECUWerkbank
> nicht — der Inhalt gehört dir.

**Vorbefüllen aus dem Scan:** Im Label-Editor legt der Knopf **„⬇ Aus letztem
Scan anlegen“** für jedes Steuergerät aus dem letzten Scan des aktiven Fahrzeugs
eine fertige Datei an — mit Adresse, Name, Teilenummer und CAN-IDs aus *deinem*
Scan. Nur Steuergeräte ohne vorhandene Datei werden angelegt, nichts wird
überschrieben. Fehlercodes und Messwerte ergänzt du dann im Editor. Das greift
vor allem bei TP-2.0/KWP-Steuergeräten (ältere Autos), weil nur die eine feste
Adresse melden.

Noch schneller geht es im GUI-Reiter **„Eigene Namen“** (Gruppe Werkzeuge):
Steuergerät-Adresse, Block und Feld eingeben, Namen tippen, speichern — der Name
gilt danach überall (Live-Werte, Testlauf, Rekorder, Kurven) und bleibt in
`messwerttexte.toml`. Das ist der empfohlene Weg für eigene Messwert-Namen.
Ganze Steuergeräte auf einmal: unter **„📋 Ganzes Steuergerät benennen“** die
Adresse eingeben und **„🔍 Alle Blöcke lesen“** — danach steht jedes Feld mit
seinem aktuellen Wert und der Art (Drehzahl, Temperatur …) in einer Liste, du
tippst nur noch die Namen ein. Mit Fortschritt („37 von 120 Feldern benannt“),
Filter „nur Felder ohne Namen“ und Suche.

## Testen ohne Auto und ohne sudo

```bash
cargo test --lib          # TP 2.0 + KWP2000 gegen simuliertes Steuergerät (im Speicher)
# Ende-zu-Ende in einem privaten Netz-Namespace (vcan-Modul muss ladbar sein):
unshare -rn bash -c 'ip link add dev vcan0 type vcan && ip link set up vcan0 &&
  ./target/release/mockecu vcan0 & sleep 0.5; ./target/release/ecu-werkbank -i vcan0 autoscan'
```

`mockecu` stellt das Demo-Fahrzeug auf einem CAN-Interface bereit (drei UDS-,
drei TP-2.0-Steuergeräte, OBD-II am Motor; Werte erfunden).

### Golf-6-Simulation (nach dem echten Scan nachgebaut)

Neben dem erfundenen Demo-Fahrzeug gibt es einen Nachbau des Golf 6 (2010, 1.4 TSI)
aus dem echten Scan vom 28.10.2026: dieselben 17 Steuergeräte (5 UDS, 12 TP 2.0) mit
Adressen, CAN-IDs, Teilenummern und den 5 Fehlern (08851, P1000, 01627, 00097, 00929).
Auch die Eigenheiten sind drin: Motor (MED17) kennt nur `18 00`, antwortet erst mit
„Antwort folgt“, ABS meldet beim ersten Mal „besetzt“, Adresse 0x29 beantwortet nur den
Kanalaufbau, der Motor auf 0x7E0 spricht nur OBD-II, das Kombi meldet „Test nicht
abgeschlossen“-Einträge (keine Fehler).

```bash
ecu-werkbank -t golf6 report --voll           # Terminal
VAG_TRANSPORT=golf6 ecu-werkbank-gui          # GUI (oder oben „Demo“ → „Golf 6“ wählen)
VAG_DEMO=golf6 mockecu vcan0              # auf einem (virtuellen) CAN-Bus
```

Seit dem Testlauf am Auto (29.10.2026) haben auch die Motorblöcke 1–4 (8 Felder je
Block, wie beim MED17) und der Kilometerstand im Kombi den echten Aufbau. **Nicht echt**
sind die Fahrgestellnummer (erfunden, damit dein echter Fahrzeugordner unberührt bleibt)
und die Werte selbst — die kommen aus der Simulation.

## Kilometerstand aus dem Kombi (Golf 6)

Beim Golf 6 gibt das Motorsteuergerät über OBD keinen Kilometerstand heraus. ECUWerkbank
liest ihn darum beim Scan aus dem Kombiinstrument: UDS-Kennung 0x2203, 2 Byte in
10-km-Schritten (am Golf 6 2010 geprüft: `5E B8` = 24 248 → 242 480 km bei 242 488 km im
Tacho). Der Wert ist also auf 10 km abgerundet. Er landet im Protokoll und im
Fahrzeugordner. Zum Suchen in anderen Steuergeräten gibt es `rdid-scan` (nur lesen):

```
ecu-werkbank rdid-scan --tx 0x714 --rx 0x77E --von 0x2200 --bis 0x22FF --suche 242488
```

**Genauer Stand:** Wo das Kombi den km-genauen Stand ablegt, ist nicht veröffentlicht.
Einmal suchen (nur lesen, bis 1 Minute; Tacho genau ablesen, Auto nicht bewegen):

```
ecu-werkbank km-genau --tacho 242553   # sucht 0x2000–0x23FF, 0x0200–0x02FF, 0x1000–0x10FF im Kombi
ecu-werkbank km-genau                  # danach: genauen Stand lesen
```

Eine eindeutige Stelle wird im `profil.toml` des Fahrzeugs gemerkt (`[km_quelle]`); ab dann
nimmt jeder Scan den genauen Wert — aber nur, wenn er zum 10-km-Wert aus 0x2203 passt.
Ohne gemerkte Stelle versucht der Scan die übliche Hersteller-Kennung 0x2002 (3 Byte), ebenfalls nur
mit dieser Gegenprobe. GUI: Fahrzeug → Fahrzeuge & Wartung → Tachostand ins km-Feld,
„🔍 Stelle im Kombi suchen“. Endet der Tachostand auf **0**, wird nichts gemerkt: dann passt
auch jeder Wert in 10-km-Schritten, eine km-genaue Stelle ist nicht erkennbar. Erst suchen,
wenn die letzte Ziffer 1–9 ist. **Am Golf 6 (2010, Kombi 5K0920860F) geprüft am 07.11.2026:**
Das Kombi gibt den genauen Stand über die Diagnose **nicht** heraus (108 lesbare Kennungen,
kein Treffer bei Tacho 242 847; 0x2002 liefert nichts Passendes) — dort bleibt 0x2203 mit
10-km-Schritten die Quelle. Die Golf-6-Simulation hat einen genauen Stand nur mit
`VAG_SIM_KM_GENAU=1`.

## Service-Anzeige: lesen und zurückstellen

GUI: Fahrzeug → Service-Anzeige. CLI: `ecu-werkbank -f <Fahrzeug> service …`

Die Kennungen der Service-Werte im Kombi sind nicht veröffentlicht und je Kombi anders.
Sie stehen darum je Fahrzeug in `fahrzeug.toml` unter `[service]` und werden einmal gelernt:

```
ecu-werkbank -f "M-AB 123" service merken --name vorher     # nur lesen, ~1 min
#   Service mit den Tasten am Kombi zurückstellen  (ecu-werkbank service anleitung)
ecu-werkbank -f "M-AB 123" service merken --name nachher
ecu-werkbank -f "M-AB 123" service vergleich --uebernehmen  # geänderte Kennungen = Service-Werte
ecu-werkbank -f "M-AB 123" service                          # lesen (auch im Schnell-Check)
ecu-werkbank -f "M-AB 123" service zuruecksetzen --ja       # sichert vorher, liest nach, trägt Ölwechsel in den Wartungsplan ein (--ohne-wartung: nicht)
ecu-werkbank -f "M-AB 123" service rueckgaengig --ja        # alte Werte aus der Sicherung
```

Ändern sich Kennungen auch ohne Rückstellung (Uhrzeit …), einen dritten Stand
`--name ruhe` merken und `vergleich --ruhe ruhe` nutzen.

**Ohne Tasten-Rückstellung einrichten:** Anzeige im Kombi ablesen und suchen lassen. Mit
`--einrichten` werden eindeutige Treffer übernommen; bei der Rückstellung wird dann in
das gefundene Feld das neue Intervall geschrieben (übrige Bytes bleiben, wie sie sind):

```
ecu-werkbank -f "M-AB 123" service suche --km 4800 --tage 212 --oel-km 1200 --oel-tage 90 \
         --einrichten --intervall 30000,730,15000,365   # km,Tage Inspektion · km,Tage Ölwechsel (Serviceheft!)
ecu-werkbank -f "M-AB 123" service zuruecksetzen            # Vorschau, dann mit --ja
```

In der GUI: „Rückstellung einrichten“ → Anzeige-Werte eintragen → „Kombi jetzt lesen“ →
„Passende Kennungen suchen“ → je Wert einen Treffer anhaken → „Als Service-Rückstellung
einrichten“. Ein Treffer passt nur zufällig zur Zahl, wenn mehrere Kennungen denselben
Wert haben — darum je Wert genau einen wählen; die Sicherung macht jede Rückstellung
rückgängig. Verlangt das Kombi
eine Anmeldung (NRC 0x33), bricht die Rückstellung ab, und es wird nichts umgangen.
Das Demo-Fahrzeug hat Demo-Kennungen (0x22A0–0x22A3 am Motor, nur Simulation); im Golf-6-
Nachbau gibt es keine, weil die echten noch nicht gelernt sind.

## Umgebungsbedingungen bei TP-2.0-Fehlern

Bei UDS-Steuergeräten zeigt ECUWerkbank schon, unter welchen Bedingungen ein Fehler
auftrat. Für TP 2.0 (Golf 6: Motor, ABS, Türen …) fragt ECUWerkbank jetzt zu jedem Fehler
zusätzlich den Dienst 0x17 (Status des Fehlercodes, ISO 14230) ab — nur lesend. Der
Aufbau der Antwort ist vom Hersteller nicht offen beschrieben, darum stehen die Daten vorerst
als „Umgebungsdaten (roh)“ im Protokoll, in `kwp-dtc` und in der GUI. Kennt ein
Steuergerät den Dienst nicht, wird nicht weiter gefragt; der Fehlerspeicher wird
trotzdem normal gelesen.

Am Golf 6 (2010) kennt **kein** TP-2.0-Steuergerät den Dienst (Antwort `7F 17 11`), und
Freeze-Frames (0x12) werden mit „Bedingungen nicht erfüllt“ abgelehnt. Dort gibt es zu
TP-2.0-Fehlern also keine Umgebungsdaten — das ist kein Fehler von ecu-werkbank.

## Startseite: Fahrzeug auf einen Blick

Für das gewählte Fahrzeug zeigt die Startseite vier Karten (Kilometerstand mit km/Jahr,
gespeicherte Fehler mit Vergleich zum Scan davor, letzter Scan, nächste Wartung) und die
Steuergeräte mit Fehlern — ein Klick öffnet das Steuergerät unter „Steuergeräte“ zum Lesen
oder Löschen. Alles aus den gespeicherten Daten, ohne Verbindung. Kilometer- und
Fehlerverlauf (ab zwei Scans) stehen unter **Fahrzeug → Historie & Trends**.

### Fahrzeug automatisch erkennen

Die GUI merkt von selbst, wenn ein Auto angesteckt bzw. die Zündung eingeschaltet wird.
Rechts neben der Adapter-Anzeige steht dann „🚗 *Fahrzeug*“ statt „kein Auto“, und das
Fahrzeug wird anhand der Fahrgestellnummer gewählt (neu: angelegt). Gefragt wird nur
lesend und nur, wenn der Bus seit 5 s frei ist: erst das Kombi (UDS 0x714, Kennung
0xF190), sonst OBD Mode 09. Danach alle 10 s ein „Tester Present“ an dasselbe
Steuergerät — antwortet es nicht mehr, heißt es „Auto getrennt“. Ohne Auto (Adapter in
ERROR-PASSIVE) wird nicht dauernd gesendet, sondern nur auf die Zähler des Adapters
geschaut. Nur SocketCAN und Demo, nicht ELM327. Klick auf die Anzeige schaltet die
Erkennung aus/an (gemerkt in `~/.local/share/ecu-werkbank/erkennung-aus`).

## Stand & Fahrplan

Ausführlich: [ROADMAP.md](ROADMAP.md) (50 Funktionen nach Nutzen/Aufwand),
Aufbau: [ARCHITECTURE.md](ARCHITECTURE.md), Sicherheit: [SECURITY_REVIEW.md](SECURITY_REVIEW.md),
Änderungen: [CHANGELOG.md](CHANGELOG.md), Mitmachen: [CONTRIBUTING.md](CONTRIBUTING.md),
Erweitern: [PLUGIN_DEVELOPMENT.md](PLUGIN_DEVELOPMENT.md).

Funktionsumfang gängiger Werkstatt-Diagnose (Funktionsnummern nach VAG-155x/VAS-505x)
und Stand in ECUWerkbank:

| Nr. | Funktion | UDS (ab ~2008) | TP 2.0 / KWP2000 |
|---|---|---|---|
| 00 | Auto-Scan / Report (PDF) | ✅ `autoscan`, `report` | ✅ `autoscan`, `report` |
| 01 | Steuergerät-Identifikation | ✅ `rdid` (0xF187, 0xF190 …) | ✅ `kwp-ident` (Softwarestand am Golf 6 bestätigt) |
| 02 | Fehlerspeicher lesen | ✅ `dtc` (VW-Fehlernummern wie beim Werkstattgerät) | ✅ `kwp-dtc` (Fehlerart am Golf 6 bestätigt) |
| 02 | … mit Umgebungsbedingungen | ✅ Snapshot je Fehler (km, Drehzahl … — unbekannte Werte roh) | 🟡 Steuergeräte am Golf 6 lehnen ab; per Auto-Scan-Import ✅ |
| – | Auto-Scan-Protokolle einlesen (Verlauf, Umgebung, Fehlertexte) | ✅ `autoscan-import` | ✅ `autoscan-import` |
| 03 | Stellglieddiagnose | 🟡 `iocontrol` (roh) | ✅ `kwp-stellglied`, GUI Steuergeräte → Steuergerät wählen → „▶ Stellglieddiagnose“ (Ablauf per Mitschnitt an der Golf-6-Tür gelernt) |
| 04 | Grundeinstellung | 🟡 `routine` (roh) | 🟡 nur nach eigenem Mitschnitt (GUI) |
| 05 | Fehlerspeicher löschen | ✅ `clear` | ✅ `kwp-clear` |
| 06 | Ausgabe beenden | ✅ | ✅ Kanal wird sauber geschlossen |
| 07 | Codierung | ✅ `coding`, `code-write` | ✅ lesen `kwp-coding` (lang, kurz und Gateway — am Golf 6 bestätigt); Kurzcodierung schreiben `kwp-codierung-schreiben` (Golf-6-Tür geprüft); lange Codierung schreiben `kwp-langcodierung --neu` (🟡 eingebaut, am Auto noch Probelauf) |
| – | Codierungs-Sicherung / Vergleich / Zurückschreiben | ✅ `coding-*` | 🟡 nur sichern |
| – | Fahrzeug-Historie, Vorher/Nachher | ✅ `history`, `compare` | ✅ |
| 08 | Messwertblöcke | ✅ `rdid` + Labels | ✅ `kwp-block`, `kwp-blockscan`, Namen in einer Liste; Formeln am Golf 6 mit Werkstattgerät verglichen |
| 09 | Einzelmesswerte | – | ❌ |
| 10 | Anpassung | 🟡 sichern/vergleichen `anpassung-*`, Kennungen benennen; Schreiben nur mit Login | 🟡 Kanäle lesen `kwp-anpassung` (Golf 6 Bordnetz/Einparkhilfe geprüft), sichern; Schreiben `kwp-anpassung-schreiben` (🟡 eingebaut, am Auto noch Probelauf) |
| 11/16 | Login / Zugriffsberechtigung | 🟡 `login` (SecurityAccess, Bytecode vom Nutzer) | 🟡 `login --kwp` (am Auto ungeprüft) |
| 15 | Readiness | ✅ `obd-info` + Fahranleitung (GUI) | – |
| 18 | Fehlerpfade (alle möglichen Fehlercodes) | ✅ `fehler-geruest`, GUI Werkzeuge → Fehlerbilder | ❌ Dienst gibt es bei KWP nicht |
| 1A | Erweiterte Identifikation | ✅ `ident` (inkl. Betriebsnummer der letzten Codierung) | 🟡 `kwp-ident` |
| 33 | OBD-II/EOBD (Modi 01, 02, 03, 04, 07, 09, 0A) | ✅ `obd-*`, GUI „OBD-II“ (inkl. Freeze-Frame) | – |
| – | Messwerte protokollieren (CSV) | ✅ `obd-live --csv` | ✅ `kwp-block --csv` |
| – | Mitschnitt aufzeichnen/auswerten | ✅ `record`, `decode-log` | ✅ inkl. Prüfbericht |
| – | Mitschnitt als PCAP/ASC, DBC-Signale | ✅ `log-export`, `dbc-decode` | ✅ |
| – | Live-CAN-Monitor mit Rauschfilter | ✅ GUI „CAN-Monitor“ (nur zuhören) | ✅ |
| – | Steuergerät neu starten, Fehleranzahl | ✅ `ecu-reset` (mit `--ja`), `dtc-anzahl` | – |
| – | Service-Anzeige lesen / zurückstellen | 🟡 `service` (Kennungen je Fahrzeug lernen; Golf 6: km seit Service bestätigt) | – |
| – | Verbauliste (verbaute Steuergeräte, nach System) | ✅ `verbauliste`, GUI Steuergeräte → „📋 Verbauliste nach System“ | ✅ aus dem Scan |
| – | Gateway-Verbauliste direkt aus dem Gateway | ⏳ geplant (neuere Autos mit UDS-Gateway) | ✅ `gateway-verbauliste`, GUI-Knopf (Golf 6 geprüft) |
| – | Transportmodus | ❌ | ❌ |
| – | K-Line (vor ~2004) | – nicht über SocketCAN | – |

✅ geht · 🟡 teilweise · ⏳ geplant · ❌ geht nicht · – gibt es dort nicht

Als Nächstes:
- Restliche Messwert-Formeln (Bremsdruck, Beschleunigung, Drehrate …) mit einer
  gleichzeitigen Vergleichsmessung festnageln
- KWP2000: lange Codierung und Anpassung schreiben am Golf 6 per Mitschnitt bestätigen
  (bis dahin zeigt ECUWerkbank am echten Auto nur, was es senden würde), Grundeinstellung
- Umgebungsbedingungen bei TP-2.0-Fehlern direkt lesen (Mitschnitt am Golf auswerten)
- Gateway-Verbauliste bei neueren UDS-Gateways
- Erkenntnisse vom Golf 6 an einem zweiten Auto (Polo 6R) prüfen
- **Steuergeräte-Adresskarte** aus eigenen `discover`-Mitschnitten

## Lizenz, Marken, Quellen

ECUWerkbank steht wahlweise unter der **MIT-Lizenz** ([LICENSE-MIT](LICENSE-MIT)) oder der
**Apache-Lizenz 2.0** ([LICENSE-APACHE](LICENSE-APACHE)). © 2026 dflow81.

**Marken:** ECUWerkbank ist ein unabhängiges Projekt und **nicht** mit der Volkswagen AG,
ihren Marken (VW, Audi, Seat, Škoda) oder mit Ross-Tech LLC verbunden. Genannte
Marken gehören ihren Inhabern und werden nur beschreibend verwendet.

**Fremde Daten:** ECUWerkbank liefert **keine** Label-Dateien, Datenbanken oder
Schlüssel von Ross-Tech oder Volkswagen mit und liest keine Label-Dateien anderer
Diagnoseprogramme. Login-Verfahren (SA2) muss der
Nutzer selbst vorgeben. Protokolle (ISO-TP, UDS, KWP2000, TP 2.0) sind nach
öffentlichen Normen bzw. öffentlicher Community-Dokumentation selbst umgesetzt.

**Quellen der Login-Prüfwerte:** Die Prüfwerte der Seed/Key-Verfahren (SA2, KWP-XOR)
stammen aus öffentlichen Projekten; Herkunft und MIT-Hinweis siehe
[CREDITS.md](CREDITS.md).

**Mitgelieferte Bibliotheken:** alle unter freien Lizenzen (MIT, Apache-2.0, BSD, Zlib …;
Liste: `cargo metadata`). Die Oberfläche enthält über `egui` Schriften unter der
SIL Open Font License und der Ubuntu Font Licence — bei Weitergabe fertiger Programme
deren Lizenztexte beilegen.
