//! Eingebautes Demo-Fahrzeug: simuliert mehrere Steuergeräte ohne Adapter.
//!
//! - UDS-Steuergeräte (moderne Fahrzeuge) inkl. OBD-II-Diensten am Motor
//! - TP-2.0/KWP2000-Steuergeräte (ältere Fahrzeuge)
//! - Messwerte ändern sich über die Zeit (simulierte Fahrt)
//! - Fehlerspeicher lassen sich löschen (bis zum Programmneustart)
//!
//! Zwei Modelle:
//! - [`Modell::Demo`]: frei erfundenes Fahrzeug (Teilenummern, Codes, Werte).
//! - [`Modell::Golf6`]: nachgebaut nach dem Scan eines echten Golf 6 (2010):
//!   gleiche Steuergeräte, Adressen, CAN-IDs, Kennungen und Fehler, auch die
//!   Eigenheiten (MED17 kennt nur `18 00`, „Antwort folgt“, ABS kurz besetzt,
//!   0x29 antwortet nur auf den Kanalaufbau, kein Steuergerät kennt 0x17).
//!   Motorblöcke 1–4 (8 Felder, MED17) und der Kilometerstand im Kombi
//!   (0x2203, 10-km-Schritte) haben den Aufbau vom echten Auto; die Werte
//!   kommen aus der Simulation. Blöcke ab 5 fehlen.
//!   Die Fahrgestellnummer ist erfunden, damit echte Fahrzeugordner unberührt bleiben.
//!
//! Genutzt vom Backend „demo“ (CLI `-t demo` / `-t golf6`, GUI „Demo“) und von `mockecu`.

use crate::tp20::{CanIo, EcuSim};
use crate::transport::Transport;
use anyhow::{bail, Result};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const VIN: &str = "WVWZZZ1KZAW000001";
/// Fahrgestellnummer der Golf-6-Simulation (erfunden, gültiger Aufbau).
pub const GOLF6_VIN: &str = "WVWZZZ1KZAM000017";
/// Kilometerstand der Golf-6-Simulation (Tacho am 29.10.2026).
pub const GOLF6_KM: u32 = 242_488;
/// Kennung für den Kilometerstand im Golf-6-Kombi (am Auto geprüft:
/// 2 Byte, 10-km-Schritte).
pub const GOLF6_KM_DID: u16 = crate::report::KOMBI_KM_DID;

/// Genauen Kilometerstand im Golf-6-Kombi simulieren (Kennung 0x2002, 3 Byte)?
/// Am echten Auto noch nicht geprüft — darum standardmäßig aus.
/// Einschalten: in Tests `set_km_genau(true)`, von Hand `VAG_SIM_KM_GENAU=1`.
static KM_GENAU: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_km_genau(an: bool) {
    KM_GENAU.store(an, std::sync::atomic::Ordering::Relaxed);
}

fn km_genau() -> bool {
    KM_GENAU.load(std::sync::atomic::Ordering::Relaxed) || (!cfg!(test) && std::env::var("VAG_SIM_KM_GENAU").is_ok_and(|v| v == "1"))
}

/// Welches Fahrzeug simuliert wird.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Modell {
    #[default]
    Demo,
    Golf6,
}

impl Modell {
    pub const ALL: [Modell; 2] = [Modell::Demo, Modell::Golf6];

    pub fn name(self) -> &'static str {
        match self {
            Modell::Demo => crate::trs!("Demo-Fahrzeug"),
            Modell::Golf6 => crate::trs!("Golf 6 (2010, nachgebaut)"),
        }
    }

    /// Kurzname für `-t …` und `VAG_DEMO`.
    pub fn kurz(self) -> &'static str {
        match self {
            Modell::Demo => "demo",
            Modell::Golf6 => "golf6",
        }
    }

    pub fn parse(s: &str) -> Option<Modell> {
        match s.trim().to_lowercase().as_str() {
            "demo" | "" => Some(Modell::Demo),
            "golf6" | "golf" | "golf-6" | "demo-golf6" => Some(Modell::Golf6),
            _ => None,
        }
    }
}

#[cfg(not(test))]
fn modell_cell() -> &'static Mutex<Modell> {
    static M: OnceLock<Mutex<Modell>> = OnceLock::new();
    M.get_or_init(|| Mutex::new(std::env::var("VAG_DEMO").ok().and_then(|v| Modell::parse(&v)).unwrap_or_default()))
}

// In Tests: jeder Test-Thread hat sein eigenes simuliertes Fahrzeug,
// damit parallel laufende Tests sich nicht gegenseitig umschalten.
#[cfg(test)]
fn modell_cell() -> &'static Mutex<Modell> {
    thread_local!(static M: &'static Mutex<Modell> = Box::leak(Box::new(Mutex::new(Modell::Demo))));
    M.with(|m| *m)
}

/// Aktuell simuliertes Fahrzeug.
pub fn modell() -> Modell {
    *modell_cell().lock().unwrap()
}

/// Simuliertes Fahrzeug wechseln (setzt es in den Ausgangszustand).
/// Ist es schon das gewählte Modell, passiert nichts.
pub fn set_modell(m: Modell) {
    {
        let mut cur = modell_cell().lock().unwrap();
        if *cur == m {
            return;
        }
        *cur = m;
    }
    reset();
}

/// Fahrgestellnummer des simulierten Fahrzeugs.
pub fn vin() -> &'static str {
    match modell() {
        Modell::Demo => VIN,
        Modell::Golf6 => GOLF6_VIN,
    }
}

/// Kilometerstand des simulierten Fahrzeugs.
pub fn odometer_km() -> u32 {
    match modell() {
        Modell::Demo => ODOMETER_KM,
        Modell::Golf6 => GOLF6_KM,
    }
}

struct UdsModule {
    addr: u16,
    tx: u16,
    rx: u16,
    name: &'static str,
    part: &'static str,
    /// Softwarestand (F189)
    sw: &'static str,
    /// Systemname (F197)
    system: &'static str,
    dtcs: Vec<([u8; 3], u8)>,
    coding: Vec<u8>,
    /// Nur OBD-II (Modus 01–09), keine UDS-Dienste (Golf 6: Motor ist TP 2.0)
    nur_obd: bool,
}

/// Identifikation eines TP-2.0-Steuergeräts.
enum Ident {
    /// Demo-Aufbau: Teilenummer, 00 01, Name
    Demo { part: &'static str, name: &'static str },
    /// 0x1A 0x9B mit festen Feldern wie am Golf 6: Teilenummer (12 Zeichen),
    /// Softwarestand (4 Byte, teils mit gesetztem Bit 7), 9 Byte, Name
    Vag { teil: &'static str, sw: [u8; 4], name: &'static str },
    /// Kein 0x1A, nur 0x22 F187/F189/F197/F191 (Teil, SW, System, HW)
    Nur22 { teil: &'static str, sw: &'static str, system: &'static str, hw: &'static str },
}

/// Eigenheiten echter Steuergeräte.
#[derive(Default, Clone, Copy)]
struct Eigenheit {
    /// 0x18 mit Typ 0x02 → NRC 0x12, nur Typ 0x00 geht (MED17)
    dtc_nur_typ0: bool,
    /// 0x1A zuerst mit NRC 0x78 („Antwort folgt“)
    ident_78: bool,
    /// erste Sitzungsanfrage mit NRC 0x21 („besetzt“)
    erst_besetzt: bool,
}

struct KwpModule {
    addr: u8,
    ident: Ident,
    dtcs: Vec<(u16, u8)>,
    eigen: Eigenheit,
    besetzt_gemeldet: bool,
    /// CAN-ID, auf der der Tester sendet (None = 0x740 + Adresse)
    tester_tx: Option<u16>,
    /// Lange Codierung (nur Geräte mit 0x22; Werte erfunden)
    codierung: Vec<u8>,
}

