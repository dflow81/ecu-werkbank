//! Eigene Texte zu Messwertblöcken (TP 2.0 / KWP): Name des Blocks und jedes
//! Felds, einmal eingetragen und danach in GUI, Terminal, Testlauf und
//! Kurven genutzt (`<Datenordner>/messwerttexte.toml`).
//!
//! Ohne eigenen Text zeigt ECUWerkbank eine Art aus der Umrechnungsformel
//! (Drehzahl, Temperatur, Spannung …). Die sagt nur, *was für ein* Wert es
//! ist, nicht welcher — darum steht sie mit „~“ davor.

use crate::kwp::BlockValue;
use crate::tr;
use anyhow::{Context, Result};
use std::collections::BTreeMap;

fn path() -> std::path::PathBuf {
    crate::history::data_dir().join("messwerttexte.toml")
}

/// Schlüssel: „01-001“ für den Block, „01-001-3“ für Feld 3 (Adresse hex, Block dezimal).
pub fn schluessel(adresse: u8, block: u8, feld: Option<u8>) -> String {
    match feld {
        Some(f) => format!("{adresse:02X}-{block:03}-{f}"),
        None => format!("{adresse:02X}-{block:03}"),
    }
}

/// Alle eigenen Texte.
pub fn alle() -> BTreeMap<String, String> {
    std::fs::read_to_string(path()).ok().and_then(|t| toml::from_str(&t).ok()).unwrap_or_default()
}

/// Text setzen (`feld` None = Name des Blocks); leerer Text löscht.
pub fn setzen(adresse: u8, block: u8, feld: Option<u8>, text: &str) -> Result<()> {
    let mut m = alle();
    let k = schluessel(adresse, block, feld);
    if text.trim().is_empty() {
        m.remove(&k);
    } else {
        m.insert(k, text.trim().to_string());
    }
    let p = path();
    std::fs::create_dir_all(p.parent().expect("hat Eltern"))?;
    std::fs::write(&p, toml::to_string(&m)?).with_context(|| tr!("Schreiben: {a}", a = p.display()))
}

/// Mehrere Texte auf einmal (Schlüssel wie [`schluessel`]); leerer Text löscht.
pub fn mehrere_setzen(liste: &[(String, String)]) -> Result<()> {
    if liste.is_empty() {
        return Ok(());
    }
    let mut m = alle();
    for (k, t) in liste {
        if t.trim().is_empty() {
            m.remove(k);
        } else {
            m.insert(k.clone(), t.trim().to_string());
        }
    }
    let p = path();
    std::fs::create_dir_all(p.parent().expect("hat Eltern"))?;
    std::fs::write(&p, toml::to_string(&m)?).with_context(|| tr!("Schreiben: {a}", a = p.display()))
}

/// Art des Werts aus der Umrechnungsformel (nur grob).
pub fn art(formel: u8) -> Option<&'static str> {
    Some(match formel {
        0x01 => crate::trs!("Drehzahl"),
        0x02 | 0x17 | 0x21 => crate::trs!("Anteil"),
        0x03 => crate::trs!("Winkel"),
        0x05 => crate::trs!("Temperatur"),
        0x06 | 0x15 => crate::trs!("Spannung"),
        0x07 => crate::trs!("Geschwindigkeit"),
        0x0F | 0x16 => crate::trs!("Zeit"),
        0x10 => crate::trs!("Bits"),
        0x12 => crate::trs!("Druck"),
        0x14 => crate::trs!("Abweichung"),
        0x1B => crate::trs!("Zündwinkel"),
        0x22 => crate::trs!("Leistung"),
        0x23 => crate::trs!("Verbrauch"),
        0x24 => crate::trs!("Kilometerstand"),
        0x04 => crate::trs!("Zündwinkel"),
        0x11 => crate::trs!("Text"),
        0x18 => crate::trs!("Strom"),
        0x19 => crate::trs!("Luftmasse"),
        0x1A => crate::trs!("Temperatur"),
        0x27 | 0x31 | 0x33 => crate::trs!("Menge je Hub"),
        0x37 => crate::trs!("Zeit"),
        0x5E | 0x34 => crate::trs!("Drehmoment"),
        0x1F => crate::trs!("Lambda"),
        0x51 => crate::trs!("Winkel"),
        0x3C => crate::trs!("Zeit"),
        0x41 => crate::trs!("Weg"),
        0x42 => crate::trs!("Spannung"),
        0x60 => crate::trs!("Druck"),
        0x5F => crate::trs!("Text"),
        _ => return None,
    })
}

