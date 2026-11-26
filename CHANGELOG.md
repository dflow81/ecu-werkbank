# Änderungen

Alle für Nutzer wichtigen Änderungen. Neueste oben.

## Unveröffentlicht

### Neu
- **K-Leitungs-Steuergeräte hinter dem Gateway** (z. B. Polo 6R: ABS und Lenkhilfe): Das Gateway
  tunnelt die K-Leitung über CAN. ECUWerkbank erkennt das am Kanalaufbau und liest Identifikation
  und Fehlerspeicher (KW1281 oder KWP2000) — im Scan und mit `ktunnel --adresse 0x03`. Am Polo 6R
  (2010) geprüft: alle 12 Steuergeräte wie beim Werkstattgerät.
- Umgebungsbedingungen: Hinweis, wenn das Datum ungültig ist (Uhr im Kombi nicht gestellt).
- **Umgebungsbedingungen bei TP 2.0 im Klartext**: Fehlerstatus, Priorität, Häufigkeit,
  Verlernzähler, km-Stand, Datum/Uhrzeit und weitere Messwerte (z. B. Temperatur) zu jedem
  Fehler — in GUI, Terminal, PDF und Historie. Gelesen wie ein Werkstattgerät (Freeze-Frame
  `12 00 04 <Code>`); am Golf 6 Byte für Byte mit einem Werkstattgerät verglichen. Vorher
  waren die Umgebungsdaten bei diesen Steuergeräten leer.
- Mitschnitt-Übersetzer benennt die KWP-Dienste 0x12, 0x17 und 0x22.
- **Spannungswächter bei langen Schreibabläufen**: Beim Zurückschreiben vieler Anpassungen wird
  die Bordspannung zwischendurch gemessen (höchstens alle 10 s) und vor dem nächsten Schreiben
  abgebrochen, wenn sie unter 12,0 V fällt. „Steuergerät stellt Wert“ prüft sie ebenfalls und
  gibt sonst die Kontrolle zurück.

- **Einzelmesswert**: 🔍 an einem Feld des Messwertblocks zeigt diesen Wert groß an (gut ablesbar, läuft live mit).
- **Speicher lesen** (`speicher-lesen`, nur lesen): Steuergeräte-Speicher über UDS 0x23 in Blöcken auslesen und als `.bin` sichern; gesperrte Bereiche werden als Lücke mit Grund vermerkt.
- **REST-Schnittstelle mit Live-Werten**: `api --live` liefert zusätzlich OBD-Werte vom Auto unter `/api/v1/live/obd?pids=0C,0D` (nur lesen, nur dieser Rechner, mit Schlüssel).
- **Fehler-Statistik aus der eigenen Historie**: Prüfplan und Fehleranzeige sagen, wie oft ein Fehler bei deinen Autos auftrat, nach welcher Reparatur er weg war (mit Anteil) und wie oft er wiederkam.
- **DoIP** (Diagnose über Ethernet, ISO 13400): neuer Transport `doip` für neuere Fahrzeuge — Fahrzeugsuche, Routing-Aktivierung, UDS über das Gateway (gegen ein simuliertes Gateway getestet).
- **CAN FD** für ISO-TP: Frames bis 64 Byte und Nachrichten über 4095 Byte (`VAG_CANFD=1`, Schnittstelle mit MTU 72).
- **ODX-Import mit Umrechnung**: Messwerte aus ODX bekommen Faktor, Offset und Einheit (lineare Umrechnung).
- „Andere Marken“ erkennt die Marke an der Fahrgestellnummer und sagt, was bei ihr geht.
- **K-Leitung (KW1281)** für Autos bis etwa 2004 mit KKL-Kabel: `kline --adresse 01 [--gruppe 1]` liest Identifikation, Fehlerspeicher und Messwertgruppen (nur lesen; gegen ein simuliertes Steuergerät getestet).
- **J2534-Adapter unter Windows** (Tactrix, Mongoose …): Transport `j2534`, Adapter aus der Registry oder DLL-Pfad (64-Bit-Treiber nötig; ungetestet).
- GUI: Verbindungsarten „DoIP (Ethernet)“ und — unter Windows — „J2534-Adapter“.
- **Konfiguration vergleichen in der GUI**: Fahrzeug → Codier-Sicherung → „Ganze Konfiguration vergleichen“ (vorher/nachher oder anderes Fahrzeug).

### Behoben
- „Andere Marken“ mit CAN-Adapter: Die VW-Sammeladresse 0x700 (alle UDS-Steuergeräte antworten)
  wird erkannt, jedes Gerät mit seiner eigenen Adresse gelesen; Motoren, die nur OBD sprechen,
  werden gefunden; Systemnamen als Text statt Hex; herstellerspezifische Fehlercodes (P1xxx …)
  bekommen nur einen Text, der zur Teilenummer passt.
