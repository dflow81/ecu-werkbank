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
| A1 | GUI friert nicht ein (Roadmap 25) | GUI: TP 2.0 → Messwertblock **Live**; dabei Fenster bewegen, andere Seite öffnen | Fenster bleibt flüssig, Drehrad unten links | ✅ 22.11.2026 Golf (GUI): Live flüssig beim Verschieben/Seitenwechsel |
| A2 | Live-Grafik mit mehreren TP-2.0-Blöcken | Live-Grafik: Motor Block 1 + 2, dazu OBD Drehzahl | alle Kurven laufen; offener Kanal wird weitergereicht | ✅ 19.11.2026 am Polo: Bordnetz + Klima (TP 2.0) + OBD gleichzeitig in den Armaturen, Werte wie im Werkstattgerät; einmal ⚠ an der Bordnetz-Zeile (einzelner Lesefehler?) — ohne Auto nachsehen |
| A3 | Adapter während Live abziehen | A1 laufen lassen, Adapter abziehen | Live stoppt mit Meldung, kein Absturz/Hängen | ✅ 22.11.2026 Golf (GUI): Adapter während Live abgezogen — Meldung, kein Absturz |
| A4 | Doppelklick „Fehler lesen“ | schnell zweimal klicken | „läuft schon — bitte warten“ | ✅ 22.11.2026 Golf (GUI): „läuft schon — bitte warten“ |
| A5 | Bordspannung plausibel (Roadmap 38) | `ecu-werkbank obd-live --pids 42 --count 5` neben Multimeter an der Batterie | Abweichung ≤ 0,3 V | ✅ 22.11.2026 Golf: Programm 11,7 V (OBD-PID 42, Zündung an), an der Batterie gemessen 11,8 V — Abweichung 0,1 V |
| A6 | CAN-Monitor am OBD-Stecker (Roadmap 22) | Werkzeuge → CAN-Monitor, dann Fehler lesen | Diagnose-IDs (0x200, 0x7E0 …) erscheinen; Rate plausibel | ✅ 22.11.2026 Golf (GUI): CAN-Monitor zeigt Diagnose-Kennungen, Raten plausibel |
| A7 | Mitschnitt-Auffälligkeiten (Roadmap 44) | GUI Mitschnitt: einen neuen Mitschnitt öffnen | keine falschen ⚠ | ⏳ |
| A8 | Polo 6R: kompletter Scan (Anpassung: 30 Kanäle Bordnetz gleich wie Werkstattgerät; Blöcke Klima/Radio gleich, Gateway/Bordnetz ohne Blöcke > 127 → B11) | `autoscan`, `report`, `verbauliste` | alle Steuergeräte gefunden, Namen passen | 🟡 19.11.2026: 9 von 11 gefunden (UDS: Motor, Wegfahrsperre, Kombi, Airbag, Lenkwinkel; TP 2.0: Gateway, Bordnetz, Klima, Radio), Fehler-PDF ok. Fehlen: 03 ABS (antwortet auf TP 2.0 nur kurz „00 D0 A3“ → B10), 44 Lenkhilfe (vermutlich K-Line → Roadmap 33). Namen Gateway/Klima nachgetragen. Messwertblöcke Bordnetz mit Werkstattgerät-Export verglichen: gleiche Blöcke; Formel F25 = Text, fehlt noch |
| A10 | Fremdes Auto mit 29-Bit-Adressen (falls eines greifbar ist, z. B. bei Bekannten) | `fremd` bzw. GUI „Andere Marken“ mit SocketCAN, danach mit ELM327 | Steuergeräte `0x18da…` gefunden, Kennung/Fehler lesbar | ⏳ |
| A11 | Steuergerätebaum (Roadmap 21) — **nur Golf 6** (Polo-Gateway hat keine Verbauliste) | GUI Steuergeräte → Verbauliste → „aus dem Gateway lesen“ nach einem Scan; `gateway-verbauliste` | Bus-Zuordnung stimmt; Bus 06 und 14 benennen (vermutlich Kombi-CAN / Gateway selbst) | ✅ 22.11.2026 Golf (Terminal): 20 verbaut, 3 mit Fehler (passt zum Fehlerspeicher). Bus 06 = Kombi + Wegfahrsperre → „Kombi-CAN“, Bus 14 = nur Gateway → „Gateway selbst“. Fehler behoben: UDS-Geräte galten als „im Scan nicht gefunden“ (Scan speichert dort keine Adresse) → jetzt Abgleich über den Namen. Übrig: 04 Lenkwinkelsensor (steckt in J527), 46 Komfortsystem (Doppelrolle Bordnetz). |
| A12 | Fehlerart mit Text (TP 2.0) | Fehlerspeicher eines TP-2.0-Steuergeräts mit gespeichertem Fehler lesen (Golf: Einparkhilfe/Tür) | „Fehlerart 14: Bauteil defekt“ o. ä.; Nummer passt zum Werkstattgerät | ✅ 22.11.2026 Golf: Einparkhilfe 01627 „Fehlerart 14: Bauteil defekt“, Tür BF 00097 „Fehlerart 12: elektrischer Fehler im Stromkreis“ — wie VCDS-Scan |
| A13 | Anpassungsnamen über die Teilenummer | Teilenummer per „🔎 Teilenummer“ prüfen, dann TP 2.0 → Anpassung lesen | Zeile „Namen aus der Fahrzeug-Datenbank (…)“, Kanalnamen in Klammern; „Bekannte Kanäle … alle eintragen“ | ✅ 22.11.2026 Golf (GUI): Bordnetz 5K0937086: bekannte Kanäle, Namen beim Lesen, Codier-Bits benannt (Terminal ebenso; Kennung über 0x22 behoben) |
| A14 | ELM327 am Golf 6 (Gegenprobe nach den Umbauten) | `-t elm327 obd-info`, `autoscan`, `report` | OBD, alle 5 UDS-Steuergeräte, Fehlerspeicher | ✅ 22.11.2026: OBD + Spannung (11,7 V) ok. Zwei Fehler gefunden und behoben: autoscan fand über ELM kein UDS-Gerät (kein Suchlauf möglich) → jetzt Profil des Fahrzeugs bzw. Liste üblicher VAG-Kennungen (ohne Profil ~1 min); Airbag im Bericht „nicht lesbar“ (antwortet nach NRC 0x31 auf F190 auf die nächste Anfrage nicht) → Fehlerspeicher wird einmal wiederholt. Danach alles wie am CAN-Adapter |
| A15 | „Andere Marken“ am Golf 6 | GUI Andere Marken bzw. `-t elm327 fremd` | mindestens der Motor wird gefunden | ✅ 22.11.2026 (ELM327): OBD-Rückfall eingebaut, Motor gefunden. ✅ 25.11.2026 (SocketCAN): erst 5 halbe Treffer — VW-Sammeladresse 0x700 (alle UDS-Geräte antworten) wurde als ein Gerät gelesen, Motor (nur OBD) fehlte, Systemnamen als Hex, Radio bekam fremden Motortext für P1000. Behoben → 7 Geräte: IMMO 0x711, KOMBI 0x714, Airbag 0x715, Climatronic 0x746, RNS310 0x76C, Motor OBD 0x7E0 (+ 0x7F1 unbekannt); herstellerspezifische Codes nur mit Text zur Teilenummer |
| A16 | Messwertblöcke mit Namen und Sollwerten aus der Datenbank | Motor 0x01: TP 2.0 → Messwertblock 1, 2, 4 lesen (ohne vorherigen Scan), Motor warm im Leerlauf; dann `kwp-block --address 01 --block 1` | Feldnamen wie „Motordrehzahl (G28)“, darunter „Soll … (Leerlauf)“; im Leerlauf grün, bei Gas nur Hinweis, keine Warnung | ✅ 25.11.2026 Golf, Motor 03C906016, Leerlauf kalt: „Motordrehzahl (G28) 680 1/min ✓“, „Kühlmittel 54 °C (Soll 80–110 °C (Warm))“ nur Hinweis, „Spannung (Klemme 30) 13,8 V ✓“ |
| A17 | Stellglieddiagnose mit Bauteilnamen | TP 2.0 → Tür (0x22) Stellglieddiagnose, Fahrzeug gesichert | „Bauteil 01482 wird angesteuert: Motoren für Zentralverriegelung“ o. ä. — Name passt zum Bauteil, das sich bewegt | ✅ 25.11.2026 Golf, Tür Fahrer 0x22 (1K0959701AE): alle Bauteile benannt (Namen wie im VCDS-Stellgliedtest). Gesehen: Zentralverriegelung auf/zu ✓ (Rückmeldung entriegelt→safe→entriegelt), Spiegel ✓, Fensterheber runter/hoch (16:32). „Beleuchtung“ nichts sichtbar, „Ansteuerung Blinkleuchten“ blinkt nicht (keine Rückmeldung — Spiegelblinker wohl nicht an der Tür), Umfeldbeleuchtung „n. verbaut“. Behoben: Ende bei Bauteil 01195 „ENDE“ ohne Timeout-Fehler. Hinweis: das erste „Weiter“ nach Bauteil 1 ignoriert die Tür |
| A18 | Identifikation mit Datenbank-Wissen | GUI Erweiterte Identifikation 0x7E0 (bzw. ein UDS-Gerät aus der Datenbank) und `kwp-ident --address 20` | Zeile „Fahrzeug-Datenbank: …“; bei UDS ASAM-Datensatz aus der Datenbank = Wert F19E des Steuergeräts | ✅ 25.11.2026 Golf: Motor/Bordnetz mit Datenbank-Zeile; Kombi 5K0920860F meldet F19E EV_KombiUDSMM9RM10 / A04010 = ASAM-Datensatz der Datenbank |
| A19 | Service-Seite zeigt Datenbank-Wissen (Golf, Kombi 5K0920860F) | GUI Fahrzeug → Service-Anzeige | Aufklappbereich „Service-Anpassungen dieses Kombis laut Fahrzeug-Datenbank (13)“; Lesen der Werte 0x2243/0x2244 wie bisher | ✅ 25.11.2026 Golf: „Service-Anpassungen … (17)“, Werte 20 500 km / 506 Tage seit Service. Behoben: Seite las am Bus-Arbeiter vorbei (Zusammenstoß mit Fahrzeugerkennung beim Start); Hintergrund-Ergebnisse wurden erst nach Mausbewegung gezeichnet |
| A9 | Polo 6R: Fahrzeugtyp / Gateway-Verbauliste (Roadmap 16) | GUI „Fahrzeugtyp erkennen“, Verbauliste aus dem Gateway | UDS oder TP 2.0 richtig erkannt | ✅ 19.11.2026: UDS + TP 2.0 gemischt erkannt. Gateway 6N0 909 901 kennt keine Verbauliste (NRC 0x11) — Meldung jetzt in Klartext |

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
| B10 | Polo 6R: ABS (03) und Lenkhilfe (44) | Werkstattgerät öffnet ABS und Lenkhilfe (nur Identifikation), Y-Kabel-Mitschnitt | ABS: wie sieht der TP-2.0-Kanalaufbau aus (unsere Anfrage `200#03C00010000301` → nur `203#00D0A3`)? Lenkhilfe: geht sie über CAN oder K-Line? | ⏳ |
| B11 | Hohe Messwertblöcke (> 127) | Werkstattgerät liest am Polo Gateway Block 201 und Bordnetz Block 220 | Wir bekommen dort „requestOutOfRange“ — welche Sitzung/welcher Dienst? | ⏳ |
| B9 | Batterie anlernen (Roadmap 19) | falls vorhanden: Bordnetz/Batterie-Anpassung | prüfen, ob es das beim Golf 6 gibt | ⏳ |

Zusatz zu B (nur wenn ohnehin ein Ölwechsel/Service ansteht): **Service-Rückstellung lernen** —
Service-Anzeige → „vorher“ merken, Service mit den Kombi-Tasten zurückstellen, „nachher“
merken, vergleichen, „Ins Fahrzeug übernehmen“, dann „📚 In die Fahrzeug-Datenbank
übernehmen“. Danach gilt sie für jedes Auto mit Kombi 5K0920860F. (Die Tasten-Rückstellung
setzt die echte Service-Anzeige zurück — darum nur nach einem echten Service.)

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
| D3 | ODX/PDX-Import mit einer echten, rechtmäßig erworbenen Datei (Roadmap 57/58) | `db odx --datei … --hersteller …`, dann `db quellen`, `fehlertext --code …` | ⏳ |
| D2 | Integrationstests über vcan (Roadmap 28) | wie D1, dann `mockecu` auf vcan0 | ⏳ |

Nach dem Termin: Ergebnisse hier eintragen, Konstanten in `src/kwp.rs` nur
nach bestätigtem Mitschnitt umstellen, ROADMAP/CHANGELOG nachziehen.
