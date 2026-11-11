#!/usr/bin/env python3
"""Baut das Handbuch (PDF) von ecu-werkbank.

Aufruf:  python3 docs/handbuch/build_handbuch.py
Ergebnis: docs/handbuch/ecu-werkbank-Handbuch.pdf

Die Bilder in bilder/ sind Bildschirmfotos der GUI mit der Golf-6-Simulation
(Beispielfahrzeug M-AB 123, erfundene Fahrgestellnummer). Neu erzeugen — die GUI
fotografiert sich selbst und schließt sich danach:
  VAG_TRANSPORT=golf6 VAG_HELL=1 VAG_FENSTER=1000x880 VAG_TAB=<seite> \
  VAG_FOTO=bild.ppm VAG_FOTO_WARTEN=<sekunden> ecu-werkbank-gui
Seiten (VAG_TAB) z. B.: start, steuergeraete, obd, tp20, tp20-block, grundeinstellung,
batterie, diagnose, fahrzeuge, service-anzeige, historie, sicherung, software, testlauf,
sicherheit, ident, codierhelfer, lexikon, namen, editor-scan, service-frage, lernen.
Mit eigenem Datenordner (VAG_DATA_DIR) und Label-Ordner (VAG_LABELS), damit keine
echten Daten ins Bild kommen.
Braucht: python3-reportlab, Schrift DejaVu Sans.
"""

from pathlib import Path

from reportlab.lib import colors
from reportlab.lib.enums import TA_CENTER
from reportlab.lib.pagesizes import A4
from reportlab.lib.styles import ParagraphStyle
from reportlab.lib.units import mm
from reportlab.pdfbase import pdfmetrics
from reportlab.pdfbase.ttfonts import TTFont
from reportlab.platypus import (
    Image,
    KeepTogether,
    PageBreak,
    Paragraph,
    SimpleDocTemplate,
    Spacer,
    Table,
    TableStyle,
)
from reportlab.platypus.tableofcontents import TableOfContents

HIER = Path(__file__).resolve().parent
BILDER = HIER / "bilder"
AUSGABE = HIER / "ecu-werkbank-Handbuch.pdf"
STAND = "11.11.2026"

FONTS = Path("/usr/share/fonts/truetype/dejavu")
pdfmetrics.registerFont(TTFont("DV", str(FONTS / "DejaVuSans.ttf")))
pdfmetrics.registerFont(TTFont("DV-B", str(FONTS / "DejaVuSans-Bold.ttf")))
pdfmetrics.registerFont(TTFont("DV-M", str(FONTS / "DejaVuSansMono.ttf")))
pdfmetrics.registerFontFamily("DV", normal="DV", bold="DV-B", italic="DV", boldItalic="DV-B")

BLAU = colors.HexColor("#1f4e79")
GRAU = colors.HexColor("#555555")
HELL = colors.HexColor("#eef3f8")
GELB = colors.HexColor("#fff6d6")
ROT = colors.HexColor("#fde8e8")

S = {
    "titel": ParagraphStyle("titel", fontName="DV-B", fontSize=30, leading=36, textColor=BLAU, alignment=TA_CENTER),
    "unter": ParagraphStyle("unter", fontName="DV", fontSize=14, leading=20, textColor=GRAU, alignment=TA_CENTER),
    "h1": ParagraphStyle("h1", fontName="DV-B", fontSize=18, leading=23, textColor=BLAU, spaceBefore=4, spaceAfter=8, keepWithNext=1),
    "h2": ParagraphStyle("h2", fontName="DV-B", fontSize=13, leading=17, textColor=BLAU, spaceBefore=10, spaceAfter=4, keepWithNext=1),
    "text": ParagraphStyle("text", fontName="DV", fontSize=10, leading=14.5, spaceAfter=5),
    "klein": ParagraphStyle("klein", fontName="DV", fontSize=8.5, leading=11.5, textColor=GRAU, spaceAfter=4),
    "liste": ParagraphStyle("liste", fontName="DV", fontSize=10, leading=14.5, leftIndent=14, bulletIndent=3, spaceAfter=2),
    "code": ParagraphStyle("code", fontName="DV-M", fontSize=8.6, leading=12, leftIndent=6, textColor=colors.HexColor("#1b1b1b")),
    "zelle": ParagraphStyle("zelle", fontName="DV", fontSize=9, leading=12),
    "zelle_b": ParagraphStyle("zelle_b", fontName="DV-B", fontSize=9, leading=12),
    "toc1": ParagraphStyle("toc1", fontName="DV", fontSize=11, leading=17, leftIndent=8),
}

story = []
B = BLAU.hexval()[2:]


def h1(t):
    p = Paragraph(t, S["h1"])
    p._toc = t
    story.append(p)


def h2(t):
    story.append(Paragraph(t, S["h2"]))


def p(t):
    story.append(Paragraph(t, S["text"]))


def klein(t):
    story.append(Paragraph(t, S["klein"]))


def liste(*punkte):
    for t in punkte:
        story.append(Paragraph(t, S["liste"], bulletText="•"))
    story.append(Spacer(1, 3))


def schritte(*punkte):
    for i, t in enumerate(punkte, 1):
        story.append(Paragraph(t, S["liste"], bulletText=f"{i}."))
    story.append(Spacer(1, 3))


def code(*zeilen):
    rows = [[Paragraph(z.replace("&", "&amp;").replace("<", "&lt;").replace(" ", "&nbsp;"), S["code"])] for z in zeilen]
    t = Table(rows, colWidths=[170 * mm])
    t.setStyle(TableStyle([
        ("BACKGROUND", (0, 0), (-1, -1), colors.HexColor("#f2f2f2")),
        ("BOX", (0, 0), (-1, -1), 0.5, colors.HexColor("#cccccc")),
        ("TOPPADDING", (0, 0), (-1, -1), 1.5),
        ("BOTTOMPADDING", (0, 0), (-1, -1), 1.5),
    ]))
    story.append(t)
    story.append(Spacer(1, 6))


def kasten(titel, text, farbe=HELL):
    inhalt = [Paragraph(f"<b>{titel}</b>", S["zelle"]), Paragraph(text, S["zelle"])]
    t = Table([[inhalt]], colWidths=[170 * mm])
    t.setStyle(TableStyle([
        ("BACKGROUND", (0, 0), (-1, -1), farbe),
        ("BOX", (0, 0), (-1, -1), 0.6, colors.HexColor("#b8c4d0")),
        ("LEFTPADDING", (0, 0), (-1, -1), 8),
        ("RIGHTPADDING", (0, 0), (-1, -1), 8),
        ("TOPPADDING", (0, 0), (-1, -1), 6),
        ("BOTTOMPADDING", (0, 0), (-1, -1), 6),
    ]))
    story.append(Spacer(1, 3))
    story.append(t)
    story.append(Spacer(1, 7))


