//! Softwarestände aller Steuergeräte: Teilenummer, Software-/Hardware-
//! version, Systembezeichnung, Seriennummer. Gespeichert im Fahrzeugordner
//! (`software/<Zeit>.toml`); Vergleich zeigt getauschte oder neu
//! programmierte Steuergeräte.

use crate::tr;
use crate::labels::LabelDb;
use crate::transport::Backend;
use crate::uds::Uds;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ModulSw {
    pub protocol: String,
    pub adresse: String,
    pub name: String,
    #[serde(default)]
    pub teilenummer: String,
    #[serde(default)]
    pub software: String,
    #[serde(default)]
    pub hardware_nr: String,
    #[serde(default)]
    pub hardware_ver: String,
    #[serde(default)]
    pub system: String,
    #[serde(default)]
    pub seriennummer: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Stand {
    pub created: String,
    pub timestamp: u64,
    #[serde(default)]
    pub vin: Option<String>,
    pub module: Vec<ModulSw>,
}

/// Standard-Identifikations-Kennungen (UDS, ISO 14229 + VAG), mit Klartext-Namen.
/// Reihenfolge wie in VCDS „Advanced ID". Nur lesen.
pub const STD_DIDS: &[(u16, &str)] = &[
    (0xF190, "Fahrgestellnummer (VIN)"),
    (0xF187, "VW-Teilenummer (Software)"),
    (0xF189, "Software-Version"),
    (0xF191, "VW-Teilenummer (Hardware)"),
    (0xF1A3, "Hardware-Version"),
    (0xF197, "System-/Gerätename"),
    (0xF1A2, "Codierung (ASAM/ODX)"),
    (0xF1A5, "Betriebsnummer (letzte Codierung)"),
    (0xF19E, "ASAM/ODX-Datei"),
    (0xF18C, "Seriennummer (ECU)"),
    (0xF17C, "Fertigungskennung (FAZIT)"),
    (0xF199, "Programmierdatum"),
    (0xF1AA, "VW-Werkstatt-System-Name"),
    (0xF18A, "Zulieferer-Kennung"),
    (0xF192, "Zulieferer-Hardwarenummer"),
    (0xF193, "Zulieferer-Hardwareversion"),
    (0xF194, "Zulieferer-Softwarenummer"),
    (0xF195, "Zulieferer-Softwareversion"),
    (0xF1DF, "ECU-Programmierzustand"),
    (0xF186, "Aktive Diagnosesitzung"),
];

/// Was die Fahrzeug-Datenbank zur Teilenummer weiß, als Zeilen für die
/// Identifikation: Zusammenfassung und ASAM-Datensätze (zum Vergleich mit F19E).
/// Leer, wenn die Datenbank die Teilenummer nicht kennt.
pub fn datenbank_zeilen(teilenummer: &str) -> Vec<(String, String)> {
    let Some(w) = crate::db::teile::wissen_falls_vorhanden(teilenummer) else { return Vec::new() };
    let mut z = vec![(tr!("Fahrzeug-Datenbank"), w.zusammenfassung())];
    for r in &w.diagnose {
        for a in &r.asam {
            z.push((tr!("ASAM-Datensatz (Datenbank)"), format!("{} {} · {}", a.name, a.version, a.rod)));
        }
    }
    z
}

/// Ein Datum (Programmierdatum) aus BCD-Bytes JJ MM TT lesbar machen.
fn vielleicht_datum(d: &[u8]) -> Option<String> {
    if d.len() == 3 && d.iter().all(|b| (*b >> 4) <= 9 && (*b & 0xF) <= 9) {
        return Some(format!("20{:02x}-{:02x}-{:02x}", d[0], d[1], d[2]));
    }
    None
}

/// Betriebsnummer der letzten Codierung aus 0xF1A5 (6 Byte = 48 Bit):
/// Händler (21 Bit) | Importeur (10 Bit) | Werkstattcode (17 Bit).
/// Am Golf 6 mit der Anzeige eines Werkstatt-Diagnosegeräts verglichen
/// (ABS, Kombi, Bordnetz; 15.11.2026).
pub fn betriebsnummer(d: &[u8]) -> Option<(u32, u16, u32)> {
    let b: [u8; 6] = d.try_into().ok()?;
    let v = b.iter().fold(0u64, |a, &x| (a << 8) | u64::from(x));
    let werkstatt = (v & 0x1_FFFF) as u32;
    let importeur = ((v >> 17) & 0x3FF) as u16;
    let haendler = ((v >> 27) & 0x1F_FFFF) as u32;
    Some((werkstatt, importeur, haendler))
}

/// Rohbytes einer Identifikations-Kennung lesbar machen: Text, Datum oder Hex.
pub fn ident_wert(did: u16, d: &[u8]) -> String {
    if d.is_empty() {
        return String::new();
    }
    if did == 0xF1A5 {
        if let Some((w, i, h)) = betriebsnummer(d) {
            return tr!("Werkstatt {w} · Importeur {i} · Händler {h}", w = format!("{w:05}"), i = format!("{i:03}"), h = format!("{h:05}"));
        }
    }
    let txt = ascii(d);
    // Programmierdatum als BCD
    if did == 0xF199 {
        if let Some(dt) = vielleicht_datum(d) {
            return dt;
        }
    }
    // überwiegend druckbarer Text → als Text, sonst Hex
    let druckbar = d.iter().filter(|b| (0x20..=0x7E).contains(*b)).count();
    if txt.len() >= 2 && druckbar * 10 >= d.len() * 8 {
        txt
    } else {
        crate::backup::hex(d)
    }
}

fn ascii(d: &[u8]) -> String {
    d.iter().filter(|b| (0x20..=0x7E).contains(*b)).map(|&b| b as char).collect::<String>().trim().to_string()
}

/// Alle erreichbaren Steuergeräte abfragen.
pub fn read(be: &Backend, db: &LabelDb, with_tp20: bool, mut progress: impl FnMut(&str)) -> Result<Stand> {
    let mut module = Vec::new();
    let mut vin = None;
    let profil = crate::profil::fuer_scan(be);
    let targets = crate::report::uds_targets_mit(be, db, profil.as_ref().map(|p| &p.1));
    for (addr, tx, rx) in targets {
        progress(&tr!("UDS {tx}…", tx = format!("{:#05x}", tx)));
        let Ok(tp) = be.open(tx, rx) else { continue };
        let uds = Uds::new(&*tp);
        if uds.diagnostic_session(crate::uds::SESSION_DEFAULT).is_err() {
            continue;
        }
        let r = |did: u16| uds.read_data_by_id(did).map(|d| ascii(&d)).unwrap_or_default();
        if vin.is_none() {
            let v = r(0xF190);
            if v.len() == 17 {
                vin = Some(v);
            }
        }
        module.push(ModulSw {
            protocol: "UDS".into(),
            adresse: addr.map(|a| format!("{a:#04x}")).unwrap_or_else(|| format!("ID {tx:#05x}")),
            name: addr
                .and_then(|a| db.ecu_name(a).map(str::to_string))
                .unwrap_or_else(|| crate::kwp::tp20_name(0, &[r(0xF197)])),
            teilenummer: r(0xF187),
            software: r(0xF189),
            hardware_nr: r(0xF191),
            hardware_ver: r(0xF1A3),
            system: r(0xF197),
            seriennummer: r(0xF18C),
        });
    }
    if with_tp20 && be.supports_tp20() {
        let found = match &profil {
            Some((_, p)) => crate::kwp::sweep_addrs(be, &p.tp20, |a| progress(&tr!("TP 2.0 {a}…", a = format!("{:#04x}", a))))?,
            None => crate::kwp::sweep(be, |a| progress(&tr!("TP 2.0 {a}…", a = format!("{:#04x}", a))))?,
        };
        for f in found {
            // ident = ["Teilenummer  SW", "System", optional "HW …"] (siehe kwp::ident_9b / ident_22)
            let (teil, sw) = teil_und_sw(f.ident.first().map(String::as_str).unwrap_or(""));
            module.push(ModulSw {
                protocol: "TP 2.0".into(),
                adresse: format!("{:#04x}", f.address),
                name: if f.name.is_empty() { f.ident.get(1).cloned().unwrap_or_default() } else { f.name.clone() },
                teilenummer: teil,
                system: f.ident.get(1).filter(|s| !s.starts_with("HW ")).cloned().unwrap_or_default(),
                software: sw,
                hardware_nr: f.ident.iter().find_map(|s| s.strip_prefix("HW ")).unwrap_or_default().to_string(),
                ..Default::default()
            });
        }
    }
    Ok(Stand { created: crate::report::now_local(), timestamp: crate::report::unix_now(), vin, module })
}

/// "03C906016  5049" → ("03C906016", "5049"); ohne erkennbaren Softwarestand
/// bleibt alles Teilenummer.
fn teil_und_sw(s: &str) -> (String, String) {
    match s.trim().rsplit_once(' ') {
        Some((t, sw)) if sw.len() == 4 && t.trim().len() >= 9 => (t.trim().to_string(), sw.to_string()),
        _ => (s.trim().to_string(), String::new()),
    }
}

pub fn save(vehicle: &str, s: &Stand) -> Result<std::path::PathBuf> {
    let dir = crate::vehicle::dir(vehicle).join("software");
    std::fs::create_dir_all(&dir)?;
    let p = dir.join(format!("{}.toml", s.timestamp));
    crate::ablage::schreiben(&p, toml::to_string(s)?).with_context(|| tr!("Schreiben: {a}", a = p.display()))?;
    Ok(p)
}

/// Alle gespeicherten Stände eines Fahrzeugs, älteste zuerst.
/// Teilenummer eines Steuergeräts aus der neuesten Software-Sicherung.
/// `geraet` = TP-2.0-Adresse (< 0x100) oder UDS-Anfrage-ID.
pub fn teilenummer_fuer(vehicle: &str, geraet: u16) -> Option<String> {
    let st = list(vehicle).pop()?;
    let gesucht = if geraet < 0x100 { format!("{geraet:#04x}") } else { format!("ID {geraet:#05x}") };
    st.module.into_iter().find(|m| m.adresse == gesucht).map(|m| m.teilenummer).filter(|t| !t.is_empty())
}

pub fn list(vehicle: &str) -> Vec<Stand> {
    let dir = crate::vehicle::dir(vehicle).join("software");
    let mut v: Vec<Stand> = crate::ablage::liste(&dir)
        .into_iter()
        .filter_map(|p| crate::ablage::lesen(p).ok())
        .filter_map(|t| toml::from_str(&t).ok())
        .collect();
    v.sort_by_key(|s| s.timestamp);
    v
}

/// Unterschiede alt → neu: (Steuergerät, Beschreibung).
pub fn compare(old: &Stand, new: &Stand) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for n in &new.module {
        let title = format!("{} {}", n.adresse, if n.name.is_empty() { &n.system } else { &n.name });
        match old.module.iter().find(|o| o.adresse == n.adresse && o.protocol == n.protocol) {
            None => out.push((title, crate::tr!("neu hinzugekommen (nachgerüstet?)"))),
            Some(o) => {
                if !o.seriennummer.is_empty() && !n.seriennummer.is_empty() && o.seriennummer != n.seriennummer || o.teilenummer != n.teilenummer {
                    out.push((title.clone(), tr!("getauscht: Teilenummer {a} → {b}{c}", a = o.teilenummer, b = n.teilenummer, c = if o.seriennummer != n.seriennummer && !n.seriennummer.is_empty() { format!(", Seriennr. {} → {}", o.seriennummer, n.seriennummer) } else { String::new() })));
                } else if o.software != n.software {
                    out.push((title, tr!("neue Software: {a} → {b}", a = o.software, b = n.software)));
                }
            }
        }
    }
    for o in &old.module {
        if !new.module.iter().any(|n| n.adresse == o.adresse && n.protocol == o.protocol) {
            out.push((format!("{} {}", o.adresse, o.name), crate::tr!("antwortet nicht mehr (ausgebaut oder defekt?)")));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn betriebsnummer_wie_am_golf6() {
        // ABS, Kombi, Bordnetz — Werte wie vom Werkstatt-Diagnosegerät angezeigt
        assert_eq!(betriebsnummer(&[0x80, 0x00, 0x00, 0x02, 0xDA, 0x73]), Some((55923, 1, 1_048_576)));
        assert_eq!(betriebsnummer(&[0x00, 0x04, 0xA0, 0x00, 0x00, 0x42]), Some((66, 0, 148)));
        assert_eq!(betriebsnummer(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x42]), Some((66, 0, 0)));
        assert_eq!(betriebsnummer(&[0x00, 0x42]), None);
    }

    #[test]
    fn ident_wert_text_datum_hex() {
        assert_eq!(ident_wert(0xF187, b"5K0920860F"), "5K0920860F");
        assert_eq!(ident_wert(0xF199, &[0x20, 0x10, 0x07]), "2020-10-07"); // BCD-Datum
        assert_eq!(ident_wert(0xF1A2, &[0x00, 0xFF, 0x1A]), "00 FF 1A"); // nicht-Text → Hex
        assert_eq!(ident_wert(0xF190, &[]), "");
    }

    #[test]
    fn read_and_compare_demo() {
        let a = read(&Backend::Demo, &LabelDb::default(), true, |_| {}).unwrap();
        let motor = a.module.iter().find(|m| m.adresse == "ID 0x7e0").unwrap();
        assert_eq!((motor.software.as_str(), motor.system.as_str()), ("0010", "1.4l R4 TSI"));
        let mut b = a.clone();
        b.module[0].software = "0012".into();
        b.module.retain(|m| m.adresse != "0x17");
        b.module.push(ModulSw { protocol: "UDS".into(), adresse: "0x69".into(), name: "Anhängerkupplung".into(), ..Default::default() });
        let d = compare(&a, &b);
        assert!(d.iter().any(|(_, t)| t.contains("neue Software: 0010 → 0012")), "{d:?}");
        assert!(d.iter().any(|(_, t)| t.contains("neu hinzugekommen")));
        assert!(d.iter().any(|(_, t)| t.contains("antwortet nicht mehr")));
    }
}
