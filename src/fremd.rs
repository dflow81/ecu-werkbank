//! Andere Marken (z. B. Mercedes, BMW, Opel): Steuergeräte am CAN finden,
//! Kennung und Fehlerspeicher mit den genormten Diensten lesen.
//!
//! Was geht: UDS (ISO 14229) und KWP2000 auf CAN (ISO 14230 über ISO-TP),
//! 11-Bit-IDs und 29-Bit-IDs nach ISO 15765-4 (0x18DAxxF1), 500 kbit/s. Fehlercodes werden genormt angezeigt (P/C/B/U);
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
    /// (Anfrage-ID, Antwort-ID); über 0x7FF = 29 Bit
    pub geraete: Vec<(u32, u32)>,
    /// So viele IDs hatten schon Verkehr (nicht benutzt)
    pub verkehr: usize,
    /// Nur OBD-Adressen gefragt (wegen viel Verkehr oder ohne SocketCAN)
    pub nur_obd: bool,
}

fn obd_paare() -> Vec<(u32, u32)> {
    (0x7E0u32..=0x7E7).map(|t| (t, t + 8)).collect()
}

/// 29-Bit-Paar nach ISO 15765-4: Tester 0xF1 → Ziel `ziel`.
pub fn paar_29(ziel: u8) -> (u32, u32) {
    (0x18DA_00F1 | (u32::from(ziel) << 8), 0x18DA_F100 | u32::from(ziel))
}

/// Übliche OBD-Ziele bei 29 Bit (Motor 0x10, Getriebe 0x18, weitere).
const ZIELE_29: [u8; 4] = [0x10, 0x18, 0x17, 0x28];

/// CAN-ID als Text: 11 Bit „0x7e0“, 29 Bit „0x18da10f1“.
pub fn id_text(id: u32) -> String {
    if id > 0x7FF {
        format!("{id:#010x}")
    } else {
        format!("{id:#05x}")
    }
}

/// „0x7e0/0x7e8“ oder „0x18da10f1/0x18daf110“ zurück in IDs.
pub fn ids_aus_text(s: &str) -> Option<(u32, u32)> {
    let (a, b) = s.split_once('/')?;
    let h = |x: &str| u32::from_str_radix(x.trim().trim_start_matches("0x"), 16).ok().filter(|v| *v <= 0x1FFF_FFFF);
    Some((h(a)?, h(b)?))
}

/// Antwortet hier ein Steuergerät? (Tester Present, UDS und KWP gleich; auf den
/// OBD-Adressen sonst eine OBD-Anfrage — der Golf-6-Motor MED17 kennt dort nur OBD)
fn antwortet(be: &Backend, tx: u32, rx: u32) -> bool {
    let Ok(tp) = be.open_id(tx, rx) else { return false };
    let r = Uds::new(&*tp).tester_present();
    if r.is_ok() || r.is_err_and(|e| format!("{e:#}").contains("NRC")) {
        return true;
    }
    ist_obd_adresse(tx) && nur_obd(&*tp)
}

/// Gehört die Anfrage-ID zu den genormten OBD-Adressen (11 oder 29 Bit)?
fn ist_obd_adresse(tx: u32) -> bool {
    (0x7E0..=0x7E7).contains(&tx) || tx & 0xFFFF_00FF == 0x18DA_00F1
}

/// Antwortet das Steuergerät auf OBD (Mode 01, PID 00 — muss jedes OBD-Gerät können)?
fn nur_obd(tp: &dyn crate::transport::Transport) -> bool {
    crate::obd::Obd::new(tp).read_pid(0x00).is_ok()
}

/// Anfrage-IDs, auf die mehrere Steuergeräte geantwortet haben (Sammeladresse,
/// beim VW-Konzern 0x700). Über die lässt sich kein Gerät einzeln lesen.
fn sammeladressen(roh: &[(u16, u16)]) -> BTreeSet<u16> {
    let mut je: std::collections::BTreeMap<u16, BTreeSet<u16>> = Default::default();
    for (t, r) in roh {
        je.entry(*t).or_default().insert(*r);
    }
    je.into_iter().filter(|(_, r)| r.len() > 1).map(|(t, _)| t).collect()
}