def tabelle(kopf, zeilen, breiten):
    rows = [[Paragraph(k, S["zelle_b"]) for k in kopf]]
    rows += [[Paragraph(str(c), S["zelle"]) for c in z] for z in zeilen]
    t = Table(rows, colWidths=[b * mm for b in breiten], repeatRows=1)
    t.setStyle(TableStyle([
        ("BACKGROUND", (0, 0), (-1, 0), HELL),
        ("GRID", (0, 0), (-1, -1), 0.4, colors.HexColor("#c8c8c8")),
        ("VALIGN", (0, 0), (-1, -1), "TOP"),
        ("TOPPADDING", (0, 0), (-1, -1), 3),
        ("BOTTOMPADDING", (0, 0), (-1, -1), 3),
    ]))
    story.append(t)
    story.append(Spacer(1, 8))


def bild(name, unterschrift, hoehe_mm=150, ausschnitt=None):
    """Bildschirmfoto einfügen; `ausschnitt` = (oben, unten) in Pixeln."""
    pfad = BILDER / f"{name}.png"
    if ausschnitt:
        from PIL import Image as PILImage
        im = PILImage.open(pfad)
        oben, unten = ausschnitt
        teil = BILDER / f"_{name}_{oben}_{unten}.png"
        im.crop((0, oben, im.width, unten)).save(teil)
        pfad = teil
    from PIL import Image as PILImage
    w, h = PILImage.open(pfad).size
    hoehe = hoehe_mm * mm
    breite = hoehe * w / h
    if breite > 170 * mm:
        breite = 170 * mm
        hoehe = breite * h / w
    img = Image(str(pfad), width=breite, height=hoehe)
    rahmen = Table([[img]], colWidths=[breite + 2])
    rahmen.setStyle(TableStyle([("BOX", (0, 0), (-1, -1), 0.6, colors.HexColor("#999999")),
                                ("LEFTPADDING", (0, 0), (-1, -1), 0), ("RIGHTPADDING", (0, 0), (-1, -1), 0),
                                ("TOPPADDING", (0, 0), (-1, -1), 0), ("BOTTOMPADDING", (0, 0), (-1, -1), 0)]))
    story.append(KeepTogether([Spacer(1, 4), rahmen, Paragraph(unterschrift, S["klein"])]))
    story.append(Spacer(1, 6))


# ---------------------------------------------------------------- Titelseite
story += [Spacer(1, 60 * mm), Paragraph("ECUWerkbank", S["titel"]), Spacer(1, 6 * mm),
          Paragraph("Handbuch", S["unter"]), Spacer(1, 3 * mm),
          Paragraph("Diagnose &amp; Codierung für deutsche Autos — unter Linux", S["unter"]),
          Spacer(1, 70 * mm),
          Paragraph(f"Stand {STAND} · geprüft an einem Golf 6 (2010, 1.4 TSI)", S["unter"]),
          PageBreak()]

# ---------------------------------------------------------------- Inhalt
toc = TableOfContents()
toc.levelStyles = [S["toc1"]]
story += [Paragraph("Inhalt", S["h1"]), toc, PageBreak()]

# ---------------------------------------------------------------- 1
h1("1  Was ist ECUWerkbank?")
p("ECUWerkbank ist ein Diagnoseprogramm für Fahrzeuge des VW-Konzerns. Es liest Fehlerspeicher, Messwerte, "
  "Kilometerstand, Software- und Codierungsstände aus allen Steuergeräten — über einen günstigen CAN-Adapter "
  "an der OBD-Buchse. Es gibt eine Oberfläche mit Fenstern (GUI) und Befehle für das Terminal.")
h2("Was es kann")
liste(
    "Alle Steuergeräte finden, auch ältere mit dem Protokoll <b>TP 2.0</b> (etwa 2004–2010) und neuere mit <b>UDS</b>.",
    "Fehler lesen und verständlich anzeigen, als PDF speichern und mit dem letzten Mal vergleichen.",
    "Live-Werte, Messwertblöcke mit eigenen Namen und Favoriten, Batterie- und Starttest, Gemisch-Prüfung, "
    "Abgas-Prüfergebnisse.",
    "Je Fahrzeug einen Ordner führen: Scans, Kilometer, Wartung, Codierungs-Sicherungen, Berichte.",
    "Gebrauchtwagen-Check: gelöschte Fehler? passt der Tacho? gleiche Fahrgestellnummer überall?",
    "Nachschlagen: erweiterte Identifikation, Codier-Helfer, Lexikon der Grundeinstellungen.",
)
h2("Was es bewusst nicht kann")
liste(
    "Keine Eingriffe in Wegfahrsperre, Komponentenschutz oder andere Schutzfunktionen.",
    "<b>Kein Tacho-Verstellen.</b> ECUWerkbank liest den Kilometerstand, schreibt ihn aber nie — jeder "
    "Schreibweg lehnt die Kilometer-Kennungen ab.",
    "Es liefert keinen fremden Programmcode und keine fremden Beschriftungen (Labels) mit. Eigene "
    "Beschreibungen legst du selbst an (Kapitel 12).",
)
kasten("Lesen ist immer sicher",
       "Fast alles in ECUWerkbank <b>liest nur</b>. Funktionen, die etwas am Auto ändern (Fehler löschen, Codierung "
       "schreiben, Grundeinstellung), sind deutlich gekennzeichnet, <b>fragen vorher nach</b> und sichern vorher "
       "den alten Stand. Mehr dazu in Kapitel 13.")

# ---------------------------------------------------------------- 2
h1("2  Was du brauchst")
tabelle(["Teil", "Hinweis"], [
    ["Laptop mit Linux", "Getestet mit Linux Mint. Ein Kernel mit SocketCAN (Standard)."],
    ["CAN-Adapter", "Jeder SocketCAN-Adapter, z. B. DSD TECH SH-C31x, CANable, candleLight, PCAN. "
                    "Er erscheint als <b>can0</b>."],
    ["OBD-Kabel", "Pin 6 = CAN-High, Pin 14 = CAN-Low, Pin 5 = Masse."],
    ["Fahrzeug", "VW, Audi, Seat, Škoda mit CAN-Diagnose (etwa ab 2004)."],
], [40, 130])
h2("CAN-Adapter oder ELM327?")
p("Ein günstiger <b>ELM327</b>-Adapter (USB, Bluetooth, WLAN) geht auch, kann aber deutlich weniger. "
  "Für ältere VW wie den Golf 6 führt am CAN-Adapter kein Weg vorbei.")
