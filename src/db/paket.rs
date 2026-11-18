//! Datenpaket: gemeinsames Austauschformat der Fahrzeug-Datenbank (TOML oder
//! JSON). Dient für Community-Daten im Repo, eigene Sammlungen und den Export.
//!
//! ```toml
//! [quelle]
//! name = "VW Adressliste"
//! version = "1"
//! lizenz = "CC0-1.0"
//! herkunft = "eigene Bezeichnungen"
//!
//! [[hersteller]]
//! name = "Volkswagen"
//! wmi = ["WVW", "WV1"]
//!
//! [[hersteller.steuergeraet]]
//! adresse = 0x09
//! name = "Bordnetz"
//! fehler = [{ code = "B1234", text = "…" }]
//! ```
//!
//! Export nimmt nur Quellen, die [`super::darf_weitergeben`] erlaubt.

use super::{Db, ImportErgebnis, QuellArt};
use anyhow::{bail, Context, Result};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Datenpaket {
    pub quelle: PaketQuelle,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hersteller: Vec<PaketHersteller>,
    /// Fehlertexte ohne festes Steuergerät (z. B. genormte P-Codes)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fehler: Vec<PaketFehler>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PaketQuelle {
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub lizenz: String,
    #[serde(default)]
    pub herkunft: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PaketHersteller {
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub wmi: Vec<String>,
    #[serde(default, rename = "steuergeraet", skip_serializing_if = "Vec::is_empty")]
    pub steuergeraete: Vec<PaketSteuergeraet>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PaketSteuergeraet {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adresse: Option<u16>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub gruppe: String,
    /// UDS | TP20 | KWP1281 | OBD
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub protokoll: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tx: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rx: Option<u16>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub teilenummer: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fehler: Vec<PaketFehler>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dids: Vec<PaketDid>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub codierung: Vec<PaketCodierung>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub anpassung: Vec<PaketAnpassung>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PaketFehler {
    pub code: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PaketDid {
    pub did: u16,
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub einheit: String,
    #[serde(default = "eins")]
    pub faktor: f64,
    #[serde(default)]
    pub offset: f64,
}

fn eins() -> f64 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PaketCodierung {
    pub byte: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bit: Option<u8>,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PaketAnpassung {
    pub kanal: u16,
    pub name: String,
}

impl Datenpaket {
    /// Aus TOML oder JSON lesen (nach der Dateiendung, sonst nach dem Inhalt).
    pub fn lesen(pfad: &Path) -> Result<Datenpaket> {
        let text = std::fs::read_to_string(pfad).with_context(|| crate::tr!("{a} lesen", a = pfad.display()))?;
        let json = pfad.extension().is_some_and(|e| e.eq_ignore_ascii_case("json")) || text.trim_start().starts_with('{');
        Self::aus_text(&text, json).with_context(|| crate::tr!("{a} ist kein gültiges Datenpaket", a = pfad.display()))
    }

    pub fn aus_text(text: &str, json: bool) -> Result<Datenpaket> {
        let p: Datenpaket = if json { serde_json::from_str(text)? } else { toml::from_str(text)? };
        if p.quelle.name.trim().is_empty() {
            bail!(crate::tr!("[quelle] name fehlt"));
        }
        Ok(p)
    }

    /// Als TOML oder JSON schreiben (nach der Endung).
    pub fn schreiben(&self, pfad: &Path) -> Result<()> {
        let text = if pfad.extension().is_some_and(|e| e.eq_ignore_ascii_case("json")) { serde_json::to_string_pretty(self)? } else { toml::to_string(self)? };
        std::fs::write(pfad, text).with_context(|| crate::tr!("{a} schreiben", a = pfad.display()))
    }
}

impl Db {
    /// Datenpaket als eigene Quelle übernehmen. Eine frühere Quelle mit
    /// gleichem Namen und gleicher Art wird ersetzt (Update).
    pub fn paket_importieren(&self, p: &Datenpaket, art: QuellArt) -> Result<ImportErgebnis> {
        if matches!(art, QuellArt::Odx | QuellArt::Pdx | QuellArt::Dbc) {
            bail!(crate::tr!("Datenpakete sind COMMUNITY, USER oder MANUAL"));
        }
        let tx = self.con.unchecked_transaction()?;
        tx.execute("DELETE FROM data_source WHERE source_type = ?1 AND source_name = ?2", params![art.text(), p.quelle.name])?;
        let q = self.quelle_anlegen(art, &p.quelle.name, &p.quelle.version, &p.quelle.lizenz, &p.quelle.herkunft)?;
        let mut erg = ImportErgebnis { quelle_id: q, ..Default::default() };
        for f in &p.fehler {
            tx.execute("INSERT INTO dtc (dtc_code, title, source_id) VALUES (?1, ?2, ?3)", params![norm_code(&f.code), f.text, q])?;
            erg.fehler += 1;
        }
        for h in &p.hersteller {
            let wmi: Vec<&str> = h.wmi.iter().map(String::as_str).collect();
            let hid = self.hersteller(&h.name, &wmi, Some(q))?;
            for s in &h.steuergeraete {
                tx.execute(
                    "INSERT INTO ecu (manufacturer_id, ecu_name, ecu_family, software_number, protocol, diagnostic_address, can_tx, can_rx, source_id)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    params![hid, s.name, s.gruppe, s.teilenummer, s.protokoll, s.adresse, s.tx, s.rx, q],
                )?;
                let ecu = tx.last_insert_rowid();
                erg.steuergeraete += 1;
                for f in &s.fehler {
                    tx.execute("INSERT INTO dtc (ecu_id, dtc_code, title, source_id) VALUES (?1, ?2, ?3, ?4)", params![ecu, norm_code(&f.code), f.text, q])?;
                    erg.fehler += 1;
                }
                for d in &s.dids {
                    tx.execute(
                        "INSERT INTO did (ecu_id, did, name, unit, factor, \"offset\", source_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                        params![ecu, d.did, d.name, d.einheit, d.faktor, d.offset, q],
                    )?;
                    erg.messwerte += 1;
                }
                for c in &s.codierung {
                    tx.execute("INSERT INTO coding_item (ecu_id, byte_index, bit_index, label, source_id) VALUES (?1, ?2, ?3, ?4, ?5)", params![ecu, c.byte, c.bit, c.name, q])?;
                    erg.codierung += 1;
                }
                for a in &s.anpassung {
                    tx.execute("INSERT INTO adaptation (ecu_id, channel, name, source_id) VALUES (?1, ?2, ?3, ?4)", params![ecu, a.kanal, a.name, q])?;
                    erg.anpassung += 1;
                }
            }
        }
        tx.commit()?;
        Ok(erg)
    }

    /// CSV mit Fehlertexten (`code;text`) oder DIDs (`did;name;einheit;faktor;offset`)
    /// als Quelle „USER“ übernehmen. Trennzeichen `;` oder `,`; erste Zeile darf eine Kopfzeile sein.
    pub fn csv_importieren(&self, pfad: &Path, lizenz: &str) -> Result<ImportErgebnis> {
        let text = std::fs::read_to_string(pfad).with_context(|| crate::tr!("{a} lesen", a = pfad.display()))?;
        let name = pfad.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        self.csv_text_importieren(&text, &name, lizenz)
    }

    pub fn csv_text_importieren(&self, text: &str, name: &str, lizenz: &str) -> Result<ImportErgebnis> {
        let trenner = if text.lines().next().is_some_and(|z| z.contains(';')) { ';' } else { ',' };
        let zeilen: Vec<Vec<String>> = text
            .lines()
            .filter(|z| !z.trim().is_empty() && !z.trim_start().starts_with('#'))
            .map(|z| z.split(trenner).map(|f| f.trim().trim_matches('"').to_string()).collect())
            .collect();
        let Some(erste) = zeilen.first() else { bail!(crate::tr!("CSV ist leer")) };
        let kopf = erste.first().is_some_and(|f| matches!(f.to_ascii_lowercase().as_str(), "code" | "did" | "fehler"));
        let daten = &zeilen[usize::from(kopf)..];
        let did_art = daten.first().is_some_and(|z| z.len() >= 3 || z.first().is_some_and(|f| f.starts_with("0x")));
        let tx = self.con.unchecked_transaction()?;
        let q = self.quelle_anlegen(QuellArt::User, name, "", lizenz, name)?;
        let mut erg = ImportErgebnis { quelle_id: q, ..Default::default() };
        for (i, z) in daten.iter().enumerate() {
            let feld = |n: usize| z.get(n).map(String::as_str).unwrap_or("");
            if did_art {
                let did = parse_zahl(feld(0)).with_context(|| crate::tr!("Zeile {a}: DID", a = i + 1 + usize::from(kopf)))?;
                let zahl = |n: usize, std: f64| feld(n).replace(',', ".").parse().unwrap_or(std);
                tx.execute(
                    "INSERT INTO did (did, name, unit, factor, \"offset\", source_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![did, feld(1), feld(2), zahl(3, 1.0), zahl(4, 0.0), q],
                )?;
                erg.messwerte += 1;
            } else {
                if feld(0).is_empty() || feld(1).is_empty() {
                    continue;
                }
                tx.execute("INSERT INTO dtc (dtc_code, title, source_id) VALUES (?1, ?2, ?3)", params![norm_code(feld(0)), feld(1), q])?;
                erg.fehler += 1;
            }
        }
        tx.commit()?;
        Ok(erg)
    }

    /// Alle weitergebbaren Daten als Datenpaket (für Beiträge zur Community).
    /// ODX/PDX und Quellen ohne Lizenz bleiben draußen.
    pub fn paket_exportieren(&self, name: &str, lizenz: &str) -> Result<(Datenpaket, Vec<String>)> {
        let mut weggelassen = Vec::new();
        let mut erlaubt = Vec::new();
        for q in self.quellen()? {
            if super::darf_weitergeben(q.art, &q.lizenz) {
                erlaubt.push(q.id);
            } else {
                weggelassen.push(format!("{} „{}“", q.art.text(), q.name));
            }
        }
        let mut p = Datenpaket { quelle: PaketQuelle { name: name.into(), version: String::new(), lizenz: lizenz.into(), herkunft: crate::tr!("Export aus ECUWerkbank") }, ..Default::default() };
        let mut hst = self.con.prepare("SELECT id, name, vin_wmi FROM manufacturer ORDER BY name")?;
        let hersteller: Vec<(i64, String, String)> = hst.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<rusqlite::Result<_>>()?;
        let mut est = self.con.prepare(
            "SELECT id, ecu_name, ecu_family, protocol, diagnostic_address, can_tx, can_rx, software_number, source_id FROM ecu WHERE manufacturer_id = ?1 ORDER BY diagnostic_address, id",
        )?;
        for (hid, hname, wmi) in hersteller {
            let mut h = PaketHersteller { name: hname, wmi: wmi.split(',').filter(|w| !w.is_empty()).map(str::to_string).collect(), steuergeraete: Vec::new() };
            let ecus: Vec<(i64, PaketSteuergeraet, i64)> = est
                .query_map(params![hid], |r| {
                    Ok((
                        r.get(0)?,
                        PaketSteuergeraet { name: r.get(1)?, gruppe: r.get(2)?, protokoll: r.get(3)?, adresse: r.get(4)?, tx: r.get(5)?, rx: r.get(6)?, teilenummer: r.get(7)?, ..Default::default() },
                        r.get(8)?,
                    ))
                })?
                .collect::<rusqlite::Result<_>>()?;
            for (eid, mut s, q) in ecus {
                if !erlaubt.contains(&q) {
                    continue;
                }
                s.fehler = self.kinder(eid, "SELECT dtc_code, title FROM dtc WHERE ecu_id = ?1 ORDER BY id", |r| Ok(PaketFehler { code: r.get(0)?, text: r.get(1)? }))?;
                s.dids = self.kinder(eid, "SELECT did, name, unit, factor, \"offset\" FROM did WHERE ecu_id = ?1 ORDER BY did", |r| {
                    Ok(PaketDid { did: r.get(0)?, name: r.get(1)?, einheit: r.get(2)?, faktor: r.get(3)?, offset: r.get(4)? })
                })?;
                s.codierung = self.kinder(eid, "SELECT byte_index, bit_index, label FROM coding_item WHERE ecu_id = ?1 ORDER BY byte_index, bit_index", |r| {
                    Ok(PaketCodierung { byte: r.get(0)?, bit: r.get(1)?, name: r.get(2)? })
                })?;
                s.anpassung = self.kinder(eid, "SELECT channel, name FROM adaptation WHERE ecu_id = ?1 ORDER BY channel", |r| Ok(PaketAnpassung { kanal: r.get(0)?, name: r.get(1)? }))?;
                h.steuergeraete.push(s);
            }
            if !h.steuergeraete.is_empty() {
                p.hersteller.push(h);
            }
        }
        let mut fst = self.con.prepare("SELECT dtc_code, title, source_id FROM dtc WHERE ecu_id IS NULL ORDER BY dtc_code")?;
        for r in fst.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)))? {
            let (code, text, q) = r?;
            if erlaubt.contains(&q) {
                p.fehler.push(PaketFehler { code, text });
            }
        }
        Ok((p, weggelassen))
    }

    fn kinder<T>(&self, ecu: i64, sql: &str, f: impl FnMut(&rusqlite::Row) -> rusqlite::Result<T>) -> Result<Vec<T>> {
        let mut st = self.con.prepare(sql)?;
        let v = st.query_map(params![ecu], f)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    }

    /// Mitgelieferte Community-Daten (im Programm eingebaut, aus `daten/community/`)
    /// übernehmen bzw. auf den Stand dieses Programms bringen.
    pub fn community_uebernehmen(&self) -> Result<Vec<(String, ImportErgebnis)>> {
        let mut out = Vec::new();
        for (datei, text) in COMMUNITY {
            let p = Datenpaket::aus_text(text, datei.ends_with(".json")).with_context(|| crate::tr!("Community-Datei {a}", a = datei))?;
            let e = self.paket_importieren(&p, QuellArt::Community)?;
            out.push((p.quelle.name, e));
        }
        Ok(out)
    }
}

/// Community-Daten, die mit dem Programm ausgeliefert werden (offen lizenziert).
pub const COMMUNITY: &[(&str, &str)] = &[("vw/adressliste.toml", include_str!("../../daten/community/vw/adressliste.toml"))];

fn norm_code(c: &str) -> String {
    c.trim().to_ascii_uppercase()
}

fn parse_zahl(s: &str) -> Result<u16> {
    let s = s.trim();
    Ok(match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        Some(h) => u16::from_str_radix(h, 16)?,
        None => s.parse()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn beispiel() -> Datenpaket {
        Datenpaket::aus_text(
            r#"
fehler = [{ code = "p0301", text = "Zylinder 1 Zündaussetzer" }]

[quelle]
name = "Test"
version = "1"
lizenz = "CC0-1.0"

[[hersteller]]
name = "Volkswagen"
wmi = ["WVW"]

[[hersteller.steuergeraet]]
adresse = 0x09
name = "Bordnetz"
protokoll = "TP20"
fehler = [{ code = "01314", text = "Motorsteuergerät" }]
codierung = [{ byte = 0, bit = 1, name = "Tagfahrlicht" }]
anpassung = [{ kanal = 23, name = "Komfortblinken" }]
dids = [{ did = 0xF40C, name = "Drehzahl", einheit = "U/min", faktor = 0.25 }]
"#,
            false,
        )
        .unwrap()
    }

    #[test]
    fn paket_import_ersetzt_gleiche_quelle() {
        let db = Db::im_speicher().unwrap();
        let e = db.paket_importieren(&beispiel(), QuellArt::Community).unwrap();
        assert_eq!((e.steuergeraete, e.fehler, e.codierung, e.anpassung, e.messwerte), (1, 2, 1, 1, 1));
        db.paket_importieren(&beispiel(), QuellArt::Community).unwrap();
        assert_eq!(db.quellen().unwrap().len(), 1, "Update ersetzt die alte Fassung");
        assert_eq!(db.fehlertext("P0301").unwrap().as_deref(), Some("Zylinder 1 Zündaussetzer"));
        let vw = db.hersteller_zur_vin("WVWZZZ1KZAM000017").unwrap().unwrap().0;
        assert_eq!(db.steuergeraet_name(vw, 9).unwrap().as_deref(), Some("Bordnetz"));
        assert!(db.paket_importieren(&beispiel(), QuellArt::Odx).is_err());
    }

    #[test]
    fn export_laesst_odx_und_lizenzlose_weg_und_rundreise() {
        let db = Db::im_speicher().unwrap();
        db.paket_importieren(&beispiel(), QuellArt::Community).unwrap();
        // gekaufte Daten: dürfen nie im Export landen
        let q = db.quelle_anlegen(QuellArt::Odx, "gekauft.odx", "", "Kaufvertrag", "").unwrap();
        let vw = db.hersteller("Volkswagen", &[], None).unwrap();
        db.con.execute("INSERT INTO ecu (manufacturer_id, ecu_name, diagnostic_address, source_id) VALUES (?1, 'GEHEIM', 1, ?2)", params![vw, q]).unwrap();
        db.con.execute("INSERT INTO dtc (dtc_code, title, source_id) VALUES ('P9999', 'GEHEIM', ?1)", params![q]).unwrap();
        let (p, weg) = db.paket_exportieren("Export", "CC0-1.0").unwrap();
        let text = toml::to_string(&p).unwrap();
        assert!(!text.contains("GEHEIM"), "{text}");
        assert!(weg.iter().any(|w| w.contains("gekauft.odx")));
        // Rundreise: Export wieder einlesen ergibt dieselben Steuergeräte
        let zurueck = Datenpaket::aus_text(&text, false).unwrap();
        assert_eq!(zurueck.hersteller[0].steuergeraete, beispiel().hersteller[0].steuergeraete);
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(Datenpaket::aus_text(&json, true).unwrap(), zurueck);
    }

    #[test]
    fn tippfehler_werden_gemeldet() {
        let e = Datenpaket::aus_text("[quelle]\nname = \"x\"\nfehler = []\n", false).unwrap_err();
        assert!(format!("{e:#}").contains("fehler"), "{e:#}");
        assert!(Datenpaket::aus_text("[quelle]\nname = \"\"\n", false).is_err(), "Name Pflicht");
    }

    #[test]
    fn csv_fehler_und_dids() {
        let db = Db::im_speicher().unwrap();
        let e = db.csv_text_importieren("code;text\nP0420;Katalysator Wirkung zu gering\n\n# Kommentar\nu0100,ohne Trenner\n", "f.csv", "").unwrap();
        assert_eq!(e.fehler, 1, "Zeile ohne ; wird übersprungen");
        assert_eq!(db.fehlertext("p0420").unwrap().as_deref(), Some("Katalysator Wirkung zu gering"));
        let e = db.csv_text_importieren("did,name,einheit,faktor,offset\n0xF40C,Drehzahl,U/min,0.25,0\n61452,Nochmal,,1,0\n", "d.csv", "").unwrap();
        assert_eq!(e.messwerte, 2);
        assert!(db.csv_text_importieren("did,name,einheit\nkaputt,x,y\n", "k.csv", "").is_err());
    }

    #[test]
    fn mitgelieferte_community_daten_passen_zum_programm() {
        let db = Db::im_speicher().unwrap();
        let l = db.community_uebernehmen().unwrap();
        assert!(!l.is_empty());
        let vw = db.hersteller_zur_vin("WVWZZZ1KZAM000017").unwrap().unwrap().0;
        // Adressliste im Repo und eingebaute Namen dürfen nicht auseinanderlaufen
        for a in 0..=0xFFu16 {
            let eingebaut = crate::systems::name(a);
            let daten = db.steuergeraet_name(vw, a).unwrap().unwrap_or_default();
            assert_eq!(eingebaut, daten, "Adresse {a:#04x}");
        }
        for q in db.quellen().unwrap() {
            assert!(super::super::darf_weitergeben(q.art, &q.lizenz), "Community-Daten brauchen eine Lizenz: {}", q.name);
        }
    }
}
