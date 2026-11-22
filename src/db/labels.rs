//! Eigene Labels in der Fahrzeug-Datenbank (Weg A): Jede frühere
//! Label-Datei ist ein Steuergerät der Quelle „Eigene Labels“ mit seinen
//! Fehlertexten, Messwerten, Anpassungen, Codier-Bits, Sollwerten und
//! Stellfunktionen. Der Label-Ordner wird einmal übernommen; danach liest
//! und schreibt ECUWerkbank nur noch hier. Die Dateien bleiben unverändert
//! liegen (Sicherung), werden aber nicht mehr gelesen.

use super::{eigen::mit_db, Db, QuellArt};
use crate::labels::{Adaptation, Coding, DtcLabel, EcuHeader, LabelFile, Measuring, Sollwert};
use anyhow::Result;
use rusqlite::{params, OptionalExtension};
use std::path::Path;

/// Name der Quelle, zu der alle eigenen Labels gehören.
pub const QUELLE: &str = "Eigene Labels";

/// id der Quelle „Eigene Labels“ (wird bei Bedarf angelegt).
fn quelle(db: &Db) -> Result<i64> {
    if let Some(id) = db
        .con
        .query_row("SELECT id FROM data_source WHERE source_type = 'USER' AND source_name = ?1 ORDER BY id LIMIT 1", params![QUELLE], |r| r.get(0))
        .optional()?
    {
        return Ok(id);
    }
    db.quelle_anlegen(QuellArt::User, QUELLE, "", "", crate::trs!("eigene Label-Einträge"))
}