struct Car {
    start: Instant,
    /// TP-2.0-Adressen, die nur den Kanalaufbau beantworten (kein 0xA1)
    nur_setup: Vec<u8>,
    uds: Vec<UdsModule>,
    kwp: Vec<KwpModule>,
    /// Eingestellte Verläufe je Messwert (Simulation)
    muster: Vec<(Param, crate::control::Muster, Instant)>,
    fehlerbild: Fehlerbild,
    manipuliert: bool,
    /// Stellglied „Leerlauf-Solldrehzahl“ aktiv?
    idle_target: Option<f64>,
    idle_rpm: f64,
    idle_last: Instant,
    /// Letzte OBD-Abfrage an den Motor (Golf 6: sperrt TP 2.0 für ~3 s)
    obd_zuletzt: Option<Instant>,
    /// Demo-Service-Anzeige: km/Tage bis Inspektion, km/Tage bis Ölwechsel
    service: [i16; 4],
    /// Demo-Login (SecurityAccess) am Motor erfolgreich?
    unlocked: bool,
    /// Demo-Anpassungswert, der nur nach Login geschrieben werden darf
    demo_login_val: u16,
}

/// Demo-Service-Anzeige im Ausgangszustand (Inspektion 350 km überfällig).
const DEMO_SERVICE_START: [i16; 4] = [-350, -12, 4_650, 153];
/// Werte nach einer Rückstellung (30 000 km / 2 Jahre, Öl 15 000 km / 1 Jahr).
const DEMO_SERVICE_NEU: [i16; 4] = [30_000, 730, 15_000, 365];

/// So lange sperrt der Golf-6-Motor TP 2.0 nach einer OBD-Abfrage (am Auto gemessen).
pub const GOLF6_OBD_SPERRE: Duration = Duration::from_secs(3);

/// Erste Demo-Kennung der Service-Anzeige am Demo-Motor (4 Kennungen, nur
/// Simulation — beim echten Fahrzeug sitzen sie im Kombi und sind andere).
pub const DEMO_SERVICE_DID: u16 = 0x22A0;

/// Simuliert die Rückstellung über die Tasten am Kombi.
pub fn service_tasten_reset() {
    car().lock().unwrap().service = DEMO_SERVICE_NEU;
}

/// Demo-Kennung der Stellfunktion „Leerlauf-Solldrehzahl“ (nur Simulation).
pub const DEMO_IDLE_DID: u16 = 0xF1C0;

/// Demo-Kennung, die nur nach erfolgreichem Login geschrieben werden darf (nur Simulation).
pub const DEMO_LOGIN_DID: u16 = 0x2A00;

/// Demo-SA2-Bytecode (nur Simulation; am echten Auto kommt der aus den Flash-Daten).
pub const DEMO_SA2: [u8; 31] = [0x68, 0x02, 0x81, 0x49, 0x93, 0xa5, 0x5a, 0x55, 0xaa, 0x4a, 0x05, 0x87, 0x81, 0x05, 0x95, 0x26, 0x68, 0x05, 0x82, 0x49, 0x84, 0x5a, 0xa5, 0xaa, 0x55, 0x87, 0x03, 0xf7, 0x80, 0x6a, 0x4c];
/// Fester Demo-Seed am Motor (nur Simulation).
pub const DEMO_SEED: u32 = 0x1234_5678;

#[cfg(test)]
pub(crate) static TEST_LOCK: Mutex<()> = Mutex::new(());

/// Simulierbare Messwerte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Param {
    Drehzahl,
    Geschwindigkeit,
    Kuehlmittel,
    Last,
    Spannung,
}

impl Param {
    pub const ALL: [Param; 5] = [Param::Drehzahl, Param::Geschwindigkeit, Param::Kuehlmittel, Param::Last, Param::Spannung];

    pub fn name(self) -> &'static str {
        match self {
            Param::Drehzahl => "Drehzahl",
            Param::Geschwindigkeit => "Geschwindigkeit",
            Param::Kuehlmittel => "Kühlmittel",
            Param::Last => "Motorlast",
            Param::Spannung => "Bordspannung",
        }
    }

    pub fn parse(s: &str) -> Option<Param> {
        let s = s.trim().to_lowercase();
        Param::ALL.into_iter().find(|p| {
            let n = p.name().to_lowercase();
            n.starts_with(&s) || (s == "kuehlmittel" && *p == Param::Kuehlmittel) || (s == "spannung" && *p == Param::Spannung) || (s == "last" && *p == Param::Last)
        })
    }
}

/// Verlauf für einen Messwert setzen (None = normale simulierte Fahrt).
pub fn set_muster(p: Param, m: Option<crate::control::Muster>) {
    let mut c = car().lock().unwrap();
    c.muster.retain(|(q, _, _)| *q != p);
    if let Some(m) = m {
        c.muster.push((p, m, Instant::now()));
    }
}

pub fn clear_muster() {
    car().lock().unwrap().muster.clear();
}

/// Aktuell eingestellte Verläufe.
pub fn muster() -> Vec<(Param, crate::control::Muster)> {
    car().lock().unwrap().muster.iter().map(|(p, m, _)| (*p, *m)).collect()
}

/// Sollwert der Demo-Stellfunktion (None = Steuergerät regelt selbst).
pub fn idle_target() -> Option<f64> {
    car().lock().unwrap().idle_target
}

/// Verläufe aus Text setzen, z. B. "drehzahl=schwanken:2000:3000:10;geschwindigkeit=halten:0".
pub fn apply_spec(spec: &str) -> Result<()> {
    for part in spec.split(';').map(str::trim).filter(|x| !x.is_empty()) {
        let (k, v) = part.split_once('=').ok_or_else(|| anyhow::anyhow!("'{part}': Format param=muster"))?;
        if k.trim().eq_ignore_ascii_case("kauf") {
            set_manipuliert(v.trim().eq_ignore_ascii_case("manipuliert"));
            continue;
        }
        if k.trim().eq_ignore_ascii_case("fehler") {
            let f = Fehlerbild::ALL
                .into_iter()
                .find(|f| f.name().to_lowercase().starts_with(&v.trim().to_lowercase()) || format!("{f:?}").eq_ignore_ascii_case(v.trim()))
                .ok_or_else(|| anyhow::anyhow!("Fehlerbild '{v}' unbekannt (keins, falschluft, kraftstoffdruck, fett)"))?;
            set_fehlerbild(f);
            continue;
        }
        let p = Param::parse(k).ok_or_else(|| anyhow::anyhow!("Unbekannter Messwert '{k}' (drehzahl, geschwindigkeit, kuehlmittel, last, spannung)"))?;
        set_muster(p, Some(crate::control::Muster::parse(v)?));
    }
    Ok(())
}

impl Car {
    fn new() -> Self {
        let (uds, kwp, nur_setup) = match modell() {
            Modell::Demo => demo_module(),
            Modell::Golf6 => golf6_module(),
        };
        Self {
            start: Instant::now(),
            nur_setup,
            uds,
            kwp,
            muster: Vec::new(),
            fehlerbild: Fehlerbild::Keins,
            manipuliert: false,
            idle_target: None,
            idle_rpm: 800.0,
            idle_last: Instant::now(),
            obd_zuletzt: None,
            service: DEMO_SERVICE_START,
            unlocked: false,
            demo_login_val: 0x0000,
        }
    }
}

fn uds(addr: u16, tx: u16, name: &'static str, part: &'static str, system: &'static str, dtcs: Vec<([u8; 3], u8)>, coding: Vec<u8>) -> UdsModule {
    UdsModule { addr, tx, rx: tx + 8, name, part, sw: "0010", system, dtcs, coding, nur_obd: false }
}

fn kwp(addr: u8, ident: Ident, dtcs: Vec<(u16, u8)>) -> KwpModule {
    KwpModule { addr, ident, dtcs, eigen: Eigenheit::default(), besetzt_gemeldet: false, tester_tx: None, codierung: vec![0x0F, 0x00, 0x0A, 0x00, 0x41, 0x08] }
}

type Module = (Vec<UdsModule>, Vec<KwpModule>, Vec<u8>);

