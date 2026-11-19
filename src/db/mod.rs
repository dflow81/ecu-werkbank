//! Fahrzeug-Datenbank (SQLite): Hersteller, Steuergeräte, Kennungen, Fehler,
//! Codierung, Messwerte, CAN-Signale — getrennt vom Programmkern.
//!
//! Plan und Begründungen: ROADMAP.md, „Plan: Fahrzeug-Datenbank“.
//!
//! - Jede Zeile gehört zu einer Quelle (`data_source`); eine Quelle lässt sich
//!   als Ganzes entfernen.
//! - Schema-Änderungen nur über nummerierte Migrationen (`schema_NNN.sql`);
//!   vor jeder Migration einer bestehenden Datei wird eine Kopie angelegt.
//! - ODX/PDX-Daten (selbst gekauft) dürfen die Datenbank nie verlassen:
//!   [`darf_weitergeben`].

use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};

pub mod api;
pub mod eigen;
pub mod labels;
pub mod odx;
pub mod paket;

/// Migrationen in Reihenfolge; Index + 1 = Schema-Version.
const MIGRATIONEN: &[&str] = &[include_str!("schema_001.sql"), include_str!("schema_002.sql"), include_str!("schema_003.sql"), include_str!("schema_004.sql")];

/// Aktuelle Schema-Version dieses Programms.
pub const SCHEMA_VERSION: u32 = MIGRATIONEN.len() as u32;

/// Art einer Datenquelle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuellArt {
    /// offene Community-Daten (frei lizenziert, nachprüfbar)
    Community,
    /// ASAM ODX, selbst gekauft — bleibt lokal
    Odx,
    /// ODX-Paket (ZIP) — bleibt lokal
    Pdx,
    /// CAN-Datenbank
    Dbc,
    /// eigene Daten des Nutzers (Labels, Mitschnitte)
    User,
    /// von Hand eingetragen / im Programm mitgeliefert
    Manual,
}

impl QuellArt {
    pub fn text(self) -> &'static str {
        match self {
            QuellArt::Community => "COMMUNITY",
            QuellArt::Odx => "ODX",
            QuellArt::Pdx => "PDX",
            QuellArt::Dbc => "DBC",
            QuellArt::User => "USER",
            QuellArt::Manual => "MANUAL",
        }
    }

    pub fn aus_text(s: &str) -> Option<Self> {
        Some(match s.to_ascii_uppercase().as_str() {
            "COMMUNITY" => QuellArt::Community,
            "ODX" => QuellArt::Odx,
            "PDX" => QuellArt::Pdx,
            "DBC" => QuellArt::Dbc,
            "USER" => QuellArt::User,
            "MANUAL" => QuellArt::Manual,
            _ => return None,
        })
    }
}

/// Darf ein Datensatz dieser Quelle exportiert / mit anderen geteilt werden?
/// ODX/PDX nie (Herstellerdaten mit Nutzungsvertrag). DBC und Community nur
/// mit angegebener Lizenz; eigene und mitgelieferte Daten immer.
pub fn darf_weitergeben(art: QuellArt, lizenz: &str) -> bool {
    match art {
        QuellArt::Odx | QuellArt::Pdx => false,
        QuellArt::Dbc | QuellArt::Community => !lizenz.trim().is_empty(),
        QuellArt::User | QuellArt::Manual => true,
    }
}

/// Eine Datenquelle mit Anzahl der zugehörigen Zeilen.
#[derive(Debug, Clone, PartialEq)]
pub struct Quelle {
    pub id: i64,
    pub art: QuellArt,
    pub name: String,
    pub version: String,
    pub lizenz: String,
    pub herkunft: String,
    pub importiert: String,
    pub zeilen: u64,
}

/// Was ein Import angelegt hat.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ImportErgebnis {
    pub quelle_id: i64,
    pub steuergeraete: usize,
    pub fehler: usize,
    pub messwerte: usize,
    pub codierung: usize,
    pub anpassung: usize,
    pub botschaften: usize,
    pub signale: usize,
}

