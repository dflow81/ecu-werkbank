# Roadmap

Stand: 18.11.2026. Vergleich mit den bekannten Werkstatt- und Hersteller-Programmen
(VCDS, ODIS, ISTA+, E-Sys, Xentry, Autel MaxiSys) — **nur nach dem, was diese
Programme öffentlich können**, nicht nach ihren Daten. ECUWerkbank übernimmt keine
Inhalte (Labels, Datenbanken, Flash-Dateien, Schlüssel) aus diesen Programmen.

**Grenzen eines offenen Projekts:** Hersteller-Programme leben von Daten, die nur der
Hersteller hat (Steuergeräte-Beschreibungen, Flash-Container, Online-Freischaltungen wie
SFD/FEC/Login-Server). Diese Lücke schließt ECUWerkbank über **eigene, offene Daten**:
Mitschnitte, eigene Labels, gelernte Abläufe — Schritt für Schritt und nachprüfbar.

Legende: Nutzen/Aufwand/Risiko = niedrig · mittel · hoch. Risiko = Gefahr für Auto oder Daten.
Status: ✅ erledigt · 🟡 teilweise · ⏳ offen

## Die 50 wichtigsten fehlenden Funktionen

| # | Funktion | Vorbild | Nutzen | Aufwand | Risiko | Priorität | Stufe | Status |
|---|---|---|---|---|---|---|---|---|
| 1 | UDS ECUReset (0x11) | alle | mittel | niedrig | niedrig | hoch | Quick Win | ✅ `ecu-reset` |
| 2 | UDS CommunicationControl / ControlDTCSetting (0x28/0x85) | ODIS, E-Sys | mittel | niedrig | mittel | hoch | Quick Win | ✅ Bausteine in `uds.rs` |
| 3 | UDS Download/Upload/TransferData/Exit (0x34–0x37) | ODIS, E-Sys, Xentry | hoch | niedrig | mittel | hoch | Quick Win | ✅ Bausteine (kein Flash-Ablauf) |
| 4 | Fehleranzahl und erweiterte Fehlerdaten (0x19 01/06) | alle | mittel | niedrig | niedrig | hoch | Quick Win | ✅ |
| 5 | Vollständige NRC-Klartexte | alle | mittel | niedrig | niedrig | hoch | Quick Win | ✅ |
| 6 | Begrenztes Warten bei „Antwort folgt“ | – | mittel | niedrig | niedrig | hoch | Quick Win | ✅ 60 s |
| 7 | Mitschnitt als PCAP (Wireshark) / ASC | Autel, CANalyzer | mittel | niedrig | niedrig | hoch | Quick Win | ✅ `log-export` |
| 8 | DBC-Signale entschlüsseln | CANalyzer, SavvyCAN | hoch | niedrig | niedrig | hoch | Quick Win | ✅ `dbc-decode` |
| 9 | Codier-Helfer: Rückgängig / alles zurück | VCDS, ODIS | mittel | niedrig | niedrig | hoch | Quick Win | ✅ |
| 10 | Handy-Anzeige mit Zugangsschlüssel | – | mittel | niedrig | niedrig | hoch | Quick Win | ✅ |
| 11 | TP 2.0: lange Codierung schreiben | VCDS | hoch | mittel | hoch | hoch | 1.0 | 🟡 eingebaut, Probelauf bis Mitschnitt |
| 12 | TP 2.0: Anpassung schreiben | VCDS | hoch | mittel | hoch | hoch | 1.0 | 🟡 eingebaut, Probelauf bis Mitschnitt |
| 13 | TP 2.0: Einzelmesswerte (Funktion 09) | VCDS | niedrig | niedrig | niedrig | mittel | 1.0 | ⏳ |
| 14 | TP 2.0: Umgebungsbedingungen direkt | VCDS | mittel | mittel | niedrig | mittel | 1.0 | ⏳ |
| 15 | UDS: Anpassungskanäle mit Namen + Schreiben ohne Login, wo erlaubt | VCDS, ODIS | hoch | mittel | mittel | hoch | 1.0 | 🟡 lesen/sichern |
| 16 | UDS: Gateway-Verbauliste (neuere Gateways) | VCDS, ODIS | hoch | mittel | niedrig | hoch | 1.0 | ⏳ Mitschnitt nötig |
| 17 | Geführte Funktionen (Bremsbelag-Wechselstellung, Lenkwinkel-Grundeinstellung, Drosselklappe) | VCDS, Autel | hoch | mittel | hoch | hoch | 1.0 | 🟡 Drosselklappe/KWP |
| 18 | Service-Rückstellung für weitere Modelle | Autel, VCDS | hoch | mittel | mittel | hoch | 1.0 | 🟡 Golf 6 |
| 19 | Batterie anlernen (BEM/Batteriewechsel) | Autel, ISTA+ | hoch | mittel | mittel | hoch | 1.0 | ⏳ |
| 20 | Herstellerdaten als Daten-Ordner (`manufacturers/<marke>/`) | alle | hoch | mittel | niedrig | hoch | 1.0 | ⏳ Plan unten |
| 21 | Steuergerätebaum (Bus → Gateway → Steuergeräte) in der GUI | ODIS, ISTA+ | mittel | mittel | niedrig | mittel | 1.0 | ⏳ |
| 22 | Live-CAN-Monitor in der GUI (IDs, Rate, Änderungen) | SavvyCAN, Autel | hoch | mittel | niedrig | hoch | 1.0 | ⏳ (CLI `record` vorhanden) |
| 23 | DBC in der GUI (Signale live, ins Scope) | CANalyzer | hoch | mittel | niedrig | mittel | 1.0 | ⏳ |
| 24 | Mitschnitt abspielen (Replay auf vcan) | CANalyzer | mittel | niedrig | mittel | mittel | 1.0 | ⏳ nur auf vcan, nie am Auto |
| 25 | Bus-Arbeit der GUI im Hintergrund (kein Einfrieren) | – | hoch | mittel | niedrig | hoch | 1.0 | ✅ `gui/hintergrund.rs` |
| 26 | Linux-CI: Tests + Clippy + cargo audit bei jedem Push | – | hoch | niedrig | niedrig | hoch | 1.0 | ⏳ |
| 27 | CLI in Gruppen-Dateien aufteilen | – | mittel | mittel | niedrig | mittel | 1.0 | ⏳ |
| 28 | Integrationstests über `vcan0` + `mockecu` | – | mittel | mittel | niedrig | mittel | 1.0 | 🟡 mockecu vorhanden |
| 29 | Codierung: Klartext-Assistent mit Abhängigkeiten (Bit A nur mit Bit B) | ODIS, E-Sys | hoch | mittel | mittel | mittel | 2.0 | ⏳ |
| 30 | Fahrzeug-Konfiguration als Ganzes vergleichen (vorher/nachher, zwei Autos) | E-Sys | mittel | niedrig | niedrig | mittel | 1.0 | 🟡 `compare`, Nachrüst-Assistent |
| 31 | DoIP (Diagnose über Ethernet, neuere VW/BMW) | ODIS, ISTA+, E-Sys | hoch | hoch | mittel | mittel | 2.0 | ⏳ |
| 32 | 29-Bit-CAN-IDs und CAN FD | Xentry, Autel | mittel | mittel | niedrig | mittel | 2.0 | 🟡 Mitschnitt liest sie |
| 33 | K-Line (KWP1281/KWP2000 vor ~2004) | VCDS | mittel | hoch | mittel | niedrig | 2.0 | ⏳ braucht anderen Adapter |
| 34 | J2534 Pass-Thru-Adapter (Windows) | ODIS, Xentry | hoch | hoch | mittel | mittel | 2.0 | ⏳ |
| 35 | Weitere Marken: Basis-Diagnose Opel/Ford/Mercedes/BMW über UDS + OBD | Autel | hoch | hoch | mittel | mittel | 2.0 | 🟡 `fremd.rs` findet Steuergeräte |
| 36 | ECU-Sicherung (Speicher auslesen, wo erlaubt) | Autel, Flash-Tools | mittel | mittel | mittel | mittel | 2.0 | ⏳ Baustein `read_memory` |
| 37 | Flash-Ablauf (siehe Abschnitt Flashing) | ODIS, E-Sys, Xentry | hoch | hoch | **hoch** | niedrig | Vision | ⏳ |
| 38 | Spannungsüberwachung während langer Abläufe | ODIS, ISTA+ | hoch | niedrig | niedrig | hoch | 1.0 | 🟡 Batterietest vorhanden |
| 39 | REST-Schnittstelle (lokal, mit Schlüssel) | – | mittel | mittel | mittel | mittel | 2.0 | ⏳ (`web.rs` als Grundlage) |
| 40 | Skripte (Lua oder Rhai, eingebettet) | – | mittel | mittel | mittel | niedrig | 2.0 | ⏳ |
| 41 | Python-Anbindung (pyo3 oder REST) | – | mittel | mittel | niedrig | niedrig | Vision | ⏳ |
| 42 | Plugin-Schnittstelle für Fahrzeug-Funktionen | – | mittel | hoch | mittel | niedrig | Vision | ⏳ |
| 43 | Fehler-Erklärung lokal (eigene Wissensbasis + Wahrscheinlichkeit aus eigener Historie) | Autel | mittel | mittel | niedrig | mittel | 2.0 | 🟡 Fehlerbilder, Prüfpläne |
| 44 | Mitschnitt-Analyse mit Auffälligkeiten (Zeitüberschreitungen, NRC-Häufung) | – | mittel | niedrig | niedrig | mittel | 1.0 | 🟡 Prüfbericht in `canlog` |
| 45 | MDF4-/BLF-Export | CANalyzer | niedrig | hoch | niedrig | niedrig | Vision | ⏳ |
| 46 | Dashboard / frei belegbare Instrumente | Autel | mittel | mittel | niedrig | mittel | 2.0 | 🟡 Armaturen/Scope |
| 47 | Drag-and-Drop (Mitschnitte, Sicherungen, DBC ins Fenster ziehen) | – | niedrig | niedrig | niedrig | mittel | 1.0 | ⏳ |
| 48 | Responsive Layout (kleine Bildschirme, Tablet) | Autel | mittel | mittel | niedrig | mittel | 2.0 | 🟡 |
| 49 | Berichte mit Firmenkopf / Kunde (Werkstattbetrieb) | Autel, ODIS | mittel | niedrig | niedrig | mittel | 1.0 | 🟡 PDF vorhanden |
| 50 | Online-Funktionen der Hersteller (SFD, FEC, Online-Login) | ODIS, E-Sys | – | – | – | **nicht geplant** | – | ❌ nur mit Herstellerzugang, nicht offen nachbaubar |

