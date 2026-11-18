//! Firmenkopf für Berichte (Werkstattbetrieb): Name, Anschrift, Telefon,
//! E-Mail. Ist er eingetragen, steht er in der Fußzeile jeder PDF-Seite.
//! Gespeichert in `<Datenordner>/firmenkopf.toml`.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Firmenkopf {
    pub name: String,
    pub adresse: String,
    pub telefon: String,
    pub email: String,
    /// freie Zusatzzeile, z. B. Meisterbetrieb, USt-IdNr.
    pub zusatz: String,
}

impl Firmenkopf {
    pub fn ist_leer(&self) -> bool {
        [&self.name, &self.adresse, &self.telefon, &self.email, &self.zusatz].iter().all(|s| s.trim().is_empty())
    }

    /// Eine Zeile für die Fußzeile: „Name · Adresse · Tel. … · Mail · Zusatz“.
    pub fn zeile(&self) -> String {
        let tel = if self.telefon.trim().is_empty() { String::new() } else { crate::tr!("Tel. {a}", a = self.telefon.trim()) };
        [self.name.trim(), self.adresse.trim(), tel.as_str(), self.email.trim(), self.zusatz.trim()]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("  ·  ")
    }
}

pub fn pfad() -> PathBuf {
    crate::history::data_dir().join("firmenkopf.toml")
}

/// Gespeicherten Firmenkopf lesen (leer, wenn keiner da ist).
pub fn laden() -> Firmenkopf {
    std::fs::read_to_string(pfad()).ok().and_then(|t| toml::from_str(&t).ok()).unwrap_or_default()
}

/// Speichern; ein leerer Firmenkopf löscht die Datei.
pub fn speichern(f: &Firmenkopf) -> Result<()> {
    let p = pfad();
    if f.ist_leer() {
        if p.exists() {
            std::fs::remove_file(&p)?;
        }
        return Ok(());
    }
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(&p, toml::to_string(f)?).with_context(|| crate::tr!("{a} schreiben", a = p.display()))
}

/// Fußzeile für PDFs, falls ein Firmenkopf eingetragen ist.
pub fn fusszeile() -> Option<String> {
    let f = laden();
    (!f.ist_leer()).then(|| f.zeile())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zeile_ohne_leere_teile() {
        let f = Firmenkopf { name: "Kfz Muster".into(), telefon: " 0123 456 ".into(), ..Default::default() };
        assert_eq!(f.zeile(), "Kfz Muster  ·  Tel. 0123 456");
        assert!(Firmenkopf::default().ist_leer());
        assert!(!f.ist_leer());
    }

    #[test]
    fn fusszeile_im_pdf() {
        let mut d = crate::pdf::Doc::new();
        d.new_page();
        d.new_page();
        let mit = String::from_utf8_lossy(&d.to_bytes_mit("t", Some("Kfz Muster"))).to_string();
        assert_eq!(mit.matches("(Kfz Muster) Tj").count(), 2, "auf jeder Seite");
        let ohne = String::from_utf8_lossy(&d.to_bytes_mit("t", None)).to_string();
        assert!(!ohne.contains("Kfz Muster"));
    }
}
