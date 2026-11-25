//! Steuergeräte-Speicher auslesen (UDS ReadMemoryByAddress 0x23) — **nur lesen**.
//!
//! Wofür: eine Sicherung des lesbaren Speichers (z. B. EEPROM-Bereich mit
//! Anpassungswerten) vor einer Reparatur, oder um zu sehen, was ein Steuergerät
//! überhaupt herausgibt. Geschrieben wird nie — es gibt in diesem Modul keine
//! Funktion dafür.
//!
//! Viele Steuergeräte geben Speicher nur in der erweiterten Sitzung oder nach
//! einer Anmeldung heraus, manche gar nicht. Abgelehnte Blöcke werden als Lücke
//! vermerkt (mit Grund), der Rest wird weiter gelesen. Nach mehreren
//! Ablehnungen in Folge wird aufgehört, damit ein gesperrtes Gerät nicht
//! minutenlang gefragt wird.

use crate::tr;
use crate::transport::Backend;
use crate::uds::Uds;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

/// Nach so vielen abgelehnten Blöcken in Folge wird aufgehört.
pub const MAX_ABLEHNUNGEN: usize = 8;

/// Ein ausgelesener Bereich.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Auszug {
    pub tx: u16,
    pub rx: u16,
    pub von: u32,
    pub laenge: u32,
    pub erstellt: String,
    /// gelesene Stücke: (Adresse, Daten)
    #[serde(skip)]
    pub stuecke: Vec<(u32, Vec<u8>)>,
    /// nicht lesbare Bereiche: (Adresse, Länge, Grund)
    pub luecken: Vec<(u32, u32, String)>,
    /// vorzeitig beendet (zu viele Ablehnungen)
    pub abgebrochen: bool,
}

impl Auszug {
    /// Anzahl gelesener Bytes.
    pub fn gelesen(&self) -> usize {
        self.stuecke.iter().map(|(_, d)| d.len()).sum()
    }

    /// Als zusammenhängendes Abbild; nicht lesbare Stellen = 0xFF.
    pub fn abbild(&self) -> Vec<u8> {
        let mut v = vec![0xFF; self.laenge as usize];
        for (a, d) in &self.stuecke {
            let ab = (a - self.von) as usize;
            let bis = (ab + d.len()).min(v.len());
            v[ab..bis].copy_from_slice(&d[..bis - ab]);
        }
        v
    }

    /// Kurzbericht.
    pub fn zusammenfassung(&self) -> String {
        let mut t = tr!(
            "{a} von {b} Byte gelesen ab {c}",
            a = self.gelesen(),
            b = self.laenge,
            c = format!("{:#010x}", self.von)
        );
        if !self.luecken.is_empty() {
            t += &tr!(", {a} Lücke(n)", a = self.luecken.len());
        }
        if self.abgebrochen {
            t += &tr!(" — vorzeitig beendet (zu viele Ablehnungen)");
        }
        t
    }
}

/// Einstellungen fürs Lesen.
#[derive(Debug, Clone)]
pub struct Auftrag {
    pub tx: u16,
    pub rx: u16,
    pub von: u32,
    pub laenge: u32,
    /// Bytes je Anfrage (Standard 128; manche Geräte wollen weniger)
    pub block: u16,
    /// Diagnosesitzung vorher (z. B. 0x03 erweitert); None = Standardsitzung
    pub sitzung: Option<u8>,
}

/// Grund einer Ablehnung in Worten (NRC-Text, sonst die Fehlermeldung).
fn grund(e: &anyhow::Error) -> String {
    let t = format!("{e:#}");
    for (nrc, text) in [
        ("NRC 0x31", tr!("Adresse außerhalb des Bereichs")),
        ("NRC 0x33", tr!("gesperrt — Anmeldung nötig")),
        ("NRC 0x22", tr!("Bedingungen nicht erfüllt (andere Sitzung?)")),
        ("NRC 0x11", tr!("Dienst nicht unterstützt")),
        ("NRC 0x7F", tr!("in dieser Sitzung nicht erlaubt")),
        ("NRC 0x13", tr!("Länge nicht angenommen (kleinere Blöcke versuchen)")),
    ] {
        if t.contains(nrc) {
            return text;
        }
    }
    t
}

