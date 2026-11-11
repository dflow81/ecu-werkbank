//! Codierungs-Sicherung: Codierung aller Steuergeräte sichern, vergleichen,
//! zurückschreiben.
//!
//! - UDS: Codierung per ReadDataByIdentifier (DID aus dem Label `coding_did`,
//!   sonst 0x0600 = VAG-Codierung). Zurückschreiben per 0x2E mit Kontrolle.
//! - TP 2.0: Die Codierung steckt in der Identifikation (KWP 0x1A 0x9B). Die
//!   Rohdaten werden gesichert; Zurückschreiben ist noch nicht möglich, weil der
//!   Aufbau erst gegen einen Mitschnitt geprüft werden muss.
//!
//! Ablage im Fahrzeugordner: `…/fahrzeuge/<Kennzeichen|VIN>/codierung/<Unix-Zeit>.toml`.


use crate::tr;
use crate::labels::LabelDb;
use crate::transport::Backend;
use crate::uds::Uds;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Standard-DID der VAG-Codierung bei UDS-Steuergeräten.
pub const VAG_CODING_DID: u16 = 0x0600;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CodingEntry {
    pub protocol: String,
    pub address: Option<u16>,
    pub name: String,
    pub tx: u16,
    pub rx: u16,
    /// UDS: DID der Codierung; TP 2.0: keiner
    pub did: Option<u16>,
    pub part: String,
    /// Codierung als Hex (UDS) bzw. Roh-Identifikation (TP 2.0)
    pub data_hex: String,
    #[serde(default)]
    pub note: String,
}

impl CodingEntry {
    pub fn title(&self) -> String {
        let addr = match self.address {
            Some(a) => format!("{a:#04x}"),
            None => format!("ID {:#05x}", self.tx),
        };
        let name = if self.name.is_empty() { self.part.as_str() } else { self.name.as_str() };
        format!("{} {addr} {name}", self.protocol)
    }

    pub fn bytes(&self) -> Vec<u8> {
        parse_hex(&self.data_hex).unwrap_or_default()
    }

    /// Schlüssel für eigene Codiertexte: UDS = Request-CAN-ID, TP 2.0 = 0x200 +
    /// Adresse (die Kanal-IDs von TP 2.0 werden jedes Mal neu ausgehandelt).
    pub fn text_key(&self) -> u16 {
        match (self.protocol.as_str(), self.address) {
            ("TP 2.0", Some(a)) => crate::codiertexte::tp20_key(a as u8),
            _ => self.tx,
        }
    }

    /// Bit-genau anzeigbar: eine echte Codierung (UDS oder TP 2.0 mit 0x0600).
    pub fn hat_codierung(&self) -> bool {
        self.did.is_some() && !self.data_hex.is_empty()
    }

    pub fn can_restore(&self) -> bool {
        self.protocol == "UDS" && self.did.is_some() && !self.data_hex.is_empty()
    }

