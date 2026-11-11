//! Gerüst für eigene Fehlertexte: alle Fehlercodes, die die UDS-Steuergeräte
//! kennen (nicht nur die gespeicherten), als Datei zum Ausfüllen. Ausgefüllte
//! Texte werden mit [`uebernehmen`] in `fehlertexte.toml` übernommen und gelten
//! danach in allen Anzeigen und Berichten.
//!
//! Nur lesen (ReadDTCInformation 0x19 0x0A bzw. 0x02 0xFF). TP 2.0 / KWP2000
//! kennt keine solche Abfrage — dort gibt es nur die gespeicherten Fehler.

use crate::labels::LabelDb;
use crate::transport::Backend;
use crate::tr;
use crate::uds::Uds;
use anyhow::{Context, Result};
use std::collections::{BTreeMap, BTreeSet};

/// Ein Steuergerät mit allen Codes, die es kennt.
#[derive(Debug, Clone)]
pub struct Geraet {
    pub name: String,
    pub tx: u16,
    pub rx: u16,
    pub teil: String,
    /// Code (z. B. „B104B“) → Fehlerarten (drittes Byte)
    pub codes: BTreeMap<String, BTreeSet<u8>>,
    /// true = über 0x19 0x0A (alle bekannten Codes); false = Ersatzweg 0x02 0xFF
    pub vollstaendig: bool,
}

/// Alle UDS-Steuergeräte nach ihren möglichen Fehlercodes fragen.
pub fn lesen(be: &Backend, db: &LabelDb, mut fortschritt: impl FnMut(&str)) -> Result<Vec<Geraet>> {
    let profil = crate::profil::fuer_scan(be);
    let ziele = crate::report::uds_targets_mit(be, db, profil.as_ref().map(|p| &p.1));
    let mut out = Vec::new();
    for (addr, tx, rx) in ziele {
        fortschritt(&format!("{tx:#05x}"));
        let r = (|| -> Result<(Vec<u8>, Vec<u8>, (Vec<crate::uds::Dtc>, bool))> {
            let tp = be.open(tx, rx)?;
            let uds = Uds::new(&*tp);
            uds.diagnostic_session(crate::uds::SESSION_DEFAULT)?;
            let teil = uds.read_data_by_id(0xF187).unwrap_or_default();
            let sys = uds.read_data_by_id(0xF197).unwrap_or_default();
            Ok((teil, sys, uds.read_supported_dtcs()?))
        })();
        let Ok((teil, sys, (dtcs, vollstaendig))) = r else { continue };
        if dtcs.is_empty() {
            continue;
        }
        let teil = text(&teil);
        let sys = text(&sys);
        let ident: Vec<String> = [teil.clone(), sys.clone()].into_iter().filter(|s| !s.is_empty()).collect();
        let name = addr
            .and_then(|a| db.ecu_name(a).map(str::to_string))
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| crate::kwp::tp20_name(0, &ident));
        let name = if name.is_empty() { sys } else { name };
        let mut codes: BTreeMap<String, BTreeSet<u8>> = BTreeMap::new();
        for d in &dtcs {
            codes.entry(d.code()).or_default().insert(d.fehlerart());
        }
        out.push(Geraet { name, tx, rx, teil, codes, vollstaendig });
    }
    Ok(out)
}

fn text(d: &[u8]) -> String {
    String::from_utf8_lossy(d).chars().filter(|c| !c.is_control()).collect::<String>().trim().to_string()
}

/// TOML-Zeichenkette (mit Anführungszeichen).
fn zk(s: &str) -> String {
    toml::Value::String(s.to_string()).to_string()
}

