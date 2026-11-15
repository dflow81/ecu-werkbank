//! Andere Marken (z. B. Mercedes, BMW, Opel): Steuergeräte am CAN finden,
//! Kennung und Fehlerspeicher mit den genormten Diensten lesen.
//!
//! Was geht: UDS (ISO 14229) und KWP2000 auf CAN (ISO 14230 über ISO-TP),
//! 11-Bit-IDs, 500 kbit/s. Fehlercodes werden genormt angezeigt (P/C/B/U);
//! Klartext gibt es nur für die genormten Codes (P0xxx …). Herstellereigene
//! Texte, Codierung und Anpassungen gibt es hier nicht.
//!
//! Sicherheit bei der Suche: Erst wird 1,5 s nur zugehört. Kennungen, die
//! schon Verkehr haben, werden nie benutzt. Ist sehr viel los (die OBD-Buchse
//! hängt direkt an einem Fahrzeug-Bus, nicht an einem eigenen Diagnose-Bus),
//! werden nur die OBD-Adressen 0x7E0–0x7E7 gefragt. Gesendet wird nur
//! „Tester Present“ (0x3E) — ein reiner Lebenszeichen-Befehl.

use crate::labels::LabelDb;
use crate::report::{DtcEntry, ModuleReport, ScanReport};
use crate::tr;
use crate::transport::Backend;
use crate::uds::Uds;
use anyhow::Result;
use std::collections::BTreeSet;
use std::time::{Duration, Instant};

/// Ab so vielen fremden IDs gilt der Bus als Fahrzeug-Bus (nicht nur Diagnose).
const VIEL_VERKEHR: usize = 20;

/// Ergebnis der Steuergeräte-Suche.
#[derive(Debug, Clone, Default)]
pub struct Suche {
    /// (Anfrage-ID, Antwort-ID)
    pub geraete: Vec<(u16, u16)>,
    /// So viele IDs hatten schon Verkehr (nicht benutzt)
    pub verkehr: usize,
    /// Nur OBD-Adressen gefragt (wegen viel Verkehr oder ohne SocketCAN)
    pub nur_obd: bool,
}

fn obd_paare() -> Vec<(u16, u16)> {
    (0x7E0u16..=0x7E7).map(|t| (t, t + 8)).collect()
}

/// Antwortet hier ein Steuergerät? (Tester Present, UDS und KWP gleich)
fn antwortet(be: &Backend, tx: u16, rx: u16) -> bool {
    let Ok(tp) = be.open(tx, rx) else { return false };
    let r = Uds::new(&*tp).tester_present();
    r.is_ok() || r.is_err_and(|e| format!("{e:#}").contains("NRC"))
}