impl ImportErgebnis {
    pub fn text(&self) -> String {
        let mut t = Vec::new();
        for (n, was) in [
            (self.steuergeraete, crate::trs!("Steuergeräte")),
            (self.fehler, crate::trs!("Fehlertexte")),
            (self.messwerte, crate::trs!("Messwerte")),
            (self.codierung, crate::trs!("Codier-Einträge")),
            (self.anpassung, crate::trs!("Anpassungskanäle")),
            (self.botschaften, crate::trs!("CAN-Botschaften")),
            (self.signale, crate::trs!("CAN-Signale")),
        ] {
            if n > 0 {
                t.push(format!("{n} {was}"));
            }
        }
        if t.is_empty() {
            crate::tr!("nichts")
        } else {
            t.join(", ")
        }
    }
}

/// Standard-Speicherort der Nutzer-Datenbank.
pub fn standard_pfad() -> PathBuf {
    crate::history::data_dir().join("fahrzeugdaten.sqlite")
}

pub struct Db {
    pub(crate) con: Connection,
}

impl Db {
    /// Datenbank öffnen (oder anlegen) und auf den neuesten Stand migrieren.
    pub fn oeffnen(pfad: &Path) -> Result<Db> {
        if let Some(d) = pfad.parent() {
            std::fs::create_dir_all(d).with_context(|| crate::tr!("Ordner {a} anlegen", a = d.display()))?;
        }
        let neu = !pfad.exists();
        let con = Connection::open(pfad).with_context(|| crate::tr!("Datenbank {a} öffnen", a = pfad.display()))?;
        con.pragma_update(None, "journal_mode", "WAL")?;
        let db = Db { con };
        db.migrieren(if neu { None } else { Some(pfad) })?;
        Ok(db)
    }

