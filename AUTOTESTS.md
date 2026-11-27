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
| A8 | Polo 6R: kompletter Scan (Anpassung: 30 Kanäle Bordnetz gleich wie Werkstattgerät; Blöcke Klima/Radio gleich, Gateway/Bordnetz ohne Blöcke > 127 → B11) | `autoscan`, `report`, `verbauliste` | alle Steuergeräte gefunden, Namen passen | ✅ 26.11.2026: alle 12 Steuergeräte (wie VCDS), ABS und Lenkhilfe über den K-Leitungs-Tunnel des Gateways (siehe B10); Umgebungsbedingungen am Klima-Fehler 00898 |
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
| A20 | Umgebungsbedingungen TP 2.0 | `kwp-dtc --address 1d` / `23`, GUI TP 2.0 Fehlerspeicher | Werte wie im Werkstattgerät | ✅ 25.11.2026 Golf: Einparkhilfe 01627 und Tür BF 00097/00929 — alle Felder gleich wie VCDS-Protokoll vom selben Stand (Mitschnitt in logs/) |
| A9 | Polo 6R: Fahrzeugtyp / Gateway-Verbauliste (Roadmap 16) | GUI „Fahrzeugtyp erkennen“, Verbauliste aus dem Gateway | UDS oder TP 2.0 richtig erkannt | ✅ 19.11.2026: UDS + TP 2.0 gemischt erkannt. Gateway 6N0 909 901 kennt keine Verbauliste (NRC 0x11) — Meldung jetzt in Klartext |

## B. Mitschnitte mit dem Werkstattgerät (Y-Kabel, Werkstattgerät liest/schreibt)

Jeweils: Mitschnitt starten → Ablauf im Werkstattgerät → Mitschnitt stoppen →
Anzeige des Werkstattgeräts notieren (Foto). Nur eigene Mitschnitte, keine
Dateien aus dem Werkstattprogramm übernehmen.

