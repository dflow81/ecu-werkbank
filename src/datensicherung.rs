//! Automatische Datensicherung: der ganze Datenordner (Fahrzeuge, Scans,
//! Sicherungen, eigene Texte) und die eigenen Labels als ZIP in einen Ordner
//! nach Wahl — z. B. USB-Stick oder Cloud-Ordner. Die GUI sichert beim Start
//! von selbst, wenn die letzte Sicherung älter als eingestellt ist und der
//! Zielordner erreichbar ist (Stick eingesteckt). Ältere Sicherungen werden
//! bis auf die neuesten N gelöscht.

use crate::tr;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const PREFIX: &str = "ecu-werkbank-sicherung";
/// Ordnernamen im ZIP
const DATEN: &str = "ecu-werkbank-daten";
const LABELS: &str = "ecu-werkbank-labels";
/// Namen aus der Zeit vor der Umbenennung (vag-diag) — solche Sicherungen
/// werden weiter erkannt, aufgeräumt und eingespielt.
const ALT_PREFIX: &str = "vag-diag-sicherung";
const ALT_DATEN: &str = "vag-diag-daten";
const ALT_LABELS: &str = "vag-diag-labels";

/// Datum/Uhrzeit-Teil eines Sicherungsnamens (ohne Präfix), sonst None.
fn zeitstempel(name: &str) -> Option<&str> {
    let rest = name.strip_prefix(PREFIX).or_else(|| name.strip_prefix(ALT_PREFIX))?;
    rest.ends_with(".zip").then_some(rest)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Einstellung {
    /// Zielordner (leer = keine automatische Sicherung)
    #[serde(default)]
    pub ziel: String,
    /// Alle wie viele Tage
    #[serde(default = "sieben")]
    pub tage: u32,
    /// Wie viele Sicherungen bleiben liegen
    #[serde(default = "acht")]
    pub behalten: usize,
    /// Unix-Zeit der letzten Sicherung
    #[serde(default)]
    pub zuletzt: u64,
}

fn sieben() -> u32 {
    7
}
fn acht() -> usize {
    8
}

impl Default for Einstellung {
    fn default() -> Self {
        Self { ziel: String::new(), tage: sieben(), behalten: acht(), zuletzt: 0 }
    }
}

fn pfad() -> PathBuf {
    crate::history::data_dir().join("datensicherung.toml")
}

pub fn laden() -> Einstellung {
    crate::ablage::lesen(pfad()).ok().and_then(|t| toml::from_str(&t).ok()).unwrap_or_default()
}

pub fn speichern(e: &Einstellung) -> Result<()> {
    let p = pfad();
    std::fs::create_dir_all(p.parent().expect("hat Eltern"))?;
    crate::ablage::schreiben(&p, toml::to_string(e)?).with_context(|| tr!("Schreiben: {a}", a = p.display()))
}

impl Einstellung {
    /// Ist eine Sicherung dran (eingerichtet, Ziel erreichbar, lange genug her)?
    pub fn faellig(&self, jetzt: u64) -> bool {
        !self.ziel.trim().is_empty() && Path::new(self.ziel.trim()).is_dir() && jetzt.saturating_sub(self.zuletzt) >= u64::from(self.tage.max(1)) * 86_400
    }

    /// Wäre fällig, wenn der Ordner da wäre?
    pub fn faellig_ohne_ziel(&self, jetzt: u64) -> bool {
        !self.ziel.trim().is_empty() && jetzt.saturating_sub(self.zuletzt) >= u64::from(self.tage.max(1)) * 86_400
    }

    /// Eingerichtet, aber der Ordner fehlt gerade (z. B. Stick nicht eingesteckt)?
    pub fn ziel_fehlt(&self) -> bool {
        !self.ziel.trim().is_empty() && !Path::new(self.ziel.trim()).is_dir()
    }
}

/// Vorhandene Sicherungen im Ordner, neueste zuerst.
pub fn liste(ziel: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(ziel)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).and_then(zeitstempel).is_some())
        .collect();
    // Name enthält Datum und Uhrzeit → ohne Präfix sortiert = Zeitordnung
    // (alte und neue Namen gemischt)
    let key = |p: &PathBuf| p.file_name().and_then(|n| n.to_str()).and_then(zeitstempel).unwrap_or("").to_string();
    v.sort_by_key(|p| std::cmp::Reverse(key(p)));
    v
}

