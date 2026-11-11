//! Eigene Namen für Kennungen (Anpassungen / Werte, die über „Anpassungen
//! sichern“ gelesen wurden), z. B. 714-2243 = „Strecke seit Inspektion“
//! (`<Datenordner>/kennungstexte.toml`).
//!
//! Schlüssel: Steuergerät + Kennung, beides hex. UDS-Steuergeräte an ihrer
//! Request-CAN-ID (714 = Kombi), TP-2.0-Steuergeräte als 200 + Adresse
//! (Bordnetz 20 → 220) — wie bei den Codier-Texten.

use crate::tr;
use anyhow::{Context, Result};
use std::collections::BTreeMap;

fn path() -> std::path::PathBuf {
    crate::history::data_dir().join("kennungstexte.toml")
}

/// Schlüssel-Teil für ein gesichertes Steuergerät.
pub fn geraet(g: &crate::anpassung::Geraet) -> u16 {
    match g.tp20 {
        Some(a) => crate::codiertexte::tp20_key(a),
        None => g.tx,
    }
}

/// „714-2243“
pub fn schluessel(geraet: u16, did: u16) -> String {
    format!("{geraet:03X}-{did:04X}")
}

pub fn alle() -> BTreeMap<String, String> {
    std::fs::read_to_string(path()).ok().and_then(|t| toml::from_str(&t).ok()).unwrap_or_default()
}

/// Mehrere Namen setzen; leerer Text löscht den Eintrag.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namen_speichern() {
        let _g = crate::history::TEST_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("vag-kt-{}", std::process::id()));
        std::env::set_var("VAG_DATA_DIR", &dir);
        let g = crate::anpassung::Geraet { name: "Kombi".into(), tx: 0x714, rx: 0x77E, kennungen: Default::default(), tp20: None };
        let k = schluessel(geraet(&g), 0x2243);
        assert_eq!(k, "714-2243");
        let bn = crate::anpassung::Geraet { tp20: Some(0x20), ..g };
        assert_eq!(schluessel(geraet(&bn), 0x0600), "220-0600");
        mehrere_setzen(&[(k.clone(), " Strecke seit Inspektion ".into()), ("220-0600".into(), "".into())]).unwrap();
        assert_eq!(alle().get(&k).map(String::as_str), Some("Strecke seit Inspektion"));
        mehrere_setzen(&[(k, String::new())]).unwrap();
        assert!(alle().is_empty());
        std::env::remove_var("VAG_DATA_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