tabelle(["Funktion", "CAN-Adapter", "ELM327"], [
    ["OBD-II, Schnell-Check, Batterietest", "ja", "ja"],
    ["UDS-Steuergeräte (Fehler, Codierung, Service …)", "ja", "nur bekannte Geräte"],
    ["Alle Steuergeräte automatisch finden", "ja", "nur OBD-Adressen"],
    ["TP 2.0: Messwertblöcke, Favoriten, Motor-Checks, Grundeinstellung", "ja", "nein"],
    ["CAN-Mitschnitt, Auto-Erkennung", "ja", "nein"],
], [100, 30, 40])
klein("Am Golf 6 geprüft ist nur der CAN-Adapter. Der ELM327-Teil ist noch nicht an echter Hardware getestet.")
h2("Installieren")
p("Im Projektordner einmal bauen und für deinen Benutzer installieren (ohne sudo):")
code("cargo build --release", "./packaging/install.sh")
p("Danach steht <b>ECUWerkbank</b> im Startmenü. Optional richtet <font face='DV-M'>./packaging/install.sh --can</font> "
  "den Adapter so ein, dass er beim Einstecken automatisch bereit ist (fragt einmal nach dem Passwort).")

# ---------------------------------------------------------------- 3
h1("3  Adapter anschließen")
schritte(
    "Adapter in den Laptop stecken, Kabel an die OBD-Buchse (beim Golf links unter dem Lenkrad).",
    "<b>Zündung einschalten.</b> Den Motor brauchst du nur für Live-Werte, Gemisch- und Batterietest.",
    "Wenn du <font face='DV-M'>install.sh --can</font> nicht genutzt hast, den Adapter einschalten:",
)
code("sudo ip link set can0 type can bitrate 500000", "sudo ip link set can0 up")
p("In der GUI oben auf <b>Verbindung</b> klicken. Der Verbindungs-Assistent prüft Adapter, Bitrate und ob ein "
  "Steuergerät antwortet, und sagt dir, was zu tun ist.")
kasten("Adapter DSD TECH SH-C31x startet im Update-Modus",
       "Manchmal meldet sich dieser Adapter nach dem Einstecken als „STM Device in DFU Mode“ "
       "(<font face='DV-M'>lsusb</font>). Dann gibt es kein can0. So holst du ihn zurück:"
       "<br/><font face='DV-M'>sudo dfu-util -a 0 -s 0x08000000:leave</font><br/>"
       "Etwa 2 Sekunden warten und dann die beiden <font face='DV-M'>ip link</font>-Befehle von oben.", GELB)

# ---------------------------------------------------------------- 4
h1("4  Die Oberfläche")
p("Ganz oben stehen immer: die <b>Verbindungsart</b> (echter Adapter oder Simulation), das <b>aktive Fahrzeug</b>, "
  "die Knöpfe <b>Verbindung</b>, <b>Schnell-Check</b>, <b>Fehler-PDF</b>, die Sprache und der Schalter Hell/Dunkel. "
  "Darunter die fünf Bereiche:")
tabelle(["Bereich", "Unterseiten"], [
    ["Start", "Fahrzeug auf einen Blick"],
    ["Diagnose", "Steuergeräte · Prüfen &amp; Fehlersuche · OBD-II · Sicherheit &amp; Assistenz · TP 2.0 (ältere) · "
                 "Andere Marken · Grundeinstellung / Stellglied · Erweiterte Identifikation"],
    ["Messen", "Live-Grafik · Zielbereich · Batterie · Partikelfilter · Motor-Checks · Fahrt-Rekorder · Rohwerte umrechnen"],
    ["Fahrzeug", "Fahrzeuge &amp; Wartung · Service-Anzeige · Historie &amp; Trends · Softwarestände · Nachrüstung · "
                 "Codier-Sicherung · Codierung"],
    ["Werkzeuge", "Testlauf · Mitschnitt &amp; Lernhilfe · Fehlerbilder · Label-Editor · Eigene Namen · "
                  "Codier-Helfer (Label) · Grundeinstellungen (Referenz)"],
], [32, 138])
h2("Sprache")
p("Oben rechts wählst du die Sprache (Feld mit der Weltkugel). Eingebaut sind Deutsch und Englisch. Eine weitere Sprache legst du "
  "mit <font face='DV-M'>ecu-werkbank sprache --vorlage fr</font> an: Die Datei enthält alle Texte, rechts trägst du die "
  "Übersetzung ein. Leere Zeilen bleiben deutsch. Beim nächsten Start steht die Sprache zur Wahl.")
h2("Ohne Auto ausprobieren")
p("Oben bei der Verbindungsart <b>Demo-Fahrzeug</b> wählen. Daneben kannst du das Modell wählen: "
  "„Demo-Fahrzeug“ (frei erfunden) oder <b>„Golf 6 (2010, nachgebaut)“</b> — ein Nachbau eines echten Golf 6 "
  "mit denselben 17 Steuergeräten und Fehlern. Alle Bilder in diesem Handbuch stammen aus dieser Simulation "
  "(Beispielfahrzeug M-AB 123, erfundene Fahrgestellnummer).")

# ---------------------------------------------------------------- 5
h1("5  Startseite: Fahrzeug auf einen Blick")
bild("start", "Startseite mit dem Beispiel-Golf: vier Karten, Steuergeräte mit Fehlern, offene Wartung "
              "und Fehler-Übersicht.", 132)
liste(
    "<b>Kilometerstand</b> mit Datum und gefahrenen km pro Jahr.",
    "<b>Fehler gespeichert</b> — und ob es mehr oder weniger sind als beim Scan davor.",
    "<b>Letzter Scan</b> und <b>nächste Wartung</b> (gelb = bald fällig).",
    "<b>Steuergeräte mit Fehlern</b> — ein Klick öffnet das Steuergerät zum Lesen oder Löschen.",
    "<b>Fehler-Übersicht</b>: neu, weiterhin, behoben, und welche Fehler immer wiederkommen.",
)
p("Alles auf der Startseite kommt aus dem Fahrzeugordner. Dafür muss das Auto nicht angeschlossen sein. "
  "<b>Kilometerverlauf</b> und <b>Fehler je Scan</b> (ab zwei Scans) stehen unter Fahrzeug → Historie &amp; Trends.")
h2("Auto automatisch erkennen")
p("Oben neben der Adapter-Anzeige steht „kein Auto“. Steckst du den Adapter ans Auto und schaltest die Zündung "
  "ein, erscheint nach ein paar Sekunden ein Auto-Symbol mit dem Fahrzeug, und ECUWerkbank wählt es selbst aus. Gefragt wird "
  "nur lesend und nur, wenn gerade nichts anderes läuft. Ein Klick auf die Anzeige schaltet die Erkennung aus "
  "oder wieder an.")