/// Fehlerarten lesbar, z. B. „13 Unterbrechung, 31 kein Signal“.
pub fn arten_text(arten: &BTreeSet<u8>) -> String {
    arten
        .iter()
        .map(|&a| crate::dtc_db::fehlerart_text(a).map(|t| format!("{a:02X} {t}")).unwrap_or_else(|| format!("{a:02X}")))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Die Datei zum Ausfüllen. Codes mit eigenem Text sind vorbelegt; Texte aus
/// Label-Dateien oder der OBD-Liste stehen als Hinweis dahinter. Kommt ein Code
/// in mehreren Steuergeräten vor, steht er nur beim ersten (ein Text je Code).
pub fn geruest(geraete: &[Geraet], db: &LabelDb, fahrzeug: &str, datum: &str) -> String {
    let eigene = crate::fehlertexte::alle();
    let mut s = String::new();
    s += &tr!("# ECUWerkbank: Fehlertexte zum Ausfüllen — {a}, {b}\n", a = fahrzeug, b = datum);
    s += &tr!("# Klartext zwischen die Anführungszeichen schreiben, z. B.  \"B104B\" = \"Wegfahrsperre: kein Signal vom Motor\"\n");
    s += &tr!("# Leere Einträge werden übersprungen. Danach übernehmen mit:\n");
    s += &tr!("#   ecu-werkbank fehlertexte-uebernehmen \"<diese Datei>\"\n");
    s += &tr!("# Die Fehlerart hinter dem Code (z. B. „kein Signal“) zeigt ecu-werkbank selbst an — hier nur das Bauteil/den Fehler beschreiben.\n");
    let mut gesehen: BTreeMap<String, String> = BTreeMap::new();
    for g in geraete {
        s += "\n";
        let weg = if g.vollstaendig { "" } else { crate::trs!(" (Ersatzweg, evtl. unvollständig)") };
        s += &tr!("# ── {a}  {b}  {c}: {d} mögliche Fehler{e}\n", a = g.name, b = format!("{:#05x}", g.tx), c = g.teil, d = g.codes.len(), e = weg);
        for (code, arten) in &g.codes {
            if let Some(wo) = gesehen.get(code) {
                s += &tr!("# {a} steht schon oben bei {b}\n", a = code, b = wo);
                continue;
            }
            gesehen.insert(code.clone(), g.name.clone());
            let wert = eigene.get(code).cloned().unwrap_or_default();
            let mut hinweis = tr!("Fehlerarten: {a}", a = arten_text(arten));
            if wert.is_empty() {
                if let Some(t) = db.dtc_description(code).filter(|t| !t.is_empty()) {
                    hinweis += &tr!(" · bekannt: {a}", a = t);
                }
            }
            s += &format!("{} = {:<40} # {hinweis}\n", zk(code), zk(&wert));
        }
    }
    s
}

/// Ausgefüllte Datei übernehmen. Gibt (neu, geändert, unverändert) zurück.
pub fn uebernehmen(inhalt: &str) -> Result<(usize, usize, usize)> {
    let m: BTreeMap<String, String> = toml::from_str(inhalt).context(tr!("Datei ist kein gültiges Gerüst (Anführungszeichen vergessen?)"))?;
    let alt = crate::fehlertexte::alle();
    let (mut neu, mut geaendert, mut gleich) = (0, 0, 0);
    let mut setzen = Vec::new();
    for (code, text) in m {
        let (code, text) = (crate::fehlertexte::norm(&code), text.trim().to_string());
        if text.is_empty() {
            continue;
        }
        match alt.get(&code) {
            None => neu += 1,
            Some(t) if *t == text => {
                gleich += 1;
                continue;
            }
            Some(_) => geaendert += 1,
        }
        setzen.push((code, text));
    }
    crate::fehlertexte::mehrere_setzen(&setzen)?;
    Ok((neu, geaendert, gleich))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geruest_und_uebernehmen() {
        let _g = crate::history::TEST_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("vag-fg-{}", std::process::id()));
        std::env::set_var("VAG_DATA_DIR", &dir);
        crate::fehlertexte::setzen("B1000", "schon da").unwrap();
        let g = |name: &str, codes: &[(&str, u8)]| {
            let mut m: BTreeMap<String, BTreeSet<u8>> = BTreeMap::new();
            for (c, a) in codes {
                m.entry(c.to_string()).or_default().insert(*a);
            }
            Geraet { name: name.into(), tx: 0x711, rx: 0x77B, teil: "6R0920860J".into(), codes: m, vollstaendig: true }
        };
        let geraete = [g("Wegfahrsperre", &[("B104B", 0x31), ("B104B", 0x13), ("B1000", 0x29)]), g("Kombi", &[("B104B", 0x31), ("U0100", 0x00)])];
        let db = LabelDb::load_dir(&dir.join("keine-labels")).unwrap();
        let s = geruest(&geraete, &db, "Polo", "08.11.2026");
        // ein Eintrag je Code, auch wenn er in zwei Geräten vorkommt
        assert_eq!(s.lines().filter(|l| l.starts_with("\"B104B\" =")).count(), 1);
        assert!(s.contains("steht schon oben bei Wegfahrsperre"));
        assert!(s.contains("\"B1000\" = \"schon da\""));
        let m: BTreeMap<String, String> = toml::from_str(&s).unwrap();
        assert_eq!(m.len(), 3);
        // ausfüllen und übernehmen
        let s = s.replacen("\"B104B\" = \"\"", "\"B104B\" = \"Wegfahrsperre: kein Signal\"", 1);
        assert_eq!(uebernehmen(&s).unwrap(), (1, 0, 1));
        assert_eq!(crate::fehlertexte::alle().get("B104B").map(String::as_str), Some("Wegfahrsperre: kein Signal"));
        assert_eq!(crate::fehlertexte::alle().get("B1000").map(String::as_str), Some("schon da"));
        std::env::remove_var("VAG_DATA_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn lesen_in_der_simulation() {
        let _g = crate::history::TEST_LOCK.lock().unwrap();
        let db = LabelDb::load_dir(std::path::Path::new("/nonexistent")).unwrap();
        let g = lesen(&Backend::Demo, &db, |_| {}).unwrap();
        assert!(!g.is_empty());
        assert!(g.iter().all(|x| x.vollstaendig && x.codes.len() >= 2));
    }
}
