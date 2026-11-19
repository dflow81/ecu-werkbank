//! Eigene Fehlertexte: Klartext zu einem Fehlercode, einmal eingetragen und
//! danach in allen Anzeigen und Berichten genutzt (Fahrzeug-Datenbank, Bereich „fehler“; früher `fehlertexte.toml`).
//! Gilt vor den Label-Dateien und der eingebauten OBD-Liste.

use crate::tr;
use anyhow::Result;
use std::collections::BTreeMap;

/// Code vereinheitlichen: Großbuchstaben, ohne Leerzeichen.
pub fn norm(code: &str) -> String {
    code.trim().to_uppercase().replace(' ', "")
}

/// Alle eigenen Texte (Code → Text).
pub fn alle() -> BTreeMap<String, String> {
    crate::db::eigen::alle("fehler")
}

/// Text setzen; leerer Text löscht den Eintrag.
pub fn setzen(code: &str, text: &str) -> Result<()> {
    let code = norm(code);
    if code.is_empty() {
        anyhow::bail!(tr!("kein Fehlercode angegeben"));
    }
    crate::db::eigen::setzen("fehler", &[(code, text.to_string())])
}

/// Mehrere Texte auf einmal setzen (leere werden übersprungen).
pub fn mehrere_setzen(liste: &[(String, String)]) -> Result<()> {
    let liste: Vec<(String, String)> = liste.iter().map(|(c, t)| (norm(c), t.trim().to_string())).filter(|(c, t)| !c.is_empty() && !t.is_empty()).collect();
    crate::db::eigen::setzen("fehler", &liste)
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