impl Db {
    /// Alle eigenen Labels: (id, Inhalt wie eine Label-Datei).
    pub fn eigene_labels(&self) -> Result<Vec<(i64, LabelFile)>> {
        let q = quelle(self)?;
        let mut st = self.con.prepare("SELECT id, diagnostic_address, ecu_name, software_number, can_tx, can_rx, coding_did FROM ecu WHERE source_id = ?1 ORDER BY diagnostic_address, id")?;
        let koepfe: Vec<(i64, EcuHeader)> = st
            .query_map(params![q], |r| {
                let teil: String = r.get(3)?;
                Ok((
                    r.get(0)?,
                    EcuHeader { address: r.get(1)?, name: r.get(2)?, part: (!teil.is_empty()).then_some(teil), tx: r.get(4)?, rx: r.get(5)?, coding_did: r.get(6)? },
                ))
            })?
            .collect::<rusqlite::Result<_>>()?;
        let mut out = Vec::new();
        for (id, ecu) in koepfe {
            let liste = |sql: &str| -> Result<rusqlite::Statement> { Ok(self.con.prepare(sql)?) };
            let dtc = liste("SELECT dtc_code, title FROM dtc WHERE ecu_id = ?1 ORDER BY id")?
                .query_map(params![id], |r| Ok(DtcLabel { code: r.get(0)?, description: r.get(1)? }))?
                .collect::<rusqlite::Result<_>>()?;
            // Messwerte: aus Label-Dateien übernommen (measurement) und im Datenbank-Editor gespeichert (did)
            let mut measuring: Vec<Measuring> = liste("SELECT pid, name, unit, factor, \"offset\" FROM measurement WHERE ecu_id = ?1 ORDER BY id")?
                .query_map(params![id], |r| Ok(Measuring { id: r.get(0)?, name: r.get(1)?, unit: r.get(2)?, factor: r.get(3)?, offset: r.get(4)? }))?
                .collect::<rusqlite::Result<_>>()?;
            let aus_did: Vec<Measuring> = liste("SELECT did, name, unit, factor, \"offset\" FROM did WHERE ecu_id = ?1 ORDER BY id")?
                .query_map(params![id], |r| Ok(Measuring { id: r.get(0)?, name: r.get(1)?, unit: r.get(2)?, factor: r.get(3)?, offset: r.get(4)? }))?
                .collect::<rusqlite::Result<_>>()?;
            measuring.extend(aus_did);
            let adaptation = liste("SELECT channel, name FROM adaptation WHERE ecu_id = ?1 ORDER BY id")?
                .query_map(params![id], |r| Ok(Adaptation { channel: r.get(0)?, name: r.get(1)? }))?
                .collect::<rusqlite::Result<_>>()?;
            let teile = |s: String| s.split(',').map(str::trim).filter(|t| !t.is_empty()).map(str::to_string).collect::<Vec<_>>();
            let coding = liste("SELECT byte_index, bit_index, label, requires, excludes FROM coding_item WHERE ecu_id = ?1 ORDER BY id")?
                .query_map(params![id], |r| Ok(Coding { byte: r.get(0)?, bit: r.get(1)?, name: r.get(2)?, benoetigt: teile(r.get(3)?), schliesst_aus: teile(r.get(4)?) }))?
                .collect::<rusqlite::Result<_>>()?;
            let sollwert = liste("SELECT quelle, kennung, feld, min, max, name FROM sollwert WHERE ecu_id = ?1 ORDER BY id")?
                .query_map(params![id], |r| Ok(Sollwert { quelle: r.get(0)?, id: r.get(1)?, feld: r.get(2)?, min: r.get(3)?, max: r.get(4)?, name: r.get(5)? }))?
                .collect::<rusqlite::Result<_>>()?;
            let stell: Vec<String> = liste("SELECT inhalt FROM stellfunktion WHERE ecu_id = ?1 ORDER BY id")?.query_map(params![id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
            let stellfunktion = stell.iter().filter_map(|t| toml::from_str(t).ok()).collect();
            out.push((id, LabelFile { ecu, dtc, measuring, adaptation, coding, sollwert, stellfunktion }));
        }
        Ok(out)
    }

    /// Ein eigenes Label speichern. `id` = bestehender Eintrag (wird ersetzt),
    /// None = neu. Gibt die (neue) id zurück.
    pub fn label_speichern(&self, id: Option<i64>, lf: &LabelFile) -> Result<i64> {
        let q = quelle(self)?;
        let tx = self.con.unchecked_transaction()?;
        if let Some(id) = id {
            tx.execute("DELETE FROM ecu WHERE id = ?1 AND source_id = ?2", params![id, q])?;
        }
        let vw = self.hersteller("Volkswagen", &["WVW", "WV1", "WV2", "WV3"], None)?;
        let e = &lf.ecu;
        tx.execute(
            "INSERT INTO ecu (manufacturer_id, ecu_name, software_number, protocol, diagnostic_address, can_tx, can_rx, coding_did, source_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![vw, e.name, e.part.clone().unwrap_or_default(), if e.tx.is_some() { "UDS" } else { "" }, e.address, e.tx, e.rx, e.coding_did, q],
        )?;
        let neu = tx.last_insert_rowid();
        for d in &lf.dtc {
            tx.execute("INSERT INTO dtc (ecu_id, dtc_code, title, source_id) VALUES (?1, ?2, ?3, ?4)", params![neu, d.code.trim().to_ascii_uppercase(), d.description, q])?;
        }
        for m in &lf.measuring {
            tx.execute(
                "INSERT INTO measurement (ecu_id, kind, pid, name, unit, factor, \"offset\", source_id) VALUES (?1, 'UDS', ?2, ?3, ?4, ?5, ?6, ?7)",
                params![neu, m.id, m.name, m.unit, m.factor, m.offset, q],
            )?;
        }
        for a in &lf.adaptation {
            tx.execute("INSERT INTO adaptation (ecu_id, channel, name, source_id) VALUES (?1, ?2, ?3, ?4)", params![neu, a.channel, a.name, q])?;
        }
        for c in &lf.coding {
            tx.execute(
                "INSERT INTO coding_item (ecu_id, byte_index, bit_index, label, requires, excludes, source_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![neu, c.byte, c.bit, c.name, c.benoetigt.join(","), c.schliesst_aus.join(","), q],
            )?;
        }
        for s in &lf.sollwert {
            tx.execute(
                "INSERT INTO sollwert (ecu_id, quelle, kennung, feld, min, max, name, source_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![neu, s.quelle, s.id, s.feld, s.min, s.max, s.name, q],
            )?;
        }
        for f in &lf.stellfunktion {
            tx.execute("INSERT INTO stellfunktion (ecu_id, inhalt, source_id) VALUES (?1, ?2, ?3)", params![neu, toml::to_string(f)?, q])?;
        }
        tx.commit()?;
        Ok(neu)
    }

    /// Sollwerte und Stellfunktionen aller ANDEREN Quellen (Datenpakete, im
    /// Datenbank-Editor eingetragen) — die eigenen Labels liefert [`Db::eigene_labels`].
    pub fn sollwerte_und_stellfunktionen_fremd(&self) -> Result<(Vec<(Option<u16>, Sollwert)>, Vec<crate::control::Stellfunktion>)> {
        let q = quelle(self)?;
        let mut st = self.con.prepare(
            "SELECT e.diagnostic_address, s.quelle, s.kennung, s.feld, s.min, s.max, s.name FROM sollwert s LEFT JOIN ecu e ON e.id = s.ecu_id WHERE s.source_id <> ?1 ORDER BY s.source_id, s.id",
        )?;
        let soll = st
            .query_map(params![q], |r| Ok((r.get(0)?, Sollwert { quelle: r.get(1)?, id: r.get(2)?, feld: r.get(3)?, min: r.get(4)?, max: r.get(5)?, name: r.get(6)? })))?
            .collect::<rusqlite::Result<_>>()?;
        let mut st = self.con.prepare("SELECT inhalt FROM stellfunktion WHERE source_id <> ?1 ORDER BY source_id, id")?;
        let texte: Vec<String> = st.query_map(params![q], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        Ok((soll, texte.iter().filter_map(|t| toml::from_str(t).ok()).collect()))
    }

    /// Ein eigenes Label entfernen.
    pub fn label_entfernen(&self, id: i64) -> Result<bool> {
        let q = quelle(self)?;
        Ok(self.con.execute("DELETE FROM ecu WHERE id = ?1 AND source_id = ?2", params![id, q])? > 0)
    }

    /// Label-Ordner einmal übernehmen. Bei gleicher Adresse gewinnt, was schon
    /// in der Datenbank steht. Gibt die Zahl übernommener Dateien zurück.
    pub fn label_ordner_uebernehmen(&self, ordner: &Path) -> Result<usize> {
        let schluessel = format!("labels:{}", std::fs::canonicalize(ordner).unwrap_or_else(|_| ordner.to_path_buf()).display());
        let schon: i64 = self.con.query_row("SELECT COUNT(*) FROM uebernahme WHERE datei = ?1", params![schluessel], |r| r.get(0))?;
        if schon > 0 || !ordner.is_dir() {
            return Ok(0);
        }
        let vorhanden: Vec<Option<u16>> = self.eigene_labels()?.into_iter().map(|(_, l)| l.ecu.address).collect();
        let mut n = 0;
        for (pfad, lf) in crate::labels::dateien_lesen_mit_pfad(ordner) {
            // die mitgelieferte Beispieldatei ist kein echtes Steuergerät
            if pfad.file_name().is_some_and(|n| n == "beispiel.toml") {
                continue;
            }
            if lf.ecu.address.is_some() && vorhanden.contains(&lf.ecu.address) {
                continue;
            }
            self.label_speichern(None, &lf)?;
            n += 1;
        }
        self.con.execute("INSERT INTO uebernahme (datei, zeitpunkt) VALUES (?1, ?2)", params![schluessel, crate::report::now_local()])?;
        Ok(n)
    }
}

/// Label-Ordner übernehmen (einmal) und alle eigenen Labels lesen.
pub fn laden(ordner: &Path) -> Result<Vec<LabelFile>> {
    mit_db(|db| {
        db.label_ordner_uebernehmen(ordner)?;
        Ok(db.eigene_labels()?.into_iter().map(|(_, l)| l).collect())
    })
}

/// Alle eigenen Labels als Label-Dateien nach `ordner` schreiben (für Sicherungen).
pub fn ausgeben(ordner: &Path) -> Result<usize> {
    let l = mit_db(|db| db.eigene_labels())?;
    if l.is_empty() {
        return Ok(0);
    }
    std::fs::create_dir_all(ordner)?;
    for (id, lf) in &l {
        let name = format!("{}-{id}.toml", crate::labels::dateiname(lf.ecu.address.unwrap_or(0), &lf.ecu.name));
        std::fs::write(ordner.join(name), lf.to_toml())?;
    }
    Ok(l.len())
}

/// Eine Label-Datei (z. B. aus einer Sicherung) einspielen. Gibt es schon ein
/// eigenes Label mit derselben Adresse, bleibt es — außer `ueberschreiben`.
/// Gibt true zurück, wenn eingespielt wurde.
pub fn datei_einspielen(text: &str, ueberschreiben: bool) -> Result<bool> {
    let lf: LabelFile = toml::from_str(text)?;
    mit_db(|db| {
        let gleich = db.eigene_labels()?.into_iter().find(|(_, l)| l.ecu.address.is_some() && l.ecu.address == lf.ecu.address).map(|(id, _)| id);
        match (gleich, ueberschreiben) {
            (Some(_), false) => Ok(false),
            (id, _) => {
                db.label_speichern(id, &lf)?;
                Ok(true)
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn beispiel() -> LabelFile {
        toml::from_str(
            r#"
[ecu]
address = 0x09
name = "Bordnetz"
tx = 0x70E
rx = 0x778
coding_did = 0x0600
part = "5K0937086"

[[dtc]]
code = "b1234"
description = "Eigener Text"

[[measuring]]
id = 0xF40C
name = "Drehzahl"
unit = "1/min"
factor = 0.25

[[adaptation]]
channel = 23
name = "Komfortblinken"

[[coding]]
byte = 0
bit = 1
name = "Zusatz"
benoetigt = ["0.0"]

[[sollwert]]
quelle = "kwp"
id = 4
feld = 2
min = 80.0
max = 105.0
name = "Kühlmittel"

[[stellfunktion]]
name = "Leerlauf"
art = "uds-io"
tx = 0x7E0
rx = 0x7E8
id = 0x1234
min = 800.0
max = 1500.0
"#,
        )
        .unwrap()
    }

    #[test]
    fn speichern_laden_ersetzen_entfernen() {
        let db = Db::im_speicher().unwrap();
        let id = db.label_speichern(None, &beispiel()).unwrap();
        let l = db.eigene_labels().unwrap();
        assert_eq!(l.len(), 1);
        let (gid, lf) = &l[0];
        assert_eq!(*gid, id);
        assert_eq!((lf.ecu.address, lf.ecu.tx, lf.ecu.coding_did, lf.ecu.part.as_deref()), (Some(9), Some(0x70E), Some(0x0600), Some("5K0937086")));
        assert_eq!(lf.dtc[0].code, "B1234");
        assert_eq!(lf.measuring[0].factor, 0.25);
        assert_eq!(lf.adaptation[0].channel, 23);
        assert_eq!(lf.coding[0].benoetigt, vec!["0.0"]);
        assert_eq!(lf.sollwert[0].max, Some(105.0));
        assert_eq!(lf.stellfunktion[0].id, 0x1234);
        // ersetzen
        let mut neu = lf.clone();
        neu.ecu.name = "Bordnetz neu".into();
        neu.dtc.clear();
        let id2 = db.label_speichern(Some(id), &neu).unwrap();
        let l = db.eigene_labels().unwrap();
        assert_eq!(l.len(), 1);
        assert_eq!((l[0].0, l[0].1.ecu.name.as_str(), l[0].1.dtc.len()), (id2, "Bordnetz neu", 0));
        assert!(db.label_entfernen(id2).unwrap());
        assert!(db.eigene_labels().unwrap().is_empty());
    }

    #[test]
    fn ordner_wird_einmal_uebernommen() {
        let dir = std::env::temp_dir().join(format!("ecuwb-lblord-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("09.toml"), "[ecu]\naddress = 0x09\nname = \"Datei\"\n").unwrap();
        std::fs::write(dir.join("kaputt.toml"), "das ist kein toml [").unwrap();
        std::fs::write(dir.join("beispiel.toml"), "[ecu]\naddress = 0x01\nname = \"Beispiel\"\n").unwrap();
        let db = Db::im_speicher().unwrap();
        assert_eq!(db.label_ordner_uebernehmen(&dir).unwrap(), 1);
        // zweiter Aufruf: nichts mehr; Änderung an der Datei wird nicht mehr gelesen
        std::fs::write(dir.join("17.toml"), "[ecu]\naddress = 0x17\nname = \"später\"\n").unwrap();
        assert_eq!(db.label_ordner_uebernehmen(&dir).unwrap(), 0);
        assert_eq!(db.eigene_labels().unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