/// Jetzt sichern. Gibt (ZIP-Datei, Anzahl Dateien) zurück.
pub fn sichern(ziel: &Path, behalten: usize) -> Result<(PathBuf, usize)> {
    let daten = crate::history::data_dir();
    if !ziel.is_dir() {
        bail!(tr!("Sicherungsordner nicht gefunden: {a} (USB-Stick eingesteckt?)", a = ziel.display()));
    }
    let (z, d) = (ziel.canonicalize().unwrap_or_else(|_| ziel.to_path_buf()), daten.canonicalize().unwrap_or_else(|_| daten.clone()));
    if z.starts_with(&d) {
        bail!(tr!("Der Sicherungsordner darf nicht im Datenordner liegen — bitte z. B. einen USB-Stick oder Cloud-Ordner wählen."));
    }
    std::fs::create_dir_all(&daten)?;
    // Fahrzeug-Datenbank: offene Änderungen in die Hauptdatei schreiben, damit die
    // Kopie im ZIP für sich allein stimmig ist (sonst stünde ein Teil nur in *-wal)
    if crate::db::standard_pfad().exists() {
        crate::db::eigen::mit_db(|db| db.festschreiben()).context(tr!("Fahrzeug-Datenbank für die Sicherung vorbereiten"))?;
    }
    // Inhalt wie früher als lesbare Dateien zusammenstellen: Dokumente, Einträge der
    // Ablage, eigene Texte als *.toml und eigene Labels als Label-Dateien — so lässt
    // sich die Sicherung ohne Programm lesen und auch in ältere Versionen einspielen.
    let stufe = std::env::temp_dir().join(format!("ecu-werkbank-sicherung-{}-{}", std::process::id(), crate::report::unix_now()));
    let _ = std::fs::remove_dir_all(&stufe);
    let (s_daten, s_labels) = (stufe.join("daten"), stufe.join("labels"));
    std::fs::create_dir_all(&s_daten)?;
    crate::ablage::als_ordner(&daten, &s_daten)?;
    crate::db::eigen::ausgeben(&s_daten)?;
    let mit_labels = crate::db::labels::ausgeben(&s_labels)? > 0;
    let mut ordner: Vec<(&Path, &str)> = vec![(&s_daten, DATEN)];
    if mit_labels {
        ordner.push((&s_labels, LABELS));
    }
    let name = crate::vehicle::stamped(PREFIX, "zip");
    let fertig = ziel.join(&name);
    // erst unter anderem Namen schreiben — eine halbe Datei soll nie wie eine Sicherung aussehen
    let teil = ziel.join(format!("{name}.teil"));
    let n = crate::zip::zip_dirs(&ordner, &teil);
    let _ = std::fs::remove_dir_all(&stufe);
    let n = n?;
    std::fs::rename(&teil, &fertig).with_context(|| tr!("Schreiben: {a}", a = fertig.display()))?;
    for alt in liste(ziel).into_iter().skip(behalten.max(1)) {
        let _ = std::fs::remove_file(alt);
    }
    let mut e = laden();
    e.zuletzt = crate::report::unix_now();
    speichern(&e)?;
    Ok((fertig, n))
}

