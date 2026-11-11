//! Eigene Fehlertexte: Klartext zu einem Fehlercode, einmal eingetragen und
//! danach in allen Anzeigen und Berichten genutzt (`<Datenordner>/fehlertexte.toml`).
//! Gilt vor den Label-Dateien und der eingebauten OBD-Liste.

use crate::tr;
use anyhow::{Context, Result};
use std::collections::BTreeMap;

fn path() -> std::path::PathBuf {
    crate::history::data_dir().join("fehlertexte.toml")
}

/// Code vereinheitlichen: Großbuchstaben, ohne Leerzeichen.
pub fn norm(code: &str) -> String {
    code.trim().to_uppercase().replace(' ', "")
}

/// Alle eigenen Texte (Code → Text).
pub fn alle() -> BTreeMap<String, String> {
    std::fs::read_to_string(path()).ok().and_then(|t| toml::from_str(&t).ok()).unwrap_or_default()
}

/// Text setzen; leerer Text löscht den Eintrag.
pub fn setzen(code: &str, text: &str) -> Result<()> {
    let code = norm(code);
    if code.is_empty() {
        anyhow::bail!(tr!("kein Fehlercode angegeben"));
    }
    let mut m = alle();
    if text.trim().is_empty() {
        m.remove(&code);
    } else {
        m.insert(code, text.trim().to_string());
    }
    let p = path();
    std::fs::create_dir_all(p.parent().expect("hat Eltern"))?;
    std::fs::write(&p, toml::to_string(&m)?).with_context(|| tr!("Schreiben: {a}", a = p.display()))
}

/// Mehrere Texte auf einmal setzen (leere werden übersprungen).
pub fn mehrere_setzen(liste: &[(String, String)]) -> Result<()> {
    if liste.is_empty() {
        return Ok(());
    }
    let mut m = alle();
    for (c, t) in liste {
        let (c, t) = (norm(c), t.trim());
        if !c.is_empty() && !t.is_empty() {
            m.insert(c, t.to_string());
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
    fn eigene_texte_setzen_und_loeschen() {
        let _g = crate::history::TEST_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("vag-ft-{}", std::process::id()));
        std::env::set_var("VAG_DATA_DIR", &dir);
        setzen(" 01627 ", "Einparkhilfe: Sensor hinten").unwrap();
        assert_eq!(alle().get("01627").map(String::as_str), Some("Einparkhilfe: Sensor hinten"));
        let mut db = crate::labels::LabelDb::load_dir(&dir.join("keine-labels")).unwrap();
        assert_eq!(db.dtc_description("01627"), Some("Einparkhilfe: Sensor hinten"));
        db.set_dtc_text("b10b8", "Klima: Test");
        assert_eq!(db.dtc_description("B10B8"), Some("Klima: Test"));
        setzen("01627", "").unwrap();
        assert!(alle().is_empty());
        std::env::remove_var("VAG_DATA_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
