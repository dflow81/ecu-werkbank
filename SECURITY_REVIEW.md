# Sicherheitsprüfung

Stand: 18.11.2026. „Sicherheit“ heißt hier zweierlei: **das Auto** (nichts kaputt
schreiben) und **der Rechner/die Daten** (keine Angriffsfläche, keine Datenlecks).

## 1. Sicherheit am Fahrzeug

| Thema | Stand | Bewertung |
|---|---|---|
| Schreiben nur nach Bestätigung | `--ja` in der CLI, Häkchen + zweiter Klick in der GUI | ✅ |
| Sicherung vor dem Schreiben | Codierung (UDS + TP 2.0 lang), Anpassungen, Notfall-Komplettsicherung | ✅ |
| Kontrolle nach dem Schreiben | neu lesen und vergleichen (Codierung, Kurzcodierung, Anpassung) | ✅ |
| Ungeprüfte Schreibwege | TP-2.0-Langcodierung, Gateway-Codierung, TP-2.0-Anpassung: am echten Auto nur **Probelauf**, bis ein Mitschnitt die Anfrage bestätigt (`kwp::*_GEPRUEFT`) | ✅ |
| Kilometerstand | wird nie geschrieben, durch Test abgesichert | ✅ |
| Fremde Codierung aufs Auto | Sicherungen werden nur für das gewählte Fahrzeug (VIN) angeboten | ✅ |
| Mitschnitt abspielen (`replay`) | nur auf `vcan`; Art des Interfaces wird vorher über `ip -details` geprüft, echte Adapter (`can`, `slcan`) werden abgelehnt | ✅ |
| ECUReset, Stellglieder, Grundeinstellung | nur mit `--ja`; Stellglieder mit Warnhinweis in der GUI | ✅ |
| Datenübertragung (0x34–0x37) | nur Bausteine, kein Flash-Ablauf | ✅ bewusst |
| Flashen | **nicht eingebaut.** Braucht Spannungsüberwachung, VIN-/Teilenummer-Prüfung, Prüfsummen, Wiederaufnahme nach Abbruch und signierte Daten. Ohne all das droht ein unbrauchbares Steuergerät. | ⏳ siehe ROADMAP |
| Warten auf „Antwort folgt“ | jetzt höchstens 60 s (vorher unbegrenzt) | ✅ behoben |
| TP-2.0-Adresse | war ohne `0x` dezimal gelesen (`22` → 0x16): falsches Steuergerät möglich | ✅ behoben, jetzt immer hex |
| Login-Verfahren | nur öffentlich dokumentierte Verfahren (siehe CREDITS.md), Bytecode-Automat mit Schleifen-Grenze | ✅ |

## 2. Rechner und Daten

| Thema | Stand | Bewertung |
|---|---|---|
| Handy-Anzeige (`web.rs`) | lauscht auf allen Netzwerkkarten (`0.0.0.0`), nur lesend; **neu:** zufälliger Schlüssel in der Adresse, sonst 404 | ✅ behoben |
| HTTP ohne TLS | im Heimnetz lesbar für Mitlauscher (nur Live-Werte, keine VIN) | 🟡 bewusst einfach; nur im eigenen WLAN nutzen |
| `unsafe` | 5 Stellen: `localtime_r`/`localtime_s` (Zeit), `ioctl`/Netzwerk-Abfragen (CAN) | ✅ eng begrenzt |
| Externe Programme | `ip` (CAN-Status), `xdg-open`/`open`, `notify-send`, `paplay` — feste Argumente, keine Shell | ✅ |
| ZIP entpacken (`zip.rs`) | Notfall-Sicherung zurückspielen: Pfade prüfen (kein `..`, keine absoluten Pfade) | ✅ (Test vorhanden) — bei Änderungen erneut prüfen |
| TOML-Dateien | Labels, Sprachen, Fahrzeugdaten mit `serde` gelesen; Fehler werden gemeldet, nicht ausgeführt | ✅ |
| Fahrzeug-Datenbank (SQLite) | nur Platzhalter-Parameter in SQL, keine zusammengesetzten Abfragen aus Eingaben; ODX/PDX technisch nicht weitergebbar (`db::darf_weitergeben`); Security-Access nur als Beschreibung, keine Schlüssel | ✅ |
| DBC/candump lesen | reiner Text, Grenzen geprüft (Signal ≤ 64 Bit, Frame ≤ 64 Byte) | ✅ |
| Personenbezogene Daten | VIN, Kennzeichen, Kilometerstand liegen nur lokal; Simulation und Handbuch nutzen erfundene VINs | ✅ |
| Veröffentlichung | `.gitignore` schließt `label/`, `labels/` (außer Beispielen), `logs/` aus; vor jedem Push Prüfung auf VINs/Mitschnitte | ✅ (Ablauf) |
| Abhängigkeiten | wenige, verbreitete Crates; `Cargo.lock` im Repo | ✅ `cargo audit` bei jedem Push |

## 3. Offene Punkte

1. ~~CI mit `cargo audit`, `cargo clippy -D warnings`, `cargo test` bei jedem Push~~ — erledigt (`pruefen.yml`).
2. ~~GUI: Bus-Aufrufe aus dem Zeichen-Thread nehmen~~ — erledigt: ein Arbeiter-Thread, Aufträge der Reihe nach, doppelte Klicks werden abgewiesen.
3. Flash-Funktion erst mit vollständigem Schutzkonzept (siehe ROADMAP „Flashing“).
4. Fremde Plugins/Skripte (geplant) nur mit klaren Rechten: Lesen frei, Schreiben nur über dieselben Bestätigungen wie im Programm.