# ---------------------------------------------------------------- 6
h1("6  Fehler lesen")
h2("Am schnellsten: Fehler-PDF")
p("Oben auf <b>Fehler-PDF</b> klicken. ECUWerkbank fragt alle Steuergeräte ab, speichert ein PDF im "
  "Fahrzeugordner und vergleicht mit dem letzten Scan. Beim ersten Mal sucht es alle Adressen ab (Golf 6: "
  "etwa 50 Sekunden). Danach kennt es die Steuergeräte des Autos und braucht nur noch etwa 20 Sekunden "
  "(<b>Schnell-Scan</b>). Mit dem Haken <b>Voll-Scan</b> wird wieder alles abgesucht — sinnvoll nach dem Einbau "
  "eines neuen Steuergeräts.")
bild("steuergeraete", "Diagnose → Steuergeräte: alle Steuergeräte aus dem letzten Scan mit Adresse, "
                      "Teilenummer und Fehlerzahl. Ein Klick liest die Fehler dieses Geräts live.", 140)
h2("Was die Angaben bedeuten")
tabelle(["Angabe", "Bedeutung"], [
    ["aktuell vorhanden", "Der Fehler besteht gerade."],
    ["sporadisch", "War da, ist gerade nicht vorhanden (z. B. Wackelkontakt)."],
    ["wird geprüft", "Noch nicht bestätigt."],
    ["Fehlerart 12 (TP 2.0)", "Nummer der Fehlerart; die Bedeutung legt VW fest."],
    ["Fehlerart 13: Unterbrechung", "Bei UDS-Steuergeräten genormt — ECUWerkbank schreibt sie als Klartext dazu."],
    ["Warnlampe an", "Der Fehler schaltet eine Kontrollleuchte ein."],
    ["NEU / weiterhin / BEHOBEN", "Vergleich mit dem letzten Scan, in dem das Steuergerät lesbar war."],
], [45, 125])
h2("Woher kommen die Fehlertexte?")
p("ECUWerkbank braucht dafür <b>keine Label-Dateien</b>. Es sucht in dieser Reihenfolge:")
schritte(
    "<b>Eigene Fehlertexte</b> — was du selbst eingetragen hast (Werkzeuge → Fehlerbilder → „Eigene Fehlertexte“).",
    "<b>Label-Dateien</b> — nur wenn du welche angelegt hast (freiwillig, Kapitel 12).",
    "<b>Eingebaute Liste</b> der genormten OBD-Codes (Pxxxx) mit deutschen Texten.",
)
p("Beim Motor rechnet ECUWerkbank die 5-stellige Herstellernummer in den OBD-Code um. Beim Golf 6 ist zum Beispiel "
  "<b>08851 = P2293</b> (Hochdruck-Regelventil). Die übrigen Steuergeräte (Türen, Einparkhilfe …) melden "
  "herstellereigene Bauteil-Nummern wie 01627, für die es keine offene Liste gibt. Hier hilft ein eigener Text: "
  "Neben einem Code ohne Text steht ein <b>✏</b>-Knopf. Klicken, Text schreiben, Enter. Er erscheint danach "
  "überall, auch im PDF.")
kasten("Gut zu wissen",
       "Antwortet ein Steuergerät nicht (z. B. Radio aus oder gerade beim Hochfahren), steht es als "
       "„nicht lesbar“ im Protokoll. Seine Fehler gelten dann <b>nicht</b> als behoben.")

# ---------------------------------------------------------------- 7
h1("7  Messen")
h2("Live-Werte (OBD-II)")
p("Diagnose → OBD-II zeigt Fahrgestellnummer, Motorkontrollleuchte, Readiness (Abgas-Tests) und Live-Werte. "
  "Haken bei den gewünschten Werten setzen und <b>Live</b> starten; mit „CSV aufzeichnen“ wird mitgeschrieben. "
  "Der Readiness-Assistent sagt dir, wie du offene Abgas-Tests durch Fahren abschließt.")
bild("obd", "Diagnose → OBD-II mit Readiness, Readiness-Assistent und Live-Werten.", 100)
h2("Messwertblöcke (TP 2.0)")
p("Ältere Steuergeräte liefern ihre Werte in <b>Messwertblöcken</b> mit bis zu 8 Feldern. Unter Diagnose → "
  "TP 2.0 (ältere) <b>Steuergeräte suchen</b>, Adresse wählen (Motor = 0x01) und einen Block lesen — einmal "
  "oder fortlaufend (<b>Live</b>). <b>Alle Blöcke lesen</b> zeigt in einem Durchgang, welche Blöcke ein "
  "Steuergerät überhaupt hat.")
bild("tp20", "Diagnose → TP 2.0: alle gefundenen TP-2.0-Steuergeräte mit Kennung und Fehlerzahl.", 96)
h2("Favoriten und eigene Namen")
p("Ohne Beschriftung zeigt ECUWerkbank nur die <b>Art</b> eines Werts, mit „~“ davor (z. B. „~Drehzahl“). Den "
  "richtigen Namen trägst du einmal selbst ein — mit dem <b>✏</b> neben dem Feld oder dem Blocknamen. Er gilt "
  "danach überall: Live-Werte, Testlauf, Rekorder und Kurven.")
p("Oft gebrauchte Blöcke merkst du dir mit dem <b>Stern</b> (☆ → ★) neben dem Blocknamen. Sie stehen dann als "
  "Leiste <b>★ Favoriten</b> über dem Block; ein Klick öffnet und liest den Block sofort. Entfernen mit ✖ oder "
  "nochmal dem Stern.")
bild("favoriten", "Messwertblock 2 des Motors mit eigenen Feldnamen und der Favoriten-Leiste darüber.", 105,
     ausschnitt=(380, 612))
p("Alle eigenen Namen auf einen Blick — und schnell neue eintragen — unter Werkzeuge → <b>Eigene Namen</b>: "
  "Adresse, Block, Feld (leer = Name des ganzen Blocks) und Name eingeben, <b>Speichern</b>.")
bild("namen", "Werkzeuge → Eigene Namen: Schnell-Eingabe und Liste aller gespeicherten Namen.", 80)
h2("Batterie- und Starttest")
schritte("Zündung an, Motor aus.", "Messen → Batterie → <b>Test starten</b>.",
         "Bei „JETZT MOTOR STARTEN“ den Motor starten und laufen lassen.")
bild("batterie", "Messen → Batterie: Spannungsverlauf beim Anlassen (Simulation).", 76)
tabelle(["Messpunkt", "Gut", "Gemessen am echten Golf 6 (29.10.2026)"], [
    ["Vor dem Start", "12,2–12,6 V", "11,7 V"],
    ["Tiefster Wert beim Anlassen", "über ca. 10 V", "<b>9,4 V — Batterie schwach</b>"],
    ["Nach dem Start", "13,5–14,5 V", "13,8 V — Lichtmaschine lädt"],
], [55, 40, 75])
p("Bricht die Spannung so stark ein, starten manche Steuergeräte (z. B. das Radio) neu. Sie antworten dann ein "
  "paar Sekunden lang nicht oder verspätet.")