/// Bereich lesen. `fortschritt(gelesen, gesamt)` nach jedem Block.
pub fn lesen(be: &Backend, a: &Auftrag, mut fortschritt: impl FnMut(u32, u32)) -> Result<Auszug> {
    if a.laenge == 0 || a.block == 0 {
        bail!(tr!("Länge und Blockgröße müssen größer als 0 sein."));
    }
    let tp = be.open(a.tx, a.rx)?;
    let uds = Uds::new(&*tp);
    match a.sitzung {
        Some(s) => uds.diagnostic_session(s).with_context(|| tr!("Sitzung {a} abgelehnt", a = format!("{s:#04x}")))?,
        None => uds.diagnostic_session(crate::uds::SESSION_DEFAULT).context(tr!("Steuergerät antwortet nicht (Zündung an?)"))?,
    };
    let mut z = Auszug { tx: a.tx, rx: a.rx, von: a.von, laenge: a.laenge, erstellt: crate::report::now_local(), ..Default::default() };
    let ende = a.von.saturating_add(a.laenge);
    let mut adr = a.von;
    let mut ablehnungen = 0;
    while adr < ende {
        let n = u32::from(a.block).min(ende - adr);
        match uds.read_memory(adr, n as u16) {
            Ok(d) if !d.is_empty() => {
                ablehnungen = 0;
                let d: Vec<u8> = d.into_iter().take(n as usize).collect();
                z.stuecke.push((adr, d));
            }
            Ok(_) => z.luecken.push((adr, n, tr!("leere Antwort"))),
            Err(e) => {
                ablehnungen += 1;
                let g = grund(&e);
                // gleiche Lücke direkt dahinter zusammenfassen
                match z.luecken.last_mut() {
                    Some((la, ll, lg)) if *la + *ll == adr && *lg == g => *ll += n,
                    _ => z.luecken.push((adr, n, g)),
                }
                if ablehnungen >= MAX_ABLEHNUNGEN {
                    z.abgebrochen = true;
                    break;
                }
            }
        }
        adr += n;
        fortschritt(adr - a.von, a.laenge);
    }
    if z.gelesen() == 0 && !z.luecken.is_empty() {
        let g = z.luecken[0].2.clone();
        bail!(tr!("Nichts lesbar: {a}", a = g));
    }
    Ok(z)
}

/// Als `<name>.bin` (Abbild) und `<name>.toml` (Beschreibung, Lücken) speichern.
pub fn speichern(z: &Auszug, bin: &std::path::Path) -> Result<std::path::PathBuf> {
    if let Some(d) = bin.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(bin, z.abbild()).with_context(|| tr!("Schreiben: {a}", a = bin.display()))?;
    let meta = bin.with_extension("toml");
    std::fs::write(&meta, toml::to_string(z)?)?;
    Ok(meta)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simulation_mit_luecke() {
        let _g = crate::sim::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::sim::set_modell(crate::sim::Modell::Demo);
        // Demo: 0x0000–0x7FFF lesbar, 0x8000–0x8FFF gesperrt (NRC 0x33)
        let a = Auftrag { tx: 0x7E0, rx: 0x7E8, von: 0x7F00, laenge: 0x200, block: 0x80, sitzung: Some(0x03) };
        let z = lesen(&Backend::Demo, &a, |_, _| {}).unwrap();
        assert_eq!(z.gelesen(), 0x100);
        assert_eq!(z.luecken.len(), 1, "{:?}", z.luecken);
        assert_eq!(z.luecken[0].0, 0x8000);
        assert_eq!(z.luecken[0].1, 0x100);
        assert!(z.luecken[0].2.contains("Anmeldung"));
        let b = z.abbild();
        assert_eq!(b.len(), 0x200);
        assert_eq!(b[0x100], 0xFF, "Lücke = FF");
        assert_eq!(b[0], crate::sim::speicher_byte(0x7F00));
        // ganz gesperrt: Fehler statt leerer Datei
        let a = Auftrag { von: 0x8000, laenge: 0x100, ..a };
        assert!(lesen(&Backend::Demo, &a, |_, _| {}).is_err());
    }

    #[test]
    fn abbruch_nach_vielen_ablehnungen() {
        let _g = crate::sim::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::sim::set_modell(crate::sim::Modell::Demo);
        let a = Auftrag { tx: 0x7E0, rx: 0x7E8, von: 0x7FF0, laenge: 0x10000, block: 0x10, sitzung: None };
        let z = lesen(&Backend::Demo, &a, |_, _| {}).unwrap();
        assert!(z.abgebrochen);
        assert_eq!(z.gelesen(), 0x10);
    }
}
