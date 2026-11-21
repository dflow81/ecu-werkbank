//! Inhalt der Fahrzeug-Datenbank ansehen und bearbeiten: was zu einer Quelle
//! gehört (Steuergeräte mit Fehlern, Kennungen, Codierung, Anpassung;
//! allgemeine Fehlertexte; CAN-Botschaften mit Signalen). Dient auch der
//! Vorschau vor einem Import ([`super::import::vorschau`]).

use super::paket::{PaketAnpassung, PaketCodierung, PaketDid, PaketFehler, PaketFehlerart, PaketSteuergeraet};
use super::{Db, Quelle};
use anyhow::{bail, Result};
use rusqlite::{params, OptionalExtension};

/// Eine CAN-Botschaft mit ihren Signalen.
#[derive(Debug, Clone, PartialEq)]
pub struct CanBotschaft {
    pub id: i64,
    pub can_id: u32,
    pub name: String,
    pub dlc: u8,
    pub signale: Vec<crate::dbc::Signal>,
}

/// Alles, was zu einer Quelle gehört.
#[derive(Debug, Clone)]
pub struct Inhalt {
    pub quelle: Quelle,
    /// (ecu-id, Hersteller, Steuergerät)
    pub steuergeraete: Vec<(i64, String, PaketSteuergeraet)>,
    /// Fehlertexte ohne festes Steuergerät
    pub fehler: Vec<PaketFehler>,
    /// Fehlerarten („defekt“ …) je Protokoll
    pub fehlerarten: Vec<PaketFehlerart>,
    pub can: Vec<CanBotschaft>,
}

impl Inhalt {
    /// Kurzbeschreibung: „3 Steuergeräte, 12 Fehlertexte …“
    pub fn zusammenfassung(&self) -> String {
        let f: usize = self.fehler.len() + self.steuergeraete.iter().map(|(_, _, s)| s.fehler.len()).sum::<usize>();
        let d: usize = self.steuergeraete.iter().map(|(_, _, s)| s.dids.len()).sum();
        let c: usize = self.steuergeraete.iter().map(|(_, _, s)| s.codierung.len()).sum();
        let a: usize = self.steuergeraete.iter().map(|(_, _, s)| s.anpassung.len()).sum();
        let sig: usize = self.can.iter().map(|b| b.signale.len()).sum();
        let teile = [
            (self.steuergeraete.len(), crate::tr!("Steuergeräte")),
            (f, crate::tr!("Fehlertexte")),
            (d, crate::tr!("Kennungen/Messwerte")),
            (c, crate::tr!("Codier-Einträge")),
            (a, crate::tr!("Anpassungskanäle")),
            (self.fehlerarten.len(), crate::tr!("Fehlerarten")),
            (self.can.len(), crate::tr!("CAN-Botschaften")),
            (sig, crate::tr!("CAN-Signale")),
        ];
        let t: Vec<String> = teile.iter().filter(|(n, _)| *n > 0).map(|(n, w)| format!("{n} {w}")).collect();
        if t.is_empty() {
            crate::tr!("leer")
        } else {
            t.join(", ")
        }
    }
}

fn liste_teilen(s: String) -> Vec<String> {
    s.split(',').map(str::trim).filter(|t| !t.is_empty()).map(str::to_string).collect()
}