    fn key(&self) -> String {
        format!("{} {} {:#05x}", self.protocol, self.address.map(|a| format!("{a:#04x}")).unwrap_or_default(), self.tx)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodingBackup {
    pub created: String,
    pub timestamp: u64,
    pub vin: Option<String>,
    pub transport: String,
    /// Fahrzeugordner (siehe crate::vehicle)
    #[serde(default)]
    pub vehicle: String,
    pub entries: Vec<CodingEntry>,
}

pub fn hex(d: &[u8]) -> String {
    d.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ")
}

pub fn parse_hex(s: &str) -> Option<Vec<u8>> {
    let c: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    if !c.len().is_multiple_of(2) {
        return None;
    }
    (0..c.len()).step_by(2).map(|i| u8::from_str_radix(&c[i..i + 2], 16).ok()).collect()
}

fn ascii(d: &[u8]) -> String {
    if !d.is_empty() && d.iter().all(|b| (0x20..=0x7E).contains(b)) {
        String::from_utf8_lossy(d).trim().to_string()
    } else {
        String::new()
    }
}

/// Codierung aller erreichbaren Steuergeräte lesen.
pub fn backup(be: &Backend, db: &LabelDb, with_tp20: bool, mut progress: impl FnMut(&str)) -> Result<CodingBackup> {
    let mut entries = Vec::new();
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
        let part = ascii(&uds.read_data_by_id(0xF187).unwrap_or_default());
        if vin.is_none() {
            let v = ascii(&uds.read_data_by_id(0xF190).unwrap_or_default());
            if v.len() == 17 {
                vin = Some(v);
            }
        }
        let did = addr.and_then(|a| db.coding_did(a)).unwrap_or(VAG_CODING_DID);
        let (data_hex, note) = match uds.read_data_by_id(did) {
            Ok(d) => (hex(&d), String::new()),
            Err(e) => (String::new(), tr!("Codierung nicht lesbar: {e}", e = e)),
        };
        entries.push(CodingEntry {
            protocol: "UDS".into(),
            address: addr,
            name: addr.and_then(|a| db.ecu_name(a)).unwrap_or("").to_string(),
            tx,
            rx,
            did: Some(did),
            part,
            data_hex,
            note,
        });
    }

    if with_tp20 && be.supports_tp20() {
        let found = match &profil {
            Some((_, p)) => crate::kwp::sweep_addrs(be, &p.tp20, |a| progress(&tr!("TP 2.0 {a}…", a = format!("{:#04x}", a))))?,
            None => crate::kwp::sweep(be, |a| progress(&tr!("TP 2.0 {a}…", a = format!("{:#04x}", a))))?,
        };
        for f in found {
            use crate::kwp::TpCodierung;
            let (did, data_hex, note) = match &f.codierung {
                Some(TpCodierung::Lang(b)) => (Some(crate::kwp::CODING_DID), hex(b), tr!("Codierung über TP 2.0 (Kennung 0x0600); Zurückschreiben noch nicht möglich")),
                Some(TpCodierung::Kurz { wert, .. }) => (None, hex(&f.ident_raw), tr!("Roh-Identifikation, Kurzcodierung (vermutet): {a}; Zurückschreiben noch nicht möglich", a = format!("{wert:07}"))),
                None => (None, hex(&f.ident_raw), tr!("Roh-Identifikation (enthält die Codierung); Zurückschreiben noch nicht möglich")),
            };
            entries.push(CodingEntry {
                protocol: "TP 2.0".into(),
                address: Some(u16::from(f.address)),
                name: if f.name.is_empty() { f.ident.get(1).cloned().unwrap_or_default() } else { f.name.clone() },
                tx: f.tx,
                rx: f.rx,
                did,
                part: f.ident.first().cloned().unwrap_or_default(),
                data_hex,
                note,
            });
        }
    }

    Ok(CodingBackup {
        created: crate::report::now_local(),
        timestamp: crate::report::unix_now(),
        vin,
        transport: be.describe(),
        vehicle: String::new(),
        entries,
    })
}

/// Codierungs-Ordner eines Fahrzeugs.
pub fn backup_dir(vehicle_key: &str) -> PathBuf {
    crate::vehicle::dir(vehicle_key).join("codierung")
}

/// Sicherung im Ordner ihres Fahrzeugs speichern (`b.vehicle`, sonst über die VIN).
pub fn save(b: &CodingBackup) -> Result<PathBuf> {
    let key = if b.vehicle.is_empty() {
        crate::vehicle::resolve(b.vin.as_deref(), None)?.key
    } else {
        b.vehicle.clone()
    };
    let dir = backup_dir(&key);
    std::fs::create_dir_all(&dir).with_context(|| tr!("Ordner anlegen: {a}", a = dir.display()))?;
    let path = dir.join(format!("{}.toml", b.timestamp));
    std::fs::write(&path, toml::to_string(b)?).with_context(|| tr!("Schreiben: {a}", a = path.display()))?;
    Ok(path)
}

pub fn load(path: &Path) -> Result<CodingBackup> {
    let text = std::fs::read_to_string(path).with_context(|| tr!("Lesen: {a}", a = path.display()))?;
    toml::from_str(&text).with_context(|| tr!("Keine gültige Codierungs-Sicherung: {a}", a = path.display()))
}

/// Alle Sicherungen (über alle Fahrzeuge), neueste zuerst.
pub fn list_all() -> Vec<(PathBuf, CodingBackup)> {
    let mut v = Vec::new();
    for veh in crate::vehicle::list() {
        for f in std::fs::read_dir(backup_dir(&veh.key)).into_iter().flatten().flatten() {
            let p = f.path();
            if p.extension().and_then(|e| e.to_str()) == Some("toml") {
                if let Ok(b) = load(&p) {
                    v.push((p, b));
                }
            }
        }
    }
    v.sort_by_key(|(_, b)| std::cmp::Reverse(b.timestamp));
    v
}

/// Unterschied eines Steuergeräts zwischen zwei Sicherungen.
#[derive(Debug, Clone, PartialEq)]
pub struct CodingChange {
    pub module: String,
    /// Schlüssel der eigenen Codiertexte (siehe [`CodingEntry::text_key`])
    pub tx: u16,
    pub old_hex: String,
    pub new_hex: String,
    /// (Byte, Bit, alt, neu, Name: eigener Text oder Label)
    pub bits: Vec<(usize, u8, bool, bool, String)>,
}

/// Geänderte Codierungen von `old` nach `new` (nur Steuergeräte in beiden).
pub fn compare(old: &CodingBackup, new: &CodingBackup, db: &LabelDb) -> Vec<CodingChange> {
    let texte = crate::codiertexte::Texte::laden();
    let mut out = Vec::new();
    for n in &new.entries {
        let Some(o) = old.entries.iter().find(|o| o.key() == n.key()) else { continue };
        if o.data_hex == n.data_hex {
            continue;
        }
        let (ob, nb) = (o.bytes(), n.bytes());
        // Andere Datenart (z. B. alte Sicherung mit Roh-Identifikation, neue mit
        // Codierung): nur beide Stände zeigen
        if o.did != n.did {
            out.push(CodingChange { module: n.title(), tx: n.text_key(), old_hex: o.data_hex.clone(), new_hex: n.data_hex.clone(), bits: Vec::new() });
            continue;
        }
        // Eine Seite nicht lesbar (z. B. Radio gerade beim Hochfahren): keine
        // Bit-Änderungen erfinden, nur beide Stände zeigen
        if ob.is_empty() || nb.is_empty() {
            out.push(CodingChange { module: n.title(), tx: n.text_key(), old_hex: o.data_hex.clone(), new_hex: n.data_hex.clone(), bits: Vec::new() });
            continue;
        }
        let labels = n.address.and_then(|a| db.coding(a)).unwrap_or(&[]);
        let bits = crate::codiertexte::unterschiede(&ob, &nb)
            .into_iter()
            .map(|(i, bit, a, b)| (i, bit, a, b, texte.name(n.text_key(), labels, i, Some(bit)).unwrap_or_default()))
            .collect();
        out.push(CodingChange { module: n.title(), tx: n.text_key(), old_hex: o.data_hex.clone(), new_hex: n.data_hex.clone(), bits });
    }
    out
}

/// Codierung eines Steuergeräts zurückschreiben und zur Kontrolle neu lesen.
/// Gibt die vorher gelesene Codierung zurück.
pub fn restore(be: &Backend, e: &CodingEntry, login: Option<&crate::login::Methode>) -> Result<Vec<u8>> {
    if !e.can_restore() {
        bail!(tr!("{a}: Zurückschreiben ist für diesen Eintrag nicht möglich", a = e.title()));
    }
    let did = e.did.expect("geprüft");
    crate::km::schreiben_erlaubt(did, e.tx)?;
    let want = e.bytes();
    let tp = be.open(e.tx, e.rx)?;
    let uds = Uds::new(&*tp);
    uds.diagnostic_session(crate::uds::SESSION_DEFAULT)?;
    let before = uds.read_data_by_id(did).context(crate::trs!("aktuelle Codierung lesen"))?;
    if before == want {
        return Ok(before);
    }
    uds.diagnostic_session(crate::uds::SESSION_EXTENDED)
        .context(crate::trs!("erweiterte Diagnosesitzung (0x03) abgelehnt"))?;
    let mut w = uds.write_data_by_id(did, &want);
    // Bei „Zugriff verweigert" (oder 0x24): falls ein Login hinterlegt ist, anmelden und erneut schreiben.
    if let (Err(err), Some(m)) = (&w, login) {
        if crate::login::ist_zugriff_verweigert(&format!("{err:#}")) {
            crate::login::anmelden_uds(&uds, m).context(crate::trs!("Login (SecurityAccess) fehlgeschlagen"))?;
            w = uds.write_data_by_id(did, &want);
        }
    }
    w.context(crate::trs!("Schreiben abgelehnt — evtl. ist eine Zugriffsberechtigung (Login/SecurityAccess) nötig (ecu-werkbank login …)"))?;
    let after = uds.read_data_by_id(did).context(crate::trs!("Kontroll-Lesen"))?;
    if after != want {
        bail!(tr!("Kontrolle fehlgeschlagen: gelesen {a} statt {b}", a = hex(&after), b = hex(&want)));
    }
    Ok(before)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backup_compare_restore_on_demo() {
        let be = Backend::Demo;
        let db = LabelDb::default();
        let a = backup(&be, &db, true, |_| {}).unwrap();
        let motor = a.entries.iter().find(|e| e.tx == 0x7E0).unwrap().clone();
        assert!(motor.can_restore());
        assert!(a.entries.iter().any(|e| e.protocol == "TP 2.0" && !e.data_hex.is_empty()));

        // Codierung im Demo-Fahrzeug ändern, dann vergleichen und zurückschreiben
        let mut changed = motor.clone();
        let mut bytes = changed.bytes();
        bytes[0] ^= 0b0000_0100;
        changed.data_hex = hex(&bytes);
        restore(&be, &changed, None).unwrap();
        let b = backup(&be, &db, false, |_| {}).unwrap();
        let diff = compare(&a, &b, &db);
        assert_eq!(diff.len(), 1);
        assert_eq!(diff[0].bits.len(), 1);
        assert_eq!((diff[0].bits[0].0, diff[0].bits[0].1), (0, 2));

        // Original wiederherstellen
        restore(&be, &motor, None).unwrap();
        let c = backup(&be, &db, false, |_| {}).unwrap();
        assert!(compare(&a, &c, &db).is_empty());
    }

    #[test]
    fn restore_mit_login_auf_geschuetzter_did() {
        let _s = crate::sim::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::sim::reset();
        let be = Backend::Demo;
        // geschützte Demo-Kennung (nur Simulation): Schreiben erst nach Login
        let e = CodingEntry {
            protocol: "UDS".into(),
            address: None,
            name: String::new(),
            tx: 0x7E0,
            rx: 0x7E8,
            did: Some(crate::sim::DEMO_LOGIN_DID),
            part: String::new(),
            data_hex: "AB CD".into(),
            note: String::new(),
        };
        // ohne Login: abgelehnt
        assert!(restore(&be, &e, None).is_err());
        // mit SA2-Login: klappt
        let m = crate::login::Methode::Sa2 { level: 3, bytecode: hex(&crate::sim::DEMO_SA2) };
        let before = restore(&be, &e, Some(&m)).unwrap();
        assert_eq!(before, vec![0x00, 0x00]);
        crate::sim::reset();
    }
}