fn demo_module() -> Module {
    (
        vec![
            uds(0x01, 0x7E0, "Motor", "8K0907115B", "1.4l R4 TSI", vec![([0x01, 0x01, 0x08], 0x2F), ([0x03, 0x00, 0x00], 0x24)], vec![0x00, 0x1A, 0x07]),
            uds(0x02, 0x7E1, "Getriebe", "0B5300036", "DSG DQ200", vec![([0x07, 0x30, 0x00], 0x28)], vec![0x01, 0x00]),
            uds(0x03, 0x7E2, "Bremsen (ABS)", "8K0614517", "ESP MK100", vec![], vec![0x11, 0x00, 0x07]),
        ],
        vec![
            kwp(0x01, Ident::Demo { part: "03G906016", name: "MOTORELEKTRONIK" }, vec![(16684, 0x23)]),
            kwp(0x03, Ident::Demo { part: "1K0907379", name: "ABS/ESP" }, vec![]),
            kwp(0x17, Ident::Demo { part: "1K0920851", name: "KOMBIINSTRUMENT" }, vec![(18101, 0x60)]),
        ],
        vec![],
    )
}

/// Golf 6, 2010, 1.4 TSI — nach dem echten Scan vom 28.10.2026.
fn golf6_module() -> Module {
    let u = |tx: u16, rx: u16, name, part, sw, system, dtcs| UdsModule { addr: tx, tx, rx, name, part, sw, system, dtcs, coding: vec![0x00, 0x00, 0x00], nur_obd: false };
    let uds = vec![
        // Motor beantwortet auf 0x7E0 nur OBD-II, Diagnose läuft über TP 2.0
        UdsModule { addr: 0x01, tx: 0x7E0, rx: 0x7E8, name: "Motor (OBD)", part: "", sw: "", system: "", dtcs: vec![], coding: vec![], nur_obd: true },
        u(0x711, 0x77B, "Wegfahrsperre", "5K0920860F", "0402", "IMMO", vec![]),
        // Kombi meldet Einträge „Test nicht abgeschlossen“ (Status 0x50) — sind keine Fehler
        u(0x714, 0x77E, "Kombiinstrument", "5K0920860F", "0402", "KOMBI", vec![([0x90, 0x12, 0x00], 0x50), ([0xD1, 0x00, 0x00], 0x50)]),
        u(0x715, 0x77F, "Airbag", "5K0959655", "0750", "AirbagVW10", vec![([0x95, 0x13, 0x00], 0x50)]),
        u(0x746, 0x7B0, "Klimaanlage", "5K0907044BD", "0405", "Climatronic", vec![]),
        // Radio: VW-Fehlernummer 0x100006 = 1048582 (das echte RNS 310 nennt
        // sein Format nicht — Herstelleradresse), aktuell + gespeichert
        u(0x76C, 0x7D6, "Radio/Navigation", "3C0035270", "0450", "RNS310", vec![([0x10, 0x00, 0x06], 0x09)]),
    ];
    let k = |addr: u8, tx: u16, ident: Ident, dtcs: Vec<(u16, u8)>| KwpModule { tester_tx: Some(tx), ..kwp(addr, ident, dtcs) };
    let mut motor = k(0x01, 0x740, Ident::Vag { teil: "03C906016", sw: *b"5049", name: "BMED17.5.5       G" }, vec![(8851, 0x20)]);
    motor.eigen = Eigenheit { dtc_nur_typ0: true, ident_78: true, erst_besetzt: false };
    let mut abs = k(0x03, 0x790, Ident::Nur22 { teil: "1K0907379CD", sw: "0120", system: "ESP MK60EC1", hw: "1K0907379AD" }, vec![]);
    abs.eigen.erst_besetzt = true;
    let kwp = vec![
        motor,
        abs,
        k(0x09, 0x7A8, Ident::Vag { teil: "1K0909144H", sw: *b"2901", name: "EPS_ZFLS Kl. 69" }, vec![]),
        k(0x1D, 0x7BA, Ident::Nur22 { teil: "1K9919475", sw: "0001", system: "PARKHILFE 8K", hw: "1K9919475" }, vec![(1627, 0x2E)]),
        k(0x1F, 0x32E, Ident::Vag { teil: "1K0907530AA", sw: *b"0081", name: "BJ533  Gateway   H07" }, vec![]),
        k(0x20, 0x338, Ident::Nur22 { teil: "5K0937086", sw: "0019", system: "BCM PQ35 M", hw: "5K0937086" }, vec![]),
        k(0x22, 0x310, Ident::Vag { teil: "1K0959701AE", sw: *b"1519", name: "BJ386  TUER-SG FT" }, vec![]),
        k(0x23, 0x312, Ident::Vag { teil: "1K0959702AE", sw: *b"1519", name: "BJ387  TUER-SG BT" }, vec![(97, 0x6C), (929, 0x28)]),
        k(0x24, 0x314, Ident::Vag { teil: "1K0959703AG", sw: [0xB1, b'4', b'0', b'1'], name: "BJ388   TUER-SG HL" }, vec![]),
        k(0x25, 0x316, Ident::Vag { teil: "1K0959704AG", sw: [0xB1, b'4', b'0', b'1'], name: "BJ389   TUER-SG HR" }, vec![]),
        k(0x2A, 0x2FC, Ident::Vag { teil: "1K0953549CQ", sw: [0xB0, b'1', b'1', b'1'], name: "BJ0527           053" }, vec![]),
        k(0x43, 0x336, Ident::Nur22 { teil: "1K0907383F", sw: "0100", system: "ConWysTrailer", hw: "1K0907383F" }, vec![]),
    ];
    (uds, kwp, vec![0x29])
}

#[cfg(not(test))]
fn car() -> &'static Mutex<Car> {
    static CAR: OnceLock<Mutex<Car>> = OnceLock::new();
    CAR.get_or_init(|| Mutex::new(Car::new()))
}

#[cfg(test)]
fn car() -> &'static Mutex<Car> {
    thread_local!(static CAR: &'static Mutex<Car> = Box::leak(Box::new(Mutex::new(Car::new()))));
    CAR.with(|c| *c)
}

/// Demo-Fahrzeug in den Ausgangszustand (Fehlerspeicher, Codierung, Muster).
pub fn reset() {
    *car().lock().unwrap() = Car::new();
}

/// UDS-Steuergeräte des Demo-Fahrzeugs: (Adresse, tx, rx, Name).
pub fn uds_modules() -> Vec<(u16, u16, u16, &'static str)> {
    let c = car().lock().unwrap();
    c.uds.iter().map(|m| (m.addr, m.tx, m.rx, m.name)).collect()
}

/// TP-2.0-Adressen des Demo-Fahrzeugs.
pub fn tp20_addresses() -> Vec<u8> {
    car().lock().unwrap().kwp.iter().map(|m| m.addr).collect()
}

/// Simulierte TP-2.0-Steuergeräte für einen virtuellen Bus (auch für `mockecu`).
pub fn tp20_ecus() -> Vec<EcuSim> {
    let c = car().lock().unwrap();
    let mut v: Vec<EcuSim> = c
        .kwp
        .iter()
        .map(|m| {
            let a = m.addr;
            let mut e = EcuSim::new(a, Box::new(move |req| kwp_responses(a, req)));
            if let Some(tx) = m.tester_tx {
                e.tester_tx = tx;
                e.ecu_tx = 0x300;
            }
            if a == 0x01 && modell() == Modell::Golf6 {
                e.gesperrt = Some(Box::new(|| car().lock().unwrap().obd_zuletzt.is_some_and(|t| t.elapsed() < GOLF6_OBD_SPERRE)));
            }
            e
        })
        .collect();
    for &a in &c.nur_setup {
        let mut e = EcuSim::new(a, Box::new(|_| vec![]));
        e.nur_setup = true;
        v.push(e);
    }
    v
}

/// Simulierte Fahrt: Momentanwerte nach `t` Sekunden.
#[derive(Debug, Clone, Copy)]
pub struct Live {
    pub rpm: f64,
    pub speed: f64,
    pub coolant: f64,
    pub load: f64,
    pub throttle: f64,
    pub maf: f64,
    pub map_kpa: f64,
    pub volt: f64,
    pub intake: f64,
    pub runtime: f64,
    /// Gemischkorrektur kurz-/langzeit (%)
    pub stft: f64,
    pub ltft: f64,
}

/// Simuliertes Fehlerbild für den Gemisch-Assistenten.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Fehlerbild {
    #[default]
    Keins,
    /// mager vor allem im Leerlauf
    Falschluft,
    /// mager bei allen Drehzahlen
    Kraftstoffdruck,
    /// zu fett im Leerlauf
    Fett,
}

