#!/usr/bin/env python3
"""Baut das Handbuch (PDF) von ecu-werkbank.

Aufruf:  python3 docs/handbuch/build_handbuch.py        → ecu-werkbank-Handbuch.pdf (Bilder aus bilder/)
         python3 docs/handbuch/build_handbuch.py --en   → ecu-werkbank-Manual.pdf   (Bilder aus bilder-en/)
Die Texte stehen in inhalt_de.py bzw. inhalt_en.py; hier nur Layout und Bauen.

Die Bilder in bilder/ sind Bildschirmfotos der GUI mit der Golf-6-Simulation
(Beispielfahrzeug M-AB 123, erfundene Fahrgestellnummer). Alle auf einmal neu erzeugen
(legt das Beispielfahrzeug in einem leeren Arbeitsordner an):
  docs/handbuch/bilder_erzeugen.sh [Arbeitsordner] [Seite …]
Einzeln — die GUI fotografiert sich selbst und schließt sich danach:
  VAG_TRANSPORT=golf6 VAG_HELL=1 VAG_FENSTER=1000x880 VAG_TAB=<seite> \
  VAG_FOTO=bild.ppm VAG_FOTO_WARTEN=<sekunden> ecu-werkbank-gui
Seiten (VAG_TAB) z. B.: start, steuergeraete, obd, tp20, tp20-block, grundeinstellung,
batterie, diagnose, fahrzeuge, service-anzeige, historie, sicherung, software, testlauf,
sicherheit, ident, lexikon, namen, editor-scan, service-frage, lernen.
Mit eigenem Datenordner (VAG_DATA_DIR) und Label-Ordner (VAG_LABELS), damit keine
echten Daten ins Bild kommen.
Braucht: python3-reportlab, Schrift DejaVu Sans.
"""

import sys
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
EN = "--en" in sys.argv[1:]
BILDER = HIER / ("bilder-en" if EN else "bilder")
AUSGABE = HIER / ("ecu-werkbank-Manual.pdf" if EN else "ecu-werkbank-Handbuch.pdf")
STAND = "27.11.2026"
# Ausschnitt (oben, unten in Pixeln) aus dem Bild grundeinstellung.png: Abschnitt „Grundeinstellung“
GRUND_AUSSCHNITT = (306, 462)
VERSION = "1.0.0"

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
    # ganze Fenster-Fotos (seit 0.4.0) nicht zu klein drucken
    if not ausschnitt and w >= 900:
        hoehe_mm = max(hoehe_mm, 105)
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


# ---------------------------------------------------------------- Inhalt
INHALT = HIER / ("inhalt_en.py" if EN else "inhalt_de.py")
exec(compile(INHALT.read_text(encoding="utf-8"), str(INHALT), "exec"))

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
    if EN:
        c.drawString(20 * mm, 12 * mm, f"ECUWerkbank · Manual · as of {STAND}")
        c.drawRightString(190 * mm, 12 * mm, f"Page {d.page}")
    else:
        c.drawString(20 * mm, 12 * mm, f"ECUWerkbank · Handbuch · Stand {STAND}")
        c.drawRightString(190 * mm, 12 * mm, f"Seite {d.page}")
    c.restoreState()


doc = Doc(str(AUSGABE), pagesize=A4, leftMargin=20 * mm, rightMargin=20 * mm, topMargin=18 * mm,
          bottomMargin=20 * mm, title="ECUWerkbank — Manual" if EN else "ECUWerkbank — Handbuch", author="dflow81")
doc.multiBuild(story, onFirstPage=fusszeile, onLaterPages=fusszeile)
for f in BILDER.glob("_*.png"):
    f.unlink()
print(f"Geschrieben: {AUSGABE}")