# ---------------------------------------------------------------- 8
story.append(PageBreak())
h1("8  Prüfen und Fehlersuche")
bild("diagnose", "Diagnose → Prüfen &amp; Fehlersuche: Gebrauchtwagen-Check, geführte Fehlersuche, "
                 "Reparatur-Modus, Abgas-Prüfergebnisse und Gemisch-Assistent.", 150)
h2("Gemisch-Assistent")
p("Findet Falschluft oder Probleme mit dem Kraftstoffdruck. Motor <b>betriebswarm</b> (über 80 °C), alle Verbraucher "
  "aus, Auto steht. Phase 1: 20 Sekunden Leerlauf. Phase 2: 20 Sekunden etwa 2500 1/min halten — der "
  "Assistent zeigt „mehr Gas“ oder „weniger Gas“. Danach erscheint die wahrscheinliche Ursache.")
h2("Abgas-Prüfergebnisse (Mode 06)")
p("Zeigt, wie knapp Katalysator, Lambdasonden und Zündung an ihren Grenzen liegen — bevor die Motorleuchte "
  "angeht. „Reserve“ heißt: so weit ist der Wert noch von der Grenze weg.")
h2("Gebrauchtwagen-Check")
p("Ein Durchgang: alle Steuergeräte, Fehler, wann zuletzt gelöscht, Fahrgestellnummern in allen Steuergeräten, "
  "Readiness, Abgas-Tests, Batterie und Kilometerstand aus allen Quellen. Ergebnis als PDF „Kaufberatung“. "
  "Mit <b>Aus letztem Scan</b> geht das auch ohne Auto.")
h2("Reparatur-Modus")
p("Vorher-Stand sichern → reparieren → Fehler löschen → Probefahrt → nachher prüfen. Das PDF zeigt, was "
  "behoben ist und was noch da ist.")

# ---------------------------------------------------------------- 9
h1("9  Fahrzeug, Wartung und Verlauf")
h2("Fahrzeuge &amp; Wartung")
p("Jedes Auto hat einen eigenen Ordner, benannt nach der Fahrgestellnummer. Ein Kennzeichen hilft beim Finden. "
  "Scans werden automatisch dem richtigen Auto zugeordnet — so bleiben z. B. dein Auto und das deines Partners "
  "sauber getrennt.")
bild("fahrzeuge", "Fahrzeug → Fahrzeuge &amp; Wartung: Angaben, Kilometerstand, Wartungsplan mit Ampel "
                  "und Inhalt des Fahrzeugordners.", 145)
liste(
    "<b>Vorlage</b> legt die üblichen Posten an (Öl, Filter, Kerzen, Bremsflüssigkeit, Zahnriemen, HU/AU).",
    "Bei jedem Posten <b>heute erledigt</b> klicken — ECUWerkbank rechnet aus, wann er wieder fällig ist. "
    "Gelb = bald fällig, rot = überfällig. Beim Programmstart erscheint ein Hinweis, wenn etwas fällig ist.",
    "<b>Fahrzeugakte (PDF)</b> und <b>ZIP-Sicherung</b> des ganzen Ordners.",
)
h2("Kilometerstand")
p("Der Scan liest den Kilometerstand selbst. Beim Golf 6 gibt ihn das Kombiinstrument nur "
  "<b>auf 10 km abgerundet</b> heraus (Tacho 242 488 km → 242 480 km). Den genauen Wert kannst du von Hand "
  "eintragen — oder mit <b>Genau aus dem Kombi lesen</b> einmal die Stelle suchen lassen, an der das Kombi ihn "
  "km-genau führt (nur lesen). Ein späterer Scan überschreibt den Stand nur, wenn er höher ist.")
h2("Service-Anzeige (Inspektion / Ölwechsel)")
bild("service", "Fahrzeug → Service-Anzeige: Anleitung für die Rückstellung mit den Tasten und das "
                "einmalige Einrichten.", 130)
p("Die Seite liest aus dem Kombi, in wie vielen km und Tagen Inspektion und Ölwechsel fällig sind. "
  "Überfällige Werte sind rot, bald fällige (unter 1000 km oder 30 Tage) gelb.")
p("VW veröffentlicht nicht, unter welchen Kennungen das Kombi diese Werte speichert. ECUWerkbank lernt sie deshalb "
  "<b>einmal je Fahrzeug</b> — am besten beim nächsten Ölwechsel:")
schritte(
    "Zündung an. „Rückstellung einrichten“ aufklappen, <b>Stand „vorher“ merken</b> (liest etwa eine Minute, nur lesen).",
    "Den Service ganz normal <b>mit den Tasten am Kombi</b> zurückstellen (Anleitung steht auf der Seite).",
    "<b>Stand „nachher“ merken</b>, dann <b>Vergleichen</b>.",
    "ECUWerkbank zeigt die Kennungen, die sich geändert haben, und schlägt vor, was sie bedeuten. Mit der Anzeige "
    "im Kombi vergleichen, dann <b>Ins Fahrzeug übernehmen</b>.",
)
p("Danach kann ECUWerkbank den Service selbst zurückstellen: Haken setzen, <b>Service zurückstellen</b>. "
  "Vorher werden die alten Werte gesichert, danach nachgelesen; der Ölwechsel wird im Wartungsplan als "
  "erledigt eingetragen (abschaltbar). <b>Letzte Rückstellung rückgängig</b> schreibt die gesicherten Werte "
  "zurück. Verlangt das Kombi eine Anmeldung, bricht ECUWerkbank ab — dann bleibt der Weg über die Tasten.")
h2("Historie &amp; Trends")
bild("historie", "Fahrzeug → Historie &amp; Trends: Kilometer- und Fehlerverlauf, alle Scans mit Datum, km und "
                 "Fehlerzahl, Vergleich zweier Scans.", 110)
h2("Softwarestände und Codier-Sicherung")
p("<b>Softwarestände</b> listet Teilenummer, Software- und Hardwarestand jedes Steuergeräts und vergleicht mit "
  "dem letzten Mal — so siehst du, ob in der Werkstatt etwas getauscht oder neu programmiert wurde.")
bild("software", "Fahrzeug → Softwarestände.", 72)
p("<b>Codier-Sicherung</b> speichert die Codierung aller Steuergeräte und vergleicht zwei Sicherungen. Vor jeder "
  "Änderung am Auto: erst sichern. Einzelne Steuergeräte lassen sich daraus zurückschreiben.")
bild("sicherung", "Fahrzeug → Codier-Sicherung: Sicherungen vergleichen und einzeln zurückschreiben.", 145)
h2("Wo liegen die Dateien?")
code("~/.local/share/ecu-werkbank/fahrzeuge/<Fahrgestellnummer>/",
     "    fahrzeug.toml          Angaben, km, Wartung",
     "    profil.toml            Steuergeräte für den Schnell-Scan",
     "    historie/              alle Scans",
     "    berichte/              PDFs und Testlauf-Protokolle",
     "    codierung/             Codierungs-Sicherungen",
     "    software/              Softwarestände",
     "    grundeinstellung.toml  gelernte Grundeinstellungs-Abläufe (Kapitel 13)")