impl Fehlerbild {
    pub const ALL: [Fehlerbild; 4] = [Fehlerbild::Keins, Fehlerbild::Falschluft, Fehlerbild::Kraftstoffdruck, Fehlerbild::Fett];
    pub fn name(self) -> &'static str {
        match self {
            Fehlerbild::Keins => "kein Fehler",
            Fehlerbild::Falschluft => "Falschluft",
            Fehlerbild::Kraftstoffdruck => "Kraftstoffdruck zu niedrig",
            Fehlerbild::Fett => "zu fett (Tankentlüftung offen)",
        }
    }
}

pub fn set_fehlerbild(f: Fehlerbild) {
    car().lock().unwrap().fehlerbild = f;
}

pub fn fehlerbild() -> Fehlerbild {
    car().lock().unwrap().fehlerbild
}

pub fn live_at(t: f64) -> Live {
    let speed = (50.0 + 35.0 * (t / 12.0).sin()).max(0.0);
    let rpm = 800.0 + speed * 32.0 + 120.0 * (t * 1.3).sin();
    let load = 18.0 + speed * 0.45 + 6.0 * (t * 0.7).sin();
    Live {
        rpm,
        speed,
        coolant: 90.0 - 70.0 * (-t / 120.0).exp(),
        load,
        throttle: 12.0 + speed * 0.3,
        maf: rpm * load / 2200.0,
        map_kpa: 30.0 + load * 0.7,
        volt: 14.1 + 0.1 * (t * 0.5).sin(),
        intake: 24.0 + 2.0 * (t / 30.0).sin(),
        runtime: t,
        stft: 1.5 * (t * 0.9).sin(),
        ltft: 1.6,
    }
}

fn live_now(c: &mut Car) -> Live {
    let mut l = live_at(c.start.elapsed().as_secs_f64());
    let noise = |t: f64, a: f64| a * ((t * 13.7).sin() * 0.6 + (t * 31.1).cos() * 0.4);
    for (p, m, t0) in &c.muster {
        let t = t0.elapsed().as_secs_f64();
        let v = m.ziel(t);
        match p {
            Param::Drehzahl => l.rpm = (v + noise(t, 15.0)).max(0.0),
            Param::Geschwindigkeit => l.speed = v.max(0.0),
            Param::Kuehlmittel => l.coolant = v,
            Param::Last => l.load = v.clamp(0.0, 100.0),
            Param::Spannung => l.volt = v,
        }
    }
    // Stellglied „Leerlauf-Solldrehzahl“: Fahrzeug steht, Drehzahl folgt verzögert
    let dt = c.idle_last.elapsed().as_secs_f64();
    c.idle_last = Instant::now();
    match c.idle_target {
        Some(target) => {
            l.speed = 0.0;
            c.idle_rpm += (target - c.idle_rpm) * (1.0 - (-dt / 0.6).exp());
            l.rpm = c.idle_rpm + noise(c.start.elapsed().as_secs_f64(), 12.0);
            l.load = 12.0 + (c.idle_rpm - 800.0).max(0.0) / 150.0;
        }
        None => c.idle_rpm = l.rpm,
    }
    // Fehlerbild: Gemischkorrektur abhängig von der Drehzahl
    let hi = ((l.rpm - 800.0) / 1700.0).clamp(0.0, 1.0);
    let t = c.start.elapsed().as_secs_f64();
    let (lt, st) = match c.fehlerbild {
        Fehlerbild::Keins => (1.6, 1.5 * (t * 0.9).sin()),
        Fehlerbild::Falschluft => (17.0 - 13.0 * hi, 3.5 + 1.5 * (t * 0.9).sin() - 2.5 * hi),
        Fehlerbild::Kraftstoffdruck => (12.0 + 4.0 * hi, 2.5 + 1.5 * (t * 0.9).sin()),
        Fehlerbild::Fett => (-13.0 + 10.0 * hi, -3.0 + 1.5 * (t * 0.9).sin()),
    };
    l.ltft = lt;
    l.stft = st;
    l
}

const OBD_PIDS: &[u8] = &[
    0x01, 0x02, 0x04, 0x05, 0x06, 0x07, 0x0B, 0x30, 0x0C, 0x0D, 0x0F, 0x10, 0x11, 0x1F, 0x21, 0x2F, 0x31, 0x33, 0x41, 0x42, 0x46, 0x4E, 0xA0, 0xA6,
];

/// Simulierter Kilometerstand.
pub const ODOMETER_KM: u32 = 154_300;

/// Gebrauchtwagen-Szenario „manipuliert“: Fehler gerade gelöscht, Tacho zurückgedreht,
/// Kombiinstrument mit fremder Fahrgestellnummer.
pub fn set_manipuliert(on: bool) {
    car().lock().unwrap().manipuliert = on;
}

pub fn manipuliert() -> bool {
    car().lock().unwrap().manipuliert
}

/// Batterietest-Demo: ab jetzt Spannungsverlauf „Zündung an → Start → Laden“.
pub fn start_battery_demo() {
    *battery_start().lock().unwrap() = Some(Instant::now());
}

fn battery_start() -> &'static Mutex<Option<Instant>> {
    static B: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();
    B.get_or_init(|| Mutex::new(None))
}

/// Spannung im Batterietest-Demo (t = Sekunden seit Start des Tests).
fn battery_profile(t: f64) -> f64 {
    let wobble = 0.03 * (t * 7.0).sin();
    if t < 3.0 {
        12.55 + wobble // Ruhe, Zündung an
    } else if t < 3.3 {
        12.55 - (t - 3.0) / 0.3 * 2.25 // Anlasser greift
    } else if t < 4.5 {
        10.3 + 0.15 * ((t - 3.3) * 18.0).sin() // Anlassen
    } else if t < 6.0 {
        10.3 + (t - 4.5) / 1.5 * 3.9 // Motor läuft, Lichtmaschine regelt hoch
    } else {
        14.2 + wobble
    }
}

/// Bitmaske für „unterstützte PIDs“ ab `base` (0x00, 0x20, …).
/// Golf 6 (2010): kein Kilometerstand (0xA6) und nichts ab 0xA0 über OBD.
const OBD_PIDS_GOLF6: &[u8] = &[
    0x01, 0x02, 0x04, 0x05, 0x06, 0x07, 0x0B, 0x30, 0x0C, 0x0D, 0x0F, 0x10, 0x11, 0x1F, 0x21, 0x2F, 0x31, 0x33, 0x42, 0x46,
];

fn obd_pids() -> &'static [u8] {
    match modell() {
        Modell::Demo => OBD_PIDS,
        Modell::Golf6 => OBD_PIDS_GOLF6,
    }
}

fn supported_mask(base: u8) -> [u8; 4] {
    let mut m = 0u32;
    let pids = obd_pids();
    for &p in pids {
        if p > base && p <= base + 0x20 {
            m |= 1 << (32 - (p - base));
        }
    }
    // Nächster Bereich vorhanden?
    if pids.iter().any(|&p| p > base + 0x20) {
        m |= 1;
    }
    m.to_be_bytes()
}

