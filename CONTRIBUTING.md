# Mitmachen

Danke fürs Mitmachen! ECUWerkbank ist ein Hobby-Projekt für die offene Fahrzeugdiagnose.

## Was besonders hilft

- **Mitschnitte** (candump) von Abläufen eines Werkstatt-Diagnosegeräts am eigenen Auto,
  zusammen mit dem, was das Gerät angezeigt hat. Daraus entstehen bestätigte Funktionen.
  Vorher Fahrgestellnummer und Kennzeichen entfernen.
- **Rückmeldungen von anderen Autos** (Modell, Baujahr, Steuergeräte, was ging, was nicht).
- Fehlerberichte mit genauer Befehlszeile und Ausgabe.

## Was wir nicht annehmen können

- Label-Dateien, Datenbanken, Texte, Schlüssel oder Flash-Dateien aus Werkstatt- oder
  Hersteller-Programmen — auch nicht umbenannt, umformatiert oder abgeschrieben.
- Code, der Schutzmaßnahmen umgeht (Wegfahrsperre, Tachostand, Freischaltungen).

## Entwicklung

```bash
cargo build --release
cargo test --lib                  # ohne Auto, nutzt die Simulation
cargo clippy --all-targets        # soll ohne Warnung durchlaufen
./target/release/ecu-werkbank -t golf6 autoscan    # simulierter Golf 6
```

Tests nie gegen den echten Datenordner laufen lassen — eigene Ordner setzen:
`VAG_DATA_DIR`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME`. In Tests nur erfundene
Fahrgestellnummern verwenden (z. B. `WVWZZZ1KZAM000017`).

## Stil

- Deutsch in Texten, Kommentaren und Bezeichnern; Texte für die Oberfläche in `tr!(…)`.
- Einfache Sprache für Meldungen.
- Jede Erkenntnis aus dem Auto mit Datum und Herkunft kommentieren („am Golf 6 mitgeschnitten, 17.11.2026“).
- Schreibfunktionen nach den Regeln in [PLUGIN_DEVELOPMENT.md](PLUGIN_DEVELOPMENT.md#4-regeln-für-schreibfunktionen).
- Ein Commit je zusammenhängender Änderung; Änderungen für Nutzer in [CHANGELOG.md](CHANGELOG.md) eintragen.

## Aufbau

Siehe [ARCHITECTURE.md](ARCHITECTURE.md). Offene Aufgaben: [ROADMAP.md](ROADMAP.md).