- Service-Anzeige: Lesen läuft über die Bus-Warteschlange (stieß beim Start mit der
  Fahrzeugerkennung zusammen).
- GUI: Ergebnisse aus dem Hintergrund erscheinen sofort, nicht erst nach einer Mausbewegung.
- Ein Scan über „Andere Marken“ verdrängt nicht mehr den normalen Steuergeräte-Stand eines
  VW-Konzern-Fahrzeugs (Steuergeräte-Seite, Vergleich mit dem vorigen Scan, Fehlerübersicht,
  Reparatur-Modus, Gebrauchtwagen-Check nehmen den letzten normalen Scan).
- Stellglieddiagnose (TP 2.0) endet beim Bauteil „01195 Ende“ ohne Fehlermeldung.

## 0.5.0 — 24.11.2026

> **Hinweis:** Die Fahrzeug-Datenbank wird beim ersten Start auf Schema 7 umgestellt (mit
> Sicherungskopie vorher). Ältere Versionen können sie danach nicht mehr öffnen.

### Neu
- **Diagnose-Referenzen in der Fahrzeug-Datenbank:** Lokale Datenpakete können Diagnose-Referenzen
  zu Steuergeräten, alternative Fehlertexte und UDS-Fehlerkombinationen importieren
  (`nur_lokal = true` hält eine Quelle vom Export und der Schnittstelle fern). Die Datenbank
  zeigt die Referenzen nach Teilenummer; „Fehler suchen“ bzw. `db fehler <code> --art <hex>`
  sucht Fehlercode und Fehlerart zusammen.
- **Diagnose-Referenzen werden überall genutzt:** Messwertblöcke (TP 2.0) zeigen Feldnamen und
  Sollbereiche aus der Datenbank, passend zur Teilenummer des Steuergeräts — im Reiter TP 2.0,
  im Blockscan, in der Live-Grafik, im Testlauf, im Fahrt-Rekorder und bei `kwp-block`/
  `kwp-blockscan`. Eigene Texte haben Vorrang. Sollwerte mit Bedingung („Leerlauf“, „Warm“)
  werden nur grün markiert, wenn der Wert passt — außerhalb steht nur der Hinweis, weil
  ECUWerkbank nicht weiß, ob die Bedingung gerade erfüllt ist. Ohne vorherigen Scan liest die
  GUI die Teilenummer beim ersten Lesen selbst (nur lesen).
- Fehlercodes ohne eigentlichen Text bekommen den Alternativtext aus der Datenbank.
- **Service-Rückstellung über die Fahrzeug-Datenbank:** Gelernte Service-Kennungen lassen sich
  je Kombi-Teilenummer in der Datenbank speichern („📚 In die Fahrzeug-Datenbank übernehmen“
  bzw. `service in-datenbank`). Jedes Auto mit demselben Kombi findet sie dann automatisch.
  Datenpakete können sie unter `[hersteller.steuergeraet.service]` mitbringen (Schema 7).
  Die Service-Seite zeigt außerdem die Service-Anpassungen, die das Kombi laut Datenbank kennt.
- Experten-Seite „Routine & Stellglied (UDS)“: bekannte Stellfunktionen/Routinen aus der
  Datenbank auswählen statt Kennungen abzutippen; Name der Kennung wird angezeigt.
- Stellglieddiagnose (TP 2.0) nennt das angesteuerte Bauteil beim Namen (Bauteilnummern =
  VAG-Fehlernummern, Text aus der Fahrzeug-Datenbank), z. B. „01482: Motoren für Zentralverriegelung“.
- Fehlersuche in der Datenbank zeigt die Fehlerart mit Text („13 (Unterbrechung)“).
- Erweiterte Identifikation (UDS und TP 2.0) zeigt, was die Datenbank zur Teilenummer weiß,
  samt ASAM-Datensatz zum Vergleich mit dem Wert des Steuergeräts (F19E).
- Werbevideo `werbevideo/ECUWerkbank-Werbespot.mp4`.
- **QR-Code für die Handy-Anzeige:** Beim Einschalten von „📱 Handy-Anzeige“ (Live-Grafik)
  erscheint ein Fenster mit QR-Code — mit der Handy-Kamera scannen, schon laufen die
  Messwerte im Handy-Browser (gleiches WLAN). Knopf „▦ QR-Code“ neben der Adresse; im
  Terminal zeigt `handy` den Code als Textgrafik.

## 0.4.0 — 22.11.2026

### Geändert
- **Andere Marken:** Steuergeräte, die auf den OBD-Adressen nur OBD sprechen (z. B. Golf-6-Motor
  MED17 — kein „Tester Present“), werden jetzt gefunden und über OBD ausgelesen
  (Fahrgestellnummer, gespeicherte und sporadische Fehler); Löschen dort über OBD Mode 04 mit
  Hinweis, dass die Readiness zurückgesetzt wird. Vorher fand die Suche an solchen Autos nichts.