## Stufen

**Quick Wins (erledigt am 18.11.2026):** Punkte 1–10.

**Version 1.0 — „Golf 6 vollständig, zweites Auto geprüft“:**
11/12 am Auto bestätigen, 13–19, 20 (Herstellerdaten), 21–28, 30, 38, 44, 47, 49.
Prüfstein: Polo 6R und Golf 6 komplett ohne Werkstattgerät diagnostizierbar.

**Version 2.0 — „neuere Autos und andere Marken“:**
29, 31–36, 39, 40, 43, 46, 48.

**Langfristige Vision:** 37 (Flashen), 41, 42, 45.

## Plan: Herstellerdaten (Punkt 20)

Heute steckt Fahrzeugwissen in `systems.rs`, `kwp::tp20_name`, `profil.rs`, `grundeinstellung.rs`
und `labels/`. Ziel: ein Daten-Ordner je Hersteller, ohne neu zu bauen erweiterbar:

```
manufacturers/
  vw/        hersteller.toml   steuergeraete.toml   messwerte/   fehlertexte/   routinen.toml   codierung/
  audi/ seat/ skoda/           (erben von vw: erbt = "vw")
  bmw/ mercedes/ opel/ ford/   (zunächst nur OBD + gefundene UDS-Adressen)
```

- Inhalt nur aus eigenen Mitschnitten, offenen Normen und öffentlich freien Quellen — Herkunft je Eintrag (`quelle = "Mitschnitt Golf 6, 17.11.2026"`).
- Lesen über ein neues Modul `hersteller.rs`; die bisherigen Tabellen werden zu Standardwerten im Code und nach und nach in Dateien verschoben.
- Einzelheiten zum Dateiformat: [PLUGIN_DEVELOPMENT.md](PLUGIN_DEVELOPMENT.md).

