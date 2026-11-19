//! Gespeicherte Ansichten der Live-Grafik/Armaturen: welche Messwerte, in
//! welcher Darstellung (Rundinstrument, große Zahl, Balken). Z. B. eine Ansicht
//! „Motor warmfahren“ und eine „Ladedruck“. Gespeichert in
//! `<Datenordner>/ansichten.toml`.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Woher ein Messwert kommt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "art", rename_all = "lowercase")]
pub enum Quelle {
    Obd { pid: u8 },
    Kwp { adresse: u8, block: u8, feld: usize },
    Did { tx: u16, rx: u16, did: u16 },
    /// DBC-Signal aus dem CAN-Monitor
    Can { id: u32, signal: String },
}

/// Darstellung eines Instruments.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Anzeige {
    #[default]
    Rund,
    Zahl,
    Balken,
}

impl Anzeige {
    pub const ALLE: [Anzeige; 3] = [Anzeige::Rund, Anzeige::Zahl, Anzeige::Balken];

    pub fn text(self) -> String {
        match self {
            Anzeige::Rund => crate::tr!("Rundinstrument"),
            Anzeige::Zahl => crate::tr!("große Zahl"),
            Anzeige::Balken => crate::tr!("Balken"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Instrument {
    pub name: String,
    #[serde(default)]
    pub einheit: String,
    pub quelle: Quelle,
    #[serde(default)]
    pub anzeige: Anzeige,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ansicht {
    pub name: String,
    #[serde(default, rename = "instrument")]
    pub instrumente: Vec<Instrument>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Datei {
    #[serde(default, rename = "ansicht")]
    ansichten: Vec<Ansicht>,
}

pub fn pfad() -> PathBuf {
    crate::history::data_dir().join("ansichten.toml")
}

/// Alle gespeicherten Ansichten (leer, wenn keine Datei da ist).
pub fn laden() -> Vec<Ansicht> {
    crate::ablage::lesen(pfad()).ok().and_then(|t| toml::from_str::<Datei>(&t).ok()).map(|d| d.ansichten).unwrap_or_default()
}

fn schreiben(ansichten: Vec<Ansicht>) -> Result<()> {
    let p = pfad();
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    crate::ablage::schreiben(&p, toml::to_string(&Datei { ansichten })?).with_context(|| crate::tr!("{a} schreiben", a = p.display()))
}

/// Ansicht speichern; eine gleichnamige wird ersetzt.
pub fn speichern(a: Ansicht) -> Result<()> {
    let mut alle = laden();
    alle.retain(|x| x.name != a.name);
    alle.push(a);
    alle.sort_by_key(|x| x.name.to_lowercase());
    schreiben(alle)
}

pub fn entfernen(name: &str) -> Result<()> {
    let mut alle = laden();
    alle.retain(|x| x.name != name);
    schreiben(alle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toml_rundreise() {
        let a = Ansicht {
            name: "Motor warmfahren".into(),
            instrumente: vec![
                Instrument { name: "Drehzahl".into(), einheit: "1/min".into(), quelle: Quelle::Obd { pid: 0x0C }, anzeige: Anzeige::Rund },
                Instrument { name: "Öl".into(), einheit: "°C".into(), quelle: Quelle::Kwp { adresse: 1, block: 134, feld: 0 }, anzeige: Anzeige::Zahl },
                Instrument { name: "Blinker".into(), einheit: String::new(), quelle: Quelle::Can { id: 0x3A0, signal: "Blinker".into() }, anzeige: Anzeige::Balken },
            ],
        };
        let t = toml::to_string(&Datei { ansichten: vec![a.clone()] }).unwrap();
        assert!(t.contains("art = \"kwp\"") && t.contains("anzeige = \"zahl\""), "{t}");
        let zurueck: Datei = toml::from_str(&t).unwrap();
        assert_eq!(zurueck.ansichten, vec![a]);
        // ohne Angabe: Rundinstrument
        let alt: Datei = toml::from_str("[[ansicht]]\nname = \"x\"\n[[ansicht.instrument]]\nname = \"y\"\nquelle = { art = \"obd\", pid = 12 }\n").unwrap();
        assert_eq!(alt.ansichten[0].instrumente[0].anzeige, Anzeige::Rund);
    }
}