fn obd_pid(l: &Live, dtc_count: usize, pid: u8, manip: bool) -> Option<Vec<u8>> {
    let u8c = |v: f64| v.round().clamp(0.0, 255.0) as u8;
    let u16b = |v: f64| (v.round().clamp(0.0, 65535.0) as u16).to_be_bytes().to_vec();
    if modell() == Modell::Golf6 {
        let pids = obd_pids();
        let ok = if pid.is_multiple_of(0x20) { pid == 0 || pids.iter().any(|&p| p > pid) } else { pids.contains(&pid) };
        if !ok {
            return None;
        }
    }
    Some(match pid {
        0x00 | 0x20 | 0x40 | 0x60 | 0x80 | 0xA0 => supported_mask(pid).to_vec(),
        0x01 => {
            let mil = if dtc_count > 0 { 0x80 } else { 0 };
            // Ottomotor; Fehlzündung/Kraftstoff/Bauteile verfügbar+fertig;
            // Kat, EVAP, Lambda, Lambda-Heizung verfügbar; EVAP noch nicht fertig.
            vec![mil | dtc_count.min(0x7F) as u8, 0x07, 0x65, 0x04]
        }
        0x04 => vec![u8c(l.load * 255.0 / 100.0)],
        0x06 => vec![u8c(l.stft * 128.0 / 100.0 + 128.0)],
        0x07 => vec![u8c(l.ltft * 128.0 / 100.0 + 128.0)],
        0x05 => vec![u8c(l.coolant + 40.0)],
        0x0B => vec![u8c(l.map_kpa)],
        0x0C => u16b(l.rpm * 4.0),
        0x0D => vec![u8c(l.speed)],
        0x0F => vec![u8c(l.intake + 40.0)],
        0x10 => u16b(l.maf * 100.0),
        0x11 => vec![u8c(l.throttle * 255.0 / 100.0)],
        0x1F => u16b(l.runtime),
        0x21 => u16b(if dtc_count > 0 { 42.0 } else { 0.0 }),
        0x2F => vec![u8c(62.0 * 255.0 / 100.0)],
        0x30 => vec![if manip { 2 } else { 85 }],
        0x31 => u16b(if manip { 12.0 } else { 1234.0 }),
        // Tests in dieser Fahrt (wie PID 01 aufgebaut): Kat fertig, Lambda-Heizung läuft noch
        0x41 => vec![0x00, 0x07, 0x61, 0x40],
        0x4E => u16b(if manip { 15.0 } else { 2600.0 }),
        0x33 => vec![101],
        0x42 => {
            let v = match *battery_start().lock().unwrap() {
                Some(t0) => battery_profile(t0.elapsed().as_secs_f64()),
                None => l.volt,
            };
            u16b(v * 1000.0)
        }
        0xA6 => ((if manip { 98_000 } else { odometer_km() }) * 10).to_be_bytes().to_vec(),
        0x46 => vec![u8c(18.0 + 40.0)],
        _ => return None,
    })
}

