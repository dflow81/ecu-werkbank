# Änderungen

Alle für Nutzer wichtigen Änderungen. Neueste oben.

## 0.3.0 — 19.11.2026

### Neu
- **Ältere Steuergeräte (TP 2.0): lange Codierung schreiben** (Bordnetz, ABS, Einparkhilfe, Gateway) —
  `kwp-langcodierung --neu … --ja`, GUI Seite „Codierung“; vorher Sicherung, danach Kontrolle.
- **TP 2.0: Anpassungskanal ändern** — `kwp-anpassung-schreiben`, GUI „Anpassung → Wert ändern“.
- Beide sind am Auto noch nicht per Mitschnitt bestätigt: **am echten Auto läuft nur ein Probelauf**
  (zeigt die Anfrage, sendet nichts). In der Simulation wird geschrieben.
- UDS-Dienste: ECUReset (`ecu-reset`, nur mit `--ja`), Fehleranzahl (`dtc-anzahl`), erweiterte
  Fehlerdaten, CommunicationControl, ControlDTCSetting, Speicher lesen, Download/Upload-Bausteine.
- Mitschnitte umwandeln: `log-export --out x.pcap` (Wireshark) oder `x.asc` (Vector/SavvyCAN).
- Eigene DBC-Dateien: `dbc-decode --dbc … --log …` zeigt CAN-Signale im Klartext.
- Codier-Helfer: **Rückgängig** und **Alles zurück**.
- **Live-CAN-Monitor** (Werkzeuge → CAN-Monitor): alle IDs mit Daten, Rate und geänderten Bits,
  „Rauschen merken“ zum Finden von Signalen, Mitschnitt speichern, eigene DBC. Nur zuhören.
- DBC-Signale aus dem CAN-Monitor mit 📈 in die **Live-Grafik** übernehmen (mit Grenzwert-Warnung und CSV).
- **Fahrzeug-Datenbank** (SQLite, nur lokal): `ecu-werkbank db` mit `eingebaut`, `labels`, `dbc`,
  `quellen`, `entfernen`, `sichern`. Jede Quelle mit Lizenz; selbst gekaufte Herstellerdaten
  (ODX/PDX, später) bleiben technisch gesperrt für die Weitergabe. Fehlertexte aus der Datenbank
  erscheinen überall (GUI, Befehle) — nach den eigenen Labels, vor der eingebauten Liste.
- Datenpakete (TOML/JSON) und CSV übernehmen, alles Weitergebbare als Paket exportieren
  (`db paket`, `db csv`, `db export`); mitgelieferte Community-Daten unter `daten/community/`.
- **Eigene Texte für Messwerte mit Textnummer** (TP 2.0, Formel F25: „roh F25 02 FD“ → „normal“):
  `wertetexte-lernen` gleicht den Block-Export des eigenen Werkstattgeräts mit den eigenen Rohwerten ab,
  `wertetext` setzt einzelne von Hand. Die Tabelle bleibt nur auf diesem Rechner (`wertetexte.toml`).
- **Armaturen frei belegbar:** je Instrument Rundinstrument, große Zahl oder Balken (Rechtsklick);
  Messwerte samt Darstellung als **Ansicht** speichern und laden (z. B. „Warmfahren“).
- **Steuergerätebaum:** Gateway → Bus (Antrieb, Komfort, Infotainment …) → Steuergeräte, abgeglichen mit
  dem letzten Scan — zeigt, was laut Gateway verbaut ist, aber nicht antwortet.
- **29-Bit-CAN-Adressen** (ISO 15765-4, z. B. `18DA10F1`): SocketCAN, ELM327 und Simulation;
  „Andere Marken“ findet solche Steuergeräte jetzt mit (ein Rundruf „Tester Present“).
- **Skripte** (Rhai): eigene Abläufe mit Lese-Funktionen (OBD, UDS, TP 2.0, Fehler), `skript datei.rhai`
  oder Werkzeuge → Skripte; Beispiel `beispiele/messwerte.rhai`. Keine Schreibfunktionen.
- `db odx --datei … --hersteller …`: ODX/PDX-Dateien (rechtmäßig erworben) übernehmen — Steuergeräte,
  Fehlertexte, Lese-Kennungen. Bleiben immer lokal: kein Export, nicht über die Schnittstelle.