/// Mögliche eigene Anfrage-IDs zu einer Antwort-ID, in der Reihenfolge, in der
/// sie probiert werden: gesehene Anfragen (auch die 3 davor — späte Antworten
/// landen bei der nächsten ID), dann die üblichen Abstände (VW-Konzern:
/// Antwort = Anfrage + 0x6A, OBD: + 8), ganz zuletzt eine Sammeladresse.
fn kandidaten_fuer(roh: &[(u16, u16)], sammel: &BTreeSet<u16>, rx: u16) -> Vec<u16> {
    let gesehen: Vec<u16> = roh.iter().filter(|(_, r)| *r == rx).map(|(t, _)| *t).collect();
    let mut k: Vec<u16> = Vec::new();
    let dazu = |t: u16, k: &mut Vec<u16>| {
        if t != rx && !k.contains(&t) {
            k.push(t);
        }
    };
    for t in gesehen.iter().filter(|t| !sammel.contains(t)) {
        for d in 0..4u16 {
            if let Some(x) = t.checked_sub(d).filter(|x| !sammel.contains(x)) {
                dazu(x, &mut k);
            }
        }
    }
    for abstand in [0x6A, 8] {
        if let Some(x) = rx.checked_sub(abstand).filter(|x| !sammel.contains(x)) {
            dazu(x, &mut k);
        }
    }
    for t in gesehen.iter().filter(|t| sammel.contains(t)) {
        dazu(*t, &mut k);
    }
    k
}

