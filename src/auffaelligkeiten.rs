//! Auffälligkeiten in einem Mitschnitt: unbeantwortete Anfragen, langsame
//! Antworten, gehäufte Fehlercodes (NRC), lange Pausen.
//!
//! Arbeitet auf den fertig zusammengesetzten Nachrichten des
//! [`crate::canlog::Decoder`] (UDS, OBD, KWP über TP 2.0).

use crate::canlog::{Message, Proto};
use std::collections::BTreeMap;
use std::fmt::Write as _;

/// Ab hier gilt eine Antwort als langsam (s). ISO 14229 P2: 50 ms, P2*: 5 s.
const LANGSAM: f64 = 1.0;
/// Ohne Antwort nach so vielen Sekunden: „keine Antwort“.
const ZEITUEBERSCHREITUNG: f64 = 2.0;
/// Ab so vielen gleichen Fehlercodes: auffällig.
const HAEUFUNG: usize = 3;
/// Pause im Mitschnitt (s), die erwähnt wird.
const PAUSE: f64 = 5.0;

#[derive(Debug, Clone, PartialEq)]
pub struct Auffaelligkeit {
    pub t: Option<f64>,
    pub text: String,
    /// false = nur zur Einordnung (z. B. Pause), true = eher ein Problem
    pub warnung: bool,
}

