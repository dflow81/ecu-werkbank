//! Ablage: alle Daten-Dateien des Datenordners liegen in der Fahrzeug-Datenbank
//! (Tabelle `ablage`, Schlüssel = Pfad relativ zum Datenordner).
//!
//! Die Funktionen nehmen dieselben Pfade wie früher `std::fs` — so ändert sich
//! in den Modulen nur der Aufruf. Pfade **außerhalb** des Datenordners (z. B.
//! eine Datei, die man mit `--datei` übergibt) gehen weiter direkt aufs
//! Dateisystem. Dokumente (PDF, CSV, Mitschnitte) bleiben ebenfalls Dateien.
//!
//! Umstieg: Beim ersten Zugriff werden alle vorhandenen Daten-Dateien
//! übernommen und nach `<Datenordner>/vor-datenbank/` verschoben (Sicherung).

use crate::db::eigen::mit_db;
use crate::db::Db;
use rusqlite::{params, OptionalExtension};
use std::io;
use std::path::{Path, PathBuf};

/// Ordner, in den die übernommenen Dateien verschoben werden.
pub const ALT_ORDNER: &str = "vor-datenbank";

/// Diese übernimmt schon [`crate::db::eigen`] (eigene Texte).
const EIGENE_TEXTE: [&str; 5] = ["fehlertexte.toml", "messwerttexte.toml", "codiertexte.toml", "kennungstexte.toml", "wertetexte.toml"];

/// Einstellungs-Dateien ohne Endung.
const OHNE_ENDUNG: [&str; 3] = ["aktives-fahrzeug", "erkennung-aus", "lbl-ordner"];

/// Gehört der Pfad in die Ablage? Gibt den Schlüssel zurück.
fn schluessel(p: &Path) -> Option<String> {
    let basis = crate::history::data_dir();
    let rel = p.strip_prefix(&basis).ok()?;
    let name = rel.file_name()?.to_str()?;
    let teile: Vec<&str> = rel.iter().filter_map(|c| c.to_str()).collect();
    // Sprache und eigene Sprachdateien bleiben Dateien: Übersetzungen werden überall
    // gebraucht, auch mitten in einem Datenbankzugriff und bevor es eine Datenbank gibt.
    if teile.first() == Some(&ALT_ORDNER) || teile.first() == Some(&"sprachen") || name == "sprache" {
        return None;
    }
    let passt = name.ends_with(".toml") && !(teile.len() == 1 && EIGENE_TEXTE.contains(&name)) || (teile.len() == 1 && OHNE_ENDUNG.contains(&name));
    passt.then(|| teile.join("/"))
}

fn fehler(e: anyhow::Error) -> io::Error {
    io::Error::other(format!("{e:#}"))
}

/// Alle passenden Dateien einmal übernehmen und nach `vor-datenbank/` verschieben.
fn uebernehmen(db: &Db) -> anyhow::Result<()> {
    const MARKE: &str = "ablage";
    let schon: i64 = db.con.query_row("SELECT COUNT(*) FROM uebernahme WHERE datei = ?1", params![MARKE], |r| r.get(0))?;
    if schon > 0 {
        return Ok(());
    }
    let basis = crate::history::data_dir();
    let mut dateien = Vec::new();
    let mut stapel = vec![basis.clone()];
    while let Some(d) = stapel.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                if p.file_name().is_some_and(|n| n != ALT_ORDNER) {
                    stapel.push(p);
                }
            } else if let Some(k) = schluessel(&p) {
                dateien.push((p, k));
            }
        }
    }
    let jetzt = crate::report::now_local();
    let tx = db.con.unchecked_transaction()?;
    for (p, k) in &dateien {
        if let Ok(t) = std::fs::read_to_string(p) {
            tx.execute("INSERT OR IGNORE INTO ablage (pfad, inhalt, geaendert) VALUES (?1, ?2, ?3)", params![k, t, jetzt])?;
        }
    }
    tx.execute("INSERT INTO uebernahme (datei, zeitpunkt) VALUES (?1, ?2)", params![MARKE, jetzt])?;
    tx.commit()?;
    // erst nach dem Festschreiben verschieben — so geht nie etwas verloren
    for (p, k) in dateien {
        let ziel = basis.join(ALT_ORDNER).join(&k);
        if let Some(d) = ziel.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::rename(&p, &ziel);
    }
    Ok(())
}

