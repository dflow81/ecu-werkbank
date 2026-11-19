//! Favoriten-Messwerte: oft gebrauchte Messwertblöcke (Steuergerät + Block)
//! mit eigenem Namen, für schnellen Zugriff per Klick.
//!
//! Gespeichert in `<Datenordner>/messwert_favoriten.toml`. Der Name ist nur
//! eine Merkhilfe; die eigentlichen Feldnamen kommen weiter aus
//! [`crate::messwerttexte`]. Die Liste gilt geräteübergreifend — bei mehreren
//! Autos stehen die Adressen/Blöcke des jeweiligen Modells drin.

use crate::tr;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Ein gemerkter Messwertblock.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Favorit {
    /// Logikadresse des Steuergeräts (TP 2.0 / KWP), z. B. 0x01.
    pub adresse: u8,
    /// Blocknummer (1–255).
    pub block: u8,
    /// Merkname (frei), z. B. „Motor Temperaturen“.
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Datei {
    #[serde(default)]
    favorit: Vec<Favorit>,
}

fn path() -> std::path::PathBuf {
    crate::history::data_dir().join("messwert_favoriten.toml")
}

/// Alle Favoriten (nach Adresse, dann Block sortiert).
pub fn alle() -> Vec<Favorit> {
    let mut v = crate::ablage::lesen(path())
        .ok()
        .and_then(|t| toml::from_str::<Datei>(&t).ok())
        .map(|d| d.favorit)
        .unwrap_or_default();
    v.sort_by_key(|f| (f.adresse, f.block));
    v
}

fn speichern(list: &[Favorit]) -> Result<()> {
    let p = path();
    std::fs::create_dir_all(p.parent().expect("hat Eltern"))?;
    let d = Datei { favorit: list.to_vec() };
    crate::ablage::schreiben(&p, toml::to_string(&d)?).with_context(|| tr!("Schreiben: {a}", a = p.display()))
}

/// Ist dieser Block schon gemerkt?
pub fn enthalten(adresse: u8, block: u8) -> bool {
    alle().iter().any(|f| f.adresse == adresse && f.block == block)
}

/// Favorit hinzufügen — oder, wenn Adresse+Block schon da sind, den Namen ändern.
pub fn hinzufuegen(adresse: u8, block: u8, name: &str) -> Result<()> {
    let mut list = alle();
    match list.iter_mut().find(|f| f.adresse == adresse && f.block == block) {
        Some(f) => f.name = name.trim().to_string(),
        None => list.push(Favorit { adresse, block, name: name.trim().to_string() }),
    }
    speichern(&list)
}

/// Favorit entfernen.
pub fn entfernen(adresse: u8, block: u8) -> Result<()> {
    let mut list = alle();
    list.retain(|f| !(f.adresse == adresse && f.block == block));
    speichern(&list)
}

/// An/aus schalten: fehlt der Block, wird er mit `name` gemerkt; sonst entfernt.
/// Liefert `true`, wenn er danach gemerkt ist.
pub fn umschalten(adresse: u8, block: u8, name: &str) -> Result<bool> {
    if enthalten(adresse, block) {
        entfernen(adresse, block)?;
        Ok(false)
    } else {
        hinzufuegen(adresse, block, name)?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hinzufuegen_umschalten_entfernen() {
        let _g = crate::history::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("vag-fav-{}", std::process::id()));
        std::env::set_var("VAG_DATA_DIR", &dir);
        let _ = std::fs::remove_dir_all(&dir);

        assert!(alle().is_empty());
        assert!(!enthalten(0x01, 4));

        // Umschalten merkt den Block …
        assert!(umschalten(0x01, 4, "Motor Temperaturen").unwrap());
        assert!(enthalten(0x01, 4));
        // … nochmal umschalten entfernt ihn wieder.
        assert!(!umschalten(0x01, 4, "").unwrap());
        assert!(!enthalten(0x01, 4));

        // Zwei Favoriten, sortiert nach Adresse/Block.
        hinzufuegen(0x09, 2, "Komfort").unwrap();
        hinzufuegen(0x01, 1, "Leerlauf").unwrap();
        let v = alle();
        assert_eq!(v.len(), 2);
        assert_eq!((v[0].adresse, v[0].block), (0x01, 1));
        assert_eq!((v[1].adresse, v[1].block), (0x09, 2));

        // Gleicher Block erneut: nur Name geändert, kein Duplikat.
        hinzufuegen(0x01, 1, "Motor Leerlauf").unwrap();
        let v = alle();
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].name, "Motor Leerlauf");

        std::env::remove_var("VAG_DATA_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