/// Ablehnungen, die beim Abtasten normal sind (Kennung/Dienst gibt es nicht).
fn normale_ablehnung(nrc: u8) -> bool {
    matches!(nrc, 0x11 | 0x12 | 0x31)
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Analyse {
    pub anfragen: usize,
    pub beantwortet: usize,
    /// Antwortzeiten in s (nur mit Zeitstempel)
    pub zeiten: Vec<f64>,
    /// (Dienst, NRC) → Anzahl
    pub nrc: BTreeMap<(u8, u8), usize>,
    /// „Antwort folgt“ (NRC 0x78)
    pub warten: usize,
    pub punkte: Vec<Auffaelligkeit>,
}

/// Gesprächspartner: TP-2.0-Adresse oder Paar aus Anfrage-/Antwort-ID (ISO-TP).
fn partner(m: &Message) -> (Proto, u32) {
    match (m.proto, m.address) {
        (Proto::Kwp, Some(a)) => (Proto::Kwp, u32::from(a)),
        // OBD/UDS 11 Bit: Antwort-ID = Anfrage-ID + 8 (0x7E0 → 0x7E8); VW nutzt
        // eigene Paare — dann zählt die Reihenfolge (nächste Antwort gehört zur Anfrage).
        (p, _) => (p, 0),
    }
}

pub fn analysieren(nachrichten: &[Message]) -> Analyse {
    let mut a = Analyse::default();
    // offene Anfrage je Gesprächspartner: (Zeit, Dienst, Text-Zeit der letzten 0x78)
    let mut offen: BTreeMap<(Proto, u32), (Option<f64>, u8)> = BTreeMap::new();
    let mut letzte_zeit: Option<f64> = None;
    for m in nachrichten {
        if let (Some(t), Some(v)) = (m.t, letzte_zeit) {
            if t - v >= PAUSE {
                a.punkte.push(Auffaelligkeit { t: Some(v), text: crate::tr!("{s} s ohne Diagnose-Verkehr", s = format!("{:.0}", t - v)), warnung: false });
            }
        }
        if m.t.is_some() {
            letzte_zeit = m.t;
        }
        let Some(&sid) = m.data.first() else { continue };
        let k = partner(m);
        if m.from_tester {
            // Tester-Present (0x3E) erwartet oft keine Antwort (Unterdrückungs-Bit)
            if sid == 0x3E && m.data.get(1).is_some_and(|b| b & 0x80 != 0) {
                continue;
            }
            if let Some((t0, alt)) = offen.insert(k, (m.t, sid)) {
                a.punkte.push(Auffaelligkeit { t: t0, text: crate::tr!("Keine Antwort auf {a} ({b})", a = dienst(k.0, alt), b = wer(k)), warnung: true });
            }
            a.anfragen += 1;
            continue;
        }
        // Antwort vom Steuergerät
        let (bezug, nrc) = if sid == 0x7F { (m.data.get(1).copied().unwrap_or(0), m.data.get(2).copied()) } else { (sid.wrapping_sub(0x40), None) };
        if nrc == Some(0x78) {
            a.warten += 1;
            continue; // Anfrage bleibt offen, endgültige Antwort folgt
        }
        let Some((t0, req)) = offen.get(&k).copied() else { continue };
        if req != bezug {
            continue;
        }
        offen.remove(&k);
        a.beantwortet += 1;
        if let (Some(t0), Some(t1)) = (t0, m.t) {
            let dt = t1 - t0;
            a.zeiten.push(dt);
            if dt >= LANGSAM {
                a.punkte.push(Auffaelligkeit { t: Some(t0), text: crate::tr!("Langsame Antwort auf {a}: {s} s ({b})", a = dienst(k.0, req), s = format!("{dt:.1}"), b = wer(k)), warnung: true });
            }
        }
        if let Some(n) = nrc {
            *a.nrc.entry((bezug, n)).or_default() += 1;
        }
    }
    // am Ende noch offen: nur melden, wenn danach noch genug Zeit im Mitschnitt war
    for (k, (t0, req)) in offen {
        let lang_genug = match (t0, letzte_zeit) {
            (Some(t0), Some(ende)) => ende - t0 >= ZEITUEBERSCHREITUNG,
            _ => false,
        };
        if lang_genug {
            a.punkte.push(Auffaelligkeit { t: t0, text: crate::tr!("Keine Antwort auf {a} ({b})", a = dienst(k.0, req), b = wer(k)), warnung: true });
        }
    }
    for ((sid, nrc), n) in &a.nrc {
        if *n >= HAEUFUNG && !normale_ablehnung(*nrc) {
            a.punkte.push(Auffaelligkeit { t: None, text: crate::tr!("{n}× abgelehnt: {a} → {b}", n = n, a = dienst(Proto::Uds, *sid), b = nrc_text(*nrc)), warnung: true });
        }
    }
    a.punkte.sort_by(|x, y| x.t.unwrap_or(f64::MAX).total_cmp(&y.t.unwrap_or(f64::MAX)));
    a
}

fn wer(k: (Proto, u32)) -> String {
    match k.0 {
        Proto::Kwp => crate::tr!("TP 2.0, Steuergerät {a}", a = format!("{:02X}", k.1)),
        Proto::Uds => "UDS".into(),
        Proto::Obd => "OBD".into(),
    }
}

fn dienst(p: Proto, sid: u8) -> String {
    if p == Proto::Obd {
        return crate::tr!("OBD-Modus {a}", a = format!("{sid:02X}"));
    }
    format!("{sid:#04X}")
}

fn nrc_text(nrc: u8) -> String {
    format!("{nrc:#04X} {}", crate::uds::nrc_name(nrc))
}

impl Analyse {
    pub fn text(&self) -> String {
        let mut s = String::new();
        let _ = writeln!(s, "\n{}", crate::tr!("Auffälligkeiten"));
        let _ = writeln!(s, "  {}", crate::tr!("Anfragen: {a}, beantwortet: {b}, „Antwort folgt“: {c}×", a = self.anfragen, b = self.beantwortet, c = self.warten));
        if !self.zeiten.is_empty() {
            let mut z = self.zeiten.clone();
            z.sort_by(f64::total_cmp);
            let median = z[z.len() / 2] * 1000.0;
            let max = z[z.len() - 1] * 1000.0;
            let _ = writeln!(s, "  {}", crate::tr!("Antwortzeit: Median {a} ms, längste {b} ms", a = format!("{median:.0}"), b = format!("{max:.0}")));
        }
        if !self.nrc.is_empty() {
            let _ = writeln!(s, "  {}", crate::tr!("Abgelehnte Anfragen (Dienst → Grund: Anzahl):"));
            for ((sid, nrc), n) in &self.nrc {
                let _ = writeln!(s, "    {sid:#04X} → {}: {n}", nrc_text(*nrc));
            }
        }
        if !self.punkte.iter().any(|p| p.warnung) {
            let _ = writeln!(s, "  {}", crate::tr!("✓ nichts Auffälliges (Ablehnungen wie „requestOutOfRange“ sind beim Abtasten normal)"));
        }
        for p in &self.punkte {
            let t = p.t.map(|t| format!("{t:.3}  ")).unwrap_or_default();
            let _ = writeln!(s, "  {} {t}{}", if p.warnung { "⚠" } else { "ℹ" }, p.text);
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(t: f64, tester: bool, data: &[u8]) -> Message {
        Message { t: Some(t), id: if tester { 0x7E0 } else { 0x7E8 }, proto: Proto::Uds, from_tester: tester, address: None, data: data.to_vec() }
    }

    #[test]
    fn antwortzeiten_nrc_und_keine_antwort() {
        let mut v = vec![
            m(0.000, true, &[0x22, 0xF1, 0x90]),
            m(0.020, false, &[0x62, 0xF1, 0x90, 0x41]),
            // 0x78 dann langsame positive Antwort
            m(1.000, true, &[0x31, 0x01, 0x02, 0x03]),
            m(1.010, false, &[0x7F, 0x31, 0x78]),
            m(2.500, false, &[0x71, 0x01, 0x02, 0x03]),
            // keine Antwort, dann neue Anfrage
            m(3.000, true, &[0x22, 0x06, 0x00]),
            m(3.100, true, &[0x22, 0x06, 0x01]),
            m(3.120, false, &[0x7F, 0x22, 0x31]),
        ];
        for i in 0..3 {
            v.push(m(4.0 + f64::from(i), true, &[0x27, 0x01]));
            v.push(m(4.01 + f64::from(i), false, &[0x7F, 0x27, 0x35]));
        }
        // Pause und am Ende unbeantwortet
        v.push(m(20.0, true, &[0x19, 0x02, 0xFF]));
        v.push(m(23.0, true, &[0x3E, 0x80]));
        let a = analysieren(&v);
        assert_eq!(a.anfragen, 8);
        assert_eq!(a.beantwortet, 6);
        assert_eq!(a.warten, 1);
        assert_eq!(a.nrc[&(0x27, 0x35)], 3);
        let t = a.text();
        assert!(t.contains("Langsame Antwort auf 0x31"), "{t}");
        assert!(t.contains("Keine Antwort auf 0x22"), "{t}");
        assert!(t.contains("3× abgelehnt: 0x27"), "{t}");
        assert!(!t.contains("abgelehnt: 0x22"), "requestOutOfRange beim Abtasten ist normal: {t}");
        assert!(t.contains("ohne Diagnose-Verkehr"), "{t}");
        assert!(t.contains("Keine Antwort auf 0x19"), "{t}");
        assert!(!t.contains("0x3E"), "Tester-Present mit Unterdrückung ist normal: {t}");
    }

    #[test]
    fn demo_mitschnitt_ist_unauffaellig() {
        let d = crate::canlog::Decoder::decode_text(&crate::canlog::demo_log());
        let a = analysieren(&d.messages);
        assert!(a.anfragen > 5);
        assert_eq!(a.anfragen, a.beantwortet, "{}", a.text());
        assert!(!a.punkte.iter().any(|p| p.warnung), "{}", a.text());
    }
}
