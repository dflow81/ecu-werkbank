//! Fahrzeugprofil für den Schnell-Scan: welche Steuergeräte das Fahrzeug hat.
//!
//! Nach jedem vollen Scan wird `profil.toml` im Fahrzeugordner geschrieben.
//! Beim nächsten Scan wird zuerst die VIN gelesen (OBD-II); gibt es dazu ein
//! Profil, fragt der Scan nur diese Steuergeräte ab (Golf 6: ~10 s statt ~40 s).
//! Neu verbaute Steuergeräte findet nur der volle Scan.

use serde::{Deserialize, Serialize};

use crate::transport::Backend;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Profil {
    pub erstellt: String,
    /// UDS-Steuergeräte (Anfrage-ID, Antwort-ID)
    #[serde(default)]
    pub uds: Vec<(u16, u16)>,
    /// TP-2.0-Logikadressen
    #[serde(default)]
    pub tp20: Vec<u8>,
    /// Wo der genaue Kilometerstand steht (einmal gesucht, siehe `km`)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub km_quelle: Option<crate::km::KmQuelle>,
}

const DATEI: &str = "profil.toml";

/// Profil zu einer VIN laden (nur bekannte Fahrzeuge).
pub fn load(vin: &str) -> Option<Profil> {
    let v = crate::vehicle::find_by_vin(vin)?;
    let text = std::fs::read_to_string(v.dir().join(DATEI)).ok()?;
    toml::from_str(&text).ok()
}

/// Profil speichern, wenn das Fahrzeug bekannt ist. Liefert true bei Erfolg.
pub fn save(vin: &str, p: &Profil) -> bool {
    if cfg!(test) {
        return false; // Tests sollen keine echten Fahrzeugordner verändern
    }
    let Some(v) = crate::vehicle::find_by_vin(vin) else { return false };
    // Nur erweitern, nie verkleinern: ein Scan bei ausgeschalteter Zündung
    // findet am Golf 6 nur 4 von 17 Steuergeräten (29.10.2026)
    let p = match load(vin) {
        Some(alt) => zusammen(&alt, p),
        None => p.clone(),
    };
    let Ok(text) = toml::to_string(&p) else { return false };
    std::fs::write(v.dir().join(DATEI), text).is_ok()
}

/// Zwei Profile vereinen (alle Steuergeräte aus beiden).
pub fn zusammen(alt: &Profil, neu: &Profil) -> Profil {
    let mut p = neu.clone();
    p.km_quelle = neu.km_quelle.or(alt.km_quelle);
    p.uds.extend(alt.uds.iter().copied());
    p.tp20.extend(alt.tp20.iter().copied());
    p.uds.sort_unstable();
    p.uds.dedup();
    p.tp20.sort_unstable();
    p.tp20.dedup();
    p
}

/// VIN schnell über OBD-II lesen (Mode 09).
pub fn quick_vin(be: &Backend) -> Option<String> {
    let tp = be.open(crate::obd::DEFAULT_TX, crate::obd::DEFAULT_RX).ok()?;
    let v = crate::obd::Obd::new(&*tp).vin().ok()?;
    (v.len() == 17).then_some(v)
}

/// Profil für den Schnell-Scan: VIN lesen, Profil laden. `None` = voller Scan.
pub fn fuer_scan(be: &Backend) -> Option<(String, Profil)> {
    if cfg!(test) {
        return None;
    }
    let vin = quick_vin(be)?;
    let p = load(&vin).filter(|p| !p.uds.is_empty() || !p.tp20.is_empty())?;
    Some((vin, p))
}

/// Profil aus einem Scan-Ergebnis bauen.
pub fn aus_scan(r: &crate::report::ScanReport) -> Profil {
    aus_modulen(&r.modules)
}

pub fn aus_modulen(modules: &[crate::report::ModuleReport]) -> Profil {
    let mut p = Profil { erstellt: crate::vehicle::today(), ..Default::default() };
    for m in modules {
        match m.protocol.as_str() {
            "TP 2.0" => {
                if let Some(a) = m.address {
                    p.tp20.push(a as u8);
                }
            }
            "UDS" => {
                if let Some((tx, rx)) = parse_can(&m.can) {
                    p.uds.push((tx, rx));
                }
            }
            _ => {}
        }
    }
    p.tp20.sort_unstable();
    p.tp20.dedup();
    p.uds.sort_unstable();
    p.uds.dedup();
    p
}

/// "0x714/0x77e" → (0x714, 0x77E)
pub fn parse_can(s: &str) -> Option<(u16, u16)> {
    let (a, b) = s.split_once('/')?;
    let h = |x: &str| u16::from_str_radix(x.trim().trim_start_matches("0x"), 16).ok();
    Some((h(a)?, h(b)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profil_aus_scan_und_toml() {
        assert_eq!(parse_can("0x714/0x77e"), Some((0x714, 0x77E)));
        let p = Profil { erstellt: "28.10.2026".into(), uds: vec![(0x714, 0x77E)], tp20: vec![0x01, 0x1F], ..Default::default() };
        let t = toml::to_string(&p).unwrap();
        assert_eq!(toml::from_str::<Profil>(&t).unwrap(), p);
        // Scan mit Zündung aus findet weniger: Profil bleibt vollständig
        let wenig = Profil { erstellt: "29.10.2026".into(), uds: vec![(0x711, 0x77B)], tp20: vec![0x1F], ..Default::default() };
        let z = zusammen(&p, &wenig);
        assert_eq!(z.uds, vec![(0x711, 0x77B), (0x714, 0x77E)]);
        assert_eq!(z.tp20, vec![0x01, 0x1F]);
    }
}