- `ecu-werkbank api`: REST-Schnittstelle auf die Fahrzeug-Datenbank — nur lesen, nur dieser Rechner,
  zufälliger Schlüssel in der Adresse; Python-Beispiel in `beispiele/api_abfrage.py`.
- `konfig-vergleich`: Codierung, Anpassungen und Softwarestände in einem Bericht vergleichen —
  vorher/nachher oder zwei Fahrzeuge (`--a`, `--b`).
- **Codier-Bits mit Abhängigkeiten:** in eigenen Labels `benoetigt = ["1.3"]` / `schliesst_aus = ["1.4"]`;
  der Codier-Helfer und `decode-coding`/`kwp-langcodierung` warnen, wenn eine Regel verletzt ist.
  Fahrzeug-Datenbank dafür auf Schema 2 (Umstellung automatisch, mit Kopie vorher).
- **Firmenkopf für Berichte:** Werkstattname, Anschrift, Telefon in der Fußzeile jeder PDF-Seite
  (`firmenkopf --name …`, GUI Fahrzeug → Fahrzeuge & Wartung).
- Mitschnitt-Prüfbericht mit **Auffälligkeiten**: Antwortzeiten, fehlende Antworten, gehäufte
  Ablehnungen, Pausen (`decode-log`, GUI Mitschnitt).
- `replay`: Mitschnitt auf ein **virtuelles** CAN-Interface (vcan) abspielen, z. B. für den
  CAN-Monitor. Echte Adapter werden abgelehnt.
- **Dateien ins Fenster ziehen:** Mitschnitt (.log/.asc), DBC, Aufzeichnung (.csv) und Label (.toml)
  öffnen sich auf der passenden Seite.
- Dokumentation: ARCHITECTURE, ROADMAP, CONTRIBUTING, PLUGIN_DEVELOPMENT, SECURITY_REVIEW.

### Geändert
- **Bordspannung wird vor jedem Schreiben geprüft** (Codierung, Anpassung, Service-Rückstellung,
  Routinen, Stellglieder): unter 12,0 V wird nichts geschrieben. Ist sie nicht lesbar, wird nicht blockiert.
- **Fenster friert nicht mehr ein**, wenn ein Steuergerät langsam oder gar nicht antwortet: alle
  Abfragen ans Auto laufen im Hintergrund, der Reihe nach. Ein Drehrad unten links zeigt, dass
  gerade gearbeitet wird; doppelte Klicks werden abgewiesen.
- TP-2.0-Adressen werden immer hexadezimal gelesen, mit oder ohne `0x` (`20` = Bordnetz).
- Handy-Anzeige: Adresse enthält einen zufälligen Schlüssel; andere Geräte im WLAN sehen nichts mehr.
- Warten auf „Antwort folgt“ (UDS) höchstens 60 s.

### Für Entwickler
- `tokio` auf 1.54.1 (Sicherheitsmeldung RUSTSEC-2026-0384; kommt über den Datei-Dialog).
- Automatische Prüfung bei jedem Push: Tests, clippy ohne Warnungen, `cargo audit`.

### Behoben
- TP-2.0-Befehle: `--address 22` ohne `0x` sprach Steuergerät 0x16 an statt 0x22.

## 0.2.0 — 17.11.2026
- GUI neu geordnet: Steuergerät wählen, dann Funktion wählen; Menü aufgeräumt.
- TP 2.0: Stellglieddiagnose, Anpassungskanäle lesen, Kurzcodierung schreiben (am Golf 6 geprüft).
- Auto-Scan liest zuerst die Verbauliste aus dem Gateway.
- Eigene Namen für ganze Steuergeräte, Scope mit eigenen Messwertnamen.

## 0.1.6 — 15.11.2026
- Gateway-Verbauliste und Gateway-Codierung, weitere Messwert-Formeln, ELM327 am Auto geprüft.

## 0.1.5 — 15.11.2026
- Keine fremden Label-Dateien mehr; UDS-Fehlercodes als Herstellernummer; Fehlerart bei TP 2.0 korrigiert.

## 0.1.4 — 11.11.2026
- Neuer Name ECUWerkbank (vorher vag-diag); Daten werden automatisch umgezogen.