fn mit_ablage<T>(f: impl FnOnce(&Db) -> anyhow::Result<T>) -> io::Result<T> {
    mit_db(|db| {
        uebernehmen(db)?;
        f(db)
    })
    .map_err(fehler)
}

/// Wie `std::fs::read_to_string`.
pub fn lesen(p: impl AsRef<Path>) -> io::Result<String> {
    let p = p.as_ref();
    let Some(k) = schluessel(p) else { return std::fs::read_to_string(p) };
    mit_ablage::<Option<String>>(|db| Ok(db.con.query_row("SELECT inhalt FROM ablage WHERE pfad = ?1", params![k], |r| r.get::<_, String>(0)).optional()?))?
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("{} nicht in der Ablage", p.display())))
}

/// Wie `std::fs::write` (Ordner werden bei Bedarf angelegt).
pub fn schreiben(p: impl AsRef<Path>, inhalt: impl AsRef<[u8]>) -> io::Result<()> {
    let p = p.as_ref();
    let Some(k) = schluessel(p) else {
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d)?;
        }
        return std::fs::write(p, inhalt);
    };
    let text = String::from_utf8_lossy(inhalt.as_ref()).to_string();
    mit_ablage(|db| {
        db.con.execute(
            "INSERT INTO ablage (pfad, inhalt, geaendert) VALUES (?1, ?2, ?3) ON CONFLICT(pfad) DO UPDATE SET inhalt = excluded.inhalt, geaendert = excluded.geaendert",
            params![k, text, crate::report::now_local()],
        )?;
        Ok(())
    })
}

/// Gibt es den Eintrag (bzw. die Datei)?
pub fn existiert(p: impl AsRef<Path>) -> bool {
    let p = p.as_ref();
    match schluessel(p) {
        Some(_) => lesen(p).is_ok(),
        None => p.exists(),
    }
}

/// Wie `std::fs::remove_file`.
pub fn entfernen(p: impl AsRef<Path>) -> io::Result<()> {
    let p = p.as_ref();
    let Some(k) = schluessel(p) else { return std::fs::remove_file(p) };
    mit_ablage(|db| Ok(db.con.execute("DELETE FROM ablage WHERE pfad = ?1", params![k])?)).map(|_| ())
}

/// Daten-Einträge direkt in einem Ordner (ohne Unterordner), als volle Pfade,
/// sortiert. Außerhalb des Datenordners: die Dateien des Ordners.
pub fn liste(ordner: &Path) -> Vec<PathBuf> {
    let basis = crate::history::data_dir();
    let Ok(rel) = ordner.strip_prefix(&basis) else {
        let mut v: Vec<PathBuf> = std::fs::read_dir(ordner).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_file()).collect();
        v.sort();
        return v;
    };
    let praefix = rel.iter().filter_map(|c| c.to_str()).collect::<Vec<_>>().join("/");
    let praefix = if praefix.is_empty() { String::new() } else { format!("{praefix}/") };
    let mut v: Vec<PathBuf> = mit_ablage(|db| {
        let mut st = db.con.prepare("SELECT pfad FROM ablage WHERE pfad LIKE ?1 ESCAPE '\\'")?;
        let muster = format!("{}%", praefix.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
        let v = st
            .query_map(params![muster], |r| r.get::<_, String>(0))?
            .filter_map(|k| k.ok())
            .filter(|k| !k[praefix.len()..].contains('/'))
            .map(|k| basis.join(k))
            .collect();
        Ok(v)
    })
    .unwrap_or_default();
    v.sort();
    v
}

/// Unterordner, die Einträge enthalten (z. B. alle Fahrzeuge unter `fahrzeuge/`).
pub fn unterordner(ordner: &Path) -> Vec<PathBuf> {
    let basis = crate::history::data_dir();
    let Ok(rel) = ordner.strip_prefix(&basis) else { return Vec::new() };
    let praefix = rel.iter().filter_map(|c| c.to_str()).collect::<Vec<_>>().join("/");
    let praefix = if praefix.is_empty() { String::new() } else { format!("{praefix}/") };
    let mut namen: Vec<String> = mit_ablage(|db| {
        let mut st = db.con.prepare("SELECT pfad FROM ablage")?;
        let v = st.query_map([], |r| r.get::<_, String>(0))?.filter_map(|k| k.ok()).collect::<Vec<_>>();
        Ok(v)
    })
    .unwrap_or_default()
    .into_iter()
    .filter_map(|k| k.strip_prefix(&praefix).and_then(|r| r.split_once('/')).map(|(o, _)| o.to_string()))
    .collect();
    namen.sort();
    namen.dedup();
    namen.into_iter().map(|n| ordner.join(n)).collect()
}