## Plan: Flashing (Punkt 37)

Bewusst zuletzt — ein Fehler macht ein Steuergerät unbrauchbar. Voraussetzungen, bevor
überhaupt ein Byte geschrieben wird:

1. Spannung laufend prüfen (≥ 12,5 V, Ladegerät Pflicht), Abbruch unter Grenzwert.
2. VIN, Teilenummer, Hardware- und Softwarestand gegen die Flash-Datei prüfen.
3. Vorher vollständige Sicherung (Codierung, Anpassungen, Identifikation).
4. Prüfsummen (CRC) der Datei und der übertragenen Blöcke; Blockzähler (in `uds::transfer_data` schon geprüft).
5. Wiederaufnahme nach Abbruch (Recovery: Bootloader-Sitzung erneut öffnen).
6. Nur offene Flash-Dateien (z. B. selbst gebaute Firmware für Prüf-Steuergeräte) — keine Hersteller-Container.
7. Erst am Prüfstand (`vcan` + Simulation, dann ein Steuergerät auf dem Tisch), nie zuerst am Auto.

## Plan: Entwicklerplattform (Punkte 39–42)

1. **REST** auf Basis von `web.rs`: nur `127.0.0.1` als Standard, Schlüssel wie bei der Handy-Anzeige;
   lesen frei, schreiben nur mit Bestätigung im Programm.
2. **Skripte**: eingebettete Sprache (Rhai oder Lua über `mlua`) mit denselben Funktionen wie die CLI.
3. **Python**: über die REST-Schnittstelle (kein eigener Build nötig).