impl Db {
    /// Ein Steuergerät mit allem, was daran hängt (Messwerte aus Labels erscheinen als Kennungen).
    pub fn sg_laden(&self, ecu: i64) -> Result<(String, PaketSteuergeraet)> {
        let kopf = self
            .con
            .query_row(
                "SELECT e.ecu_name, e.diagnostic_address, e.ecu_family, e.protocol, e.can_tx, e.can_rx, e.software_number, COALESCE(m.name, '')
                 FROM ecu e LEFT JOIN manufacturer m ON m.id = e.manufacturer_id WHERE e.id = ?1",
                params![ecu],
                |r| {
                    Ok((
                        r.get::<_, String>(7)?,
                        PaketSteuergeraet { name: r.get(0)?, adresse: r.get(1)?, gruppe: r.get(2)?, protokoll: r.get(3)?, tx: r.get(4)?, rx: r.get(5)?, teilenummer: r.get(6)?, ..Default::default() },
                    ))
                },
            )
            .optional()?;
        let Some((hersteller, mut s)) = kopf else { bail!(crate::tr!("Steuergerät {a} nicht gefunden", a = ecu)) };
        let mut st = self.con.prepare("SELECT dtc_code, title FROM dtc WHERE ecu_id = ?1 ORDER BY id")?;
        s.fehler = st.query_map(params![ecu], |r| Ok(PaketFehler { code: r.get(0)?, text: r.get(1)? }))?.collect::<rusqlite::Result<_>>()?;
        let mut st = self.con.prepare("SELECT did, name, unit, factor, \"offset\" FROM did WHERE ecu_id = ?1 ORDER BY id")?;
        s.dids = st.query_map(params![ecu], |r| Ok(PaketDid { did: r.get(0)?, name: r.get(1)?, einheit: r.get(2)?, faktor: r.get(3)?, offset: r.get(4)? }))?.collect::<rusqlite::Result<_>>()?;
        let mut st = self.con.prepare("SELECT pid, name, unit, factor, \"offset\" FROM measurement WHERE ecu_id = ?1 ORDER BY id")?;
        let messwerte: Vec<PaketDid> = st.query_map(params![ecu], |r| Ok(PaketDid { did: r.get(0)?, name: r.get(1)?, einheit: r.get(2)?, faktor: r.get(3)?, offset: r.get(4)? }))?.collect::<rusqlite::Result<_>>()?;
        s.dids.extend(messwerte);
        let mut st = self.con.prepare("SELECT byte_index, bit_index, label, requires, excludes FROM coding_item WHERE ecu_id = ?1 ORDER BY byte_index, bit_index")?;
        s.codierung = st
            .query_map(params![ecu], |r| Ok(PaketCodierung { byte: r.get(0)?, bit: r.get(1)?, name: r.get(2)?, benoetigt: liste_teilen(r.get(3)?), schliesst_aus: liste_teilen(r.get(4)?) }))?
            .collect::<rusqlite::Result<_>>()?;
        let mut st = self.con.prepare("SELECT channel, name FROM adaptation WHERE ecu_id = ?1 ORDER BY channel")?;
        s.anpassung = st.query_map(params![ecu], |r| Ok(PaketAnpassung { kanal: r.get(0)?, name: r.get(1)? }))?.collect::<rusqlite::Result<_>>()?;
        Ok((hersteller, s))
    }

