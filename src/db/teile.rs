//! Wissen zu einem Steuergerät über seine Teilenummer: Anpassungskanal-Namen,
//! Codier-Bits, Messwerte (DIDs) und Fehlertexte aus ALLEN Quellen der
//! Fahrzeug-Datenbank.
//!
//! Warum Teilenummer statt Adresse: Bei TP 2.0 sind die Kanal-Adressen nicht
//! die gewohnten Diagnoseadressen (Golf 6: Gateway 0x1F statt 0x19, Bordnetz
//! 0x20 statt 0x09). Die Teilenummer („5K0937086“) steht in der Kennung jedes
//! Steuergeräts und passt immer.

use super::paket::{PaketCodierung, PaketDid, PaketFehler};
use super::Db;
use anyhow::Result;
use rusqlite::params;
use std::collections::BTreeMap;

/// Was die Datenbank zu einer Teilenummer weiß (alle passenden Quellen zusammen).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SgWissen {
    /// Teilenummer, wie sie in der Datenbank steht
    pub teilenummer: String,
    pub name: String,
    /// Kanal → Name
    pub anpassung: BTreeMap<u16, String>,
    pub codierung: Vec<PaketCodierung>,
    pub dids: Vec<PaketDid>,
    pub fehler: Vec<PaketFehler>,
}

impl SgWissen {
    pub fn ist_leer(&self) -> bool {
        self.anpassung.is_empty() && self.codierung.is_empty() && self.dids.is_empty() && self.fehler.is_empty()
    }

    /// Name eines Anpassungskanals, falls bekannt.
    pub fn kanal(&self, kanal: u16) -> Option<&str> {
        self.anpassung.get(&kanal).map(String::as_str)
    }

    /// Kurzbeschreibung: „5K0937086: 12 Anpassungskanäle, 3 Codier-Bits“.
    pub fn zusammenfassung(&self) -> String {
        let mut t = Vec::new();
        for (n, w) in [
            (self.anpassung.len(), crate::tr!("Anpassungskanäle")),
            (self.codierung.len(), crate::tr!("Codier-Bits")),
            (self.dids.len(), crate::tr!("Messwerte")),
            (self.fehler.len(), crate::tr!("Fehlertexte")),
        ] {
            if n > 0 {
                t.push(format!("{n} {w}"));
            }
        }
        format!("{}: {}", self.teilenummer, t.join(", "))
    }
}

/// Teilenummer vergleichbar machen: ohne Softwarestand und Leerzeichen, groß
/// („1K0907530AA  0081“ → „1K0907530AA“, „5K0 937 086 B“ → „5K0937086B“).
pub fn norm(t: &str) -> String {
    let gross = |x: &str| -> String { x.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_uppercase()).collect() };
    let teile: Vec<&str> = t.split_whitespace().collect();
    let Some(erstes) = teile.first() else { return String::new() };
    // ungruppiert („1K0907530AA 0081“): erster Block ist die Teilenummer
    if gross(erstes).len() >= 9 {
        return gross(erstes);
    }
    // gruppiert („5K0 937 086 B 0019“): drei Dreiergruppen, dann evtl. Index-Buchstaben
    let mut s: String = teile.iter().take(3).map(|x| gross(x)).collect();
    if let Some(z) = teile.get(3).filter(|z| z.len() <= 3 && z.chars().all(|c| c.is_ascii_alphabetic())) {
        s += &gross(z);
    }
    s
}

/// Platzhalter ohne Information („Kanal 4“, „Kanal 004“).
pub fn ist_platzhalter(name: &str) -> bool {
    let n = name.trim();
    n.is_empty() || n.strip_prefix("Kanal").is_some_and(|r| !r.trim().is_empty() && r.trim().chars().all(|c| c.is_ascii_digit()))
}

/// Wie gut passt `gespeichert` zu `gesucht`? 3 = gleich, 2 = einer beginnt mit
/// dem anderen (fehlender Index-Buchstabe), 1 = gleiche Grundnummer (9 Zeichen).
fn passt(gesucht: &str, gespeichert: &str) -> u8 {
    if gesucht.len() < 9 || gespeichert.len() < 9 {
        return 0;
    }
    if gesucht == gespeichert {
        3
    } else if gesucht.starts_with(gespeichert) || gespeichert.starts_with(gesucht) {
        2
    } else if gesucht[..9] == gespeichert[..9] {
        1
    } else {
        0
    }
}