/// Steuergeräte suchen. Dauert mit SocketCAN etwa 30 s.
pub fn suchen(be: &Backend, mut fortschritt: impl FnMut(&str)) -> Result<Suche> {
    let Backend::SocketCan { iface } = be else {
        // ELM327 / Demo: nur die OBD-Adressen, über die normale Verbindung
        fortschritt(&tr!("Frage die OBD-Adressen ab…"));
        let mut geraete: Vec<(u32, u32)> = obd_paare().into_iter().filter(|&(t, r)| antwortet(be, t, r)).collect();
        if geraete.is_empty() {
            // kein 11-Bit-OBD: vielleicht ein Fahrzeug mit 29-Bit-Adressen
            fortschritt(&tr!("Keine Antwort mit 11 Bit — versuche 29-Bit-Adressen…"));
            geraete = ZIELE_29.iter().map(|&z| paar_29(z)).filter(|&(t, r)| antwortet(be, t, r)).collect();
        }
        return Ok(Suche { geraete, verkehr: 0, nur_obd: true });
    };
    use crate::can::{CanFrame, CanSocket, EmbeddedFrame, ExtendedId, Id, Socket, StandardId};
    let sock = CanSocket::open(iface)?;
    let _belegt = crate::bus::Belegt::neu();
    crate::connect::fix_hw_loopback(iface, &sock);
    sock.set_nonblocking(true)?;

    // 1. zuhören
    fortschritt(&tr!("Höre 1,5 s zu, welche CAN-IDs schon benutzt werden…"));
    let mut belegt: BTreeSet<u16> = BTreeSet::new();
    let mut belegt29: BTreeSet<u32> = BTreeSet::new();
    let ende = Instant::now() + Duration::from_millis(1500);
    while Instant::now() < ende {
        while let Ok(f) = sock.read_frame() {
            match f.id() {
                Id::Standard(id) => {
                    belegt.insert(id.as_raw());
                }
                Id::Extended(id) => {
                    belegt29.insert(id.as_raw());
                }
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
            // alle Paare merken: eine Sammeladresse (VW 0x700) beantworten viele Geräte,
            // die eigene Anfrage-ID kommt später noch einmal
            if ok && !roh.contains(&(zuletzt, rx)) {
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

    // 2b. 29 Bit (ISO 15765-4): einmal „Tester Present“ an alle (0x18DB33F1),
    // Antworten kommen von 0x18DAF1xx. Nur wenn diese IDs nicht schon Verkehr haben.
    let mut roh29: Vec<u8> = Vec::new();
    let frei29 = !belegt29.iter().any(|id| *id == 0x18DB_33F1 || id & 0xFFFF_FF00 == 0x18DA_F100);
    if frei29 {
        fortschritt(&tr!("Frage Steuergeräte mit 29-Bit-Adressen…"));
        if let Some(f) = ExtendedId::new(0x18DB_33F1).and_then(|id| CanFrame::new(id, &[0x02, 0x3E, 0x00, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA])) {
            let _ = sock.write_frame(&f);
        }
        let bis = Instant::now() + Duration::from_millis(300);
        while Instant::now() < bis {
            while let Ok(f) = sock.read_frame() {
                let Id::Extended(id) = f.id() else { continue };
                let id = id.as_raw();
                let d = f.data();
                let ok = matches!(d, [l, 0x7E, ..] if *l & 0xF0 == 0) || matches!(d, [l, 0x7F, 0x3E, ..] if *l & 0xF0 == 0);
                if ok && id & 0xFFFF_FF00 == 0x18DA_F100 && !roh29.contains(&((id & 0xFF) as u8)) {
                    roh29.push((id & 0xFF) as u8);
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    drop(sock);

    // 3. nachprüfen (späte Antworten können der nächsten ID zugeordnet sein)
    let antworten: BTreeSet<u16> = roh.iter().map(|(_, r)| *r).collect();
    fortschritt(&tr!("Prüfe {a} gefundene Steuergeräte nach…", a = antworten.len()));
    let sammel = sammeladressen(&roh);
    let mut geraete: Vec<(u32, u32)> = Vec::new();
    for rx in antworten {
        let treffer = kandidaten_fuer(&roh, &sammel, rx).into_iter().find(|&t| !belegt.contains(&t) && antwortet(be, u32::from(t), u32::from(rx)));
        if let Some(t) = treffer {
            geraete.push((u32::from(t), u32::from(rx)));
        }
    }
    // 3b. OBD-Adressen, die nur OBD sprechen (Golf-6-Motor MED17 antwortet nicht auf Tester Present)
    for (t, r) in obd_paare() {
        if !geraete.iter().any(|(_, x)| *x == r) && !belegt.contains(&(t as u16)) && !belegt.contains(&(r as u16)) && antwortet(be, t, r) {
            geraete.push((t, r));
        }
    }
    for z in roh29 {
        let (t, r) = paar_29(z);
        if antwortet(be, t, r) {
            geraete.push((t, r));
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
    // überwiegend Text (Leerzeichen zum Auffüllen zählen mit)? sonst Hex
    let lesbare = d.iter().filter(|b| (0x20..0x7F).contains(*b)).count();
    if !t.is_empty() && lesbare * 2 >= d.len() {
        t
    } else {
        d.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ")
    }
}

/// Herstellerspezifischer Fehlercode nach SAE J2012 (P1xxx, P30–P33, B1/B2,
/// C1/C2, U1/U2)? Dieselbe Nummer bedeutet dann je Steuergerät etwas anderes.
fn herstellerspezifisch(code: &str) -> bool {
    let b = code.as_bytes();
    match (b.first(), b.get(1), b.get(2)) {
        (Some(b'P'), Some(b'1'), _) => true,
        (Some(b'P'), Some(b'3'), Some(c)) => (b'0'..=b'3').contains(c),
        (Some(b'B' | b'C' | b'U'), Some(b'1' | b'2'), _) => true,
        _ => false,
    }
}

/// Fehlertext: genormte Codes aus allen Quellen, herstellerspezifische nur,
/// wenn die Fahrzeug-Datenbank sie für genau diese Teilenummer kennt.
fn fehlertext(db: &LabelDb, code: &str, wissen: Option<&crate::db::teile::SgWissen>) -> String {
    let eigen = wissen.and_then(|w| w.fehler.iter().find(|f| f.code.eq_ignore_ascii_case(code)).map(|f| f.text.clone()));
    match eigen {
        Some(t) => t,
        None if herstellerspezifisch(code) => String::new(),
        None => db.dtc_description(code).unwrap_or("").to_string(),
    }
}

fn nicht_unterstuetzt<T>(r: &Result<T>) -> bool {
    r.as_ref().is_err_and(|e| format!("{e:#}").contains("NRC 0x11"))
}

/// Ein Steuergerät auslesen: Kennung, Fahrgestellnummer, Fehler.
pub fn lesen(be: &Backend, db: &LabelDb, tx: u32, rx: u32) -> Result<ModuleReport> {
    let tp = be.open_id(tx, rx)?;
    let u = Uds::new(&*tp);
    let mut ident = Vec::new();
    let mut vin = None;
    let mut protokoll = "UDS";
    let erste = u.read_data_by_id(0xF190);
    // Gar keine Antwort auf UDS, aber auf OBD: nur über OBD auslesen (Golf-6-Motor MED17)
    let keine_antwort = erste.as_ref().is_err_and(|e| !format!("{e:#}").contains("NRC"));
    if keine_antwort && ist_obd_adresse(tx) && nur_obd(&*tp) {
        let o = crate::obd::Obd::new(&*tp);
        let vin = o.vin().ok().filter(|v| v.len() == 17);
        let mut codes: Vec<(String, &str)> = Vec::new();
        for (dienst, art) in [(0x03u8, crate::trn!("gespeichert")), (0x07, crate::trn!("sporadisch/ausstehend"))] {
            for c in o.read_dtcs(dienst).unwrap_or_default() {
                if !codes.iter().any(|(x, _)| *x == c) {
                    codes.push((c, art));
                }
            }
        }
        let dtcs = codes
            .into_iter()
            .map(|(code, art)| DtcEntry { text: db.dtc_description(&code).unwrap_or("").to_string(), code, alt: None, status: crate::i18n::t(art).to_string(), umgebung: vec![], frueher: vec![] })
            .collect();
        let name = match tx {
            0x7E0 | 0x18DA_10F1 => tr!("Motor (OBD)"),
            0x7E1 | 0x18DA_18F1 => tr!("Getriebe (OBD)"),
            _ => tr!("Steuergerät (OBD)"),
        };
        let ident = vec![tr!("Antwortet nur auf OBD (genormte Abgas-Diagnose), nicht auf UDS/KWP")];
        return Ok(ModuleReport { protocol: "OBD".into(), address: None, name, can: format!("{}/{}", id_text(tx), id_text(rx)), ident, vin, dtcs: Some(dtcs) });
    }
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
    // Teilenummer → Wissen der Fahrzeug-Datenbank (für herstellerspezifische Fehlertexte)
    let teil = ident.iter().find_map(|i| i.split_once(": ").filter(|(n, _)| *n == crate::i18n::t("Teilenummer")).map(|(_, v)| v.to_string()));
    let wissen = teil.as_deref().and_then(crate::db::teile::wissen_falls_vorhanden);
    // Fehlerspeicher
    let dtcs = if protokoll == "UDS" {
        u.read_dtcs(0xFF).ok().map(|list| {
            list.iter()
                .map(|d| {
                    let code = d.code();
                    DtcEntry {
                        text: fehlertext(db, &code, wissen.as_ref()),
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
                        text: fehlertext(db, &code, wissen.as_ref()),
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
        0x7E0 | 0x18DA_10F1 => tr!("Motor (OBD)"),
        0x7E1 | 0x18DA_18F1 => tr!("Getriebe (OBD)"),
        _ => system.unwrap_or_default(),
    };
    Ok(ModuleReport { protocol: protokoll.into(), address: None, name, can: format!("{}/{}", id_text(tx), id_text(rx)), ident, vin, dtcs })
}

/// Suche + alle Steuergeräte auslesen → Scan (für PDF und Historie).
pub fn scan(be: &Backend, db: &LabelDb, mut fortschritt: impl FnMut(&str)) -> Result<(ScanReport, Suche)> {
    let s = suchen(be, &mut fortschritt)?;
    let mut modules = Vec::new();
    for &(tx, rx) in &s.geraete {
        fortschritt(&tr!("Lese Steuergerät {a}…", a = id_text(tx)));
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

/// Fehlerspeicher eines Steuergeräts löschen (UDS 0x14, KWP 0x14 FF 00, bei
/// reinen OBD-Geräten Mode 04 — setzt dort auch die Readiness zurück).
/// Schreibt ins Steuergerät — nur nach Bestätigung aufrufen.
pub fn loeschen(be: &Backend, tx: u32, rx: u32, protokoll: &str) -> Result<()> {
    let tp = be.open_id(tx, rx)?;
    if protokoll == "OBD" {
        return crate::obd::Obd::new(&*tp).clear();
    }
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
    fn herstellerspezifische_codes_und_text() {
        for c in ["P1000", "P3012", "B1054", "C2100", "U1100"] {
            assert!(herstellerspezifisch(c), "{c}");
        }
        for c in ["P0300", "P2293", "P3400", "B0050", "C0031", "U0100"] {
            assert!(!herstellerspezifisch(c), "{c}");
        }
        assert_eq!(lesbar(b"KOMBI        "), "KOMBI", "aufgefüllt mit Leerzeichen");
        assert_eq!(lesbar(&[0x01, 0x02, 0x03, 0x41]), "01 02 03 41");
    }

    #[test]
    fn sammeladresse_wird_aufgeloest() {
        // Golf 6: 0x700 beantworten Gateway (0x77B), Kombi (0x77E) und Airbag (0x77F);
        // die Kombi-Antwort auf 0x700 kam zu spät und hängt an 0x701
        let roh = [(0x700, 0x77B), (0x700, 0x77F), (0x701, 0x77E), (0x714, 0x77E), (0x715, 0x77F)];
        let sammel = sammeladressen(&roh);
        assert_eq!(sammel.into_iter().collect::<Vec<_>>(), [0x700]);
        let s = sammeladressen(&roh);
        // Kombi: zuerst die gesehenen Anfragen, die Sammeladresse nie davor
        let k = kandidaten_fuer(&roh, &s, 0x77E);
        assert_eq!(k.first(), Some(&0x701));
        assert!(k.contains(&0x714));
        assert!(!k.contains(&0x700) || k.iter().position(|t| *t == 0x700) > k.iter().position(|t| *t == 0x714));
        // Gateway kam nur über 0x700: übliche VW-Paarung 0x77B − 0x6A = 0x711 vor der Sammeladresse
        let g = kandidaten_fuer(&roh, &s, 0x77B);
        assert_eq!(g.first(), Some(&0x711));
        assert_eq!(g.last(), Some(&0x700));
    }

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
        assert!(!r.modules.iter().any(|m| m.can.starts_with("0x18da")), "29 Bit nur, wenn 11 Bit nichts findet");
    }

    #[test]
    fn neunundzwanzig_bit_in_der_simulation() {
        let _g = crate::sim::TEST_LOCK.lock().unwrap();
        crate::sim::reset();
        let (t, r) = paar_29(0x10);
        assert_eq!((t, r), (0x18DA_10F1, 0x18DA_F110));
        assert!(antwortet(&Backend::Demo, t, r));
        assert!(!antwortet(&Backend::Demo, paar_29(0x77).0, paar_29(0x77).1));
        let m = lesen(&Backend::Demo, &LabelDb::default(), t, r).unwrap();
        assert_eq!(m.can, "0x18da10f1/0x18daf110");
        assert_eq!(m.name, "Motor (OBD)");
        assert_eq!(ids_aus_text(&m.can), Some((t, r)));
        assert_eq!(ids_aus_text("0x7e0/0x7e8"), Some((0x7E0, 0x7E8)));
        assert_eq!(ids_aus_text("0x7e0"), None);
    }
}