# ---------------------------------------------------------------- 10
h1("10  Testlauf und Sicherheit &amp; Assistenz")
h2("Testlauf")
p("Werkzeuge → Testlauf fragt in einem Durchgang alles Wichtige ab und schreibt ein Protokoll mit Rohdaten: "
  "Verbindung, Fahrgestellnummer, alle Steuergeräte mit Fehlerspeicher, Messwertblöcke 1–30 des Motors und den "
  "Kilometerstand im Kombi. Den Tachostand eintragen, dann vergleicht ECUWerkbank ihn mit dem Kombi. "
  "Der Testlauf liest nur und dauert etwa eine Minute.")
bild("testlauf", "Werkzeuge → Testlauf.", 45)
h2("Sicherheit &amp; Assistenz")
p("Fasst Airbag, ABS/ESP, Lenkung, Einparkhilfe und Assistenzsysteme mit ihren Fehlern zusammen — nur lesend. "
  "Arbeiten an Airbag-Systemen gehören in die Fachwerkstatt.")
bild("sicherheit", "Diagnose → Sicherheit &amp; Assistenz.", 60)

# ---------------------------------------------------------------- 11
story.append(PageBreak())
h1("11  Nachschlagen")
p("Drei Seiten helfen beim Verstehen, ohne etwas am Auto zu ändern.")
h2("Erweiterte Identifikation")
p("Diagnose → Erweiterte Identifikation liest alle genormten Kennungen eines <b>UDS</b>-Steuergeräts "
  "(Fahrgestellnummer, Teilenummern, Software- und Hardwarestand, Seriennummer, Codierung …) und benennt sie. "
  "CAN-ID eingeben (z. B. 0x714 für das Kombi), <b>Lesen</b>. Für TP-2.0-Steuergeräte steht die Identifikation "
  "im Reiter TP 2.0.")
bild("ident", "Diagnose → Erweiterte Identifikation des Kombiinstruments (0x714).", 80)
h2("Codier-Helfer")
p("Werkzeuge → Codier-Helfer zeigt zu einer Teilenummer, was die Kurzcodierung bedeutet und welche Login-Codes "
  "im Label stehen. Er braucht <b>deine eigenen</b> Ross-Tech-Labeldateien im Klartext (.lbl) — zum Beispiel aus "
  "deiner VCDS-Installation. Ordner einmal wählen, fertig. ECUWerkbank liest sie nur und kopiert nichts; "
  "verschlüsselte Labels (.clb) gehen nicht.")
bild("codierhelfer", "Werkzeuge → Codier-Helfer, noch ohne Label-Ordner.", 45)
h2("Grundeinstellungen (Referenz)")
p("Ein Lexikon der üblichen Werkstattfunktionen je System: Drosselklappe anlernen, Lenkwinkelsensor, "
  "ABS entlüften, Parkbremse, Service-Intervall … Zu jeder steht, wozu sie dient, worauf du achten musst und "
  "wie heikel sie ist (harmlos / mit Bedacht / sicherheitsrelevant). <b>Hier wird nichts ausgeführt.</b>")
bild("lexikon", "Werkzeuge → Grundeinstellungen (Referenz), nach System sortiert und durchsuchbar.", 140)

# ---------------------------------------------------------------- 12
h1("12  Eigene Beschriftungen")
p("ECUWerkbank liefert keine fremden Beschriftungen mit. Du kannst aber selbst welche anlegen — so viel oder so "
  "wenig du willst. Es gibt drei Wege:")
tabelle(["Weg", "Wofür", "Wo"], [
    ["Eigene Namen", "Namen von Messwertblöcken und ihren Feldern", "✏ am Messwertblock oder Werkzeuge → Eigene Namen"],
    ["Eigene Fehlertexte", "Klartext zu einem Fehlercode", "✏ neben dem Code"],
    ["Label-Editor", "Alles zu einem Steuergerät: Name, Fehlertexte, Messwerte, Sollwerte", "Werkzeuge → Label-Editor"],
], [35, 65, 70])
h2("Label-Editor und „Aus letztem Scan anlegen“")
p("Der Knopf <b>⬇ Aus letztem Scan anlegen</b> legt für jedes Steuergerät aus dem letzten Scan des aktiven "
  "Fahrzeugs eine fertige Datei an — mit Adresse, Name und Teilenummer aus <b>deinem</b> Scan. Steuergeräte, "
  "die schon eine Datei haben, werden übersprungen; überschrieben wird nichts. Danach Datei wählen, "
  "Fehlertexte und Messwerte ergänzen, <b>Speichern</b> (vorher wird eine Sicherung .toml.bak angelegt).")
bild("editor", "Werkzeuge → Label-Editor mit der aus dem Scan angelegten Datei des Bordnetzsteuergeräts.", 75)
kasten("Gut zu wissen",
       "Die Label-Dateien ordnen nach <b>Adresse</b> zu, nicht nach Auto. Haben zwei Autos an derselben Adresse "
       "verschiedene Steuergeräte, können sich Messwert-Namen überschneiden. Fehlertexte und Codierung sind davon "
       "meist nicht betroffen.")
h2("Ross-Tech-Labeldateien (.lbl)")
p("Hast du VCDS, kannst du dessen Klartext-Labels (.lbl) <b>direkt lesen lassen</b>: im Reiter TP 2.0 unter "
  "„Feldnamen aus Labeldateien (.lbl)“ den Ordner angeben. Die Feldnamen erscheinen dann bei den Messwertblöcken. "
  "Nichts wird kopiert. Viele Labels für wichtige Steuergeräte (Motor, ABS, Gateway) sind allerdings "
  "verschlüsselt (.clb) und nicht lesbar — dann bleibt der Weg über eigene Namen.")

# ---------------------------------------------------------------- 13
story.append(PageBreak())
h1("13  Funktionen, die etwas am Auto ändern")
kasten("Vorsicht",
       "Die folgenden Funktionen <b>schreiben</b> ins Steuergerät. Nur im Stand, Zündung an, Motor aus (außer "
       "Grundeinstellungen, die etwas anderes verlangen), Batterie gut geladen — bei schwacher Batterie ein "
       "Ladegerät anschließen. ECUWerkbank sichert vorher automatisch und <b>fragt immer vorher nach</b>. Im "
       "Terminal zeigen schreibende Befehle ohne <font face='DV-M'>--ja</font> nur, was sie tun würden.", ROT)