/// Steuergeräte suchen. Dauert mit SocketCAN etwa 30 s.
pub fn suchen(be: &Backend, mut fortschritt: impl FnMut(&str)) -> Result<Suche> {
    let Backend::SocketCan { iface } = be else {
        // ELM327 / Demo: nur die OBD-Adressen, über die normale Verbindung
        fortschritt(&tr!("Frage die OBD-Adressen ab…"));
        let geraete = obd_paare().into_iter().filter(|&(t, r)| antwortet(be, t, r)).collect();
        return Ok(Suche { geraete, verkehr: 0, nur_obd: true });
    };
    use crate::can::{CanFrame, CanSocket, EmbeddedFrame, Id, Socket, StandardId};
    let sock = CanSocket::open(iface)?;
    let _belegt = crate::bus::Belegt::neu();
    crate::connect::fix_hw_loopback(iface, &sock);
    sock.set_nonblocking(true)?;

    // 1. zuhören
    fortschritt(&tr!("Höre 1,5 s zu, welche CAN-IDs schon benutzt werden…"));
    let mut belegt: BTreeSet<u16> = BTreeSet::new();
    let ende = Instant::now() + Duration::from_millis(1500);
    while Instant::now() < ende {
        while let Ok(f) = sock.read_frame() {
            if let Id::Standard(id) = f.id() {
                belegt.insert(id.as_raw());
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let nur_obd = belegt.len() > VIEL_VERKEHR;
    let kandidaten: Vec<u16> = if nur_obd {
        (0x7E0u16..=0x7E7).collect()
    } else {
        // 0x7DF = Rundruf an alle, 0x7E8–0x7EF = OBD-Antworten: nicht als Anfrage
        (0x400u16..=0x7FF).filter(|t| *t != 0x7DF && !(0x7E8..=0x7EF).contains(t) && !belegt.contains(t)).collect()
    };

    // 2. jede freie ID kurz fragen
    let mut roh: Vec<(u16, u16)> = Vec::new();
    let lesen = |zuletzt: u16, roh: &mut Vec<(u16, u16)>| {
        while let Ok(f) = sock.read_frame() {
            let Id::Standard(id) = f.id() else { continue };
            let rx = id.as_raw();
            if belegt.contains(&rx) || rx == zuletzt {
                continue;
            }
            // Einzel-Frame: 7E 00 (positiv) oder 7F 3E xx (negativ)
            let d = f.data();
            let ok = matches!(d, [l, 0x7E, ..] if *l & 0xF0 == 0) || matches!(d, [l, 0x7F, 0x3E, ..] if *l & 0xF0 == 0);
            if ok && !roh.iter().any(|(_, r)| *r == rx) {
                roh.push((zuletzt, rx));
            }
        }
    };
    for (i, &tx) in kandidaten.iter().enumerate() {
        if i % 64 == 0 {
            fortschritt(&tr!("Suche Steuergeräte… ID {a} ({b} gefunden)", a = format!("{tx:#05x}"), b = roh.len()));
        }
        if let Some(f) = StandardId::new(tx).and_then(|id| CanFrame::new(id, &[0x02, 0x3E, 0x00, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA])) {
            let _ = sock.write_frame(&f);
        }
        let bis = Instant::now() + Duration::from_millis(25);
        while Instant::now() < bis {
            lesen(tx, &mut roh);
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    let bis = Instant::now() + Duration::from_millis(300);
    while Instant::now() < bis {
        lesen(*kandidaten.last().unwrap_or(&0), &mut roh);
        std::thread::sleep(Duration::from_millis(5));
    }
    drop(sock);

    // 3. nachprüfen (späte Antworten können der nächsten ID zugeordnet sein)
    fortschritt(&tr!("Prüfe {a} gefundene Steuergeräte nach…", a = roh.len()));
    let mut geraete: Vec<(u16, u16)> = Vec::new();
    for (tx, rx) in roh {
        let treffer = (0..4u16).filter_map(|d| tx.checked_sub(d)).find(|&t| !belegt.contains(&t) && antwortet(be, t, rx));
        if let Some(t) = treffer {
            if !geraete.iter().any(|(_, r)| *r == rx) {
                geraete.push((t, rx));
            }
        }
    }
    geraete.sort_unstable();
    Ok(Suche { geraete, verkehr: belegt.len(), nur_obd })
}

/// Genormte Kennungen (UDS ISO 14229 / ISO 15765), dazu übliche Herstellerkennungen.
const KENNUNGEN: [(u16, &str); 9] = [
    (0xF187, crate::trn!("Teilenummer")),
    (0xF111, crate::trn!("Teilenummer (Hersteller)")),
    (0xF18A, crate::trn!("Lieferant")),
    (0xF18C, crate::trn!("Seriennummer")),
    (0xF189, crate::trn!("Software")),
    (0xF121, crate::trn!("Software (Hersteller)")),
    (0xF191, crate::trn!("Hardware")),
    (0xF150, crate::trn!("Hardware-Version")),
    (0xF197, crate::trn!("System")),
];

fn lesbar(d: &[u8]) -> String {
    let t: String = d.iter().map(|&b| if (0x20..0x7F).contains(&b) { b as char } else { ' ' }).collect();
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    // überwiegend Text? sonst Hex
    if !t.is_empty() && t.len() * 2 >= d.len() {
        t
    } else {
        d.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ")
    }
}

fn nicht_unterstuetzt<T>(r: &Result<T>) -> bool {
    r.as_ref().is_err_and(|e| format!("{e:#}").contains("NRC 0x11"))
}

/// Ein Steuergerät auslesen: Kennung, Fahrgestellnummer, Fehler.
pub fn lesen(be: &Backend, db: &LabelDb, tx: u16, rx: u16) -> Result<ModuleReport> {
    let tp = be.open(tx, rx)?;
    let u = Uds::new(&*tp);
    let mut ident = Vec::new();
    let mut vin = None;
    let mut protokoll = "UDS";
    let erste = u.read_data_by_id(0xF190);
    if nicht_unterstuetzt(&erste) {
        // kein UDS-Lesen (0x22): KWP2000 auf CAN, Identifikation über 0x1A
        protokoll = "KWP2000";
        for (opt, name) in [(0x86u8, crate::trn!("Identifikation")), (0x87, crate::trn!("Teilenummer")), (0x90, crate::trn!("Fahrgestellnummer"))] {
            if let Ok(d) = u.request(0x1A, &[opt]) {
                let d = d.get(1..).unwrap_or_default();
                if opt == 0x90 {
                    vin = Some(lesbar(d)).filter(|v| v.len() == 17);
                } else if !d.is_empty() {
                    ident.push(format!("{}: {}", crate::i18n::t(name), lesbar(d)));
                }
            }
        }
    } else {
        vin = erste.ok().map(|d| lesbar(&d)).filter(|v| v.len() == 17);
        for (did, name) in KENNUNGEN {
            if let Ok(d) = u.read_data_by_id(did) {
                if !d.is_empty() {
                    ident.push(format!("{}: {}", crate::i18n::t(name), lesbar(&d)));
                }
            }
        }
    }
    // Fehlerspeicher
    let dtcs = if protokoll == "UDS" {
        u.read_dtcs(0xFF).ok().map(|list| {
            list.iter()
                .map(|d| {
                    let code = d.code();
                    DtcEntry {
                        text: db.dtc_description(&code).unwrap_or("").to_string(),
                        code: format!("{code}-{:02X}", d.fehlerart()),
                        alt: crate::dtc_db::fehlerart_kurz(d.fehlerart()),
                        status: crate::report::uds_status_text(d.status),
                        umgebung: vec![],
                        frueher: vec![],
                    }
                })
                .collect()
        })
    } else {
        // KWP2000: readDTCByStatus (0x18 02 FF 00) → [Anzahl][(hi lo Status)…]
        u.request(0x18, &[0x02, 0xFF, 0x00]).ok().map(|r| {
            r.get(1..)
                .unwrap_or_default()
                .as_chunks::<3>().0.iter()
                .map(|c| {
                    let code = crate::uds::Dtc { raw: (u32::from(c[0]) << 16) | (u32::from(c[1]) << 8), status: c[2], hersteller: false }.code();
                    DtcEntry {
                        text: db.dtc_description(&code).unwrap_or("").to_string(),
                        code,
                        alt: None,
                        status: format!("Status {:#04x}", c[2]),
                        umgebung: vec![],
                        frueher: vec![],
                    }
                })
                .collect()
        })
    };
    let system = ident.iter().find_map(|i| i.split_once(": ").filter(|(n, _)| *n == crate::i18n::t("System")).map(|(_, v)| v.to_string()));
    let name = match tx {
        0x7E0 => tr!("Motor (OBD)"),
        0x7E1 => tr!("Getriebe (OBD)"),
        _ => system.unwrap_or_default(),
    };
    Ok(ModuleReport { protocol: protokoll.into(), address: None, name, can: format!("{tx:#05x}/{rx:#05x}"), ident, vin, dtcs })
}

/// Suche + alle Steuergeräte auslesen → Scan (für PDF und Historie).
pub fn scan(be: &Backend, db: &LabelDb, mut fortschritt: impl FnMut(&str)) -> Result<(ScanReport, Suche)> {
    let s = suchen(be, &mut fortschritt)?;
    let mut modules = Vec::new();
    for &(tx, rx) in &s.geraete {
        fortschritt(&tr!("Lese Steuergerät {a}…", a = format!("{tx:#05x}")));
        if let Ok(m) = lesen(be, db, tx, rx) {
            modules.push(m);
        }
    }
    let vin = modules.iter().find_map(|m| m.vin.clone()).or_else(|| crate::profil::quick_vin(be));
    let mut transport = tr!("{a}, andere Marke (UDS/KWP2000)", a = be.describe());
    if s.nur_obd {
        transport.push_str(&tr!(" — nur OBD-Adressen"));
    }
    let r = ScanReport {
        readiness: Vec::new(),
        created: crate::report::now_local(),
        timestamp: crate::report::unix_now(),
        transport,
        vin,
        note: String::new(),
        vehicle: String::new(),
        kennzeichen: String::new(),
        fahrzeug: String::new(),
        km: None,
        reparatur: String::new(),
        freeze: None,
        check: Vec::new(),
        pruefwerte: Vec::new(),
        modules,
        comparison: None,
    };
    Ok((r, s))
}

/// Fehlerspeicher eines Steuergeräts löschen (UDS 0x14, sonst KWP 0x14 FF 00).
/// Schreibt ins Steuergerät — nur nach Bestätigung aufrufen.
pub fn loeschen(be: &Backend, tx: u16, rx: u16, protokoll: &str) -> Result<()> {
    let tp = be.open(tx, rx)?;
    let u = Uds::new(&*tp);
    if protokoll == "KWP2000" {
        u.request(0x14, &[0xFF, 0x00]).map(|_| ())
    } else {
        u.clear_dtcs()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_oder_hex() {
        assert_eq!(lesbar(b"A2049013200\0\0"), "A2049013200");
        assert_eq!(lesbar(&[0x01, 0x02, 0x03]), "01 02 03");
    }

    #[test]
    fn demo_findet_obd_steuergeraete() {
        let _g = crate::sim::TEST_LOCK.lock().unwrap();
        crate::sim::reset();
        let (r, s) = scan(&Backend::Demo, &LabelDb::default(), |_| {}).unwrap();
        assert!(s.nur_obd);
        assert!(r.modules.iter().any(|m| m.can.starts_with("0x7e0")));
    }
}
