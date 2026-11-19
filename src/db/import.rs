//! Eine Datei in die Fahrzeug-Datenbank übernehmen und dabei selbst erkennen,
//! was sie ist: DBC, Datenpaket (TOML/JSON), eigene Label-Datei, CSV, ODX/PDX.
//! Gemeinsam für GUI (Werkzeuge → Fahrzeug-Datenbank, Hineinziehen) und CLI.

use super::{paket::Datenpaket, Db, ImportErgebnis, QuellArt};
use anyhow::{bail, Context, Result};
use std::path::Path;

/// Was erkannt wurde.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Art {
    Dbc,
    Datenpaket,
    Label,
    Csv,
    Odx,
}

impl Art {
    pub fn text(self) -> String {
        match self {
            Art::Dbc => crate::tr!("CAN-Datenbank (DBC)"),
            Art::Datenpaket => crate::tr!("Datenpaket"),
            Art::Label => crate::tr!("Label-Datei"),
            Art::Csv => crate::tr!("CSV-Liste"),
            Art::Odx => crate::tr!("ODX/PDX (nur lokal)"),
        }
    }
}

/// Art einer Datei erkennen (Endung, bei TOML/JSON der Inhalt).
pub fn erkennen(pfad: &Path) -> Result<Art> {
    let endung = pfad.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    Ok(match endung.as_str() {
        "dbc" => Art::Dbc,
        "csv" | "txt" => Art::Csv,
        "odx" | "pdx" | "odx-d" | "odx-c" | "odx-v" => Art::Odx,
        "json" => Art::Datenpaket,
        "toml" => {
            let text = std::fs::read_to_string(pfad).with_context(|| crate::tr!("{a} lesen", a = pfad.display()))?;
            if text.lines().any(|z| z.trim() == "[quelle]") {
                Art::Datenpaket
            } else if text.lines().any(|z| z.trim() == "[ecu]") {
                Art::Label
            } else {
                bail!(crate::tr!("{a}: weder Datenpaket ([quelle]) noch Label-Datei ([ecu]).", a = pfad.display()))
            }
        }
        _ => bail!(crate::tr!("Unbekannte Dateiart: {a} (erwartet .dbc, .toml, .json, .csv, .odx, .pdx)", a = pfad.display())),
    })
}

/// Datei übernehmen. `hersteller`: für DBC und ODX (Pflicht bei ODX);
/// `lizenz`: für DBC und CSV (ohne Lizenz bleibt es lokal).
pub fn datei(db: &Db, pfad: &Path, hersteller: &str, lizenz: &str) -> Result<(Art, String)> {
    let art = erkennen(pfad)?;
    let name = pfad.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let text = match art {
        Art::Dbc => {
            let roh = std::fs::read(pfad).with_context(|| crate::tr!("{a} lesen", a = pfad.display()))?;
            let dbc = crate::dbc::Dbc::lesen(&String::from_utf8_lossy(&roh));
            if dbc.botschaften.is_empty() {
                bail!(crate::tr!("Keine Botschaften in {a} gefunden.", a = name));
            }
            let h = (!hersteller.trim().is_empty()).then(|| db.hersteller(hersteller.trim(), &[], None)).transpose()?;
            db.dbc_importieren(&dbc, &name, lizenz, &pfad.display().to_string(), h)?.text()
        }
        Art::Datenpaket => {
            let p = Datenpaket::lesen(pfad)?;
            db.paket_importieren(&p, QuellArt::User)?.text()
        }
        Art::Label => {
            let lf = crate::labels::LabelFile::load(pfad)?;
            db.label_speichern(None, &lf)?;
            crate::tr!("1 Steuergerät ({a})", a = if lf.ecu.name.is_empty() { name.clone() } else { lf.ecu.name.clone() })
        }
        Art::Csv => db.csv_importieren(pfad, lizenz)?.text(),
        Art::Odx => {
            if hersteller.trim().is_empty() {
                bail!(crate::tr!("Für ODX/PDX bitte den Hersteller angeben."));
            }
            let (p, a) = super::odx::lesen(pfad, hersteller.trim())?;
            let e: ImportErgebnis = db.paket_importieren_lokal(&p, a)?;
            e.text()
        }
    };
    Ok((art, text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn erkennt_und_uebernimmt() {
        let dir = std::env::temp_dir().join(format!("ecuwb-imp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let w = |n: &str, t: &str| {
            let p = dir.join(n);
            std::fs::write(&p, t).unwrap();
            p
        };
        let dbc = w("a.dbc", "BO_ 640 Motor_1: 8 XXX\n SG_ Drehzahl : 16|16@1+ (0.25,0) [0|16256] \"U/min\" XXX\n");
        let paket = w("p.toml", "[quelle]\nname = \"P\"\nlizenz = \"CC0-1.0\"\n\n[[hersteller]]\nname = \"Volkswagen\"\n\n[[hersteller.steuergeraet]]\nadresse = 0x09\nname = \"Bordnetz\"\n");
        let json = w("p.json", r#"{"quelle":{"name":"J"},"fehler":[{"code":"P0420","text":"Kat"}]}"#);
        let label = w("09.toml", "[ecu]\naddress = 0x17\nname = \"Kombi\"\n");
        let csv = w("f.csv", "code;text\nP0171;System zu mager\n");
        let kaputt = w("x.toml", "a = 1\n");
        assert_eq!(erkennen(&paket).unwrap(), Art::Datenpaket);
        assert_eq!(erkennen(&label).unwrap(), Art::Label);
        assert!(erkennen(&kaputt).is_err());
        assert!(erkennen(&dir.join("bild.png")).is_err());
        let db = Db::im_speicher().unwrap();
        assert_eq!(datei(&db, &dbc, "Volkswagen", "MIT").unwrap().0, Art::Dbc);
        assert_eq!(datei(&db, &paket, "", "").unwrap().0, Art::Datenpaket);
        assert_eq!(datei(&db, &json, "", "").unwrap().0, Art::Datenpaket);
        assert_eq!(datei(&db, &label, "", "").unwrap().0, Art::Label);
        assert_eq!(datei(&db, &csv, "", "").unwrap().0, Art::Csv);
        assert_eq!(db.fehlertext("P0420").unwrap().as_deref(), Some("Kat"));
        assert_eq!(db.fehlertext("P0171").unwrap().as_deref(), Some("System zu mager"));
        assert_eq!(db.eigene_labels().unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
