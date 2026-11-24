//! Belegte Diagnose-Referenzdaten aus Datenpaketen.

use super::paket::PaketFehler;
use super::Db;
use anyhow::{ensure, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DiagnoseReferenz {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sollbereiche: Vec<Sollbereich>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub messwertfelder: Vec<Messwertfeld>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub uds_anpassungen: Vec<UdsAnpassung>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub asam: Vec<AsamZuordnung>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Sollbereich {
    pub block: u16,
    pub feld: u8,
    pub name: String,
    pub min: f64,
    pub max: f64,
    pub einheit: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub bedingung: String,
    #[serde(default)]
    pub im_abbild: bool,
    pub labeldatei: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Messwertfeld {
    pub block: u16,
    pub feld: u8,
    pub name: String,
    #[serde(default)]
    pub im_abbild: bool,
    pub labeldatei: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct UdsAnpassung {
    pub kennung: String,
    pub name: String,
    pub asam: String,
    pub version: String,
    pub rod: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AsamZuordnung {
    pub name: String,
    pub version: String,
    pub rod: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Fehlerkombination {
    pub code: String,
    pub fehlerart: u8,
    pub gruppen: u32,
}

#[derive(Debug, Default, PartialEq)]
pub struct Fehlerreferenz {
    pub text: Option<String>,
    pub alternativen: Vec<String>,
    pub kombinationen: Vec<Fehlerkombination>,
}

impl Fehlerreferenz {
    pub fn zeilen(&self) -> Vec<String> {
        let mut zeilen = Vec::new();
        if let Some(text) = &self.text {
            zeilen.push(text.clone());
        }
        zeilen.extend(self.alternativen.iter().map(|s| crate::tr!("Alternativtext: {a}", a = s)));
        zeilen.extend(self.kombinationen.iter().map(|k| {
            let text = crate::dtc_db::eigene_fehlerart("UDS", k.fehlerart.into()).or_else(|| crate::dtc_db::fehlerart_text(k.fehlerart).map(str::to_string));
            let art = match text {
                Some(t) => format!("{:02X} ({t})", k.fehlerart),
                None => format!("{:02X}", k.fehlerart),
            };
            if k.gruppen == 1 {
                crate::tr!("{c} · Fehlerart {a} · 1 Kataloggruppe", c = &k.code, a = art)
            } else {
                crate::tr!("{c} · Fehlerart {a} · {n} Kataloggruppen", c = &k.code, a = art, n = k.gruppen)
            }
        }));
        if zeilen.is_empty() {
            zeilen.push(crate::tr!("Kein passender Katalogeintrag."));
        }
        zeilen
    }
}

impl DiagnoseReferenz {
    pub fn ist_leer(&self) -> bool {
        self.anzahl() == 0
    }

    pub fn anzahl(&self) -> usize {
        self.sollbereiche.len() + self.messwertfelder.len() + self.uds_anpassungen.len() + self.asam.len()
    }

    pub fn zeilen(&self) -> Vec<String> {
        let mut zeilen = Vec::with_capacity(self.anzahl());
        zeilen.extend(self.sollbereiche.iter().map(|s| {
            let kontext = if s.bedingung.is_empty() { String::new() } else { format!(" ({})", s.bedingung) };
            crate::tr!("Sollwert Block {b}.{f} · {n} · {lo}–{hi} {u}{k}",
                b = s.block, f = s.feld, n = &s.name, lo = s.min, hi = s.max, u = &s.einheit, k = kontext)
        }));
        zeilen.extend(self.messwertfelder.iter().map(|m| crate::tr!("Messwertblock {b}.{f} · {n}", b = m.block, f = m.feld, n = &m.name)));
        zeilen.extend(self.uds_anpassungen.iter().map(|a| crate::tr!("UDS-Anpassung {k} · {n} ({v})", k = &a.kennung, n = &a.name, v = &a.version)));
        zeilen.extend(self.asam.iter().map(|a| format!("ASAM {} {} · {}", a.name, a.version, a.rod)));
        zeilen
    }
}

pub(super) fn ecu_eintragen(con: &Connection, ecu: i64, daten: &DiagnoseReferenz) -> Result<()> {
    for s in &daten.sollbereiche {
        ensure!(s.min.is_finite() && s.max.is_finite() && s.min <= s.max && (1..=4).contains(&s.feld),
            crate::tr!("Ungültiger Referenz-Sollbereich für Block {b}.", b = s.block));
    }
    ensure!(daten.messwertfelder.iter().all(|m| (1..=4).contains(&m.feld)),
        crate::tr!("Ungültiges Feld bei einer Messwertblock-Referenz."));
    if daten.ist_leer() {
        con.execute("DELETE FROM ecu_diagnostic_reference WHERE ecu_id = ?1", [ecu])?;
    } else {
        con.execute(
            "INSERT INTO ecu_diagnostic_reference (ecu_id, content) VALUES (?1, ?2)
             ON CONFLICT (ecu_id) DO UPDATE SET content = excluded.content",
            params![ecu, serde_json::to_string(daten)?],
        )?;
    }
    Ok(())
}

pub(super) fn ecu_laden(con: &Connection, ecu: i64) -> Result<DiagnoseReferenz> {
    let text: Option<String> = con.query_row("SELECT content FROM ecu_diagnostic_reference WHERE ecu_id = ?1", [ecu], |r| r.get(0)).optional()?;
    text.map(|s| serde_json::from_str(&s).map_err(Into::into)).transpose().map(Option::unwrap_or_default)
}

pub(super) fn fehler_eintragen(con: &Connection, quelle: i64, alternativen: &[PaketFehler], kombinationen: &[Fehlerkombination]) -> Result<()> {
    let mut a = con.prepare("INSERT INTO dtc_alternative VALUES (?1, ?2, ?3)")?;
    for f in alternativen {
        ensure!(!f.code.trim().is_empty() && !f.text.trim().is_empty(), crate::tr!("Leerer Code oder Alternativtext."));
        a.execute(params![quelle, f.code.trim().to_ascii_uppercase(), f.text.trim()])?;
    }
    let mut k = con.prepare("INSERT INTO dtc_combination VALUES (?1, ?2, ?3, ?4)")?;
    for f in kombinationen {
        let code = f.code.trim().to_ascii_uppercase();
        let bytes = code.as_bytes();
        ensure!(bytes.len() == 5 && b"PCBU".contains(&bytes[0]) && (b'0'..=b'3').contains(&bytes[1])
            && bytes[2..].iter().all(u8::is_ascii_hexdigit), crate::tr!("Ungültiger SAE-Code {a}.", a = &code));
        k.execute(params![quelle, code, f.fehlerart, f.gruppen])?;
    }
    Ok(())
}

impl Db {
    pub(super) fn diagnose_fehlertabellen(&self, quelle: i64) -> Result<(Vec<PaketFehler>, Vec<Fehlerkombination>)> {
        let mut a = self.con.prepare("SELECT code, text FROM dtc_alternative WHERE source_id = ?1 ORDER BY code, text")?;
        let alternativen = a.query_map([quelle], |r| Ok(PaketFehler { code: r.get(0)?, text: r.get(1)? }))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut k = self.con.prepare("SELECT code, failure_type, groups_count FROM dtc_combination WHERE source_id = ?1 ORDER BY code, failure_type")?;
        let kombinationen = k.query_map([quelle], |r| Ok(Fehlerkombination { code: r.get(0)?, fehlerart: r.get(1)?, gruppen: r.get(2)? }))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok((alternativen, kombinationen))
    }

    /// Nach Code und optional genau der gemeldeten UDS-Fehlerart suchen.
    pub fn fehlerreferenz(&self, code: &str, art: Option<u8>) -> Result<Fehlerreferenz> {
        let code = code.trim().to_ascii_uppercase();
        let text = self.fehlertext(&code)?;
        let mut a = self.con.prepare("SELECT DISTINCT text FROM dtc_alternative WHERE code = ?1 ORDER BY text")?;
        let alternativen = a.query_map([&code], |r| r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?;
        let mut k = self.con.prepare(
            "SELECT failure_type, MAX(groups_count) FROM dtc_combination WHERE code = ?1 AND (?2 IS NULL OR failure_type = ?2)
             GROUP BY failure_type ORDER BY failure_type"
        )?;
        let kombinationen = k.query_map(params![&code, art], |r| {
            Ok(Fehlerkombination { code: code.clone(), fehlerart: r.get(0)?, gruppen: r.get(1)? })
        })?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(Fehlerreferenz { text, alternativen, kombinationen })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{paket::Datenpaket, QuellArt};

    #[test]
    fn diagnose_referenzen_ersetzen_und_lokal_halten() {
        let p = Datenpaket::aus_text(r#"
fehler = [{ code = "P0300", text = "Zündaussetzer" }]
fehleralternative = [{ code = "P0300", text = "Verbrennungsaussetzer" }]
fehlerkombination = [{ code = "P0300", fehlerart = 0x13, gruppen = 2 }]
[quelle]
name = "Lokale Testdaten"
nur_lokal = true
[[hersteller]]
name = "Volkswagen"
[[hersteller.steuergeraet]]
adresse = 1
name = "Motor"
teilenummer = "AAA111222B"
[hersteller.steuergeraet.diagnosereferenz]
sollbereiche = [{ block = 1, feld = 1, name = "Drehzahl", min = 740.0, max = 840.0, einheit = "U/min", bedingung = "Leerlauf", labeldatei = "test" }]
messwertfelder = [{ block = 1, feld = 1, name = "Drehzahl", labeldatei = "test" }]
uds_anpassungen = [{ kennung = "IDE00001", name = "Testanpassung", asam = "EV_TEST", version = "001", rod = "test.rod" }]
asam = [{ name = "EV_TEST", version = "001", rod = "test.rod" }]
"#, false).unwrap();
        let db = Db::im_speicher().unwrap();
        for _ in 0..2 {
            let e = db.paket_importieren(&p, QuellArt::User).unwrap();
            assert_eq!((e.diagnosereferenzen, e.fehleralternativen, e.fehlerkombinationen), (4, 1, 1));
        }
        let q = db.quellen().unwrap();
        assert_eq!(q.len(), 1);
        assert!(!q[0].ist_teilbar());
        let i = db.inhalt(q[0].id).unwrap();
        let (id, _, sg) = &i.steuergeraete[0];
        assert_eq!(sg.diagnosereferenz, p.hersteller[0].steuergeraete[0].diagnosereferenz);
        db.sg_speichern(*id, sg).unwrap();
        assert_eq!(db.sg_laden(*id).unwrap().1.diagnosereferenz, sg.diagnosereferenz);
        assert!(db.wissen_zur_teilenummer("AAA111222C").unwrap().is_none());
        let refenz = db.fehlerreferenz("p0300", Some(0x13)).unwrap();
        assert_eq!(refenz.text.as_deref(), Some("Zündaussetzer"));
        assert_eq!(refenz.alternativen, ["Verbrennungsaussetzer"]);
        assert_eq!(refenz.kombinationen, p.fehlerkombination);
        assert!(db.fehlerreferenz("P0300", Some(0x11)).unwrap().kombinationen.is_empty());
        let (export, ausgelassen) = db.paket_exportieren("Test", "CC0-1.0").unwrap();
        assert_eq!(ausgelassen.len(), 1);
        assert!(export.fehleralternative.is_empty() && export.fehlerkombination.is_empty() && export.hersteller.is_empty());
        db.quelle_entfernen(q[0].id).unwrap();
        assert_eq!(db.fehlerreferenz("P0300", None).unwrap(), Fehlerreferenz::default());
    }
}
