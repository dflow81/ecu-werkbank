# ECUWerkbank erweitern

Es gibt (noch) keine Plugins mit eigenem Programmcode. Erweitern kann man ECUWerkbank
heute über **Daten-Dateien**, ohne neu zu bauen, und über **Code** an festen Stellen.
Ein Daten-Ordner je Hersteller ist geplant ([ROADMAP.md](ROADMAP.md), Punkt 20).

Grundregel für alle Daten: **nur eigene Inhalte** — aus eigenen Mitschnitten, offenen
Normen oder ausdrücklich frei lizenzierten Quellen. Keine Label-Dateien, Datenbanken
oder Texte aus Werkstatt-Programmen, auch nicht umbenannt oder abgeschrieben.
Schreibt die Herkunft dazu.

## 1. Erweitern ohne neu zu bauen

| Was | Wo | Format | Befehl/GUI |
|---|---|---|---|
| Steuergeräte-Labels (Messwerte, Codier-Bits, Kennungen) | `~/.config/…/labels/<adresse>-<name>.toml` | TOML, Beispiel `labels/beispiel.toml` | Werkzeuge → Label-Editor |
| Namen für Messwertblöcke (TP 2.0) | Datenordner | TOML | `messwerttext`, Werkzeuge → Eigene Namen |
| Namen für Codier-Bits | Datenordner | TOML | `codiertext`, Seite Codierung |
| Namen für Anpassungs-Kennungen | Datenordner | TOML | Sicherung → Anpassungen |
| Fehlertexte | Datenordner | TOML | `fehlertext`, Fehlergerüst |
| Fehlerbilder (Erfahrungen je Modell) | Datenordner | TOML | Werkzeuge → Fehlerbilder |
| Grundeinstellungs-/Stellglied-Abläufe | aus eigenem Mitschnitt gelernt | TOML | Mitschnitt & Lernhilfe |
| Sprachen | `<Datenordner>/sprachen/<code>.toml` | TOML: deutscher Text = Übersetzung | `sprache vorlage <code>` |
| CAN-Signale | eigene `.dbc`-Datei | DBC | `dbc-decode --dbc … --log …` |

## 2. Geplantes Format: Herstellerdaten

```toml
# manufacturers/vw/hersteller.toml
name = "Volkswagen"
erbt = ""                      # audi/seat/skoda: erbt = "vw"
vin_wmi = ["WVW", "WV1", "WV2"]

# manufacturers/vw/steuergeraete.toml
[[steuergeraet]]
adresse = 0x09                 # Diagnoseadresse
name = "Bordnetzsteuergerät"
uds = { tx = 0x70E, rx = 0x778 }
tp20 = 0x20                    # TP-2.0-Kanal, falls vorhanden
quelle = "Gateway-Verbauliste Golf 6, eigener Mitschnitt 15.11.2026"

# manufacturers/vw/routinen.toml
[[routine]]
adresse = 0x01
name = "Drosselklappe anlernen"
art = "kwp-grundeinstellung"   # oder "uds-routine"
gruppe = 60
bedingungen = ["Zündung an", "Motor aus", "Gaspedal nicht betätigen"]
quelle = "eigener Mitschnitt"
```

## 3. Erweitern im Code

| Erweiterungspunkt | Datei | Was man tut |
|---|---|---|
| Neuer Adapter | `src/transport.rs` | Variante in `Backend` + `impl Transport` (nur `send`/`recv` einer ganzen Nachricht) |
| Neuer UDS-Dienst | `src/uds.rs` | Methode an `Uds`, Test mit dem Skript-Transport (`dienst_tests`) |
| Neuer KWP-Dienst | `src/kwp.rs` | Funktion über `Kwp::request`; Antwort-Zerleger als eigene `pub fn` mit Test aus echtem Mitschnitt |
| Simuliertes Steuergerät | `src/sim.rs` | in `demo_module()`/`golf6_module()` eintragen; Antworten in `uds_reply`/`kwp_responses` |
| Neue Messwert-Formel (TP 2.0) | `kwp::BlockValue::decode` | nur mit Vergleichsmessung am Auto, Datum im Kommentar |
| Neuer CLI-Befehl | `src/main.rs` (`enum Command` + `match`) | Schreibbefehle immer mit `--ja`, ohne `--ja` nur Vorschau |
| Neue GUI-Seite | `src/bin/gui/*.rs`, Eintrag in `GROUPS` (`gui.rs`) | Bus-Arbeit nie im Zeichen-Thread: kurze Aufträge mit `self.im_hintergrund(…)` (`gui/hintergrund.rs`), lange mit Fortschritt im eigenen Thread (siehe `tp20_tab.rs` Stellglieddiagnose) |
| Mitschnitt-Format | `src/spurexport.rs` | Funktion `Frame`-Liste → Bytes, Endung in `Format::aus_pfad` |

## 4. Regeln für Schreibfunktionen

1. Ohne Bestätigung nur Vorschau.
2. Vorher sichern, nachher neu lesen und vergleichen.
3. Neue Schreibwege erst mit Mitschnitt eines Werkstattgeräts bestätigen; bis dahin
   Probelauf am echten Auto (Muster: `kwp::LANGCODIERUNG_GEPRUEFT`, `kwp::darf_senden`).
4. Nie den Kilometerstand.
5. Test in `sim.rs`, der den ganzen Ablauf durchspielt.