/// Antwort eines Demo-UDS-Steuergeräts auf `req` an CAN-ID `tx`.
/// `None` = an dieser ID antwortet niemand.
pub fn uds_response(tx: u16, req: &[u8]) -> Option<Vec<u8>> {
    let tx = if tx == 0x7DF { 0x7E0 } else { tx }; // OBD-Funktionsadresse → Motor
    let mut c = car().lock().unwrap();
    let l = live_now(&mut c);
    let sid = *req.first()?;
    if tx == 0x7E0 && sid <= 0x0A && modell() == Modell::Golf6 {
        c.obd_zuletzt = Some(Instant::now());
    }
    // Demo-Login (SecurityAccess 0x27) am Motor
    if tx == 0x7E0 && modell() == Modell::Demo && sid == 0x27 {
        let level = req.get(1).copied().unwrap_or(1);
        if level % 2 == 1 {
            return Some([vec![0x67, level], DEMO_SEED.to_be_bytes().to_vec()].concat());
        }
        let erwartet = crate::login::sa2(&DEMO_SA2, DEMO_SEED).unwrap_or(0).to_be_bytes();
        if req.get(2..6) == Some(&erwartet[..]) {
            c.unlocked = true;
            return Some(vec![0x67, level]);
        }
        return Some(vec![0x7F, 0x27, 0x35]); // invalidKey
    }
    // Demo-Anpassung hinter Login (0x2A00): lesen frei, schreiben nur nach Login
    if tx == 0x7E0 && modell() == Modell::Demo && (sid == 0x22 || sid == 0x2E) && req.get(1..3) == Some(&DEMO_LOGIN_DID.to_be_bytes()[..]) {
        if sid == 0x22 {
            return Some([vec![0x62, req[1], req[2]], c.demo_login_val.to_be_bytes().to_vec()].concat());
        }
        if !c.unlocked {
            return Some(vec![0x7F, 0x2E, 0x33]); // securityAccessDenied
        }
        if req.len() != 5 {
            return Some(vec![0x7F, 0x2E, 0x13]);
        }
        c.demo_login_val = u16::from_be_bytes([req[3], req[4]]);
        return Some(vec![0x6E, req[1], req[2]]);
    }
    // Demo-Service-Anzeige: lesen (0x22) und schreiben (0x2E, nur erweiterte Sitzung nötig)
    if tx == 0x7E0 && modell() == Modell::Demo && (sid == 0x22 || sid == 0x2E) && req.len() >= 3 {
        let did = u16::from_be_bytes([req[1], req[2]]);
        if (DEMO_SERVICE_DID..DEMO_SERVICE_DID + 4).contains(&did) {
            let i = usize::from(did - DEMO_SERVICE_DID);
            if sid == 0x22 {
                return Some([vec![0x62, req[1], req[2]], c.service[i].to_be_bytes().to_vec()].concat());
            }
            if req.len() != 5 {
                return Some(vec![0x7F, 0x2E, 0x13]);
            }
            c.service[i] = i16::from_be_bytes([req[3], req[4]]);
            return Some(vec![0x6E, req[1], req[2]]);
        }
    }
    // Demo-Stellfunktion: IOControl auf DEMO_IDLE_DID am Motor
    if sid == 0x2F && tx == 0x7E0 && req.len() >= 4 && u16::from_be_bytes([req[1], req[2]]) == DEMO_IDLE_DID {
        match req[3] {
            0x03 if req.len() >= 6 => c.idle_target = Some(f64::from(u16::from_be_bytes([req[4], req[5]]))),
            0x00 => c.idle_target = None,
            _ => return Some(vec![0x7F, 0x2F, 0x31]),
        }
        return Some(vec![0x6F, req[1], req[2], req[3]]);
    }
    let manip = c.manipuliert;
    let golf = modell() == Modell::Golf6;
    let m = c.uds.iter_mut().find(|m| m.tx == tx)?;
    let arg = |i: usize| req.get(i).copied().unwrap_or(0);
    // Golf 6: Motor auf 0x7E0 kennt nur OBD-II — auf UDS-Anfragen kommt nichts
    if m.nur_obd && sid > 0x0A {
        return None;
    }
    Some(match sid {
        // --- OBD-II (nur Motor) ---
        0x01 if m.addr == 0x01 => match obd_pid(&l, m.dtcs.len(), arg(1), manip) {
            Some(d) => [vec![0x41, arg(1)], d].concat(),
            None => vec![0x7F, 0x01, 0x12],
        },
        // Mode 06: Prüfergebnisse
        0x06 if m.addr == 0x01 => {
            let mid = arg(1);
            let rec = |mid: u8, tid: u8, uas: u8, v: u16, lo: u16, hi: u16| {
                let mut r = vec![mid, tid, uas];
                r.extend_from_slice(&v.to_be_bytes());
                r.extend_from_slice(&lo.to_be_bytes());
                r.extend_from_slice(&hi.to_be_bytes());
                r
            };
            let cyl3 = if m.dtcs.is_empty() { 1 } else { 14 };
            const MIDS: &[u8] = &[0x01, 0x02, 0x21, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5];
            let body: Vec<u8> = match mid {
                0x00 | 0x20 | 0x40 | 0x60 | 0x80 | 0xA0 | 0xC0 | 0xE0 => {
                    let mut mask = 0u32;
                    for &x in MIDS {
                        if x > mid && x <= mid.saturating_add(0x20) {
                            mask |= 1 << (32 - u32::from(x - mid));
                        }
                    }
                    if MIDS.iter().any(|&x| x > mid.saturating_add(0x20)) {
                        mask |= 1;
                    }
                    [vec![mid], mask.to_be_bytes().to_vec()].concat()
                }
                0x01 => [rec(0x01, 0x01, 0x0A, 3800, 3500, 6000), rec(0x01, 0x02, 0x0A, 720, 0, 1600)].concat(),
                0x02 => rec(0x02, 0x07, 0x0A, 300, 0, 400),
                0x21 => rec(0x21, 0x01, 0x01, 400, 0, 450),
                0xA1 => rec(0xA1, 0x0B, 0x24, 3 + cyl3, 0, 0),
                0xA2 => rec(0xA2, 0x0B, 0x24, 1, 0, 0),
                0xA3 => rec(0xA3, 0x0B, 0x24, 0, 0, 0),
                0xA4 => rec(0xA4, 0x0B, 0x24, cyl3, 0, 0),
                0xA5 => rec(0xA5, 0x0B, 0x24, 2, 0, 0),
                _ => return Some(vec![0x7F, 0x06, 0x12]),
            };
            [vec![0x46], body].concat()
        }
        // Freeze-Frame: festgehaltene Werte beim ersten Fehler
        0x02 if m.addr == 0x01 => {
            let pid = arg(1);
            if m.dtcs.is_empty() && pid == 0x02 {
                return Some(vec![0x42, 0x02, 0x00, 0x00, 0x00]);
            }
            let frozen = Live { rpm: 2350.0, speed: 72.0, coolant: 88.0, load: 61.0, throttle: 34.0, maf: 31.0, map_kpa: 74.0, volt: 14.0, intake: 26.0, runtime: 900.0, stft: 2.0, ltft: 3.0 };
            match pid {
                0x02 => {
                    let c = m.dtcs.first().map(|d| d.0).unwrap_or([0; 3]);
                    vec![0x42, 0x02, 0x00, c[0], c[1]]
                }
                _ => match obd_pid(&frozen, m.dtcs.len(), pid, false) {
                    Some(d) if pid != 0x01 => [vec![0x42, pid, 0x00], d].concat(),
                    _ => vec![0x7F, 0x02, 0x12],
                },
            }
        }
        0x03 | 0x07 if m.addr == 0x01 => {
            let mut r = vec![sid + 0x40, m.dtcs.len() as u8];
            for (code, _) in &m.dtcs {
                r.extend_from_slice(&code[..2]);
            }
            r
        }
        0x04 if m.addr == 0x01 => {
            m.dtcs.clear();
            vec![0x44]
        }
        0x09 if m.addr == 0x01 => match arg(1) {
            0x00 => vec![0x49, 0x00, 0x54, 0x00, 0x00, 0x00], // 02 (VIN) unterstützt
            0x02 => [vec![0x49, 0x02, 0x01], vin().as_bytes().to_vec()].concat(),
            _ => vec![0x7F, 0x09, 0x12],
        },
        // --- UDS ---
        0x10 => vec![0x50, arg(1), 0x00, 0x32, 0x01, 0xF4],
        0x3E => vec![0x7E, 0x00],
        0x14 => {
            m.dtcs.clear();
            vec![0x54]
        }
        0x27 => {
            let level = req.get(1).copied().unwrap_or(1);
            if level % 2 == 1 {
                vec![0x67, level, 0x12, 0x34, 0x56, 0x78]
            } else {
                vec![0x67, level]
            }
        }
        0x22 => {
            let did = u16::from_be_bytes([arg(1), arg(2)]);
            let data: Vec<u8> = match did {
                0xF190 => vin().as_bytes().to_vec(),
                0xF187 | 0xF191 => m.part.as_bytes().to_vec(),
                0xF189 => m.sw.as_bytes().to_vec(),
                0xF1A3 => b"H03".to_vec(),
                0xF18C => format!("VWZ{}{:07}", m.addr, 4_711_000 + u32::from(m.addr)).into_bytes(),
                0xF197 => m.system.as_bytes().to_vec(),
                // Golf 6: Kilometerstand im Kombi (2 Byte, 10-km-Schritte, wie am Auto)
                GOLF6_KM_DID if golf && m.system == "KOMBI" => ((odometer_km() / 10) as u16).to_be_bytes().to_vec(),
                crate::km::VAG_KM_DID if golf && m.system == "KOMBI" && km_genau() => odometer_km().to_be_bytes()[1..].to_vec(),
                // Demo-Partikelfilter (Kennungen frei gewählt, nur Simulation)
                0x1100 if m.addr == 0x01 => (((l.runtime * 0.02 + 14.0).min(29.0) * 100.0) as u16).to_be_bytes().to_vec(), // Ruß g ·100
                0x1101 if m.addr == 0x01 => (612u16).to_be_bytes().to_vec(),                                             // km seit Regeneration
                0x1102 if m.addr == 0x01 => ((40.0 + l.load * 1.2) as u16).to_be_bytes().to_vec(),                    // Differenzdruck mbar
                0x1103 if m.addr == 0x01 => ((280.0 + l.load * 2.5) as u16).to_be_bytes().to_vec(),                   // Abgastemperatur °C
                0x0600 => m.coding.clone(),
                0xF40C => ((l.rpm * 4.0) as u16).to_be_bytes().to_vec(),
                0xF405 => vec![(l.coolant + 40.0) as u8],
                _ => return Some(vec![0x7F, 0x22, 0x31]),
            };
            [vec![0x62, arg(1), arg(2)], data].concat()
        }
        // Umgebungsbedingungen (Snapshot) zu einem Fehler: Kilometerstand + Drehzahl
        0x19 if arg(1) == 0x04 => {
            let dtc = [arg(2), arg(3), arg(4)];
            match m.dtcs.iter().find(|(c, _)| *c == dtc) {
                Some((c, st)) => {
                    let km = odometer_km() - 1200;
                    let mut r = vec![0x59, 0x04, c[0], c[1], c[2], *st];
                    // Datensatz 1, 2 Kennungen: 0x2002 Kilometerstand (3 Byte), 0xF40C Drehzahl (2 Byte)
                    r.extend_from_slice(&[0x01, 0x02, 0x20, 0x02]);
                    r.extend_from_slice(&km.to_be_bytes()[1..]);
                    r.extend_from_slice(&[0xF4, 0x0C]);
                    r.extend_from_slice(&((2350.0f64 * 4.0) as u16).to_be_bytes());
                    r
                }
                None => vec![0x7F, 0x19, 0x31],
            }
        }
        // Alle bekannten Codes (reportSupportedDTC): die gespeicherten plus ein
        // paar frei erfundene, die nur „Test noch nicht gelaufen“ melden
        // Fehlercode-Format: Golf 6 wie das echte RNS 310 — lehnt die Frage ab
        // (dann gilt die Herstelleradresse); Demo meldet genormte P-Codes.
        0x19 if arg(1) == 0x01 => {
            if modell() == Modell::Golf6 {
                vec![0x7F, 0x19, 0x11]
            } else {
                let n = m.dtcs.len() as u16;
                vec![0x59, 0x01, 0xFF, 0x00, (n >> 8) as u8, n as u8]
            }
        }
        0x19 if arg(1) == 0x0A => {
            let mut r = vec![0x59, 0x0A, 0xFF];
            for (code, status) in &m.dtcs {
                r.extend_from_slice(code);
                r.push(*status);
            }
            for (k, art) in [0x13u8, 0x29, 0x31, 0x08].into_iter().enumerate() {
                r.extend_from_slice(&[0x90, (m.addr as u8).wrapping_add(k as u8 / 2 * 3), art, 0x50]);
            }
            r
        }
        0x19 => {
            let mut r = vec![0x59, 0x02, 0xFF];
            for (code, status) in &m.dtcs {
                r.extend_from_slice(code);
                r.push(*status);
            }
            r
        }
        0x2E => {
            if u16::from_be_bytes([arg(1), arg(2)]) == 0x0600 {
                m.coding = req[3..].to_vec();
            }
            vec![0x6E, arg(1), arg(2)]
        }
        0x31 => vec![0x71, arg(1), arg(2), arg(3)],
        0x2F => vec![0x6F, arg(1), arg(2)],
        _ => vec![0x7F, sid, 0x11],
    })
}

