//! Fahrzeug-Erkennung im Hintergrund: merkt, wenn ein Auto angesteckt (bzw.
//! die Zündung eingeschaltet) oder wieder abgezogen wird.
//!
//! Gefragt wird nur lesend und nur, wenn der Bus frei ist (siehe [`crate::bus`]):
//! 1. Kombiinstrument über UDS (0x714) nach der Fahrgestellnummer (0xF190) —
//!    stört die TP-2.0-Steuergeräte nicht.
//! 2. Erst wenn das nichts bringt: OBD (0x7E0) Mode 09. Am Golf 6 sperrt das
//!    den Motor-TP-2.0-Zugang für ca. 3 s, darum nur beim Erkennen, nicht beim
//!    Nachschauen, ob das Auto noch da ist.
//!
//! Ohne Auto bestätigt niemand die Anfragen, der Adapter geht in „ERROR-PASSIVE“
//! und wiederholt die letzte Nachricht selbst. Dann wird nicht weiter gesendet,
//! sondern nur auf die Zähler des Adapters geschaut: Sobald ein Auto dran ist,
//! geht die wartende Nachricht raus und die Zähler steigen.

use crate::transport::Backend;
use crate::uds::Uds;
use std::time::Duration;

const KOMBI: (u16, u16) = (0x714, 0x77E);

/// Über welches Steuergerät das Auto geantwortet hat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Weg {
    Kombi,
    Obd,
}

/// Ergebnis eines Erkennungsversuchs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fund {
    /// Fahrgestellnummer, falls lesbar
    pub vin: Option<String>,
    pub weg: Weg,
}

/// Hat das Steuergerät geantwortet (auch eine Ablehnung ist eine Antwort)?
fn geantwortet<T>(r: &anyhow::Result<T>) -> bool {
    match r {
        Ok(_) => true,
        Err(e) => format!("{e:#}").contains("NRC"),
    }
}

fn vin_aus(d: &[u8]) -> Option<String> {
    let t: String = d.iter().filter(|b| b.is_ascii_alphanumeric()).map(|&b| b as char).collect();
    (t.len() == 17).then_some(t)
}

fn obd_vin(be: &Backend) -> (bool, Option<String>) {
    let Ok(tp) = be.open(crate::obd::DEFAULT_TX, crate::obd::DEFAULT_RX) else { return (false, None) };
    let r = crate::obd::Obd::new(&*tp).vin();
    (geantwortet(&r), r.ok().filter(|v| v.len() == 17))
}

/// Ein Erkennungsversuch (nur lesen). `None` = kein Auto antwortet.
pub fn suchen(be: &Backend) -> Option<Fund> {
    crate::bus::leise(|| {
        if let Ok(tp) = be.open(KOMBI.0, KOMBI.1) {
            let r = Uds::new(&*tp).read_data_by_id(0xF190);
            if geantwortet(&r) {
                drop(tp);
                let vin = r.ok().and_then(|d| vin_aus(&d)).or_else(|| obd_vin(be).1);
                return Some(Fund { vin, weg: Weg::Kombi });
            }
        }
        match obd_vin(be) {
            (true, vin) => Some(Fund { vin, weg: Weg::Obd }),
            _ => None,
        }
    })
}

/// Ist das Auto noch da? Fragt dasselbe Steuergerät wie beim Erkennen.
pub fn noch_da(be: &Backend, weg: Weg) -> bool {
    crate::bus::leise(|| {
        let (tx, rx) = match weg {
            Weg::Kombi => KOMBI,
            Weg::Obd => (crate::obd::DEFAULT_TX, crate::obd::DEFAULT_RX),
        };
        let Ok(tp) = be.open(tx, rx) else { return false };
        let u = Uds::new(&*tp);
        // ein zweiter Versuch, falls das Steuergerät gerade beschäftigt war
        geantwortet(&u.tester_present()) || geantwortet(&u.tester_present())
    })
}

/// Gesendete und empfangene Nachrichten laut Adapter (nur SocketCAN).
pub fn zaehler(be: &Backend) -> Option<(u64, u64)> {
    let Backend::SocketCan { iface } = be else { return None };
    let lies = |n: &str| std::fs::read_to_string(format!("/sys/class/net/{iface}/statistics/{n}")).ok()?.trim().parse::<u64>().ok();
    Some((lies("tx_packets")?, lies("rx_packets")?))
}

/// Lohnt sich jetzt ein Versuch? `vorher` = Zähler nach dem letzten Versuch,
/// `seit` = Zeit seit dem letzten Versuch.
pub fn versuch_lohnt(be: &Backend, vorher: Option<(u64, u64)>, seit: Duration) -> bool {
    match be {
        Backend::Demo => true,
        // ELM327: die Verbindung gehört der jeweiligen Aufgabe, nicht dazwischenfunken
        Backend::Elm327Serial { .. } | Backend::Elm327Tcp { .. } | Backend::Doip { .. } => false,
        Backend::SocketCan { iface } => {
            let aktiv = crate::connect::can_interfaces().into_iter().any(|(n, up, _)| &n == iface && up);
            if !aktiv {
                return false;
            }
            match crate::connect::can_state(iface).as_deref() {
                Some("BUS-OFF") | Some("STOPPED") => false,
                // niemand bestätigt: nur wieder senden, wenn sich am Bus etwas tut
                Some("ERROR-PASSIVE") | Some("ERROR-WARNING") => zaehler(be) != vorher || seit > Duration::from_secs(120),
                _ => true,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_wird_erkannt() {
        // je nach Simulation über das Kombi (Golf 6) oder über OBD
        let f = suchen(&Backend::Demo).expect("Demo antwortet");
        assert!(f.vin.is_some());
        assert!(noch_da(&Backend::Demo, f.weg));
    }

    #[test]
    fn vin_nur_mit_17_zeichen() {
        assert_eq!(vin_aus(b"WVWZZZ1KZAM000017"), Some("WVWZZZ1KZAM000017".into()));
        assert_eq!(vin_aus(b"\0\0abc"), None);
    }
}