/// Sicherung einspielen. Vorhandene Dateien bleiben, außer `ueberschreiben`.
/// Gibt (eingespielt, übersprungen) zurück.
pub fn zurueckspielen(zip: &Path, ueberschreiben: bool) -> Result<(usize, usize)> {
    let tmp = std::env::temp_dir().join(format!("ecu-werkbank-zurueck-{}-{}", std::process::id(), crate::report::unix_now()));
    std::fs::create_dir_all(&tmp)?;
    let r = (|| -> Result<(usize, usize)> {
        let dateien = crate::zip::unzip_to(zip, &tmp, true)?;
        if !dateien.iter().any(|d| d.starts_with(&format!("{DATEN}/")) || d.starts_with(&format!("{ALT_DATEN}/"))) {
            bail!(tr!("Das ist keine ECUWerkbank-Datensicherung: {a}", a = zip.display()));
        }
        let (mut ein, mut ueber) = (0, 0);
        let daten = crate::history::data_dir();
        for rel in &dateien {
            let quelle = tmp.join(rel);
            match rel.split_once('/') {
                // eigene Texte (oberste Ebene) → Datenbank
                Some((DATEN | ALT_DATEN, r)) if crate::db::eigen::bereich_zur_datei(r).is_some() => {
                    let b = crate::db::eigen::bereich_zur_datei(r).expect("geprüft");
                    let (e, u) = crate::db::eigen::datei_einspielen(b, &std::fs::read_to_string(&quelle)?, ueberschreiben)?;
                    ein += e;
                    ueber += u;
                }
                // Daten-Einträge → Ablage, Dokumente → Dateien
                Some((DATEN | ALT_DATEN, r)) => {
                    let ziel = daten.join(r);
                    let vorhanden = if crate::ablage::ist_daten(&ziel) { crate::ablage::existiert(&ziel) } else { ziel.exists() };
                    if vorhanden && !ueberschreiben {
                        ueber += 1;
                        continue;
                    }
                    if crate::ablage::ist_daten(&ziel) {
                        crate::ablage::schreiben(&ziel, std::fs::read(&quelle)?)?;
                    } else {
                        if let Some(p) = ziel.parent() {
                            std::fs::create_dir_all(p)?;
                        }
                        std::fs::copy(&quelle, &ziel).with_context(|| tr!("Schreiben: {a}", a = ziel.display()))?;
                    }
                    ein += 1;
                }
                // eigene Labels → Datenbank
                Some((LABELS | ALT_LABELS, _)) => match std::fs::read_to_string(&quelle).ok().map(|t| crate::db::labels::datei_einspielen(&t, ueberschreiben)) {
                    Some(Ok(true)) => ein += 1,
                    _ => ueber += 1,
                },
                _ => continue,
            }
        }
        Ok((ein, ueber))
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sichern_aufraeumen_zurueckspielen() {
        let _g = crate::history::TEST_LOCK.lock().unwrap();
        let wurzel = std::env::temp_dir().join(format!("vag-ds-{}", std::process::id()));
        let daten = wurzel.join("daten");
        let ziel = wurzel.join("stick");
        std::fs::create_dir_all(daten.join("fahrzeuge/TEST")).unwrap();
        std::fs::create_dir_all(&ziel).unwrap();
        std::env::set_var("VAG_DATA_DIR", &daten);
        std::env::set_var("XDG_CONFIG_HOME", wurzel.join("config"));
        crate::ablage::schreiben(daten.join("fahrzeuge/TEST/fahrzeug.toml"), "kennzeichen = \"T 1\"\n").unwrap();
        crate::fehlertexte::setzen("01627", "x").unwrap();
        // ein Dokument bleibt Datei und kommt mit
        std::fs::write(daten.join("fahrzeuge/TEST/bericht.pdf"), b"%PDF").unwrap();

        let e = Einstellung { ziel: ziel.display().to_string(), ..Default::default() };
        assert!(e.faellig(10 * 86_400));
        assert!(!Einstellung { zuletzt: 9 * 86_400, ..e.clone() }.faellig(10 * 86_400));
        assert!(Einstellung { ziel: "/gibt/es/nicht".into(), ..e.clone() }.ziel_fehlt());
        // nicht in den Datenordner selbst
        assert!(sichern(&daten, 3).is_err());

        let (zip, n) = sichern(&ziel, 2).unwrap();
        assert_eq!(n, 3, "Fahrzeug, Dokument, eigene Texte");
        assert!(laden().zuletzt > 0);
        // ältere werden aufgeräumt (behalten = 2)
        for i in 0..3 {
            std::fs::write(ziel.join(format!("{PREFIX}-2000-01-0{i}_000000.zip")), b"alt").unwrap();
        }
        // Sicherung unter dem alten Namen (vor der Umbenennung) zählt mit
        std::fs::write(ziel.join(format!("{ALT_PREFIX}-2001-01-01_000000.zip")), b"alt").unwrap();
        sichern(&ziel, 2).unwrap();
        // zwei bleiben, die neueste vorn (gleiche Sekunde = gleicher Name, ersetzt sich selbst)
        let l = liste(&ziel);
        assert_eq!(l.len(), 2);
        assert!(!l[0].to_string_lossy().contains("2000-01"));
        assert!(!ziel.join(format!("{PREFIX}-2000-01-00_000000.zip")).exists());
        // nach Datum sortiert, nicht nach Name: die alte Sicherung von 2001 ist
        // die zweitneueste und bleibt, die von 2000 sind weg
        assert!(l[1].to_string_lossy().contains(ALT_PREFIX));

        // zurückspielen: Fehlendes kommt wieder, Vorhandenes bleibt
        crate::fehlertexte::setzen("01627", "").unwrap();
        std::fs::remove_file(daten.join("fahrzeuge/TEST/bericht.pdf")).unwrap();
        crate::ablage::schreiben(daten.join("fahrzeuge/TEST/fahrzeug.toml"), "geaendert").unwrap();
        let zip = if zip.exists() { zip } else { liste(&ziel)[0].clone() };
        let (ein, ueber) = zurueckspielen(&zip, false).unwrap();
        assert!(ein >= 2 && ueber >= 1, "{ein} {ueber}");
        assert_eq!(crate::fehlertexte::alle().get("01627").map(String::as_str), Some("x"));
        assert!(daten.join("fahrzeuge/TEST/bericht.pdf").exists());
        assert_eq!(crate::ablage::lesen(daten.join("fahrzeuge/TEST/fahrzeug.toml")).unwrap(), "geaendert");
        zurueckspielen(&zip, true).unwrap();
        assert!(crate::ablage::lesen(daten.join("fahrzeuge/TEST/fahrzeug.toml")).unwrap().contains("T 1"));
        assert!(zurueckspielen(&ziel.join("gibtsnicht.zip"), false).is_err());

        // alte Sicherung (Ordnernamen vag-diag-…) lässt sich einspielen
        let altquelle = wurzel.join("altquelle");
        std::fs::create_dir_all(&altquelle).unwrap();
        std::fs::write(altquelle.join("wissen.toml"), "alt = 1\n").unwrap();
        let altzip = wurzel.join(format!("{ALT_PREFIX}-1999-01-01_000000.zip"));
        crate::zip::zip_dirs(&[(altquelle.as_path(), ALT_DATEN)], &altzip).unwrap();
        assert_eq!(zurueckspielen(&altzip, false).unwrap().0, 1);
        assert!(crate::ablage::existiert(daten.join("wissen.toml")), "landet in der Datenbank");

        std::env::remove_var("VAG_DATA_DIR");
        std::env::remove_var("XDG_CONFIG_HOME");
        let _ = std::fs::remove_dir_all(&wurzel);
    }
}
