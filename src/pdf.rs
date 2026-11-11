//! Minimaler PDF-Erzeuger (A4, Text, Linien) ohne externe Abhängigkeiten.
//!
//! Nutzt die PDF-Standardschriften Helvetica / Helvetica-Bold mit
//! WinAnsi-Kodierung — die zeigt jeder PDF-Betrachter an, inkl. ä ö ü ß €.
//! Zeichen außerhalb von WinAnsi werden ersetzt.

pub const A4_W: f32 = 595.0;
pub const A4_H: f32 = 842.0;

#[derive(Clone, Copy, PartialEq)]
pub enum Font {
    Regular,
    Bold,
}

/// Ein Dokument aus Seiten mit Zeichenbefehlen.
#[derive(Default)]
pub struct Doc {
    pages: Vec<String>,
}

impl Doc {
    pub fn new() -> Self {
        Self::default()
    }

    /// Neue leere Seite anhängen; weitere Befehle landen dort.
    pub fn new_page(&mut self) {
        self.pages.push(String::new());
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    fn cur(&mut self) -> &mut String {
        if self.pages.is_empty() {
            self.pages.push(String::new());
        }
        self.pages.last_mut().expect("Seite vorhanden")
    }

    /// Text an Position (x, y = Grundlinie, Ursprung unten links).
    pub fn text(&mut self, x: f32, y: f32, size: f32, font: Font, s: &str) {
        self.text_on(None, x, y, size, font, s, None);
    }

    /// Text in Grau oder Farbe (r, g, b in 0..1).
    pub fn text_rgb(&mut self, x: f32, y: f32, size: f32, font: Font, s: &str, rgb: (f32, f32, f32)) {
        self.text_on(None, x, y, size, font, s, Some(rgb));
    }

    /// Text auf eine bestimmte Seite (0-basiert), z. B. für Fußzeilen.
    pub fn text_on_page(&mut self, page: usize, x: f32, y: f32, size: f32, font: Font, s: &str) {
        self.text_on(Some(page), x, y, size, font, s, Some((0.4, 0.4, 0.4)));
    }

    #[allow(clippy::too_many_arguments)]
    fn text_on(&mut self, page: Option<usize>, x: f32, y: f32, size: f32, font: Font, s: &str, rgb: Option<(f32, f32, f32)>) {
        let f = if font == Font::Bold { "F2" } else { "F1" };
        let color = rgb.map(|(r, g, b)| format!("{r:.2} {g:.2} {b:.2} rg ")).unwrap_or_else(|| "0 g ".into());
        let op = format!("BT {color}/{f} {size:.1} Tf {x:.1} {y:.1} Td ({}) Tj ET\n", escape(s));
        match page {
            Some(p) => self.pages[p].push_str(&op),
            None => self.cur().push_str(&op),
        }
    }

    /// Linie von (x1,y1) nach (x2,y2).
    pub fn line(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, width: f32, gray: f32) {
        let op = format!("{gray:.2} G {width:.2} w {x1:.1} {y1:.1} m {x2:.1} {y2:.1} l S\n");
        self.cur().push_str(&op);
    }

    /// Linienzug durch die Punkte (für Diagramme).
    pub fn polyline(&mut self, pts: &[(f32, f32)], width: f32, rgb: (f32, f32, f32)) {
        if pts.len() < 2 {
            return;
        }
        let (r, g, b) = rgb;
        let mut op = format!("{r:.3} {g:.3} {b:.3} RG {width:.2} w 1 J 1 j {:.2} {:.2} m", pts[0].0, pts[0].1);
        for (x, y) in &pts[1..] {
            op.push_str(&format!(" {x:.2} {y:.2} l"));
        }
        op.push_str(" S\n");
        self.cur().push_str(&op);
    }

    /// Rechteck-Rahmen.
    pub fn stroke_rect(&mut self, x: f32, y: f32, w: f32, h: f32, width: f32, gray: f32) {
        let op = format!("{gray:.2} G {width:.2} w {x:.1} {y:.1} {w:.1} {h:.1} re S\n");
        self.cur().push_str(&op);
    }

    /// Gefülltes Rechteck (x, y = unten links).
    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, rgb: (f32, f32, f32)) {
        let (r, g, b) = rgb;
        let op = format!("{r:.2} {g:.2} {b:.2} rg {x:.1} {y:.1} {w:.1} {h:.1} re f\n");
        self.cur().push_str(&op);
    }