/// Antworten eines Demo-KWP2000-Steuergeräts (TP 2.0) auf `req`.
pub fn kwp_responses(addr: u8, req: &[u8]) -> Vec<Vec<u8>> {
    let mut c = car().lock().unwrap();
    let l = live_now(&mut c);
    let manip = c.manipuliert;
    let golf = modell() == Modell::Golf6;
    let Some(m) = c.kwp.iter_mut().find(|m| m.addr == addr) else { return vec![] };
    let Some(&sid) = req.first() else { return vec![] };
    let did_text = |s: &str| [vec![0x62, req.get(1).copied().unwrap_or(0), req.get(2).copied().unwrap_or(0)], s.as_bytes().to_vec()].concat();
    match sid {
        0x10 if m.eigen.erst_besetzt && !m.besetzt_gemeldet => {
            m.besetzt_gemeldet = true;
            vec![vec![0x7F, 0x10, 0x21]]
        }
        0x10 => vec![vec![0x50, req.get(1).copied().unwrap_or(0x89)]],
        // KWP-Login (SecurityAccess): Seed/Key mit festem XorKey (nur Simulation)
        0x27 if req.get(1) == Some(&0x01) => vec![[vec![0x67, 0x01], DEMO_SEED.to_be_bytes().to_vec()].concat()],
        0x27 => {
            let erwartet = crate::login::kwp_xor_key(crate::login::KWP_ECU_INDEX_STANDARD, DEMO_SEED).to_be_bytes();
            if req.get(2..6) == Some(&erwartet[..]) {
                vec![vec![0x67, 0x02]]
            } else {
                vec![vec![0x7F, 0x27, 0x35]]
            }
        }
        0x1A if req.get(1) == Some(&0x90) && !golf => {
            let vin = if manip && addr == 0x17 { "WVWZZZ1KZ8W123456" } else { VIN };
            vec![[vec![0x5A, 0x90], vin.as_bytes().to_vec()].concat()]
        }
        0x1A => {
            let r = match &m.ident {
                Ident::Demo { part, name } => {
                    let mut r = vec![0x5A, 0x9B];
                    r.extend_from_slice(format!("{part:<11}").as_bytes());
                    r.extend_from_slice(&[0x00, 0x01]);
                    r.extend_from_slice(name.as_bytes());
                    r
                }
                Ident::Vag { teil, sw, name } => {
                    let mut r = vec![0x5A, 0x9B];
                    r.extend_from_slice(format!("{teil:<12}").as_bytes());
                    r.extend_from_slice(sw);
                    // Nicht-Text-Bytes wie am MED17 gelesen
                    r.extend_from_slice(&[0x03, 0x00, 0x00, 0x48, 0x00, 0x00, 0x00, 0x00, 0x00]);
                    r.extend_from_slice(format!("{name:<20}").as_bytes());
                    r
                }
                Ident::Nur22 { .. } => vec![0x7F, 0x1A, 0x12],
            };
            if m.eigen.ident_78 {
                vec![vec![0x7F, 0x1A, 0x78], r]
            } else {
                vec![r]
            }
        }
        0x22 => {
            let did = u16::from_be_bytes([req.get(1).copied().unwrap_or(0), req.get(2).copied().unwrap_or(0)]);
            match (&m.ident, did) {
                (Ident::Nur22 { teil, .. }, 0xF187) => vec![did_text(teil)],
                (Ident::Nur22 { sw, .. }, 0xF189) => vec![did_text(sw)],
                (Ident::Nur22 { system, .. }, 0xF197) => vec![did_text(system)],
                (Ident::Nur22 { hw, .. }, 0xF191) => vec![did_text(hw)],
                // Lange Codierung: Aufbau wie UDS, Werte erfunden (Demo)
                (Ident::Nur22 { .. }, 0x0600) => vec![[vec![0x62, 0x06, 0x00], m.codierung.clone()].concat()],
                _ => vec![vec![0x7F, 0x22, 0x31]],
            }
        }
        // Lange Codierung schreiben (gleiche Länge, Demo)
        0x2E if matches!(m.ident, Ident::Nur22 { .. }) && req.get(1..3) == Some(&[0x06, 0x00]) => {
            if req.len() - 3 != m.codierung.len() {
                vec![vec![0x7F, 0x2E, 0x13]]
            } else {
                m.codierung = req[3..].to_vec();
                vec![vec![0x6E, 0x06, 0x00]]
            }
        }
        // Golf 6: kein Steuergerät kennt 0x17 (am Auto geprüft: 7F 17 11)
        0x17 if golf => vec![vec![0x7F, 0x17, 0x11]],
        // Umgebungsbedingungen zu einem Code (nur Demo, Aufbau erfunden:
        // Priorität, Häufigkeit, Verlernzähler, Kilometerstand 3 Byte)
        0x17 => {
            let code = u16::from_be_bytes([req.get(1).copied().unwrap_or(0), req.get(2).copied().unwrap_or(0)]);
            match m.dtcs.iter().find(|(c, _)| *c == code) {
                Some((c, st)) => {
                    let km = (odometer_km() - 850).to_be_bytes();
                    let mut r = vec![0x57, 0x01];
                    r.extend_from_slice(&c.to_be_bytes());
                    r.extend_from_slice(&[*st, 0x02, 0x01, 0xFF, km[1], km[2], km[3]]);
                    vec![r]
                }
                None => vec![vec![0x7F, 0x17, 0x31]],
            }
        }
        0x18 if m.eigen.dtc_nur_typ0 && req.get(1) != Some(&0x00) => vec![vec![0x7F, 0x18, 0x12]],
        0x18 => {
            let mut r = vec![0x58, m.dtcs.len() as u8];
            for (code, status) in &m.dtcs {
                r.extend_from_slice(&code.to_be_bytes());
                r.push(*status);
            }
            vec![r]
        }
        0x14 => {
            m.dtcs.clear();
            // „Antwort folgt“, dann die eigentliche Antwort
            vec![vec![0x7F, 0x14, 0x78], vec![0x54, 0xFF, 0x00]]
        }
        0x21 => {
            let block = req.get(1).copied().unwrap_or(0);
            let b = |v: f64| v.round().clamp(0.0, 255.0) as u8;
            const LEER: [u8; 3] = [0x25, 0x00, 0x00];
            let vals: Vec<[u8; 3]> = match (m.addr, block) {
                // Golf 6 / MED17: Aufbau wie am Auto gelesen (8 Felder je Block)
                (0x01, 1) if golf => vec![
                    [0x01, 0xC8, b(l.rpm / 40.0)],              // Drehzahl
                    [0x05, 0x0A, b(l.coolant + 100.0)],         // Kühlmittel
                    [0x14, 0x32, b(128.0 + l.stft * 2.56)],     // Lambda-Regelung %
                    [0x10, 0xFF, 0x33],                         // Bits
                    LEER, LEER, LEER, LEER,
                ],
                (0x01, 2) if golf => vec![
                    [0x01, 0xC8, b(l.rpm / 40.0)],
                    [0x21, 0x85, b(l.load * 1.33)],             // Last %
                    [0x16, 0xFF, b(l.load / 10.0)],             // Einspritzzeit ms
                    [0x60, 0x64, 0x65],
                    LEER, [0x25, 0x01, 0x79], [0x25, 0x02, 0x7A], LEER,
                ],
                (0x01, 3) if golf => vec![
                    [0x01, 0xC8, b(l.rpm / 40.0)],
                    [0x60, 0x64, 0x65],
                    [0x21, 0xFF, b(l.throttle * 2.55)],         // Drosselklappe %
                    [0x1B, 0x4B, 0x80],
                    LEER, LEER, LEER, LEER,
                ],
                (0x01, 4) if golf => vec![
                    [0x01, 0xC8, b(l.rpm / 40.0)],
                    [0x15, 0x5E, b(l.volt * 1000.0 / 94.0)],    // Bordspannung
                    [0x05, 0x0A, b(l.coolant + 100.0)],
                    [0x05, 0x0A, b(l.intake + 100.0)],          // Ansaugluft
                    [0x05, 0x0A, b(l.coolant + 100.0)],
                    [0x05, 0x0A, b(l.coolant + 109.0)],
                    [0x05, 0x0A, 0x7D],                          // 25 °C
                    LEER,
                ],
                (0x01, 1) => vec![
                    [0x01, 200, b(l.rpm / 40.0)],       // Drehzahl
                    [0x05, 10, b(l.coolant + 100.0)],  // Kühlmittel
                    [0x07, 100, b(l.speed)],           // Geschwindigkeit
                    [0x02, 250, b(l.load * 2.0)],      // Last %
                ],
                // Nur Demo (erfunden): Phasenlage Nockenwelle und Ladedruck soll/ist
                (0x01, 93) if !golf => vec![
                    [0x01, 200, b(l.rpm / 40.0)],
                    [0x1B, 100, b(125.0 + (l.rpm / 400.0).sin())], // ≈ −3 °
                ],
                (0x01, 115) if !golf => vec![
                    [0x01, 200, b(l.rpm / 40.0)],
                    [0x21, 0x85, b(l.load * 1.33)],
                    [0x12, 250, b((1000.0 + l.load * 10.0) / 10.0)],
                    [0x12, 250, b((970.0 + l.load * 10.0) / 10.0)],
                ],
                (0x01, 2) => vec![
                    [0x06, 100, b(l.volt * 10.0)],     // Bordspannung
                    [0x12, 250, b(l.map_kpa)],         // Saugrohrdruck (mbar)
                    [0x05, 10, b(l.intake + 100.0)],   // Ansaugluft
                ],
                (0x17, 1) if !golf => vec![
                    [0x07, 100, b(l.speed)],
                    [0x01, 200, b(l.rpm / 40.0)],
                    // Kilometerstand (Formel 0x24: a·2560 + b·10)
                    if manip { [0x24, 38, 72] } else { [0x24, 60, 70] },
                ],
                _ => return vec![vec![0x7F, 0x21, 0x31]],
            };
            let mut r = vec![0x61, block];
            for v in vals {
                r.extend_from_slice(&v);
            }
            vec![r]
        }
        _ => vec![vec![0x7F, sid, 0x11]],
    }
}