    /// Steuergerät ändern: Kopf aktualisieren, Fehler/Kennungen/Codierung/Anpassung
    /// ersetzen. Quelle und Hersteller bleiben.
    pub fn sg_speichern(&self, ecu: i64, s: &PaketSteuergeraet) -> Result<()> {
        let q: i64 = self.con.query_row("SELECT source_id FROM ecu WHERE id = ?1", params![ecu], |r| r.get(0))?;
        let tx = self.con.unchecked_transaction()?;
        tx.execute(
            "UPDATE ecu SET ecu_name = ?2, diagnostic_address = ?3, ecu_family = ?4, protocol = ?5, can_tx = ?6, can_rx = ?7, software_number = ?8 WHERE id = ?1",
            params![ecu, s.name, s.adresse, s.gruppe, s.protokoll, s.tx, s.rx, s.teilenummer],
        )?;
        for t in ["dtc", "did", "measurement", "coding_item", "adaptation"] {
            tx.execute(&format!("DELETE FROM {t} WHERE ecu_id = ?1"), params![ecu])?;
        }
        for f in &s.fehler {
            tx.execute("INSERT INTO dtc (ecu_id, dtc_code, title, source_id) VALUES (?1, ?2, ?3, ?4)", params![ecu, f.code.trim().to_ascii_uppercase(), f.text, q])?;
        }
        for d in &s.dids {
            tx.execute("INSERT INTO did (ecu_id, did, name, unit, factor, \"offset\", source_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)", params![ecu, d.did, d.name, d.einheit, d.faktor, d.offset, q])?;
        }
        for c in &s.codierung {
            tx.execute(
                "INSERT INTO coding_item (ecu_id, byte_index, bit_index, label, requires, excludes, source_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![ecu, c.byte, c.bit, c.name, c.benoetigt.join(","), c.schliesst_aus.join(","), q],
            )?;
        }
        for a in &s.anpassung {
            tx.execute("INSERT INTO adaptation (ecu_id, channel, name, source_id) VALUES (?1, ?2, ?3, ?4)", params![ecu, a.kanal, a.name, q])?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn sg_entfernen(&self, ecu: i64) -> Result<bool> {
        Ok(self.con.execute("DELETE FROM ecu WHERE id = ?1", params![ecu])? > 0)
    }

    /// Allgemeine Fehlertexte (ohne Steuergerät) einer Quelle ersetzen.
    pub fn fehler_allgemein_speichern(&self, quelle: i64, liste: &[PaketFehler]) -> Result<()> {
        let tx = self.con.unchecked_transaction()?;
        tx.execute("DELETE FROM dtc WHERE ecu_id IS NULL AND source_id = ?1", params![quelle])?;
        for f in liste {
            tx.execute("INSERT INTO dtc (dtc_code, title, source_id) VALUES (?1, ?2, ?3)", params![f.code.trim().to_ascii_uppercase(), f.text, quelle])?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Fehlerarten einer Quelle ersetzen.
    pub fn fehlerarten_speichern(&self, quelle: i64, liste: &[PaketFehlerart]) -> Result<()> {
        let tx = self.con.unchecked_transaction()?;
        tx.execute("DELETE FROM fault_type WHERE source_id = ?1", params![quelle])?;
        for f in liste {
            super::paket::fehlerart_eintragen(&tx, f, quelle)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Name einer CAN-Botschaft und ihre Signale ändern (Signale werden ersetzt).
    pub fn can_speichern(&self, b: &CanBotschaft) -> Result<()> {
        let tx = self.con.unchecked_transaction()?;
        tx.execute("UPDATE can_message SET name = ?2, dlc = ?3 WHERE id = ?1", params![b.id, b.name, b.dlc])?;
        tx.execute("DELETE FROM can_signal WHERE can_message_id = ?1", params![b.id])?;
        for s in &b.signale {
            let werte: String = {
                let mut v: Vec<_> = s.werte.iter().collect();
                v.sort();
                v.iter().map(|(k, t)| format!("{k}={}", t.replace([';', '='], " "))).collect::<Vec<_>>().join(";")
            };
            tx.execute(
                "INSERT INTO can_signal (can_message_id, signal_name, start_bit, bit_length, little_endian, is_signed, factor, \"offset\", unit, value_table)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![b.id, s.name, s.start, s.laenge, s.intel, s.vorzeichen, s.faktor, s.offset, s.einheit, werte],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Alles, was zu einer Quelle gehört.
    pub fn inhalt(&self, quelle: i64) -> Result<Inhalt> {
        let Some(q) = self.quellen()?.into_iter().find(|q| q.id == quelle) else { bail!(crate::tr!("Quelle {a} nicht gefunden", a = quelle)) };
        let ids: Vec<i64> = {
            let mut st = self.con.prepare("SELECT id FROM ecu WHERE source_id = ?1 ORDER BY diagnostic_address, id")?;
            let v = st.query_map(params![quelle], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
            v
        };
        let mut steuergeraete = Vec::new();
        for id in ids {
            let (h, s) = self.sg_laden(id)?;
            steuergeraete.push((id, h, s));
        }
        let mut st = self.con.prepare("SELECT dtc_code, title FROM dtc WHERE ecu_id IS NULL AND source_id = ?1 ORDER BY dtc_code")?;
        let fehler = st.query_map(params![quelle], |r| Ok(PaketFehler { code: r.get(0)?, text: r.get(1)? }))?.collect::<rusqlite::Result<_>>()?;
        let mut st = self.con.prepare("SELECT protocol, nr, text FROM fault_type WHERE source_id = ?1 ORDER BY protocol, nr, id")?;
        let fehlerarten = st.query_map(params![quelle], |r| Ok(PaketFehlerart { protokoll: r.get(0)?, nr: r.get(1)?, text: r.get(2)? }))?.collect::<rusqlite::Result<_>>()?;
        let mut st = self.con.prepare("SELECT id, can_id, name, dlc FROM can_message WHERE source_id = ?1 ORDER BY can_id")?;
        let kopf: Vec<(i64, u32, String, u8)> = st.query_map(params![quelle], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<rusqlite::Result<_>>()?;
        let mut sig = self.con.prepare(
            "SELECT signal_name, start_bit, bit_length, little_endian, is_signed, factor, \"offset\", unit, value_table FROM can_signal WHERE can_message_id = ?1 ORDER BY id",
        )?;
        let mut can = Vec::new();
        for (id, can_id, name, dlc) in kopf {
            let signale = sig
                .query_map(params![id], |r| {
                    let wt: String = r.get(8)?;
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
                        werte: wt.split(';').filter_map(|e| e.split_once('=')).filter_map(|(k, t)| Some((k.trim().parse().ok()?, t.to_string()))).collect(),
                    })
                })?
                .collect::<rusqlite::Result<_>>()?;
            can.push(CanBotschaft { id, can_id, name, dlc, signale });
        }
        Ok(Inhalt { quelle: q, steuergeraete, fehler, fehlerarten, can })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::QuellArt;

    #[test]
    fn inhalt_lesen_aendern() {
        let db = Db::im_speicher().unwrap();
        let p = crate::db::paket::Datenpaket::aus_text(
            "fehler = [{ code = \"P0420\", text = \"Kat\" }]\n[quelle]\nname = \"T\"\nlizenz = \"CC0-1.0\"\n[[hersteller]]\nname = \"Volkswagen\"\n[[hersteller.steuergeraet]]\nadresse = 9\nname = \"Bordnetz\"\nfehler = [{ code = \"B1\", text = \"x\" }]\ncodierung = [{ byte = 0, bit = 1, name = \"A\", benoetigt = [\"0.0\"] }]\n",
            false,
        )
        .unwrap();
        let e = db.paket_importieren(&p, QuellArt::Community).unwrap();
        let i = db.inhalt(e.quelle_id).unwrap();
        assert_eq!(i.steuergeraete.len(), 1);
        assert_eq!(i.fehler.len(), 1);
        let (id, h, mut s) = i.steuergeraete[0].clone();
        assert_eq!((h.as_str(), s.name.as_str(), s.adresse), ("Volkswagen", "Bordnetz", Some(9)));
        assert_eq!(s.codierung[0].benoetigt, vec!["0.0"]);
        s.name = "Bordnetz J519".into();
        s.fehler.push(PaketFehler { code: "b2".into(), text: "neu".into() });
        db.sg_speichern(id, &s).unwrap();
        let (_, s2) = db.sg_laden(id).unwrap();
        assert_eq!(s2.name, "Bordnetz J519");
        assert_eq!(s2.fehler.len(), 2);
        assert_eq!(s2.fehler[1].code, "B2");
        db.fehler_allgemein_speichern(e.quelle_id, &[]).unwrap();
        assert!(db.inhalt(e.quelle_id).unwrap().fehler.is_empty());
        assert!(db.sg_entfernen(id).unwrap());
        assert!(db.inhalt(e.quelle_id).unwrap().steuergeraete.is_empty());
        assert!(i.zusammenfassung().contains("1 Steuergeräte"));
    }

    #[test]
    fn can_signale_aendern() {
        let db = Db::im_speicher().unwrap();
        let dbc = crate::dbc::Dbc::lesen("BO_ 640 Motor_1: 8 XXX\n SG_ Drehzahl : 16|16@1+ (0.25,0) [0|16256] \"U/min\" XXX\n");
        let e = db.dbc_importieren(&dbc, "t.dbc", "", "", None).unwrap();
        let mut b = db.inhalt(e.quelle_id).unwrap().can.remove(0);
        assert_eq!((b.can_id, b.signale[0].faktor), (640, 0.25));
        b.name = "Motor eins".into();
        b.signale[0].einheit = "1/min".into();
        db.can_speichern(&b).unwrap();
        let b2 = db.inhalt(e.quelle_id).unwrap().can.remove(0);
        assert_eq!((b2.name.as_str(), b2.signale[0].einheit.as_str()), ("Motor eins", "1/min"));
    }
}