/// Texte für einen Block (eigene Texte + Art aus der Formel).
#[derive(Debug, Clone, Default)]
pub struct Texte {
    m: BTreeMap<String, String>,
}

impl Texte {
    /// Eigene Texte laden.
    pub fn laden() -> Self {
        Self { m: alle() }
    }

    /// Eigener Name des Blocks.
    pub fn block(&self, adresse: u8, block: u8) -> Option<&str> {
        self.m.get(&schluessel(adresse, block, None)).map(String::as_str)
    }

    /// Eigener Text eines Felds — ohne „~Art“.
    pub fn name(&self, adresse: u8, block: u8, feld: u8) -> Option<&str> {
        self.eigen(adresse, block, feld)
    }

    /// Eigener Text eines Felds (Feld ab 1).
    pub fn eigen(&self, adresse: u8, block: u8, feld: u8) -> Option<&str> {
        self.m.get(&schluessel(adresse, block, Some(feld))).map(String::as_str)
    }

    /// Name eines Felds: eigener Text, sonst „~Art“, sonst „Feld n“.
    pub fn feld(&self, adresse: u8, block: u8, feld: u8, v: &BlockValue) -> String {
        match (self.name(adresse, block, feld), art(v.formula)) {
            (Some(t), _) => t.to_string(),
            (None, Some(a)) => format!("~{a}"),
            (None, None) => tr!("Feld {a}", a = feld),
        }
    }

    /// Ein Block als Textzeile: „Drehzahl 800 1/min | ~Temperatur 90 °C | …“.
    pub fn zeile(&self, adresse: u8, block: u8, werte: &[BlockValue]) -> String {
        werte
            .iter()
            .enumerate()
            .filter(|(_, v)| !v.is_leer())
            .map(|(i, v)| format!("{}: {}", self.feld(adresse, block, i as u8 + 1, v), v.display()))
            .collect::<Vec<_>>()
            .join("  |  ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texte_setzen_und_anzeigen() {
        let _g = crate::history::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("vag-mw-{}", std::process::id()));
        std::env::set_var("VAG_DATA_DIR", &dir);
        let werte = [BlockValue { formula: 0x01, a: 200, b: 20, text: None }, BlockValue { formula: 0x05, a: 10, b: 190, text: None }, BlockValue { formula: 0x99, a: 1, b: 2, text: None }];
        let t = Texte::laden();
        assert_eq!(t.feld(1, 1, 1, &werte[0]), "~Drehzahl");
        assert_eq!(t.feld(1, 1, 3, &werte[2]), "Feld 3");

        setzen(0x01, 1, Some(2), "Kühlmitteltemperatur").unwrap();
        setzen(0x01, 1, None, "Leerlauf").unwrap();
        let t = Texte::laden();
        assert_eq!(t.block(1, 1), Some("Leerlauf"));
        assert_eq!(t.feld(1, 1, 2, &werte[1]), "Kühlmitteltemperatur");
        assert_eq!(t.feld(1, 2, 2, &werte[1]), "~Temperatur"); // anderer Block
        let z = t.zeile(1, 1, &werte);
        assert!(z.starts_with("~Drehzahl: 800.00 1/min  |  Kühlmitteltemperatur: 90.00 °C"), "{z}");
        assert_eq!(alle().keys().cloned().collect::<Vec<_>>(), ["01-001", "01-001-2"]);

        setzen(0x01, 1, Some(2), "").unwrap();
        assert_eq!(alle().len(), 1);
        std::env::remove_var("VAG_DATA_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
