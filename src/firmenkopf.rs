//! Firmenkopf für Berichte (Werkstattbetrieb): Name, Anschrift, Telefon,
//! E-Mail und Logo. Ist er eingetragen, steht er oben auf jeder PDF-Seite
//! (Seite 1 im Titelbalken). Gespeichert in `<Datenordner>/firmenkopf.toml`
//! (Logo als Base64, damit es mit in die Datensicherung kommt).

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
    /// Firmenlogo (PNG oder JPG) als Base64
    #[serde(skip_serializing_if = "String::is_empty")]
    pub logo: String,
}

/// Größtes erlaubtes Logo (Datei), damit Berichte klein bleiben.
pub const LOGO_MAX_BYTES: usize = 2 * 1024 * 1024;

impl Firmenkopf {
    pub fn ist_leer(&self) -> bool {
        [&self.name, &self.adresse, &self.telefon, &self.email, &self.zusatz, &self.logo].iter().all(|s| s.trim().is_empty())
    }

    /// Zeilen für den Kopf: Name, Anschrift, „Tel. … · Mail“, Zusatz (leere fallen weg).
    pub fn zeilen(&self) -> Vec<String> {
        let tel = if self.telefon.trim().is_empty() { String::new() } else { crate::tr!("Tel. {a}", a = self.telefon.trim()) };
        let kontakt = [tel.as_str(), self.email.trim()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("  ·  ");
        [self.name.trim().to_string(), self.adresse.trim().to_string(), kontakt, self.zusatz.trim().to_string()].into_iter().filter(|s| !s.is_empty()).collect()
    }

    /// Logo aus einer Datei übernehmen (PNG oder JPG; wird geprüft).
    pub fn logo_setzen(&mut self, roh: &[u8]) -> Result<crate::bild::Bild> {
        if roh.len() > LOGO_MAX_BYTES {
            anyhow::bail!(crate::tr!("Logo zu groß ({a} KB) — bitte höchstens 2 MB, am besten unter 300 KB.", a = roh.len() / 1024));
        }
        let b = crate::bild::Bild::lesen(roh)?;
        self.logo = crate::bild::base64(roh);
        Ok(b)
    }

    /// Das gespeicherte Logo als Bild (None ohne Logo oder wenn es unlesbar ist).
    pub fn logo_bild(&self) -> Option<crate::bild::Bild> {
        if self.logo.trim().is_empty() {
            return None;
        }
        crate::bild::base64_lesen(&self.logo).ok().and_then(|r| crate::bild::Bild::lesen(&r).ok())
    }

    /// Alles in einer Zeile: „Name · Adresse · Tel. … · Mail · Zusatz“.
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
    crate::ablage::lesen(pfad()).ok().and_then(|t| toml::from_str(&t).ok()).unwrap_or_default()
}

/// Speichern; ein leerer Firmenkopf löscht die Datei.
pub fn speichern(f: &Firmenkopf) -> Result<()> {
    let p = pfad();
    if f.ist_leer() {
        if crate::ablage::existiert(&p) {
            crate::ablage::entfernen(&p)?;
        }
        return Ok(());
    }
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    crate::ablage::schreiben(&p, toml::to_string(f)?).with_context(|| crate::tr!("{a} schreiben", a = p.display()))
}

/// Firmenkopf für PDFs, falls einer eingetragen ist.
pub fn fuer_pdf() -> Option<Firmenkopf> {
    let f = laden();
    (!f.ist_leer()).then_some(f)
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
    fn kopf_oben_im_pdf_mit_logo() {
        let mut d = crate::pdf::Doc::new();
        d.new_page();
        d.banner((0.1, 0.3, 0.5));
        d.new_page();
        let mut f = Firmenkopf { name: "Kfz Muster".into(), adresse: "Hauptstr. 1".into(), ..Default::default() };
        let mit = String::from_utf8_lossy(&d.to_bytes_mit("t", Some(&f))).to_string();
        assert_eq!(mit.matches("(Kfz Muster) Tj").count(), 2, "auf jeder Seite");
        // oben: Seite 1 im Titelbalken (weiß), Folgeseiten über dem Inhalt
        assert!(mit.contains("1.00 1.00 1.00 rg /F2 10.0 Tf"), "Seite 1 weiß im Balken");
        assert!(!mit.contains(" 28.0 Td (Kfz"), "nicht mehr in der Fußzeile");
        assert!(!mit.contains("/XObject"));
        // mit Logo (PNG 2×1, RGB)
        let png = {
            let roh = [0u8, 255, 0, 0, 0, 0, 255];
            let chunk = |t: &[u8], d: &[u8]| {
                let mut v = (d.len() as u32).to_be_bytes().to_vec();
                v.extend_from_slice(t);
                v.extend_from_slice(d);
                v.extend_from_slice(&[0; 4]);
                v
            };
            let mut p = b"\x89PNG\r\n\x1a\n".to_vec();
            p.extend(chunk(b"IHDR", &[0, 0, 0, 2, 0, 0, 0, 1, 8, 2, 0, 0, 0]));
            p.extend(chunk(b"IDAT", &miniz_oxide::deflate::compress_to_vec_zlib(&roh, 6)));
            p.extend(chunk(b"IEND", &[]));
            p
        };
        f.logo_setzen(&png).unwrap();
        let mit = String::from_utf8_lossy(&d.to_bytes_mit("t", Some(&f))).to_string();
        assert!(mit.contains("/XObject << /Logo"), "Logo eingebunden");
        assert_eq!(mit.matches("/Logo Do").count(), 2, "Logo auf jeder Seite");
        assert!(f.logo_setzen(b"kein bild").is_err());
        let ohne = String::from_utf8_lossy(&d.to_bytes_mit("t", None)).to_string();
        assert!(!ohne.contains("Kfz Muster"));
    }
}
