# Tests am Auto (gesammelt)

Alles, was sich nur am echten Fahrzeug prüfen lässt, gesammelt für einen
gemeinsamen Termin. Reihenfolge: erst nur lesen, dann Mitschnitte mit dem
Werkstattgerät, ganz zum Schluss Schreiben.

**Vorbereitung**
- Golf 6 (TP 2.0 + UDS), danach Polo 6R.
- SocketCAN-Adapter (`can0`, 500 kbit/s), Laptop am Ladegerät, **Batterie-Ladegerät am Auto**.
- Y-Kabel am OBD-Stecker für die Mitschnitte (Werkstattgerät und Adapter gleichzeitig).
- Mitschnitt starten: `candump -L can0 > logs/mitschnitt-<name>.log` (Ordner `logs/` wird nie hochgeladen).
- Vorher einmal alles sichern: `coding-backup`, `anpassung-sichern`, `software`.

Status: ⏳ offen · ✅ erledigt · ❌ Problem

## A. Nur lesen (kein Risiko)

| # | Was | Wie | Erwartung | Status |
|---|---|---|---|---|
| A1 | GUI friert nicht ein (Roadmap 25) | GUI: TP 2.0 → Messwertblock **Live**; dabei Fenster bewegen, andere Seite öffnen | Fenster bleibt flüssig, Drehrad unten links | ⏳ |
| A2 | Live-Grafik mit mehreren TP-2.0-Blöcken | Live-Grafik: Motor Block 1 + 2, dazu OBD Drehzahl | alle Kurven laufen; offener Kanal wird weitergereicht | ⏳ |
| A3 | Adapter während Live abziehen | A1 laufen lassen, Adapter abziehen | Live stoppt mit Meldung, kein Absturz/Hängen | ⏳ |
| A4 | Doppelklick „Fehler lesen“ | schnell zweimal klicken | „läuft schon — bitte warten“ | ⏳ |
| A5 | Bordspannung plausibel (Roadmap 38) | `ecu-werkbank obd-live --pids 42 --count 5` neben Multimeter an der Batterie | Abweichung ≤ 0,3 V | ⏳ |
| A6 | CAN-Monitor am OBD-Stecker (Roadmap 22) | Werkzeuge → CAN-Monitor, dann Fehler lesen | Diagnose-IDs (0x200, 0x7E0 …) erscheinen; Rate plausibel | ⏳ |
| A7 | Mitschnitt-Auffälligkeiten (Roadmap 44) | GUI Mitschnitt: einen neuen Mitschnitt öffnen | keine falschen ⚠ | ⏳ |
| A8 | Polo 6R: kompletter Scan | `autoscan`, `report`, `verbauliste` | alle Steuergeräte gefunden, Namen passen | ⏳ |
| A9 | Polo 6R: Fahrzeugtyp / Gateway-Verbauliste (Roadmap 16) | GUI „Fahrzeugtyp erkennen“, Verbauliste aus dem Gateway | UDS oder TP 2.0 richtig erkannt | ⏳ |

## B. Mitschnitte mit dem Werkstattgerät (Y-Kabel, Werkstattgerät liest/schreibt)

Jeweils: Mitschnitt starten → Ablauf im Werkstattgerät → Mitschnitt stoppen →
Anzeige des Werkstattgeräts notieren (Foto). Nur eigene Mitschnitte, keine
Dateien aus dem Werkstattprogramm übernehmen.

| # | Was | Ablauf am Werkstattgerät | Wofür | Status |
|---|---|---|---|---|
| B1 | Lange Codierung schreiben (Roadmap 11) | Bordnetz (09, TP 2.0 `20`): lange Codierung öffnen, **denselben Wert** wieder speichern | Anfrage mit `kwp-langcodierung --address 20 --neu <gleicher Wert>` (Probelauf) vergleichen → `LANGCODIERUNG_GEPRUEFT` | ⏳ |
| B2 | Gateway-Codierung schreiben (Roadmap 11) | Gateway (19, TP 2.0 `1F`): Verbauliste/Codierung **unverändert** speichern | Vergleich mit Probelauf → `GATEWAY_CODIERUNG_GEPRUEFT` | ⏳ |
| B3 | Anpassung schreiben (Roadmap 12) | Bordnetz: einen Anpassungskanal lesen und **denselben Wert** speichern | Vergleich mit `kwp-anpassung-schreiben` (Probelauf) → `ANPASSUNG_SCHREIBEN_GEPRUEFT` | ⏳ |
| B4 | Einzelmesswerte (Roadmap 13) | Motor: Funktion 09 „Einzelmesswert“ | Anfrage/Antwort lernen | ⏳ |
| B5 | Umgebungsbedingungen (Roadmap 14) | Fehlerspeicher mit Umgebungsdaten öffnen (ein Fehler muss gespeichert sein) | Umgebungsdaten entschlüsseln | ⏳ |
| B6 | UDS-Anpassungskanäle (Roadmap 15) | Kombi (17): Anpassung, Kanalliste | Kennungen + Namen den eigenen Labels zuordnen | ⏳ |
| B7 | Drosselklappe anlernen (Roadmap 17) | Motor: Grundeinstellung Gruppe 60 | Ablauf mit `lernen` prüfen | ⏳ |
| B8 | Service-Rückstellung Polo 6R (Roadmap 18) | Inspektion zurückstellen (wenn fällig) | Kennungen für den Polo | ⏳ |
| B9 | Batterie anlernen (Roadmap 19) | falls vorhanden: Bordnetz/Batterie-Anpassung | prüfen, ob es das beim Golf 6 gibt | ⏳ |

## C. Schreiben mit ECUWerkbank (erst nach B, mit Ladegerät)

| # | Was | Wie | Erwartung | Status |
|---|---|---|---|---|
| C1 | Spannungsprüfung greift | Codierung schreiben ohne Ladegerät bei Motor aus, Zündung lange an | bei < 12,0 V Abbruch „nichts geschrieben“ | ⏳ |
| C2 | Lange Codierung (nach B1) | gleichen Wert schreiben, dann 1 harmloses Bit hin und zurück | Kontrolle „geschrieben und kontrolliert“, Sicherung angelegt | ⏳ |
| C3 | Anpassung (nach B3) | Komfortblinken o. ä. ändern und zurück | Wert stimmt, Funktion am Auto ändert sich | ⏳ |
| C4 | Codier-Helfer Rückgängig | GUI Codierung: Bits ändern, „↶ Rückgängig“, „Alles zurück“, dann erst schreiben | nur der gewünschte Stand wird geschrieben | ⏳ |
| C5 | Kurzcodierung Tür (schon geprüft) – Gegenprobe mit Spannungsprüfung | `kwp-codierung-schreiben --address 22 --wert <gleich> --ja` | Spannung wird angezeigt, Schreiben klappt wie vorher | ⏳ |

## D. Ohne Auto, aber mit Administratorrechten

| # | Was | Wie | Status |
|---|---|---|---|
| D1 | Mitschnitt abspielen (Roadmap 24) | `sudo modprobe vcan && sudo ip link add vcan0 type vcan && sudo ip link set vcan0 up`, dann `replay --log … --iface vcan0` und gleichzeitig CAN-Monitor auf `vcan0` | ⏳ |
| D2 | Integrationstests über vcan (Roadmap 28) | wie D1, dann `mockecu` auf vcan0 | ⏳ |

Nach dem Termin: Ergebnisse hier eintragen, Konstanten in `src/kwp.rs` nur
nach bestätigtem Mitschnitt umstellen, ROADMAP/CHANGELOG nachziehen.