/// UDS-Transport zum Demo-Fahrzeug (auf Nachrichtenebene, ohne CAN-Frames).
pub struct DemoUds {
    tx: u16,
    inbox: RefCell<VecDeque<Vec<u8>>>,
}

impl DemoUds {
    pub fn new(tx: u16) -> Self {
        Self { tx, inbox: RefCell::default() }
    }
}

impl Transport for DemoUds {
    fn send(&self, data: &[u8]) -> Result<()> {
        if let Some(r) = uds_response(self.tx, data) {
            self.inbox.borrow_mut().push_back(r);
        }
        Ok(())
    }
    fn recv(&self) -> Result<Vec<u8>> {
        // Kleine Verzögerung wie bei echter Hardware.
        std::thread::sleep(Duration::from_millis(2));
        match self.inbox.borrow_mut().pop_front() {
            Some(r) => Ok(r),
            None => bail!("Keine Antwort (Timeout) vom Steuergerät"),
        }
    }
}

/// Virtueller CAN-Bus mit allen Demo-TP-2.0-Steuergeräten.
pub struct DemoCanIo {
    ecus: Mutex<Vec<EcuSim>>,
    inbox: Mutex<VecDeque<(u16, Vec<u8>)>>,
    log: Mutex<Option<Vec<(u16, Vec<u8>)>>>,
}

impl DemoCanIo {
    pub fn new() -> Self {
        let ecus = tp20_ecus();
        Self { ecus: Mutex::new(ecus), inbox: Mutex::default(), log: Mutex::new(None) }
    }

    /// Wie `new`, zeichnet aber jeden Frame auf (für Beispiel-Mitschnitte).
    pub fn with_log() -> Self {
        let s = Self::new();
        *s.log.lock().unwrap() = Some(Vec::new());
        s
    }

    pub fn take_log(&self) -> Vec<(u16, Vec<u8>)> {
        self.log.lock().unwrap().as_mut().map(std::mem::take).unwrap_or_default()
    }

    fn record(&self, id: u16, data: &[u8]) {
        if let Some(l) = self.log.lock().unwrap().as_mut() {
            l.push((id, data.to_vec()));
        }
    }
}

impl Default for DemoCanIo {
    fn default() -> Self {
        Self::new()
    }
}

impl CanIo for DemoCanIo {
    fn write(&self, id: u16, data: &[u8]) -> Result<()> {
        self.record(id, data);
        let mut out = Vec::new();
        for ecu in self.ecus.lock().unwrap().iter_mut() {
            out.extend(ecu.on_frame(id, data));
        }
        for (oid, d) in out {
            self.record(oid, &d);
            self.inbox.lock().unwrap().push_back((oid, d));
        }
        Ok(())
    }

    fn read(&self, id: u16) -> Result<Vec<u8>> {
        let mut inbox = self.inbox.lock().unwrap();
        while let Some((fid, d)) = inbox.pop_front() {
            if fid == id {
                return Ok(d);
            }
        }
        bail!("Timeout beim CAN-Empfang")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_mask_matches_pid_list() {
        // PID 0x01 → höchstes Bit, PID 0x20 nicht in der Liste aber Folgebereich → Bit 0
        let m = u32::from_be_bytes(supported_mask(0x00));
        assert!(m & (1 << 31) != 0);
        assert!(m & 1 != 0);
        let m40 = u32::from_be_bytes(supported_mask(0xA0));
        assert_eq!(m40 & 1, 0);
    }

    #[test]
    fn golf6_wie_echter_scan() {
        let _g = TEST_LOCK.lock().unwrap();
        set_modell(Modell::Golf6);
        let r = crate::report::scan_voll(&crate::transport::Backend::Demo, &crate::labels::LabelDb::default(), true, |_| {});
        set_modell(Modell::Demo);
        let r = r.unwrap();
        // wie am 28.10.2026 am echten Golf 6: 17 Steuergeräte, 5 Fehler
        assert_eq!(r.modules.len(), 17, "{:?}", r.modules.iter().map(|m| m.title()).collect::<Vec<_>>());
        assert_eq!(r.total_dtcs(), 5);
        let motor = r.modules.iter().find(|m| m.protocol == "TP 2.0" && m.address == Some(0x01)).unwrap();
        assert_eq!(motor.dtcs.as_ref().unwrap()[0].code, "08851");
        // Radio meldet VW-Fehlernummern (Format ISO 14229-1) — Anzeige wie VCDS
        let radio = r.modules.iter().find(|m| m.name.contains("Radio")).unwrap();
        let d = &radio.dtcs.as_ref().unwrap()[0];
        assert_eq!((d.code.as_str(), d.alt.as_deref()), ("1048582", None));
        assert!(motor.ident[0].starts_with("03C906016"));
        let abs = r.modules.iter().find(|m| m.address == Some(0x03) && m.protocol == "TP 2.0").unwrap();
        assert!(abs.ident.iter().any(|i| i == "ESP MK60EC1"), "{:?}", abs.ident);
        // Motor auf 0x7E0 ist nur OBD, kein eigener UDS-Eintrag
        assert!(!r.modules.iter().any(|m| m.can.starts_with("0x7e0")));
        assert_eq!(r.vin.as_deref(), Some(GOLF6_VIN));
        // wie am 29.10.2026: km aus dem Kombi (0x2203, auf 10 km abgerundet), TP 2.0 ohne 0x17
        assert_eq!(r.km, Some(GOLF6_KM / 10 * 10));
        assert!(r.modules.iter().filter(|m| m.protocol == "TP 2.0").flat_map(|m| m.dtcs.iter().flatten()).all(|d| d.umgebung.is_empty()));
    }

    #[test]
    fn golf6_motor_nach_obd_kurz_gesperrt() {
        let _g = TEST_LOCK.lock().unwrap();
        set_modell(Modell::Golf6);
        let be = crate::transport::Backend::Demo;
        // wie am 29.10.2026: OBD-Abfrage, danach lehnt der Motor TP 2.0 ~3 s ab
        assert!(crate::profil::quick_vin(&be).is_some());
        let direkt = be.open_tp20(0x01, Duration::from_millis(100));
        let art = direkt.as_ref().err().and_then(crate::tp20::kurzzeitig);
        let e = direkt.err().map(|e| format!("{e:#}")).unwrap_or_default();
        // Suchlauf und Kanal warten die Sperre ab
        let found = crate::kwp::sweep_addrs(&be, &[0x01], |_| {});
        set_modell(Modell::Demo);
        assert!(e.contains("vorübergehend"), "{e}");
        // erkannt an der Fehlerart, nicht am Text (geht auch auf Englisch)
        assert_eq!(art, Some(crate::tp20::Kurzzeitig::Voruebergehend));
        assert_eq!(found.unwrap().len(), 1);
    }

    #[test]
    fn unknown_id_is_silent() {
        assert!(uds_response(0x123, &[0x10, 0x01]).is_none());
    }
}