impl Db {
    /// Alles zu einer Teilenummer aus allen Quellen. Es zählen nur die am
    /// besten passenden Einträge (genau gleich vor „ohne Index“ vor „gleiche
    /// Grundnummer“); bei gleichem Kanal gewinnt die neuere Quelle.
    pub fn wissen_zur_teilenummer(&self, teilenummer: &str) -> Result<Option<SgWissen>> {
        let gesucht = norm(teilenummer);
        if gesucht.len() < 9 {
            return Ok(None);
        }
        let mut st = self.con.prepare("SELECT id, software_number, ecu_name FROM ecu WHERE software_number <> '' ORDER BY source_id, id")?;
        let alle: Vec<(i64, String, String)> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<rusqlite::Result<_>>()?;
        let bewertet: Vec<(u8, i64, String, String)> = alle.into_iter().map(|(id, t, n)| (passt(&gesucht, &norm(&t)), id, t, n)).filter(|(p, ..)| *p > 0).collect();
        let Some(beste) = bewertet.iter().map(|(p, ..)| *p).max() else { return Ok(None) };
        let mut w = SgWissen::default();
        let mut an = self.con.prepare("SELECT channel, name FROM adaptation WHERE ecu_id = ?1 ORDER BY id")?;
        let mut co = self.con.prepare("SELECT byte_index, bit_index, label, requires, excludes FROM coding_item WHERE ecu_id = ?1 ORDER BY byte_index, bit_index")?;
        let mut di = self.con.prepare("SELECT did, name, unit, factor, \"offset\" FROM did WHERE ecu_id = ?1 ORDER BY did")?;
        let mut fe = self.con.prepare("SELECT dtc_code, title FROM dtc WHERE ecu_id = ?1 ORDER BY id")?;
        let teilen = |s: String| -> Vec<String> { s.split(',').map(str::trim).filter(|t| !t.is_empty()).map(str::to_string).collect() };
        for (_, id, teil, name) in bewertet.into_iter().filter(|(p, ..)| *p == beste) {
            w.teilenummer = norm(&teil);
            if !name.trim().is_empty() {
                w.name = name;
            }
            for r in an.query_map(params![id], |r| Ok((r.get::<_, u16>(0)?, r.get::<_, String>(1)?)))? {
                let (k, n) = r?;
                if !ist_platzhalter(&n) {
                    w.anpassung.insert(k, n);
                }
            }
            for r in co.query_map(params![id], |r| {
                Ok(PaketCodierung { byte: r.get(0)?, bit: r.get(1)?, name: r.get(2)?, benoetigt: teilen(r.get(3)?), schliesst_aus: teilen(r.get(4)?) })
            })? {
                let c = r?;
                w.codierung.retain(|x| (x.byte, x.bit) != (c.byte, c.bit));
                w.codierung.push(c);
            }
            for r in di.query_map(params![id], |r| Ok(PaketDid { did: r.get(0)?, name: r.get(1)?, einheit: r.get(2)?, faktor: r.get(3)?, offset: r.get(4)? }))? {
                let d = r?;
                w.dids.retain(|x| x.did != d.did);
                w.dids.push(d);
            }
            for r in fe.query_map(params![id], |r| Ok(PaketFehler { code: r.get(0)?, text: r.get(1)? }))? {
                let f = r?;
                w.fehler.retain(|x| x.code != f.code);
                w.fehler.push(f);
            }
        }
        w.codierung.sort_by_key(|c| (c.byte, c.bit));
        w.dids.sort_by_key(|d| d.did);
        Ok((!w.ist_leer()).then_some(w))
    }

    /// Anpassungskanäle mit Platzhalter-Namen („Kanal 4“) löschen — sie sagen
    /// nichts, die Nummer zeigt das Programm ohnehin. Gibt die Anzahl zurück.
    pub fn platzhalter_entfernen(&self) -> Result<usize> {
        let mut st = self.con.prepare("SELECT id, name FROM adaptation")?;
        let weg: Vec<i64> = st
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
            .filter_map(|r| r.ok())
            .filter(|(_, n)| ist_platzhalter(n))
            .map(|(id, _)| id)
            .collect();
        let tx = self.con.unchecked_transaction()?;
        for id in &weg {
            tx.execute("DELETE FROM adaptation WHERE id = ?1", params![id])?;
        }
        tx.commit()?;
        Ok(weg.len())
    }
}

