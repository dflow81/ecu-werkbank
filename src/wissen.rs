//! Fehlerbilder-Bibliothek: eigene Erfahrungen je Modell und Fehlercode.
//!
//! - Eigene Einträge in `<Datenordner>/wissen.toml`
//! - Automatisch aus Reparatur-Notizen aller eigenen Fahrzeuge (gleiches Modell)

use crate::tr;
use crate::guide::normalize;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Eintrag {
    /// Modell-Stichwort, z. B. "Golf 6 1.4 TSI" (leer = alle)
    #[serde(default)]
    pub modell: String,
    pub code: String,
    pub text: String,
    #[serde(default)]
    pub datum: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Datei {
    #[serde(default)]
    eintrag: Vec<Eintrag>,
}

fn path() -> std::path::PathBuf {
    crate::history::data_dir().join("wissen.toml")
}

pub fn eigene() -> Vec<Eintrag> {
    crate::ablage::lesen(path()).ok().and_then(|t| toml::from_str::<Datei>(&t).ok()).map(|d| d.eintrag).unwrap_or_default()
}

pub fn hinzufuegen(e: Eintrag) -> Result<()> {
    let mut d = Datei { eintrag: eigene() };
    d.eintrag.push(Eintrag { code: normalize(&e.code), datum: if e.datum.is_empty() { crate::vehicle::today() } else { e.datum }, ..e });
    let p = path();
    std::fs::create_dir_all(p.parent().expect("hat Eltern"))?;
    crate::ablage::schreiben(&p, toml::to_string(&d)?).with_context(|| tr!("Schreiben: {a}", a = p.display()))
}

pub fn loeschen(i: usize) -> Result<()> {
    let mut d = Datei { eintrag: eigene() };
    if i < d.eintrag.len() {
        d.eintrag.remove(i);
    }
    crate::ablage::schreiben(path(), toml::to_string(&d)?)?;
    Ok(())
}

/// Wortweise Übereinstimmung: alle Wörter des Stichworts kommen im Modell vor.
fn passt(stichwort: &str, modell: &str) -> bool {
    let m = modell.to_lowercase();
    stichwort.is_empty() || stichwort.to_lowercase().split_whitespace().all(|w| m.contains(w))
}

/// Treffer zu Code + Modell: (Herkunft, Text).
pub fn suchen(code: &str, modell: &str) -> Vec<(String, String)> {
    let c = normalize(code);
    let mut out: Vec<(String, String)> = eigene()
        .into_iter()
        .filter(|e| normalize(&e.code) == c && passt(&e.modell, modell))
        .map(|e| (tr!("eigene Notiz{a}", a = if e.modell.is_empty() { String::new() } else { format!(" ({})", e.modell) }), e.text))
        .collect();
    // Aus Reparaturen gleicher Modelle
    for v in crate::vehicle::list() {
        if v.bezeichnung.is_empty() || modell.is_empty() || !passt(&v.bezeichnung, modell) && !passt(modell, &v.bezeichnung) {
            continue;
        }
        for t in crate::history::earlier_repairs(&v.key, &c) {
            out.push((tr!("Reparatur {a}", a = v.label()), t));
        }
    }
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_and_find() {
        let _g = crate::history::TEST_LOCK.lock().unwrap();
        let d = std::env::temp_dir().join(format!("vag-wissen-{}", std::process::id()));
        std::env::set_var("VAG_DATA_DIR", &d);
        hinzufuegen(Eintrag { modell: "Golf 6 TSI".into(), code: "16684".into(), text: "oft Zündspule".into(), datum: String::new() }).unwrap();
        hinzufuegen(Eintrag { modell: "Passat".into(), code: "P0300".into(), text: "anderes Modell".into(), datum: String::new() }).unwrap();
        let r = suchen("P0300", "Golf 6 1.4 TSI");
        assert_eq!(r.len(), 1, "{r:?}");
        assert!(r[0].1.contains("Zündspule"));
        let _ = std::fs::remove_dir_all(d);
    }
}