    /// Fertiges PDF als Bytes.
    pub fn to_bytes(&self, title: &str) -> Vec<u8> {
        let pages: Vec<&String> = if self.pages.is_empty() { vec![] } else { self.pages.iter().collect() };
        let n = pages.len().max(1);
        // Objekte: 1 Katalog, 2 Seitenbaum, 3/4 Schriften, 5 Info, dann je Seite: Seite + Inhalt
        let mut objs: Vec<Vec<u8>> = Vec::new();
        objs.push(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
        let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 6 + i * 2)).collect();
        objs.push(format!("<< /Type /Pages /Kids [{}] /Count {n} >>", kids.join(" ")).into_bytes());
        objs.push(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_vec());
        objs.push(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>".to_vec());
        let mut info = b"<< /Producer (ECUWerkbank) /Title (".to_vec();
        info.extend(latin1_bytes(&escape(title)));
        info.extend(b") >>");
        objs.push(info);
        let empty = String::new();
        for i in 0..n {
            let content = pages.get(i).copied().unwrap_or(&empty);
            objs.push(
                format!(
                    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {A4_W} {A4_H}] \
                     /Resources << /Font << /F1 3 0 R /F2 4 0 R >> >> /Contents {} 0 R >>",
                    7 + i * 2
                )
                .into_bytes(),
            );
            let bytes = latin1_bytes(content);
            let mut stream = format!("<< /Length {} >>\nstream\n", bytes.len()).into_bytes();
            stream.extend_from_slice(&bytes);
            stream.extend_from_slice(b"\nendstream");
            objs.push(stream);
        }

        let mut out = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
        let mut offsets = Vec::with_capacity(objs.len());
        for (i, o) in objs.iter().enumerate() {
            offsets.push(out.len());
            out.extend(format!("{} 0 obj\n", i + 1).into_bytes());
            out.extend_from_slice(o);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref = out.len();
        out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).into_bytes());
        for off in offsets {
            out.extend(format!("{off:010} 00000 n \n").into_bytes());
        }
        out.extend(
            format!("trailer\n<< /Size {} /Root 1 0 R /Info 5 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1)
                .into_bytes(),
        );
        out
    }
}

/// Text → WinAnsi-Bytes, PDF-String-sicher maskiert. Inhaltsströme bestehen
/// aus Bytes; die Seiteninhalte werden als String gesammelt, daher hier als
/// Latin-1-Zeichen zurück in einen String gelegt.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        let b: u8 = match ch {
            '\\' | '(' | ')' => {
                out.push('\\');
                ch as u8
            }
            ' '..='~' => ch as u8,
            '\u{A0}'..='\u{FF}' => ch as u32 as u8,
            '€' => 0x80,
            '‚' => 0x82,
            '„' => 0x84,
            '…' => 0x85,
            '‘' => 0x91,
            '’' => 0x92,
            '“' => 0x93,
            '”' => 0x94,
            '•' => 0x95,
            '–' => 0x96,
            '—' => 0x97,
            '✓' => {
                out.push_str("OK");
                continue;
            }
            '→' => {
                out.push_str("->");
                continue;
            }
            '\t' => b' ',
            _ => b'?',
        };
        // Als Zeichen ≤ U+00FF ablegen; to_bytes schreibt es 1:1 (latin1_bytes).
        out.push(b as char);
    }
    out
}

/// Inhalt besteht nur aus Zeichen ≤ U+00FF (siehe `escape`) → 1 Byte je Zeichen.
fn latin1_bytes(s: &str) -> Vec<u8> {
    s.chars().map(|c| u32::from(c).min(0xFF) as u8).collect()
}

/// Ungefähre Textbreite in Punkt (Helvetica-Metrik, vereinfacht).
pub fn text_width(s: &str, size: f32, font: Font) -> f32 {
    let em: f32 = s
        .chars()
        .map(|c| match c {
            'i' | 'j' | 'l' | '.' | ',' | ':' | ';' | '\'' | '!' | '|' | 'I' => 0.28,
            ' ' | 'f' | 't' | 'r' | '(' | ')' | '-' | '/' => 0.33,
            'm' | 'w' | 'M' | 'W' | '@' => 0.85,
            'A'..='Z' | 'Ä' | 'Ö' | 'Ü' => 0.68,
            _ => 0.556,
        })
        .sum();
    em * size * if font == Font::Bold { 1.06 } else { 1.0 }
}

/// Text auf eine Breite umbrechen (an Leerzeichen; zu lange Wörter hart).
pub fn wrap(s: &str, width: f32, size: f32, font: Font) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    for word in s.split_whitespace() {
        let candidate = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
        if text_width(&candidate, size, font) <= width || cur.is_empty() {
            cur = candidate;
        } else {
            lines.push(std::mem::take(&mut cur));
            cur = word.to_string();
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}