/// Wissen zu einer Teilenummer aus der Nutzer-Datenbank (nur lesend, eigene
/// Verbindung — darf auch aus Hintergrund-Arbeiten aufgerufen werden).
pub fn wissen_falls_vorhanden(teilenummer: &str) -> Option<SgWissen> {
    let p = super::standard_pfad();
    if !p.exists() {
        return None;
    }
    Db::nur_lesen(&p).ok()?.wissen_zur_teilenummer(teilenummer).ok().flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::paket::Datenpaket;
    use crate::db::QuellArt;

    #[test]
    fn normalisieren_und_platzhalter() {
        assert_eq!(norm("1K0907530AA  0081"), "1K0907530AA");
        assert_eq!(norm("5K0 937 086"), "5K0937086");
        assert_eq!(norm("5K0 937 086 0019"), "5K0937086");
        assert_eq!(norm("5k0937086"), "5K0937086");
        assert_eq!(norm("1K0907379CD 0120"), "1K0907379CD");
        assert_eq!(norm("5K0 937 086 B 0019"), "5K0937086B");
        assert!(ist_platzhalter("Kanal 4"));
        assert!(ist_platzhalter("Kanal 004"));
        assert!(!ist_platzhalter("Kanal für Standheizung"));
        assert!(!ist_platzhalter("Komfortblinken"));
    }

    #[test]
    fn findet_nach_teilenummer_ueber_quellen() {
        let db = Db::im_speicher().unwrap();
        let p = |name: &str, teil: &str, rest: &str| {
            Datenpaket::aus_text(
                &format!("[quelle]\nname = \"{name}\"\n[[hersteller]]\nname = \"Volkswagen\"\n[[hersteller.steuergeraet]]\nadresse = 0x09\nname = \"Bordnetz\"\nteilenummer = \"{teil}\"\n{rest}"),
                false,
            )
            .unwrap()
        };
        db.paket_importieren(&p("A", "5K0937086", "anpassung = [{ kanal = 7, name = \"Standheizung\" }, { kanal = 4, name = \"Kanal 4\" }]\n"), QuellArt::User).unwrap();
        db.paket_importieren(&p("B", "5K0937086  0019", "anpassung = [{ kanal = 7, name = \"Standheizung aus\" }]\ncodierung = [{ byte = 0, bit = 1, name = \"TFL\" }]\n"), QuellArt::User).unwrap();
        db.paket_importieren(&p("C", "5K0937086B", "anpassung = [{ kanal = 9, name = \"nur B-Index\" }]\n"), QuellArt::User).unwrap();
        db.paket_importieren(&p("D", "1K0907379", "anpassung = [{ kanal = 1, name = \"ABS\" }]\n"), QuellArt::User).unwrap();
        let w = db.wissen_zur_teilenummer("5K0937086  0019").unwrap().unwrap();
        // genau passend: A und B; C (anderer Index) nur, wenn nichts Besseres da ist
        assert_eq!(w.kanal(7), Some("Standheizung aus"), "neuere Quelle gewinnt");
        assert_eq!(w.kanal(4), None, "Platzhalter zählt nicht");
        assert_eq!(w.kanal(9), None);
        assert_eq!(w.codierung.len(), 1);
        let w = db.wissen_zur_teilenummer("5K0937086B").unwrap().unwrap();
        assert_eq!(w.kanal(9), Some("nur B-Index"));
        let w = db.wissen_zur_teilenummer("5K0937086C").unwrap().unwrap();
        assert!(w.kanal(7).is_some(), "ohne genauen Treffer: gleiche Grundnummer");
        assert!(db.wissen_zur_teilenummer("6R0937085").unwrap().is_none());
        assert!(db.wissen_zur_teilenummer("").unwrap().is_none());
        // Platzhalter werden schon beim Import übersprungen …
        assert_eq!(db.platzhalter_entfernen().unwrap(), 0);
        // … ältere Einträge räumt „Aufräumen“ weg
        let q = db.quellen().unwrap()[0].id;
        let e: i64 = db.con.query_row("SELECT id FROM ecu LIMIT 1", [], |r| r.get(0)).unwrap();
        db.con.execute("INSERT INTO adaptation (ecu_id, channel, name, source_id) VALUES (?1, 5, 'Kanal 5', ?2)", params![e, q]).unwrap();
        assert_eq!(db.platzhalter_entfernen().unwrap(), 1);
    }
}