/// Ordner umbenennen: Einträge in der Ablage und echte Dateien (PDFs …) gleich mit.
pub fn ordner_umbenennen(von: &Path, nach: &Path) -> io::Result<()> {
    let basis = crate::history::data_dir();
    if let (Ok(a), Ok(b)) = (von.strip_prefix(&basis), nach.strip_prefix(&basis)) {
        let a = format!("{}/", a.iter().filter_map(|c| c.to_str()).collect::<Vec<_>>().join("/"));
        let b = format!("{}/", b.iter().filter_map(|c| c.to_str()).collect::<Vec<_>>().join("/"));
        mit_ablage(|db| {
            let tx = db.con.unchecked_transaction()?;
            let alte: Vec<String> = {
                let mut st = tx.prepare("SELECT pfad FROM ablage")?;
                let v = st.query_map([], |r| r.get::<_, String>(0))?.filter_map(|k| k.ok()).filter(|k| k.starts_with(&a)).collect();
                v
            };
            for k in alte {
                tx.execute("UPDATE ablage SET pfad = ?1 WHERE pfad = ?2", params![format!("{b}{}", &k[a.len()..]), k])?;
            }
            tx.commit()?;
            Ok(())
        })?;
    }
    if von.exists() {
        if let Some(d) = nach.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::rename(von, nach)?;
    }
    Ok(())
}

/// Einträge unter `ordner` als echte Dateien nach `ziel` schreiben (für ZIP-Exporte).
/// Gibt die Zahl geschriebener Dateien zurück.
pub fn ausgeben(ordner: &Path, ziel: &Path) -> io::Result<usize> {
    let basis = crate::history::data_dir();
    let Ok(rel) = ordner.strip_prefix(&basis) else { return Ok(0) };
    let praefix = rel.iter().filter_map(|c| c.to_str()).collect::<Vec<_>>().join("/");
    let praefix = if praefix.is_empty() { String::new() } else { format!("{praefix}/") };
    let eintraege: Vec<(String, String)> = mit_ablage(|db| {
        let mut st = db.con.prepare("SELECT pfad, inhalt FROM ablage")?;
        let v = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?.filter_map(|k| k.ok()).filter(|(k, _)| k.starts_with(&praefix)).collect();
        Ok(v)
    })?;
    for (k, inhalt) in &eintraege {
        let p = ziel.join(&k[praefix.len()..]);
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::write(p, inhalt)?;
    }
    Ok(eintraege.len())
}

/// Dateien aus einem Ordner (z. B. frisch entpackte Sicherung) in die Ablage holen.
/// `ueberschreiben` = false: vorhandene Einträge bleiben. Gibt die Zahl übernommener zurück.
pub fn einlesen(ordner: &Path, ueberschreiben: bool) -> io::Result<usize> {
    let basis = crate::history::data_dir();
    let mut n = 0;
    let mut stapel = vec![ordner.to_path_buf()];
    while let Some(d) = stapel.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stapel.push(p);
                continue;
            }
            let Ok(rel) = p.strip_prefix(ordner) else { continue };
            let ziel = basis.join(rel);
            if schluessel(&ziel).is_none() {
                continue;
            }
            if !ueberschreiben && existiert(&ziel) {
                continue;
            }
            if let Ok(t) = std::fs::read_to_string(&p) {
                schreiben(&ziel, t)?;
                n += 1;
            }
        }
    }
    Ok(n)
}

/// Ist das ein Daten-Eintrag (liegt in der Datenbank statt als Datei)?
pub fn ist_daten(p: impl AsRef<Path>) -> bool {
    schluessel(p.as_ref()).is_some()
}

