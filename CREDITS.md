# Credits & Drittquellen

ecu-werkbank ist eine eigenständige Implementierung. Der gesamte Programmcode wurde
selbst geschrieben. Es werden **keine** Label-Dateien, Datenbanken oder
Schlüssel von Ross-Tech oder Volkswagen mitgeliefert.

Für zwei Login-Verfahren (SecurityAccess, ISO 14229 Dienst 0x27) nutzt das
Projekt öffentlich dokumentierte Prüfwerte und zurückentwickelte Konstanten.
Diese stammen aus den unten genannten Projekten. Es wurde **kein Programmtext
übernommen** — nur Zahlenwerte (Bytecode, Seed/Key-Paare, eine Konstantentabelle)
zum Abgleich der eigenen Umsetzung. Die Quellen sind hier und an der jeweiligen
Stelle in [`src/login.rs`](src/login.rs) genannt.

## SA2 (UDS, bytecode-basiert)

- **bri3d/sa2_seed_key** — <https://github.com/bri3d/sa2_seed_key>
  - Lizenz: MIT, © 2022 Brian Ledbetter
  - Genutzt: die Beschreibung des SA2-Bytecode-Verfahrens und die Prüfwerte aus
    `tests/test_sa2_seed_key.py` (Bytecode → erwarteter Key).
  - Hinweis: Auch jglim/UnlockECU (MIT, © 2020 JinGen Lim,
    <https://github.com/jglim/UnlockECU>) verweist in `VolkswagenSA2.cs` auf
    diese Quelle.

## XOR-Seed/Key (KWP2000 / TP 2.0)

- **bri3d/kwp2000** — <https://github.com/bri3d/kwp2000>
  - Ohne Lizenzangabe veröffentlicht. Genutzt wurden ausschließlich die
    Zahlenkonstanten aus `XorSeedKeyLogin.java` (Konstantentabelle und
    Prüfwerte) zum Abgleich der eigenen Berechnung — kein Programmtext.
  - Die Konstantentabelle ist nicht originär dieses Projekts; sie geht laut
    dessen eigenem Quelltext-Kommentar auf einen öffentlichen Forenbeitrag
    zurück (nefariousmotorsports.com, MED9.1-Thread).

## MIT-Lizenzhinweis (bri3d/sa2_seed_key)

Wie von der MIT-Lizenz verlangt, ist der Copyright- und Lizenzhinweis hier
enthalten:

```
MIT License

Copyright (c) 2022 Brian Ledbetter

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## Protokolle

ISO-TP (ISO 15765-2), UDS (ISO 14229), KWP2000 (ISO 14230) und VWs TP 2.0 sind
nach öffentlichen Normen bzw. öffentlicher Community-Dokumentation selbst
umgesetzt.

## Bibliotheken (Auswahl)

- **qrcode** (kennytm/qrcode-rust) — <https://github.com/kennytm/qrcode-rust>, Lizenz MIT oder Apache-2.0.
  Erzeugt den QR-Code für die Handy-Anzeige.