tabelle(["Funktion", "Wo", "Hinweis"], [
    ["Fehler löschen (ein Steuergerät)", "Diagnose → Steuergeräte", "Fehler erst notieren oder als PDF sichern."],
    ["Alle Fehler löschen", "Diagnose → Steuergeräte", "Löscht auch die OBD-Readiness. Danach erst mehrere Fahrten "
                                                        "bis zur AU."],
    ["Codierung schreiben / zurückspielen", "Fahrzeug → Codierung / Codier-Sicherung", "Nur mit Sicherung vorher."],
    ["Routine, Stellglied (UDS)", "Diagnose → Grundeinstellung / Stellglied", "Nur, wenn du weißt, was "
                                                                              "das Steuergerät dabei tut."],
    ["Grundeinstellung (TP 2.0)", "Diagnose → TP 2.0 (ältere)", "Erst nach einem eigenen Mitschnitt freigeschaltet."],
    ["Service zurückstellen", "Fahrzeug → Service-Anzeige", "Erst nach dem Lernen. Alte Werte werden gesichert, "
                                                            "Rückgängig ist möglich."],
], [50, 55, 65])
kasten("Kilometerstand",
       "ECUWerkbank schreibt den Kilometerstand <b>nie</b>. Jeder Schreibweg — Codierung, Anpassungen, "
       "Service-Werte und der allgemeine Schreibbefehl — lehnt die Kilometer-Kennungen ab, auch mit "
       "<font face='DV-M'>--ja</font>. Tachomanipulation ist strafbar.", GELB)
h2("Routine und Stellglied (UDS)")
p("Im Reiter Diagnose → Grundeinstellung / Stellglied gibst du Steuergerät, Routine-ID bzw. Kennung und Werte "
  "von Hand ein. Bei <b>Sub</b> bedeutet 1 = starten, 2 = stoppen, 3 = Ergebnis abfragen. Vor dem Senden "
  "erscheint immer ein gelber Kasten mit genau dem, was gesendet wird. Erst <b>Ja, senden</b> schickt es ab.")
bild("rueckfrage", "Die Rückfrage vor dem Senden einer Routine.", 70)
h2("Grundeinstellung an TP-2.0-Steuergeräten (wie VCDS „04“)")
p("Welche Anfragen VW dafür über TP 2.0 schickt, ist nicht verlässlich veröffentlicht. ECUWerkbank rät das nicht, "
  "sondern <b>lernt den Ablauf aus deinem eigenen Mitschnitt</b>. Bis dahin ist das Senden für dieses "
  "Steuergerät gesperrt (Schloss-Symbol).")
bild("grundeinstellung", "Diagnose → TP 2.0: der Abschnitt „Grundeinstellung“, noch gesperrt für den Motor (0x01).", 100,
     ausschnitt=(330, 564))
schritte(
    "CAN-Adapter und VCDS-Kabel gleichzeitig ans Auto — dafür braucht es ein <b>OBD-Y-Kabel</b>.",
    "Werkzeuge → Mitschnitt &amp; Lernhilfe: <b>Mitschnitt aufzeichnen</b>.",
    "In VCDS an diesem Steuergerät „Grundeinstellung – 04“ wählen, eine harmlose Gruppe einschalten "
    "(z. B. Drosselklappe anlernen), ein paar Sekunden laufen lassen, ausschalten.",
    "Aufnahme stoppen. In ECUWerkbank dieselbe Gruppe eintragen, <b>Mitschnitt auswerten…</b>, das Ergebnis "
    "prüfen und <b>Übernehmen</b>.",
)
p("Danach lässt sich jede Gruppe dieses Steuergeräts starten (mit Rückfrage), die Werte laufen live mit, "
  "<b>Aus</b> beendet sie. Übernommen wird nur, was das Steuergerät im Mitschnitt bestätigt hat; beim Starten "
  "wird nur die Gruppennummer ausgetauscht. Der Ablauf gilt nur für <b>dieses</b> Fahrzeug.")
bild("mitschnitt", "Werkzeuge → Mitschnitt &amp; Lernhilfe mit einem Beispiel-Mitschnitt.", 95)

# ---------------------------------------------------------------- 14
h1("14  Probleme und Lösungen")
tabelle(["Problem", "Ursache und Lösung"], [
    ["Kein can0", "Adapter im Update-Modus → <font face='DV-M'>sudo dfu-util -a 0 -s 0x08000000:leave</font>, dann "
                  "<font face='DV-M'>ip link</font>-Befehle (Kapitel 3)."],
    ["Bus-Off sofort beim ersten Senden", "CAN-High und CAN-Low vertauscht (Pin 6 / Pin 14) oder falsche Bitrate."],
    ["„ERROR-PASSIVE“, keine Antwort", "Zündung aus oder Stecker nicht richtig drin."],
    ["Nur wenige Steuergeräte gefunden", "Zündung aus: Beim Golf 6 sind dann nur 4 von 17 wach. Zündung an und neu "
                                         "scannen. Die Liste für den Schnell-Scan wird dabei nie kleiner."],
    ["„vorübergehend nicht verfügbar“", "Das Steuergerät ist kurz belegt (Golf-6-Motor bis 3 s nach einer OBD-"
                                        "Abfrage). ECUWerkbank wartet und versucht es erneut."],
    ["Radio antwortet nicht", "Radio ist aus oder startet nach dem Anlassen neu. Einschalten, kurz warten, neu lesen."],
    ["Kilometer weicht um bis zu 9 km ab", "Das Kombi rundet auf 10 km ab. Genau aus dem Kombi lesen oder von Hand "
                                           "eintragen (Kapitel 9)."],
    ["Keine Umgebungsdaten bei TP-2.0-Fehlern", "Die Golf-6-Steuergeräte liefern keine. Das ist kein Fehler."],
    ["Grundeinstellung gesperrt", "Für dieses Steuergerät gibt es noch keinen gelernten Ablauf (Kapitel 13)."],
    ["Feld heißt „~Drehzahl“ statt eines Namens", "Noch kein eigener Name eingetragen — ✏ am Feld (Kapitel 7)."],
    ["GUI hängt / zeigt nichts Neues", "Statuszeile unten lesen. Notfalls GUI schließen und neu starten; Daten "
                                       "gehen dabei nicht verloren."],
], [55, 115])

# ---------------------------------------------------------------- 15
h1("15  Befehle für das Terminal")
p("Alles aus der GUI geht auch im Terminal. Ohne Adapter: <font face='DV-M'>-t demo</font> oder "
  "<font face='DV-M'>-t golf6</font> anhängen. Hilfe zu jedem Befehl: <font face='DV-M'>ecu-werkbank &lt;befehl&gt; --help</font>.")
