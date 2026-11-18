//! DBC-Dateien (CAN-Datenbeschreibung) lesen und Signale aus Frames
//! entschlüsseln — für eigene oder offene DBC-Dateien (z. B. aus dem
//! opendbc-Projekt). ECUWerkbank bringt selbst keine Hersteller-DBC mit.
//!
//! Unterstützt: `BO_` (Botschaft), `SG_` (Signal) mit Intel (@1) und
//! Motorola (@0), mit/ohne Vorzeichen, Faktor, Offset, Einheit; `VAL_`
//! (Wertetexte). Multiplexer werden erkannt, aber nicht ausgewertet (das
//! Signal wird dann nur gezeigt, wenn es nicht gemultiplext ist).

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct Signal {
    pub name: String,
    pub start: u32,
    pub laenge: u32,
    /// true = Intel (@1, little endian), false = Motorola (@0)
    pub intel: bool,
    pub vorzeichen: bool,
    pub faktor: f64,
    pub offset: f64,
    pub einheit: String,
    /// gemultiplext (m…) — wird nicht entschlüsselt
    pub multiplex: bool,
    pub werte: HashMap<i64, String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Botschaft {
    pub id: u32,
    pub name: String,
    pub laenge: u8,
    pub signale: Vec<Signal>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Dbc {
    pub botschaften: HashMap<u32, Botschaft>,
}

/// Ein entschlüsselter Wert.
#[derive(Debug, Clone, PartialEq)]
pub struct Wert {
    pub signal: String,
    pub wert: f64,
    pub einheit: String,
    pub text: Option<String>,
}

impl Wert {
    pub fn anzeige(&self) -> String {
        match &self.text {
            Some(t) => format!("{} = {t}", self.signal),
            None => format!("{} = {} {}", self.signal, runden(self.wert), self.einheit).trim_end().to_string(),
        }
    }
}

fn runden(v: f64) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

impl Dbc {
    /// DBC-Text lesen. Unbekannte Zeilen werden übersprungen.
    pub fn lesen(text: &str) -> Dbc {
        let mut d = Dbc::default();
        let mut aktuell: Option<u32> = None;
        for zeile in text.lines() {
            let z = zeile.trim();
            if let Some(r) = z.strip_prefix("BO_ ") {
                // BO_ 640 Motor_1: 8 Vector__XXX
                let mut t = r.split_whitespace();
                let (Some(id), Some(name), Some(len)) = (t.next(), t.next(), t.next()) else { continue };
                let Ok(roh) = id.parse::<u32>() else { continue };
                // Bit 31 kennzeichnet in DBC eine 29-Bit-ID
                let id = roh & 0x1FFF_FFFF;
                d.botschaften.insert(id, Botschaft { id, name: name.trim_end_matches(':').into(), laenge: len.parse().unwrap_or(8), signale: Vec::new() });
                aktuell = Some(id);
            } else if let Some(r) = z.strip_prefix("SG_ ") {
                if let (Some(id), Some(s)) = (aktuell, signal(r)) {
                    if let Some(b) = d.botschaften.get_mut(&id) {
                        b.signale.push(s);
                    }
                }
            } else if let Some(r) = z.strip_prefix("VAL_ ") {
                wertetexte(&mut d, r);
            } else if !z.is_empty() {
                aktuell = None;
            }
        }
        d
    }

    /// Alle Signale eines Frames entschlüsseln (leer, wenn die ID unbekannt ist).
    pub fn entschluesseln(&self, id: u32, daten: &[u8]) -> Vec<Wert> {
        let Some(b) = self.botschaften.get(&id) else { return Vec::new() };
        b.signale
            .iter()
            .filter(|s| !s.multiplex)
            .filter_map(|s| {
                let roh = rohwert(s, daten)?;
                Some(Wert { signal: s.name.clone(), wert: roh as f64 * s.faktor + s.offset, einheit: s.einheit.clone(), text: s.werte.get(&roh).cloned() })
            })
            .collect()
    }
}

/// `Name [m…] : 12|8@1+ (0.5,-40) [min|max] "Einheit" Empfänger`
fn signal(r: &str) -> Option<Signal> {
    let (kopf, rest) = r.split_once(':')?;
    let mut k = kopf.split_whitespace();
    let name = k.next()?.to_string();
    let multiplex = k.next().is_some_and(|m| m.starts_with('m') && m != "M");
    let rest = rest.trim();
    let (pos, rest) = rest.split_once(' ')?;
    let (start, rest_pos) = pos.split_once('|')?;
    let (laenge, art) = rest_pos.split_once('@')?;
    let intel = art.starts_with('1');
    let vorzeichen = art.ends_with('-');
    let fo = rest.trim().strip_prefix('(')?.split_once(')')?.0;
    let (f, o) = fo.split_once(',')?;
    let einheit = rest.split('"').nth(1).unwrap_or("").to_string();
    Some(Signal {
        name,
        start: start.parse().ok()?,
        laenge: laenge.parse().ok()?,
        intel,
        vorzeichen,
        faktor: f.trim().parse().ok()?,
        offset: o.trim().parse().ok()?,
        einheit,
        multiplex,
        werte: HashMap::new(),
    })
}

/// `VAL_ 640 Gang 0 "Leerlauf" 1 "Gang 1" ;`
fn wertetexte(d: &mut Dbc, r: &str) {
    let mut t = r.split_whitespace();
    let (Some(id), Some(sig)) = (t.next().and_then(|i| i.parse::<u32>().ok()), t.next()) else { return };
    let Some(s) = d.botschaften.get_mut(&(id & 0x1FFF_FFFF)).and_then(|b| b.signale.iter_mut().find(|s| s.name == sig)) else { return };
    let rest: String = t.collect::<Vec<_>>().join(" ");
    let teile: Vec<&str> = rest.split('"').collect();
    for p in teile.chunks(2) {
        if let [zahl, text] = p {
            if let Ok(n) = zahl.trim().parse::<i64>() {
                s.werte.insert(n, (*text).to_string());
            }
        }
    }
}

/// Rohwert (ganzzahlig, mit Vorzeichen) aus den Daten holen.
pub fn rohwert(s: &Signal, daten: &[u8]) -> Option<i64> {
    if s.laenge == 0 || s.laenge > 64 {
        return None;
    }
    let mut v: u64 = 0;
    if s.intel {
        for i in 0..s.laenge {
            let bit = s.start + i;
            let byte = *daten.get((bit / 8) as usize)?;
            v |= u64::from(byte >> (bit % 8) & 1) << i;
        }
    } else {
        // Motorola: Startbit ist das höchstwertige Bit; innerhalb eines Bytes
        // abwärts, dann ins nächste Byte (Bit 7)
        let mut bit = s.start;
        for _ in 0..s.laenge {
            let byte = *daten.get((bit / 8) as usize)?;
            v = v << 1 | u64::from(byte >> (bit % 8) & 1);
            bit = if bit.is_multiple_of(8) { bit + 15 } else { bit - 1 };
        }
    }
    if s.vorzeichen && s.laenge < 64 && v >> (s.laenge - 1) & 1 == 1 {
        Some(v as i64 - (1i64 << s.laenge))
    } else {
        Some(v as i64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DBC: &str = r#"
VERSION ""

BO_ 640 Motor_1: 8 Vector__XXX
 SG_ Drehzahl : 16|16@1+ (0.25,0) [0|16383] "U/min" Vector__XXX
 SG_ Temperatur : 8|8@1+ (0.75,-48) [-48|142] "°C" Vector__XXX
 SG_ Gang : 0|4@1+ (1,0) [0|15] "" Vector__XXX

BO_ 2147484458 Erweitert: 8 Vector__XXX
 SG_ Wert_BE : 7|12@0- (1,0) [0|0] "" Vector__XXX

VAL_ 640 Gang 0 "Leerlauf" 1 "Gang 1" ;
"#;

    #[test]
    fn intel_signale_und_texte() {
        let d = Dbc::lesen(DBC);
        // Drehzahl 0x0BB8/4 = 750, Temperatur 0x8C*0.75-48 = 57, Gang 0
        let w = d.entschluesseln(640, &[0x00, 0x8C, 0xB8, 0x0B, 0, 0, 0, 0]);
        let a: Vec<String> = w.iter().map(Wert::anzeige).collect();
        assert_eq!(a, vec!["Drehzahl = 750 U/min", "Temperatur = 57 °C", "Gang = Leerlauf"]);
        assert!(d.entschluesseln(0x123, &[0; 8]).is_empty());
    }

    #[test]
    fn motorola_mit_vorzeichen_und_29bit() {
        let d = Dbc::lesen(DBC);
        // Startbit 7, 12 Bit: Byte0 komplett + obere 4 Bit von Byte1 → 0xFFE = -2
        let w = d.entschluesseln(0x32A, &[0xFF, 0xE0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(w[0].wert, -2.0);
    }
}