/// Ordner samt seinen Daten-Einträgen als echte Dateien nach `ziel` kopieren
/// (Dokumente wie PDF/CSV + Einträge aus der Datenbank) — für ZIP-Exporte.
pub fn als_ordner(ordner: &Path, ziel: &Path) -> io::Result<usize> {
    let mut n = 0;
    let mut stapel = vec![ordner.to_path_buf()];
    while let Some(d) = stapel.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let name = p.file_name().and_then(|x| x.to_str()).unwrap_or("");
            if p.is_dir() {
                if name != ALT_ORDNER {
                    stapel.push(p);
                }
                continue;
            }
            if name.starts_with("fahrzeugdaten.sqlite") || name.ends_with(".uebernommen") || ist_daten(&p) {
                continue;
            }
            let Ok(rel) = p.strip_prefix(ordner) else { continue };
            let z = ziel.join(rel);
            if let Some(d) = z.parent() {
                std::fs::create_dir_all(d)?;
            }
            std::fs::copy(&p, &z)?;
            n += 1;
        }
    }
    Ok(n + ausgeben(ordner, ziel)?)
}

/// Ordner (mit Daten-Einträgen) als ZIP schreiben; `praefix` = Ordner im ZIP.
pub fn ordner_zippen(ordner: &Path, out: &Path, praefix: &str) -> anyhow::Result<usize> {
    let stufe = std::env::temp_dir().join(format!("ecu-werkbank-zip-{}-{}", std::process::id(), crate::report::unix_now()));
    let _ = std::fs::remove_dir_all(&stufe);
    std::fs::create_dir_all(&stufe)?;
    let r = als_ordner(ordner, &stufe).map_err(anyhow::Error::from).and_then(|_| crate::zip::zip_dir(&stufe, out, praefix));
    let _ = std::fs::remove_dir_all(&stufe);
    r
}

/// Dateien aus `quelle` (z. B. entpackte Sicherung) nach `ziel_basis` einspielen:
/// Daten-Einträge in die Datenbank, Dokumente als Dateien. Vorhandenes bleibt,
/// außer `ueberschreiben`. Gibt (eingespielt, übersprungen, relative Pfade) zurück.
pub fn einspielen(quelle: &Path, ziel_basis: &Path, ueberschreiben: bool) -> io::Result<(usize, usize, Vec<String>)> {
    let (mut ein, mut ueber, mut pfade) = (0, 0, Vec::new());
    let mut stapel = vec![quelle.to_path_buf()];
    while let Some(d) = stapel.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stapel.push(p);
                continue;
            }
            let Ok(rel) = p.strip_prefix(quelle) else { continue };
            let ziel = ziel_basis.join(rel);
            let vorhanden = if ist_daten(&ziel) { existiert(&ziel) } else { ziel.exists() };
            if vorhanden && !ueberschreiben {
                ueber += 1;
                continue;
            }
            if ist_daten(&ziel) {
                schreiben(&ziel, std::fs::read(&p)?)?;
            } else {
                if let Some(d) = ziel.parent() {
                    std::fs::create_dir_all(d)?;
                }
                std::fs::copy(&p, &ziel)?;
            }
            ein += 1;
            pfade.push(rel.iter().filter_map(|c| c.to_str()).collect::<Vec<_>>().join("/"));
        }
    }
    Ok((ein, ueber, pfade))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schluessel_nur_fuer_daten_im_datenordner() {
        // andere Tests stellen den Datenordner um
        let _g = crate::history::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let b = crate::history::data_dir();
        assert_eq!(schluessel(&b.join("fahrzeuge/X/historie/1.toml")).as_deref(), Some("fahrzeuge/X/historie/1.toml"));
        assert_eq!(schluessel(&b.join("aktives-fahrzeug")).as_deref(), Some("aktives-fahrzeug"));
        assert!(schluessel(&b.join("sprache")).is_none(), "Sprache bleibt Datei");
        assert!(schluessel(&b.join("sprachen/en.toml")).is_none());
        assert!(schluessel(&b.join("fahrzeuge/X/berichte/a.pdf")).is_none(), "Dokumente bleiben Dateien");
        assert!(schluessel(&b.join("fehlertexte.toml")).is_none(), "macht db::eigen");
        assert!(schluessel(&b.join("vor-datenbank/fahrzeuge/X/fahrzeug.toml")).is_none());
        assert!(schluessel(Path::new("/irgendwo/datei.toml")).is_none(), "außerhalb: Dateisystem");
    }
}
