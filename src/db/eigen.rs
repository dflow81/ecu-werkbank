//! Eigene Texte in der Fahrzeug-Datenbank (Tabelle `eigener_text`):
//! Fehlertexte, Namen für Messwerte, Codier-Bits, Anpassungs-Kennungen und
//! Messwert-Texte. Früher je eine TOML-Datei im Datenordner — die wird beim
//! ersten Zugriff einmal übernommen und in `<name>.toml.uebernommen`
//! umbenannt (bleibt als Sicherung liegen).
//!
//! Eine gemeinsame Verbindung je Datenbank-Datei; wechselt der Datenordner
//! (Tests), wird neu geöffnet.

use super::{standard_pfad, Db};
use anyhow::{Context, Result};
use rusqlite::params;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

static VERBINDUNG: Mutex<Option<(PathBuf, Db)>> = Mutex::new(None);

/// `f` mit der Datenbank des aktuellen Datenordners ausführen.
pub fn mit_db<T>(f: impl FnOnce(&Db) -> Result<T>) -> Result<T> {
    let pfad = standard_pfad();
    let mut g = VERBINDUNG.lock().unwrap_or_else(|e| e.into_inner());
    if g.as_ref().map(|(p, _)| p != &pfad).unwrap_or(true) {
        let db = Db::oeffnen(&pfad)?;
        db.con.busy_timeout(std::time::Duration::from_secs(5))?;
        *g = Some((pfad, db));
    }
    let (_, db) = g.as_ref().expect("gerade geöffnet");
    f(db)
}

/// Zu jedem Bereich die frühere Datei im Datenordner.
fn alte_datei(bereich: &str) -> Option<&'static str> {
    Some(match bereich {
        "fehler" => "fehlertexte.toml",
        "messwert" => "messwerttexte.toml",
        "codier" => "codiertexte.toml",
        "kennung" => "kennungstexte.toml",
        "wert" => "wertetexte.toml",
        _ => return None,
    })
}

/// Alte TOML-Datei eines Bereichs einmal übernehmen (falls vorhanden).
fn uebernehmen(db: &Db, bereich: &str) -> Result<()> {
    let Some(name) = alte_datei(bereich) else { return Ok(()) };
    let p = crate::history::data_dir().join(name);
    let schon: bool = db.con.query_row("SELECT COUNT(*) FROM uebernahme WHERE datei = ?1", params![name], |r| r.get::<_, i64>(0))? > 0;
    if schon || !p.exists() {
        return Ok(());
    }
    let text = std::fs::read_to_string(&p).with_context(|| crate::tr!("{a} lesen", a = p.display()))?;
    let m: BTreeMap<String, String> = toml::from_str(&text).with_context(|| crate::tr!("{a} lesen", a = p.display()))?;
    let jetzt = crate::report::now_local();
    let tx = db.con.unchecked_transaction()?;
    for (k, v) in &m {
        tx.execute("INSERT OR REPLACE INTO eigener_text (bereich, schluessel, text, geaendert) VALUES (?1, ?2, ?3, ?4)", params![bereich, k, v, jetzt])?;
    }
    tx.execute("INSERT INTO uebernahme (datei, zeitpunkt) VALUES (?1, ?2)", params![name, jetzt])?;
    tx.commit()?;
    let _ = std::fs::rename(&p, p.with_extension("toml.uebernommen"));
    Ok(())
}

/// Alle eigenen Texte eines Bereichs.
pub fn alle(bereich: &str) -> BTreeMap<String, String> {
    mit_db(|db| {
        uebernehmen(db, bereich)?;
        let mut st = db.con.prepare("SELECT schluessel, text FROM eigener_text WHERE bereich = ?1")?;
        let m = st.query_map(params![bereich], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<BTreeMap<String, String>>>()?;
        Ok(m)
    })
    .unwrap_or_default()
}

/// Mehrere Texte setzen; leerer Text löscht den Eintrag.
pub fn setzen(bereich: &str, liste: &[(String, String)]) -> Result<()> {
    if liste.is_empty() {
        return Ok(());
    }
    mit_db(|db| {
        uebernehmen(db, bereich)?;
        let jetzt = crate::report::now_local();
        let tx = db.con.unchecked_transaction()?;
        for (k, t) in liste {
            if t.trim().is_empty() {
                tx.execute("DELETE FROM eigener_text WHERE bereich = ?1 AND schluessel = ?2", params![bereich, k])?;
            } else {
                tx.execute("INSERT OR REPLACE INTO eigener_text (bereich, schluessel, text, geaendert) VALUES (?1, ?2, ?3, ?4)", params![bereich, k, t.trim(), jetzt])?;
            }
        }
        tx.commit()?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setzen_loeschen_und_alte_datei_uebernehmen() {
        let db = Db::im_speicher().unwrap();
        // ohne Datenordner-Datei: nichts zu übernehmen
        uebernehmen(&db, "gibtsnicht").unwrap();
        db.con.execute("INSERT INTO eigener_text (bereich, schluessel, text) VALUES ('fehler', 'P0301', 'alt')", []).unwrap();
        let n: i64 = db.con.query_row("SELECT COUNT(*) FROM eigener_text", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
    }
}