| # | Was | Ablauf am Werkstattgerät | Wofür | Status |
|---|---|---|---|---|
| B1 | Lange Codierung schreiben (Roadmap 11) | Bordnetz (09, TP 2.0 `20`): lange Codierung öffnen, **denselben Wert** wieder speichern | Anfrage mit `kwp-langcodierung --address 20 --neu <gleicher Wert>` (Probelauf) vergleichen → `LANGCODIERUNG_GEPRUEFT` | ✅ 26.11.2026 Golf 6 (`logs/mitschnitt-golf-vcds-b-tests-20261126-1644.log`): Werkstattgerät schickt in Sitzung 0x89 `2E F1 99 <Datum>` (abgelehnt 7F 2E 31), `2E F1 98 <Werkstattcode>` (angenommen), dann `2E 06 00 <30 Byte>` → `6E 06 00`. **27.11.2026 selbst geschrieben:** ohne F198 → NRC 0x22; F198 mit Nullen → NRC 0x22; mit echtem Werkstattcode → angenommen, Wert unverändert. ECUWerkbank schreibt jetzt F198 vorher und verlangt einen Werkstattcode |
| B2 | Gateway-Codierung schreiben (Roadmap 11) | Gateway (19, TP 2.0 `1F`): Verbauliste/Codierung **unverändert** speichern | Vergleich mit Probelauf → `GATEWAY_CODIERUNG_GEPRUEFT` | ✅ 27.11.2026 Golf 6 (`logs/mitschnitt-golf-vcds-b2-b11-20261127.log`, eigene Tests: `logs/mitschnitt-golf-eigene-schreibtests-20261127.log`): Werkstattgerät schickt `3B 9A <Werkstattcode 6> <Softwarestand 4> 10 0B <10 Byte Codierung> FF` (erst 0x78, dann `7B 9A`). Unser Aufbau gleich, nur mit eigenem Werkstattcode vorne → selbst geschrieben, byte-gleich, angenommen, Wert unverändert → `GATEWAY_CODIERUNG_GEPRUEFT = true` |
| B3 | Anpassung schreiben (Roadmap 12) | Bordnetz: einen Anpassungskanal lesen und **denselben Wert** speichern | Vergleich mit `kwp-anpassung-schreiben` (Probelauf) → `ANPASSUNG_SCHREIBEN_GEPRUEFT` | ✅ 26.11.2026 Mitschnitt komplett: Kanal wählen `31 B9 01 03 17`, Wert testen `31 B9 01 03 00 0A`, speichern `31 BB 01 03 00 0A` + 6 Byte Werkstattcode, Ergebnis `BA` → `05 03 00 0A FF`, Ende `32 B8 01 03`. **27.11.2026 selbst gespeichert** (Kanal 23 = 10): mit Werkstattcode Nullen → NRC 0x22 beim Speichern; mit echtem Werkstattcode → `05 03 00 0A FF`, Wert 10 → `ANPASSUNG_SCHREIBEN_GEPRUEFT = true` |
| B4 | Einzelmesswerte (Roadmap 13) | Motor: Funktion 09 „Einzelmesswert“ | Anfrage/Antwort lernen | ✅ 26.11.2026: Werkstattgerät liest für Einzelmesswerte normale Blöcke (`21 01`, `21 03`, `21 04`) und zeigt ein Feld — gleiches Vorgehen wie unser 🔍 Einzelmesswert |
| B5 | Umgebungsbedingungen (Roadmap 14) | Fehlerspeicher mit Umgebungsdaten öffnen (ein Fehler muss gespeichert sein) | Umgebungsdaten entschlüsseln | ⏳ |
| B6 | UDS-Anpassungskanäle (Roadmap 15) | Kombi (17): Anpassung, Kanalliste | Kennungen + Namen den eigenen Labels zuordnen | ✅ 27.11.2026 (Mitschnitt): Werkstattgerät zeigt im Kombi 3 Kanäle, gelesen als UDS-Kennungen in dieser Reihenfolge: 0x2238 „Stunde“ (Wert 0x0E = 14, passt zur Uhrzeit), 0x2248 „Ölqualität“ (02), 0x22A0 „Strecke bis Service“ (Kombi: requestOutOfRange, auch beim Werkstattgerät Fehler); 0x22C7 ebenfalls abgelehnt. Frühere Liste vom 26.11.: 0x223E, 0x2248, 0x0910, 0x052B, 0x2247, 0x228A, 0x22D6, 0x0E34, 0x2232 |
| B7 | Drosselklappe anlernen (Roadmap 17) | Motor: Grundeinstellung Gruppe 60 | Ablauf mit `lernen` prüfen | ✅ 27.11.2026 Golf 6 (`logs/mitschnitt-golf-vcds-b7-drosselklappe-20261127.log`): VCDS sendet in Sitzung 0x89 `31 B8 00 00`, `31 B8 00 3C` (Start), fragt `31 BA 00 3C` ab (Antwort: Status 06/07 + 4 Felder), schaltet mit `31 B9 00 3C` ein („ON“, erst während des Abfragens), Ende `32 B8 00 3C`; VCDS: „ADP. OK“. Lernen kannte das Einschalten mitten im Abfragen noch nicht → nachgerüstet (`ein`). Danach **ECUWerkbank selbst** (`kwp-grundeinstellung --address 01 --gruppe 60 --ja`): gleicher Verlauf (Schritt 5→6→2→8, Textnummer 01 → 02), „Aus bestätigt“ |
| B8 | Service-Rückstellung Polo 6R (Roadmap 18) | Inspektion zurückstellen (wenn fällig) | Kennungen für den Polo | ⏳ |
| B10 | Polo 6R: ABS (03) und Lenkhilfe (44) | Werkstattgerät öffnet ABS und Lenkhilfe (nur Identifikation), Y-Kabel-Mitschnitt | ABS: wie sieht der TP-2.0-Kanalaufbau aus (unsere Anfrage `200#03C00010000301` → nur `203#00D0A3`)? Lenkhilfe: geht sie über CAN oder K-Line? | ✅ 26.11.2026 (Mitschnitt `logs/mitschnitt-polo-vcds-b10-b11-…`): beide hängen an einer K-Leitung hinter dem Gateway. Kurze Antwort `00 D0 xx` = Tunnel; Kanal `200#<Ziel> C0 A0`, Tester 0x7A0, Antwort 0x7A3 (ABS) / 0x7A9 (Lenkhilfe), 5-Baud-Adresse mit Parität als erste Nachricht. ABS: KWP2000 mit Kopf (Adresse 0x28, ESP8.2i 6R0907379AF), Lenkhilfe: KW1281 (6R0423156B TRW). Eingebaut als `ktunnel` und im Scan |
| B11 | Hohe Messwertblöcke (> 127) | Werkstattgerät liest am Polo Gateway Block 201 und Bordnetz Block 220 | Wir bekommen dort „requestOutOfRange“ — welche Sitzung/welcher Dienst? | ✅ 27.11.2026 Golf 6: Gruppe 201 → VCDS schickt `21 4A` (wie beim Polo), Antwort: 4 leere Felder, 2× Zahl 1, 2× Textnummer `F25 00 67` (VCDS: „1, 1, Aktiv, Aktiv“). Gruppe 202 → `21 4B` → requestOutOfRange (VCDS: Fehler). 201 ist also nur eine Umleitung des Werkstattgeräts auf Block 74 (gleiche Anfrage wie Gruppe 74) — keine allgemeine Regel des Steuergeräts, bei uns direkt Block 74 lesen. Bordnetz 220 gibt es nicht |
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
| D1 | Mitschnitt abspielen (Roadmap 24) | `sudo modprobe vcan && sudo ip link add vcan0 type vcan && sudo ip link set vcan0 up`, dann `replay --log … --iface vcan0` und gleichzeitig CAN-Monitor auf `vcan0` | ✅ 27.11.2026: Polo-Mitschnitt (1700 Frames, Tempo 10) auf vcan0 — mitgelesen 1700 Frames, identisch; `lo` wird abgelehnt |
| D3 | ODX/PDX-Import mit einer echten, rechtmäßig erworbenen Datei (Roadmap 57/58) | `db odx --datei … --hersteller …`, dann `db quellen`, `fehlertext --code …` | ⏳ |
| D4 | CAN FD über vcan (Roadmap 32) | `sudo ip link add vcan1 type vcan && sudo ip link set vcan1 mtu 72 up`, dann `VAG_CANFD=1 ecu-werkbank -i vcan1 …` gegen `mockecu` | ✅ 27.11.2026 auf vcan0 (MTU 72): `VAG_CANFD=1` Ident gegen `mockecu` mit FD — VIN (20 B) als ein FD-Einzel-Frame mit Längen-Escape (24 B), alle Werte richtig |
| D5 | DoIP (Roadmap 31) | nur mit einem DoIP-Fahrzeug (z. B. Golf 8) und OBD-Ethernet-Kabel: `ecu-werkbank -t doip --port 169.254.x.x ident --geraet 0x4010` | ⏳ |
| D6 | K-Leitung (Roadmap 33) | nur mit KKL-Kabel und Auto bis ~2004: `ecu-werkbank kline --port /dev/ttyUSB0 --adresse 0x01 --gruppe 1` | ⏳ |
| D7 | J2534 unter Windows (Roadmap 34) | Windows-Rechner mit J2534-Adapter (64-Bit-Treiber): `ecu-werkbank.exe -t j2534 --port <Name> obd-info` | ⏳ |
| D2 | Integrationstests über vcan (Roadmap 28) | wie D1, dann `mockecu` auf vcan0 | ✅ 27.11.2026: `obd-info`, `ident --geraet 0x7e0`, `kwp-ident`/`kwp-dtc`/`kwp-block` (TP 2.0), `report` (6 Steuergeräte, 5 Fehler) über echte CAN-Frames gegen `mockecu` |
| D8 | BMW-Adressierung über vcan (Roadmap 35) | wie D1, dann `VAG_MOCK_BMW=1 mockecu vcan0` und `ecu-werkbank -i vcan0 fremd` → erwartet: Steuergerät `0x6f1:12/0x612:f1` gefunden, Kennung und Fehler wie Motor 0x7E0 | ✅ 27.11.2026: `fremd` fand `0x6f1:12/0x612:f1` neben 0x7E0–0x7E2, Kennung (mehrteilige Antworten) und 2 Fehler wie Motor 0x7E0 (Suche 45 s) |

Nach dem Termin: Ergebnisse hier eintragen, Konstanten in `src/kwp.rs` nur
nach bestätigtem Mitschnitt umstellen, ROADMAP/CHANGELOG nachziehen.