tabelle(["Befehl", "Was er tut"], [
    ["verbindung / detect", "Verbindungs-Assistent / welche Diagnose-Art das Auto spricht"],
    ["report", "Fehlerspeicher aller Steuergeräte als PDF (Schnell-Scan; <font face='DV-M'>--voll</font> = alles)"],
    ["check", "Schnell-Check mit Ampel und PDF"],
    ["testlauf --km 242488", "Testlauf mit Tachostand"],
    ["obd-info / obd-live", "OBD-II: Readiness / Live-Werte (<font face='DV-M'>--pids 0C,0D,05</font>, hexadezimal)"],
    ["kwp-dtc / kwp-block", "TP 2.0: Fehler / Messwertblöcke (<font face='DV-M'>--address 0x01 --block 1,2</font>)"],
    ["ident --geraet 714", "Erweiterte Identifikation eines UDS-Steuergeräts"],
    ["grundeinstellung", "Lexikon der Grundeinstellungen (nur Nachschlagen)"],
    ["codier-hilfe / lbl-ordner", "Codier-Helfer aus deinen .lbl-Labels / Label-Ordner einstellen"],
    ["batterie", "Batterie- und Starttest"],
    ["gemisch", "Gemisch-Assistent"],
    ["obd-tests", "Abgas-Prüfergebnisse (Mode 06)"],
    ["kaufcheck", "Gebrauchtwagen-Check mit PDF"],
    ["software", "Softwarestände aller Steuergeräte"],
    ["sicherheit", "Sicherheit &amp; Assistenz"],
    ["coding-backup / coding-compare", "Codierung sichern / Sicherungen vergleichen"],
    ["km 242488 / km-genau", "Kilometerstand von Hand eintragen / genau aus dem Kombi lesen"],
    ["service", "Service-Anzeige lesen; <font face='DV-M'>service merken --name vorher</font>, "
                "<font face='DV-M'>service vergleich --uebernehmen</font>, <font face='DV-M'>service anleitung</font>"],
    ["wartung / wartung-vorlage / wartung-erledigt", "Wartungsplan"],
    ["history / compare", "Scans eines Fahrzeugs / zwei Scans vergleichen"],
    ["sprache --vorlage fr", "Vorlage für eine neue Sprache; <font face='DV-M'>--verwaist</font> / "
                             "<font face='DV-M'>--aufraeumen</font> pflegen eine Sprachdatei"],
    ["rdid-scan --tx 0x714 --rx 0x77E --von 0x2200 --bis 0x22FF", "Kennungen eines Steuergeräts lesen (nur lesen)"],
], [72, 98])
h2("Befehle, die schreiben")
p("Diese Befehle zeigen <b>ohne</b> <font face='DV-M'>--ja</font> nur eine Vorschau dessen, was sie senden würden:")
code("ecu-werkbank routine   --tx 0x7E0 --rx 0x7E8 --id 0x0203 --sub 1      # Vorschau",
     "ecu-werkbank routine   --tx 0x7E0 --rx 0x7E8 --id 0x0203 --sub 1 --ja # wirklich senden",
     "ecu-werkbank iocontrol --tx 0x7E0 --rx 0x7E8 --did 0x0101 --control 0x03 --state 01",
     "ecu-werkbank code-write --tx 0x714 --rx 0x77E --did 0x0600 --bytes 01",
     "ecu-werkbank service zuruecksetzen")
klein("Ebenso fragen clear, kwp-clear, obd-clear, fehler-loeschen, coding-restore, codier-bit, anpassung-zurueck "
      "und stellen vorher nach. Kilometer-Kennungen lehnt jeder dieser Wege ab. Siehe Kapitel 13.")

# ---------------------------------------------------------------- Anhang
story.append(PageBreak())
h1("Anhang: Steuergeräte im Golf 6 (2010)")
p("So meldet sich der Golf 6 (1.4 TSI, Baujahr 2010), an dem ECUWerkbank geprüft wurde. Bei TP 2.0 ist die "
  "„Adresse“ eine Logikadresse; sie ist nicht dieselbe wie die übliche Diagnoseadresse.")
tabelle(["Protokoll", "Adresse / ID", "Steuergerät", "Teilenummer"], [
    ["UDS", "0x711", "Wegfahrsperre", "5K0920860F"],
    ["UDS", "0x714", "Kombiinstrument (km: Kennung 0x2203)", "5K0920860F"],
    ["UDS", "0x715", "Airbag", "5K0959655"],
    ["UDS", "0x746", "Klimaanlage (Climatronic)", "5K0907044BD"],
    ["UDS", "0x76C", "Radio/Navigation (RNS 310)", "3C0035270"],
    ["TP 2.0", "0x01", "Motorsteuergerät (MED17.5.5)", "03C906016"],
    ["TP 2.0", "0x03", "ABS/ESP (MK60EC1)", "1K0907379CD"],
    ["TP 2.0", "0x09", "Servolenkung", "1K0909144H"],
    ["TP 2.0", "0x1D", "Einparkhilfe", "1K9919475"],
    ["TP 2.0", "0x1F", "Gateway (J533)", "1K0907530AA"],
    ["TP 2.0", "0x20", "Bordnetzsteuergerät", "5K0937086"],
    ["TP 2.0", "0x22–0x25", "Türsteuergeräte (4×)", "1K0959701AE …704AG"],
    ["TP 2.0", "0x2A", "Lenksäulenelektronik (J527)", "1K0953549CQ"],
    ["TP 2.0", "0x43", "Anhängersteuergerät", "1K0907383F"],
], [22, 27, 71, 50])
klein("Diese Teilenummern gelten für das geprüfte Auto. Andere Ausstattungen haben andere Steuergeräte.")
klein("ECUWerkbank ist ein unabhängiges Projekt und nicht mit der Volkswagen AG oder Ross-Tech verbunden. „VW“, "
      "„Golf“, „VCDS“ und „Ross-Tech“ sind Marken ihrer Inhaber. Lizenz: MIT oder Apache-2.0, © 2026 dflow81.")


# ---------------------------------------------------------------- Bauen
class Doc(SimpleDocTemplate):
    def afterFlowable(self, f):
        if hasattr(f, "_toc"):
            self.notify("TOCEntry", (0, f._toc, self.page))


def fusszeile(c, d):
    if d.page == 1:
        return
    c.saveState()
    c.setFont("DV", 8)
    c.setFillColor(GRAU)
    c.drawString(20 * mm, 12 * mm, f"ECUWerkbank · Handbuch · Stand {STAND}")
    c.drawRightString(190 * mm, 12 * mm, f"Seite {d.page}")
    c.restoreState()


doc = Doc(str(AUSGABE), pagesize=A4, leftMargin=20 * mm, rightMargin=20 * mm, topMargin=18 * mm,
          bottomMargin=20 * mm, title="ECUWerkbank — Handbuch", author="dflow81")
doc.multiBuild(story, onFirstPage=fusszeile, onLaterPages=fusszeile)
for f in BILDER.glob("_*.png"):
    f.unlink()
print(f"Geschrieben: {AUSGABE}")