    /// Bestehende Datenbank nur lesend öffnen (keine Migration, nichts wird angelegt).
    /// Fehler, wenn die Datei fehlt oder eine andere Schema-Version hat.
    pub fn nur_lesen(pfad: &Path) -> Result<Db> {
        let con = Connection::open_with_flags(pfad, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
        let db = Db { con };
        let v = db.schema_version()?;
        if v != SCHEMA_VERSION {
            bail!(crate::tr!("Datenbank hat Schema-Version {a}, erwartet {b} — einmal „ecu-werkbank db“ ausführen.", a = v, b = SCHEMA_VERSION));
        }
        Ok(db)
    }

    /// Leere Datenbank im Speicher (Tests, Probeläufe).
    pub fn im_speicher() -> Result<Db> {
        let db = Db { con: Connection::open_in_memory()? };
        db.migrieren(None)?;
        Ok(db)
    }

    pub fn schema_version(&self) -> Result<u32> {
        Ok(self.con.pragma_query_value(None, "user_version", |r| r.get(0))?)
    }

    /// Fehlende Migrationen ausführen. `sichern`: vorher eine Kopie dieser Datei anlegen.
    fn migrieren(&self, sichern: Option<&Path>) -> Result<()> {
        self.con.pragma_update(None, "foreign_keys", "ON")?;
        let ist = self.schema_version()?;
        if ist > SCHEMA_VERSION {
            bail!(crate::tr!("Die Datenbank ist neuer (Version {a}) als dieses Programm (Version {b}). Bitte ECUWerkbank aktualisieren.", a = ist, b = SCHEMA_VERSION));
        }
        if ist == SCHEMA_VERSION {
            return Ok(());
        }
        if let (Some(p), true) = (sichern, ist > 0) {
            let kopie = p.with_extension(format!("vor-v{}.sqlite", ist + 1));
            self.sichern(&kopie).context(crate::tr!("Sicherung vor der Migration"))?;
        }
        for (i, sql) in MIGRATIONEN.iter().enumerate().skip(ist as usize) {
            let v = i as u32 + 1;
            let tx = self.con.unchecked_transaction()?;
            tx.execute_batch(sql).with_context(|| crate::tr!("Migration {a}", a = v))?;
            tx.pragma_update(None, "user_version", v)?;
            tx.commit()?;
        }
        Ok(())
    }

    /// Alle Änderungen aus dem Schreibprotokoll (WAL) in die Hauptdatei übernehmen.
    pub fn festschreiben(&self) -> Result<()> {
        self.con.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        Ok(())
    }

    /// Stimmige Kopie der Datenbank schreiben (auch im laufenden Betrieb).
    pub fn sichern(&self, ziel: &Path) -> Result<()> {
        if ziel.exists() {
            std::fs::remove_file(ziel)?;
        }
        self.con.execute("VACUUM INTO ?1", params![ziel.to_string_lossy()])?;
        Ok(())
    }

    // ---------------------------------------------------------------- Quellen

    pub fn quelle_anlegen(&self, art: QuellArt, name: &str, version: &str, lizenz: &str, herkunft: &str) -> Result<i64> {
        self.con.execute(
            "INSERT INTO data_source (source_type, source_name, version, license, origin, imported_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![art.text(), name, version, lizenz, herkunft, crate::report::now_local()],
        )?;
        Ok(self.con.last_insert_rowid())
    }

    pub fn quellen(&self) -> Result<Vec<Quelle>> {
        let mut st = self.con.prepare(
            "SELECT id, source_type, source_name, version, license, origin, imported_at,
               (SELECT COUNT(*) FROM ecu WHERE source_id = s.id)
             + (SELECT COUNT(*) FROM did WHERE source_id = s.id)
             + (SELECT COUNT(*) FROM dtc WHERE source_id = s.id)
             + (SELECT COUNT(*) FROM measurement WHERE source_id = s.id)
             + (SELECT COUNT(*) FROM coding_item WHERE source_id = s.id)
             + (SELECT COUNT(*) FROM adaptation WHERE source_id = s.id)
             + (SELECT COUNT(*) FROM can_message WHERE source_id = s.id)
             FROM data_source s ORDER BY id",
        )?;
        let v = st
            .query_map([], |r| {
                let art: String = r.get(1)?;
                Ok(Quelle {
                    id: r.get(0)?,
                    art: QuellArt::aus_text(&art).unwrap_or(QuellArt::Manual),
                    name: r.get(2)?,
                    version: r.get(3)?,
                    lizenz: r.get(4)?,
                    herkunft: r.get(5)?,
                    importiert: r.get(6)?,
                    zeilen: r.get::<_, i64>(7)? as u64,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    }

    /// Quelle samt allen ihren Daten entfernen. `false`, wenn es sie nicht gab.
    pub fn quelle_entfernen(&self, id: i64) -> Result<bool> {
        Ok(self.con.execute("DELETE FROM data_source WHERE id = ?1", params![id])? > 0)
    }

    // -------------------------------------------------------------- Hersteller

    /// Hersteller holen oder anlegen; gibt die id zurück.
    pub fn hersteller(&self, name: &str, wmi: &[&str], quelle: Option<i64>) -> Result<i64> {
        if let Some(id) = self.con.query_row("SELECT id FROM manufacturer WHERE name = ?1", params![name], |r| r.get(0)).optional()? {
            return Ok(id);
        }
        let jetzt = crate::report::now_local();
        self.con.execute(
            "INSERT INTO manufacturer (name, vin_wmi, source_id, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?4)",
            params![name, wmi.join(","), quelle, jetzt],
        )?;
        Ok(self.con.last_insert_rowid())
    }

    /// Hersteller zu einer Fahrgestellnummer (über die ersten drei Zeichen).
    pub fn hersteller_zur_vin(&self, vin: &str) -> Result<Option<(i64, String)>> {
        let Some(wmi) = vin.get(..3) else { return Ok(None) };
        let mut st = self.con.prepare("SELECT id, name, vin_wmi FROM manufacturer")?;
        let mut zeilen = st.query([])?;
        while let Some(r) = zeilen.next()? {
            let liste: String = r.get(2)?;
            if liste.split(',').any(|w| w.trim().eq_ignore_ascii_case(wmi)) {
                return Ok(Some((r.get(0)?, r.get(1)?)));
            }
        }
        Ok(None)
    }

    // ------------------------------------------------------------- Abfragen

    /// Name eines Steuergeräts an einer Diagnoseadresse (neueste Quelle zuerst).
    pub fn steuergeraet_name(&self, hersteller: i64, adresse: u16) -> Result<Option<String>> {
        Ok(self
            .con
            .query_row(
                "SELECT ecu_name FROM ecu WHERE manufacturer_id = ?1 AND diagnostic_address = ?2 ORDER BY source_id DESC LIMIT 1",
                params![hersteller, adresse],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Fehlertext zu einem Code (z. B. "P0301", "01314"), neueste Quelle zuerst.
    pub fn fehlertext(&self, code: &str) -> Result<Option<String>> {
        Ok(self
            .con
            .query_row("SELECT title FROM dtc WHERE dtc_code = ?1 ORDER BY source_id DESC LIMIT 1", params![code.trim().to_ascii_uppercase()], |r| r.get(0))
            .optional()?)
    }

    /// Alle Fehlertexte (Code → Text); bei mehreren Quellen gewinnt die neueste.
    pub fn alle_fehlertexte(&self) -> Result<Vec<(String, String)>> {
        let mut st = self.con.prepare("SELECT dtc_code, title FROM dtc WHERE title <> '' ORDER BY source_id")?;
        let v = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    }

    /// Alle CAN-Botschaften eines Herstellers (oder ohne Hersteller) als DBC-Struktur,
    /// z. B. für den CAN-Monitor oder die Live-Grafik.
    pub fn dbc(&self, hersteller: Option<i64>) -> Result<crate::dbc::Dbc> {
        let mut d = crate::dbc::Dbc::default();
        let mut st = self.con.prepare(
            "SELECT m.id, m.can_id, m.name, m.dlc FROM can_message m WHERE m.manufacturer_id IS ?1 ORDER BY m.source_id",
        )?;
        let botschaften: Vec<(i64, u32, String, u8)> = st.query_map(params![hersteller], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<rusqlite::Result<_>>()?;
        let mut sig = self.con.prepare(
            "SELECT signal_name, start_bit, bit_length, little_endian, is_signed, factor, \"offset\", unit, value_table FROM can_signal WHERE can_message_id = ?1 ORDER BY id",
        )?;
        for (mid, id, name, dlc) in botschaften {
            let signale = sig
                .query_map(params![mid], |r| {
                    Ok(crate::dbc::Signal {
                        name: r.get(0)?,
                        start: r.get(1)?,
                        laenge: r.get(2)?,
                        intel: r.get::<_, i64>(3)? != 0,
                        vorzeichen: r.get::<_, i64>(4)? != 0,
                        faktor: r.get(5)?,
                        offset: r.get(6)?,
                        einheit: r.get(7)?,
                        multiplex: false,
                        werte: werte_lesen(&r.get::<_, String>(8)?),
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            d.botschaften.insert(id, crate::dbc::Botschaft { id, name, laenge: dlc, signale });
        }
        Ok(d)
    }

    /// Anzahl Zeilen je Tabelle (für „db status“).
    pub fn statistik(&self) -> Result<Vec<(&'static str, u64)>> {
        let mut v = Vec::new();
        for t in ["data_source", "manufacturer", "vehicle_model", "ecu", "did", "dtc", "measurement", "coding_item", "adaptation", "routine", "can_message", "can_signal", "uds_service", "diagnostic_session"] {
            let n: i64 = self.con.query_row(&format!("SELECT COUNT(*) FROM {t}"), [], |r| r.get(0))?;
            v.push((t, n as u64));
        }
        Ok(v)
    }

    // --------------------------------------------------------------- Importe

    /// Normdaten (ISO 14229-Dienste und Sitzungen) eintragen; mehrfacher Aufruf schadet nicht.
    pub fn normdaten_eintragen(&self) -> Result<()> {
        let tx = self.con.unchecked_transaction()?;
        for (sid, name) in UDS_DIENSTE {
            tx.execute("INSERT OR IGNORE INTO uds_service (service_id, name) VALUES (?1, ?2)", params![sid, name])?;
        }
        for (s, name, level) in [(0x01u8, "Default Session", 0u8), (0x02, "Programming Session", 1), (0x03, "Extended Diagnostic Session", 0), (0x04, "Safety System Diagnostic Session", 1)] {
            tx.execute("INSERT OR IGNORE INTO diagnostic_session (session_id, name, required_security_level) VALUES (?1, ?2, ?3)", params![s, name, level])?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Eigene Label-Dateien (TOML) als Quelle „USER“ übernehmen.
    pub fn labels_importieren(&self, ordner: &Path, hersteller: i64) -> Result<ImportErgebnis> {
        let mut dateien: Vec<PathBuf> = std::fs::read_dir(ordner)
            .with_context(|| crate::tr!("Ordner {a} lesen", a = ordner.display()))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|e| e == "toml"))
            .collect();
        dateien.sort();
        let q = self.quelle_anlegen(QuellArt::User, &crate::tr!("Eigene Labels"), "", "", &ordner.display().to_string())?;
        let mut erg = ImportErgebnis { quelle_id: q, ..Default::default() };
        let tx = self.con.unchecked_transaction()?;
        for p in dateien {
            let Ok(f) = crate::labels::LabelFile::load(&p) else { continue };
            let name = if f.ecu.name.is_empty() { p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default() } else { f.ecu.name.clone() };
            tx.execute(
                "INSERT INTO ecu (manufacturer_id, ecu_name, software_number, protocol, diagnostic_address, can_tx, can_rx, coding_did, source_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![hersteller, name, f.ecu.part.clone().unwrap_or_default(), if f.ecu.tx.is_some() { "UDS" } else { "" }, f.ecu.address, f.ecu.tx, f.ecu.rx, f.ecu.coding_did, q],
            )?;
            let ecu = tx.last_insert_rowid();
            erg.steuergeraete += 1;
            for d in &f.dtc {
                tx.execute("INSERT INTO dtc (ecu_id, dtc_code, title, source_id) VALUES (?1, ?2, ?3, ?4)", params![ecu, d.code.trim().to_ascii_uppercase(), d.description, q])?;
                erg.fehler += 1;
            }
            for m in &f.measuring {
                tx.execute(
                    "INSERT INTO measurement (ecu_id, kind, pid, name, unit, factor, \"offset\", source_id) VALUES (?1, 'UDS', ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![ecu, m.id, m.name, m.unit, m.factor, m.offset, q],
                )?;
                erg.messwerte += 1;
            }
            for c in &f.coding {
                tx.execute(
                    "INSERT INTO coding_item (ecu_id, byte_index, bit_index, label, requires, excludes, source_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![ecu, c.byte, c.bit, c.name, c.benoetigt.join(","), c.schliesst_aus.join(","), q],
                )?;
                erg.codierung += 1;
            }
            for a in &f.adaptation {
                tx.execute("INSERT INTO adaptation (ecu_id, channel, name, source_id) VALUES (?1, ?2, ?3, ?4)", params![ecu, a.channel, a.name, q])?;
                erg.anpassung += 1;
            }
        }
        tx.commit()?;
        Ok(erg)
    }

    /// DBC-Datei als Quelle „DBC“ übernehmen (eine Transaktion; schnell auch bei großen Dateien).
    pub fn dbc_importieren(&self, dbc: &crate::dbc::Dbc, name: &str, lizenz: &str, herkunft: &str, hersteller: Option<i64>) -> Result<ImportErgebnis> {
        let q = self.quelle_anlegen(QuellArt::Dbc, name, "", lizenz, herkunft)?;
        let mut erg = ImportErgebnis { quelle_id: q, ..Default::default() };
        let tx = self.con.unchecked_transaction()?;
        {
            let mut bo = tx.prepare("INSERT INTO can_message (manufacturer_id, can_id, extended, name, dlc, source_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6)")?;
            let mut sg = tx.prepare(
                "INSERT INTO can_signal (can_message_id, signal_name, start_bit, bit_length, little_endian, is_signed, factor, \"offset\", unit, value_table)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            )?;
            let mut ids: Vec<&u32> = dbc.botschaften.keys().collect();
            ids.sort();
            for id in ids {
                let b = &dbc.botschaften[id];
                bo.execute(params![hersteller, b.id, b.id > 0x7FF, b.name, b.laenge, q])?;
                let mid = tx.last_insert_rowid();
                erg.botschaften += 1;
                for s in b.signale.iter().filter(|s| !s.multiplex) {
                    sg.execute(params![mid, s.name, s.start, s.laenge, s.intel, s.vorzeichen, s.faktor, s.offset, s.einheit, werte_schreiben(&s.werte)])?;
                    erg.signale += 1;
                }
            }
        }
        tx.commit()?;
        Ok(erg)
    }
}

/// Fehlertexte aus der Nutzer-Datenbank, falls es sie gibt (sonst leer).
/// Öffnet nur lesend und legt nichts an — für den Programmstart.
pub fn fehlertexte_falls_vorhanden() -> Vec<(String, String)> {
    let p = standard_pfad();
    if !p.exists() {
        return Vec::new();
    }
    Db::nur_lesen(&p).and_then(|db| db.alle_fehlertexte()).unwrap_or_default()
}

/// Wertetexte als `0=aus;1=an` (ohne JSON-Abhängigkeit; `;` und `=` im Text werden ersetzt).
fn werte_schreiben(w: &std::collections::HashMap<i64, String>) -> String {
    let mut v: Vec<_> = w.iter().collect();
    v.sort();
    v.iter().map(|(k, t)| format!("{k}={}", t.replace([';', '='], " "))).collect::<Vec<_>>().join(";")
}

fn werte_lesen(s: &str) -> std::collections::HashMap<i64, String> {
    s.split(';').filter_map(|e| e.split_once('=')).filter_map(|(k, t)| Some((k.trim().parse().ok()?, t.to_string()))).collect()
}

/// UDS-Dienste nach ISO 14229 (Normdaten).
const UDS_DIENSTE: &[(u8, &str)] = &[
    (0x10, "DiagnosticSessionControl"),
    (0x11, "ECUReset"),
    (0x14, "ClearDiagnosticInformation"),
    (0x19, "ReadDTCInformation"),
    (0x22, "ReadDataByIdentifier"),
    (0x23, "ReadMemoryByAddress"),
    (0x24, "ReadScalingDataByIdentifier"),
    (0x27, "SecurityAccess"),
    (0x28, "CommunicationControl"),
    (0x29, "Authentication"),
    (0x2A, "ReadDataByPeriodicIdentifier"),
    (0x2C, "DynamicallyDefineDataIdentifier"),
    (0x2E, "WriteDataByIdentifier"),
    (0x2F, "InputOutputControlByIdentifier"),
    (0x31, "RoutineControl"),
    (0x34, "RequestDownload"),
    (0x35, "RequestUpload"),
    (0x36, "TransferData"),
    (0x37, "RequestTransferExit"),
    (0x38, "RequestFileTransfer"),
    (0x3D, "WriteMemoryByAddress"),
    (0x3E, "TesterPresent"),
    (0x85, "ControlDTCSetting"),
    (0x86, "ResponseOnEvent"),
    (0x87, "LinkControl"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_und_migration() {
        let db = Db::im_speicher().unwrap();
        assert_eq!(db.schema_version().unwrap(), SCHEMA_VERSION);
        db.normdaten_eintragen().unwrap();
        db.normdaten_eintragen().unwrap(); // zweimal schadet nicht
        let s: std::collections::HashMap<_, _> = db.statistik().unwrap().into_iter().collect();
        assert_eq!(s["uds_service"], UDS_DIENSTE.len() as u64);
        assert_eq!(s["diagnostic_session"], 4);
    }

    #[test]
    fn datei_oeffnen_sichern_und_wieder_oeffnen() {
        let dir = std::env::temp_dir().join(format!("ecuwb-db-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let p = dir.join("t.sqlite");
        {
            let db = Db::oeffnen(&p).unwrap();
            db.community_uebernehmen().unwrap();
            db.sichern(&dir.join("kopie.sqlite")).unwrap();
        }
        let db = Db::oeffnen(&p).unwrap();
        let vw = db.hersteller_zur_vin("WVWZZZ1KZAM000017").unwrap().unwrap();
        assert_eq!(vw.1, "Volkswagen");
        assert_eq!(db.steuergeraet_name(vw.0, 0x09).unwrap().as_deref(), Some("Bordnetz"));
        let kopie = Db::oeffnen(&dir.join("kopie.sqlite")).unwrap();
        assert_eq!(kopie.steuergeraet_name(vw.0, 0x01).unwrap().as_deref(), Some("Motor"));
        drop((db, kopie));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn migration_von_version_1_mit_sicherung() {
        let dir = std::env::temp_dir().join(format!("ecuwb-mig-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("t.sqlite");
        {
            // Datei wie von einem Programm mit Schema 1
            let con = Connection::open(&p).unwrap();
            con.execute_batch(MIGRATIONEN[0]).unwrap();
            con.pragma_update(None, "user_version", 1).unwrap();
            con.execute("INSERT INTO data_source (source_type, source_name, imported_at) VALUES ('USER', 'alt', 'x')", []).unwrap();
        }
        let db = Db::oeffnen(&p).unwrap();
        assert_eq!(db.schema_version().unwrap(), SCHEMA_VERSION);
        assert_eq!(db.quellen().unwrap().len(), 1, "Daten bleiben erhalten");
        let kopie = p.with_extension("vor-v2.sqlite");
        assert!(kopie.exists(), "Sicherung vor der Migration");
        let alt = Connection::open(&kopie).unwrap();
        assert_eq!(alt.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0)).unwrap(), 1);
        drop((db, alt));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn neuere_datenbank_wird_abgelehnt() {
        let dir = std::env::temp_dir().join(format!("ecuwb-db-neu-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("t.sqlite");
        drop(Db::oeffnen(&p).unwrap());
        Connection::open(&p).unwrap().pragma_update(None, "user_version", SCHEMA_VERSION + 1).unwrap();
        let e = Db::oeffnen(&p).err().unwrap();
        assert!(format!("{e:#}").contains("neuer"), "{e:#}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dbc_hin_und_zurueck_und_quelle_entfernen() {
        let text = "BO_ 640 Motor_1: 8 XXX\n SG_ Drehzahl : 16|16@1+ (0.25,0) [0|16256] \"U/min\" XXX\n SG_ Leerlauf : 0|1@1+ (1,0) [0|1] \"\" XXX\n\nVAL_ 640 Leerlauf 0 \"nein\" 1 \"ja\" ;\n";
        let dbc = crate::dbc::Dbc::lesen(text);
        let db = Db::im_speicher().unwrap();
        let erg = db.dbc_importieren(&dbc, "test.dbc", "MIT", "Test", None).unwrap();
        assert_eq!((erg.botschaften, erg.signale), (1, 2));
        let zurueck = db.dbc(None).unwrap();
        let w = zurueck.entschluesseln(0x280, &[0x01, 0, 0xA0, 0x0F, 0, 0, 0, 0]);
        let drehzahl = w.iter().find(|w| w.signal == "Drehzahl").unwrap();
        assert_eq!(drehzahl.wert, 1000.0);
        let ll = w.iter().find(|w| w.signal == "Leerlauf").unwrap();
        assert_eq!(ll.text.as_deref(), Some("ja"));
        let q = db.quellen().unwrap();
        assert_eq!(q[0].zeilen, 1);
        assert!(db.quelle_entfernen(erg.quelle_id).unwrap());
        assert!(db.dbc(None).unwrap().botschaften.is_empty(), "Signale mit der Quelle entfernt");
        let s: std::collections::HashMap<_, _> = db.statistik().unwrap().into_iter().collect();
        assert_eq!(s["can_signal"], 0);
    }

    #[test]
    fn labels_importieren() {
        let dir = std::env::temp_dir().join(format!("ecuwb-lbl-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("09-test.toml"),
            "[ecu]\naddress = 0x09\nname = \"Testgerät\"\ntx = 0x70E\nrx = 0x778\n\n[[dtc]]\ncode = \"b1234\"\ndescription = \"Eigener Text\"\n\n[[coding]]\nbyte = 1\nbit = 2\nname = \"Testbit\"\n",
        )
        .unwrap();
        let db = Db::im_speicher().unwrap();
        let h = db.hersteller("Volkswagen", &["WVW"], None).unwrap();
        let erg = db.labels_importieren(&dir, h).unwrap();
        assert_eq!((erg.steuergeraete, erg.fehler, erg.codierung), (1, 1, 1));
        assert_eq!(db.fehlertext("B1234").unwrap().as_deref(), Some("Eigener Text"));
        assert_eq!(db.steuergeraet_name(h, 0x09).unwrap().as_deref(), Some("Testgerät"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Größenprobe aus dem Plan: 1 Mio. CAN-Signale. Läuft nur auf Wunsch:
    /// `cargo test --release --lib db::tests::grosse_datenmenge -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn grosse_datenmenge() {
        let mut dbc = crate::dbc::Dbc::default();
        for id in 0..125_000u32 {
            let signale = (0..8)
                .map(|i| crate::dbc::Signal {
                    name: format!("S{id}_{i}"),
                    start: i * 8,
                    laenge: 8,
                    intel: true,
                    vorzeichen: false,
                    faktor: 0.5,
                    offset: 0.0,
                    einheit: "x".into(),
                    multiplex: false,
                    werte: Default::default(),
                })
                .collect();
            dbc.botschaften.insert(id, crate::dbc::Botschaft { id, name: format!("B{id}"), laenge: 8, signale });
        }
        let dir = std::env::temp_dir().join(format!("ecuwb-gross-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let p = dir.join("gross.sqlite");
        let db = Db::oeffnen(&p).unwrap();
        let t = std::time::Instant::now();
        let erg = db.dbc_importieren(&dbc, "gross", "MIT", "Probe", None).unwrap();
        let import = t.elapsed();
        assert_eq!(erg.signale, 1_000_000);
        let t = std::time::Instant::now();
        for id in (0..125_000).step_by(1250) {
            let n: i64 = db
                .con
                .query_row("SELECT COUNT(*) FROM can_signal s JOIN can_message m ON s.can_message_id = m.id WHERE m.manufacturer_id IS NULL AND m.can_id = ?1", params![id], |r| r.get(0))
                .unwrap();
            assert_eq!(n, 8);
        }
        let abfrage = t.elapsed() / 100;
        let groesse = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0) + std::fs::metadata(p.with_extension("sqlite-wal")).map(|m| m.len()).unwrap_or(0);
        println!("Import 1 Mio. Signale: {import:?}, Abfrage je Botschaft: {abfrage:?}, Größe: {} MB", groesse / 1_000_000);
        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn nur_lesen_und_fehlertexte() {
        let dir = std::env::temp_dir().join(format!("ecuwb-nl-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let p = dir.join("t.sqlite");
        assert!(Db::nur_lesen(&p).is_err(), "fehlende Datei wird nicht angelegt");
        assert!(!p.exists());
        {
            let db = Db::oeffnen(&p).unwrap();
            let q = db.quelle_anlegen(QuellArt::User, "a", "", "", "").unwrap();
            db.con.execute("INSERT INTO dtc (dtc_code, title, source_id) VALUES ('P0301', 'alt', ?1)", params![q]).unwrap();
            let q2 = db.quelle_anlegen(QuellArt::User, "b", "", "", "").unwrap();
            db.con.execute("INSERT INTO dtc (dtc_code, title, source_id) VALUES ('P0301', 'neu', ?1)", params![q2]).unwrap();
        }
        let db = Db::nur_lesen(&p).unwrap();
        let m: std::collections::HashMap<_, _> = db.alle_fehlertexte().unwrap().into_iter().collect();
        assert_eq!(m["P0301"], "neu", "neueste Quelle gewinnt");
        assert!(db.quelle_anlegen(QuellArt::User, "x", "", "", "").is_err(), "nur lesen");
        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn odx_bleibt_lokal() {
        assert!(!darf_weitergeben(QuellArt::Odx, "irgendwas"));
        assert!(!darf_weitergeben(QuellArt::Pdx, "MIT"));
        assert!(!darf_weitergeben(QuellArt::Dbc, ""));
        assert!(darf_weitergeben(QuellArt::Dbc, "MIT"));
        assert!(darf_weitergeben(QuellArt::User, ""));
        for a in ["COMMUNITY", "ODX", "PDX", "DBC", "USER", "MANUAL"] {
            assert_eq!(QuellArt::aus_text(a).unwrap().text(), a);
        }
    }
}