- **ELM327:** `autoscan` und Berichte finden die UDS-Steuergeräte jetzt auch über den ELM327
  (Profil des Fahrzeugs aus früheren Scans, sonst übliche VAG-Kennungen) — vorher wurde
  nur der OBD-Bereich abgefragt. Fehlerspeicher wird bei fehlender Antwort einmal wiederholt
  (Golf-6-Airbag über ELM327). Am Auto geprüft 22.11.2026.
- **Firmenkopf oben statt in der Fußzeile, mit Firmenlogo:** Auf Seite 1 jedes PDF-Berichts
  rechts im Titelbalken (Logo auf weißem Feld), auf Folgeseiten klein oben rechts. Logo als
  PNG (auch mit Transparenz) oder JPG — GUI „🖼 Logo wählen…“, Terminal `firmenkopf --logo`.
  Es wird mit dem Firmenkopf gespeichert und kommt so auch in die Datensicherung.
- Fehlerbericht (PDF): lange Fehlerarten bei UDS-Fehlern überlagerten die Beschreibung —
  sie stehen jetzt als graue Zeile darunter.
- **Softwarestände** zeigen immer nur den Stand des gewählten Fahrzeugs (mit Namen in der
  Überschrift); beim Fahrzeugwechsel wird neu geladen, dazu die Änderungen seit dem vorigen
  gespeicherten Stand. Vorher blieb der Stand des zuvor gezeigten Autos stehen.
- **Aufgeräumte Oberfläche — alles Eigene an einer Stelle (Werkzeuge → Fahrzeug-Datenbank):**
  - Der **Label-Editor** ist entfallen: „Eigene Labels“ ist eine normale Quelle und wird
    unter „Inhalt ansehen und bearbeiten“ bearbeitet — jetzt auch **Sollwerte** und die
    **Codier-Kennung**; „+ Steuergerät“ und „⬇ Aus letztem Scan anlegen“ sind dort.
  - **Eigene Namen** und **Eigene Fehlertexte** sind als Abschnitt **„Eigene Texte“** auf
    die Datenbank-Seite gezogen (dazu Codier-Texte, Kennungs-Namen, Wert-Texte). Benennen
    am Messwertblock (✏) und die Liste „Alle Blöcke lesen“ bleiben im TP-2.0-Reiter.
  - **Rohwerte umrechnen** ist entfallen (das Programm rechnet überall selbst um).
  - **Erweiterte Identifikation** nur noch über den Knopf 🆔 beim gewählten Steuergerät.
  - **Grundeinstellungen (Referenz)** steht unten auf „Fehlerbilder & Referenz“.
  - **Partikelfilter** und **Zielbereich** erscheinen nur, wenn das aktive Fahrzeug ein
    Diesel sein kann (DPF eingetragen, „TDI“ o. ä. in der Bezeichnung, oder unbekannt).
- **Datenpakete** kennen jetzt auch `codier_kennung`, `[[…sollwert]]` und
  `[[…stellfunktion]]`; Sollwerte und Stellfunktionen gelten aus allen Quellen.
- Label-Ordner werden nicht mehr angelegt; Label-Dateien nur noch zum Austausch (Import).
  `labels/beispiel.toml` ist entfallen — Vorlage ist `beispiele/datenpaket-alles.toml`.
- **Datenbank-Wissen wird am Auto genutzt — über die Teilenummer:** Beim Lesen von
  Anpassungskanälen (TP 2.0, GUI und `kwp-anpassung`) stehen die Kanal-Namen aus allen
  Quellen der Fahrzeug-Datenbank dabei, z. B. „Kanal 007 (Abschaltung Standheizung): Wert …“;
  dazu „Bekannte Kanäle … alle eintragen“. Die Codier-Ansicht nimmt Codier-Bits aus der
  Datenbank, wenn kein Label passt. Gesucht wird nach der Teilenummer aus der Kennung
  (genau gleich vor „ohne Index-Buchstaben“ vor „gleiche Grundnummer“), weil die
  TP-2.0-Adressen nicht die gewohnten Diagnoseadressen sind.
- Steuergerätebaum aus der Gateway-Verbauliste: Bus 06 heißt „Kombi-CAN“, Bus 14 „Gateway selbst“;
  UDS-Steuergeräte (Airbag, Klima, Kombi, Radio …) gelten nicht mehr fälschlich als
  „im letzten Scan nicht gefunden“ — am Golf 6 geprüft 22.11.2026.
- `kwp-coding` (Terminal) schlüsselt die Codierung mit Bit-Namen aus der Fahrzeug-Datenbank auf
  (über die Teilenummer). Die Teilenummer wird auch bei Steuergeräten gefunden, die ihre
  Kennung nur über 0x22 liefern (Golf-6-Bordnetz) — am Auto geprüft 22.11.2026.
- **CAN-Monitor** nimmt die CAN-Botschaften aus der Fahrzeug-Datenbank (importierte DBC-Dateien,
  Datenpakete), solange keine eigene DBC-Datei geladen ist; Knopf „🗄 Aus der Datenbank“.
- **UDS-Messwerte** (Kennungen lesen, Freeze-Frame, Live-Grafik) zeigen Name, Einheit und
  Umrechnung auch aus Datenpaketen/CSV/ODX der Fahrzeug-Datenbank, nicht nur aus eigenen Labels.
- Fahrzeug-Datenbank: **Teilenummer nachschlagen** (GUI und `db teil 5K0937086`) und
  **🧹 Aufräumen** (`db aufraeumen`): löscht Anpassungskanäle ohne echten Namen („Kanal 4“);
  solche Platzhalter werden beim Import gar nicht erst übernommen.
- **Die Fahrzeug-Datenbank ist jetzt der einzige Speicher für eigenes Wissen:** eigene Labels
  (Label-Editor), Fehlertexte, Namen für Messwerte, Codier-Bits und Anpassungen sowie
  Messwert-Texte. Beim ersten Start werden Label-Ordner und Text-Dateien einmal übernommen;
  die Dateien bleiben als Sicherung liegen (`*.toml.uebernommen`), gelesen wird nur noch die Datenbank.
- Label-Editor: Einträge statt Dateien; „Als Datei…“ zum Weitergeben, „Löschen…“ mit Rückfrage;
  hineingezogene Label-Dateien werden mit „Speichern“ übernommen.
- **Auch alle übrigen Daten liegen jetzt in der Datenbank:** Fahrzeuge, Scans, Codier-,
  Anpassungs- und Software-Sicherungen, Trends, Profile, Logins, Fehlerbilder, Ansichten,
  Firmenkopf und Einstellungen. Beim ersten Start übernommen; die alten Dateien liegen danach
  in `<Datenordner>/vor-datenbank/`. PDFs, CSV-Aufzeichnungen und Mitschnitte bleiben Dateien,
  die Spracheinstellung ebenfalls.
- **Neue Seite Werkzeuge → Fahrzeug-Datenbank:** Übersicht und Quellen (mit Entfernen), Datei
  importieren (Art wird erkannt: DBC, Datenpaket, Label-Datei, CSV, ODX/PDX), mitgelieferte Daten
  aktualisieren, Messwert-Texte lernen (Block-Export + Lesung am Auto oder gespeichert), Export.
  Datenpakete und ODX/PDX lassen sich auch ins Fenster ziehen. Im Terminal: `db import --datei …`.
- **Vorschau vor dem Import:** Die Datei wird erst in eine leere Datenbank im Speicher übernommen
  und angezeigt (Quelle, Lizenz, Hersteller › Adresse › Steuergerät mit Fehlern, Kennungen,
  Codier-Bits, Anpassungen, CAN-Botschaften). Erst „Übernehmen“ schreibt in die eigene Datenbank.
- **Inhalt ansehen und bearbeiten** (wie im Label-Editor): Quelle wählen, suchen, Steuergerät,
  allgemeine Fehlertexte oder CAN-Botschaft öffnen und ändern, löschen. Eigene Labels öffnen sich
  im Label-Editor.
- `db csv` liest auch Dateien im Windows-Zeichensatz und überspringt Kommentarzeilen (`;`);
  Exporte eines Werkstattprogramms (Messwertblöcke, Anpassungskanäle) werden erkannt und erklärt
  statt falsch eingelesen.
- CSV darf **Fehlertexte und Messwerte gemischt** enthalten: Kopfzeilen (`code;text`,
  `did;name;…`) schalten um, ohne Kopfzeile wird jede Zeile einzeln erkannt.
  Beispiele: `beispiele/datenpaket-alles.toml` (alles in einem Paket) und `beispiele/gemischt.csv`.
- **Fehlerart bei TP 2.0 mit Text:** statt nur „Fehlerart 14“ steht jetzt z. B.
  „Fehlerart 14: Bauteil defekt“ (eingebaut für die Nummern, die an Golf 6 und Polo 6R
  vorkamen). Eigene Fehlerarten lassen sich in der Fahrzeug-Datenbank anlegen
  (Datenpaket `[[fehlerart]]`, CSV-Abschnitt `fehlerart;text;protokoll`, Editor unter
  „Inhalt ansehen und bearbeiten“) — für TP20, UDS und KW1281; eigene haben Vorrang.
- Datensicherung und „Fahrzeug als ZIP“ schreiben alles weiter als lesbare Dateien (auch eigene
  Texte und Labels); Einspielen holt es zurück in die Datenbank. Alte Sicherungen lassen sich
  weiter einspielen.

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
