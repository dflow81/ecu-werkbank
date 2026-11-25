//! Fehlerspeicher-Protokoll: alle Steuergeräte abfragen, als Text oder PDF.
//!
//! Gemeinsam für CLI (`report`) und GUI („Fehler-PDF“). Eigenes Layout.

use crate::tr;
use crate::kwp;
use crate::labels::LabelDb;
use crate::pdf::{self, Doc, Font};
use crate::transport::Backend;
use crate::uds::Uds;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DtcEntry {
    /// z. B. "P0300" (UDS) oder "16684" (VAG, TP 2.0)
    pub code: String,
    /// Zusätzlicher genormter Code, z. B. "P0300" zu "16684"
    pub alt: Option<String>,
    /// Klartext aus deinen Labels (leer, wenn keiner hinterlegt)
    pub text: String,
    pub status: String,
    /// Umgebungsbedingungen (Name, Wert), z. B. (crate::trs!("Kilometerstand"), "153100 km")
    #[serde(default)]
    pub umgebung: Vec<(String, String)>,
    /// Frühere Reparaturen zu diesem Code (wird beim Erstellen ermittelt)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub frueher: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModuleReport {
    /// "UDS" oder "TP 2.0"
    pub protocol: String,
    pub address: Option<u16>,
    pub name: String,
    pub can: String,
    /// Teilenummer / Kennung
    pub ident: Vec<String>,
    /// Im Steuergerät gespeicherte Fahrgestellnummer (falls lesbar)
    #[serde(default)]
    pub vin: Option<String>,
    /// None = Fehlerspeicher nicht lesbar
    pub dtcs: Option<Vec<DtcEntry>>,
}

impl ModuleReport {
    /// Adresse oder (ohne Label) die Request-CAN-ID.
    pub fn addr_text(&self) -> String {
        match self.address {
            Some(a) => format!("{a:#04x}"),
            None => format!("ID {}", self.can.split('/').next().unwrap_or("")),
        }
    }

    pub fn title(&self) -> String {
        let name = if self.name.is_empty() { "Steuergerät" } else { &self.name };
        format!("{}  {name}", self.addr_text())
    }

    pub fn dtc_count(&self) -> usize {
        self.dtcs.as_ref().map_or(0, Vec::len)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScanReport {
    pub created: String,
    /// Unix-Zeit in Sekunden (für Sortierung/Historie)
    #[serde(default)]
    pub timestamp: u64,
    pub transport: String,
    pub vin: Option<String>,
    #[serde(default)]
    pub note: String,
    /// Fahrzeugordner (siehe crate::vehicle)
    #[serde(default)]
    pub vehicle: String,
    #[serde(default)]
    pub kennzeichen: String,
    /// Bezeichnung, z. B. "Golf 6 1.4 TSI"
    #[serde(default)]
    pub fahrzeug: String,
    /// Kilometerstand (gelesen oder eingegeben)
    #[serde(default)]
    pub km: Option<u32>,
    /// Reparatur-Notiz (nachträglich in der Historie eintragbar)
    #[serde(default)]
    pub reparatur: String,
    /// Scan über „Andere Marken“ (allgemeines UDS/OBD). Beim VW-Konzern ersetzt so
    /// ein Scan nicht den normalen Steuergeräte-Stand des Fahrzeugs.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fremd: bool,
    /// OBD-II-Freeze-Frame: (auslösender Code, Werte)
    #[serde(default)]
    pub freeze: Option<(String, Vec<(String, String)>)>,
    /// Schnell-Check-Ergebnis: (Stufe, Text)
    #[serde(default)]
    pub check: Vec<(crate::limits::Stufe, String)>,
    /// OBD-Prüfergebnisse (Mode 06) als Rohwerte
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pruefwerte: Vec<crate::obd::Pruefwert>,
    /// Readiness je Abgas-Test: (Name, fertig) — leer = nicht gelesen
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub readiness: Vec<(String, bool)>,
    pub modules: Vec<ModuleReport>,
    /// Vergleich mit dem vorherigen Scan desselben Fahrzeugs (nicht gespeichert)
    #[serde(skip)]
    pub comparison: Option<crate::history::Comparison>,
}

/// Bedeutung des UDS-Statusbytes (ISO 14229-1, DTCStatusMask).
pub fn uds_status_text(s: u8) -> String {
    let mut parts = Vec::new();
    if s & 0x01 != 0 {
        parts.push("aktuell");
        // Gerade erkannt, aber weder bestätigt noch vorgemerkt: das Steuergerät
        // hat ihn (noch) nicht abgelegt — VCDS/ODIS zeigen solche Einträge nicht.
        if s & 0x0C == 0 {
            parts.push("nicht gespeichert");
        }
    }
    if s & 0x08 != 0 {
        parts.push("gespeichert");
    }
    if s & 0x04 != 0 {
        parts.push("sporadisch");
    }
    if s & 0x80 != 0 {
        parts.push("Warnlampe");
    }
    if s & 0x2D == 0x20 {
        parts.push("früher aufgetreten");
    }
    if parts.is_empty() {
        format!("{s:#04x}")
    } else {
        format!("{} ({s:#04x})", parts.join(", "))
    }
}

pub fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Lokales Datum + Uhrzeit, z. B. "27.10.2026 16:45".
pub fn now_local() -> String {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as libc::time_t)
        .unwrap_or(0);
    // SAFETY: localtime_r/_s schreibt nur in die übergebene, gültige Struktur.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    #[cfg(unix)]
    let ok = unsafe { !libc::localtime_r(&t, &mut tm).is_null() };
    #[cfg(windows)]
    let ok = unsafe { libc::localtime_s(&mut tm, &t) == 0 };
    if !ok {
        return String::new();
    }
    format!(
        "{:02}.{:02}.{:04} {:02}:{:02}",
        tm.tm_mday,
        tm.tm_mon + 1,
        tm.tm_year + 1900,
        tm.tm_hour,
        tm.tm_min
    )
}

fn ascii(data: &[u8]) -> String {
    if !data.is_empty() && data.iter().all(|&b| (0x20..=0x7E).contains(&b)) {
        String::from_utf8_lossy(data).trim().to_string()
    } else {
        data.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ")
    }
}

/// Alle erreichbaren Steuergeräte abfragen. `progress` bekommt kurze
/// Statustexte (für die GUI).
/// UDS-Ziele: Label-Steuergeräte mit CAN-IDs, OBD-Bereich 0x7E0–0x7E7 und
/// (über SocketCAN) alles, was im schnellen VAG-Suchlauf antwortet.
/// Übliche UDS-Anfrage-IDs von VAG-Steuergeräten (gesehen an Golf 6, Polo 6R und
/// typischen PQ/MQB-Fahrzeugen) — für Adapter ohne Bus-Suchlauf (ELM327).
const VAG_UDS_UEBLICH: [u16; 18] = [0x70A, 0x710, 0x711, 0x712, 0x713, 0x714, 0x715, 0x716, 0x717, 0x740, 0x742, 0x744, 0x746, 0x74F, 0x751, 0x757, 0x76C, 0x773];

pub fn uds_targets(be: &Backend, db: &LabelDb) -> Vec<(Option<u16>, u16, u16)> {
    uds_targets_mit(be, db, None)
}

/// Mit Profil: Label-Steuergeräte + die UDS-Steuergeräte aus dem Profil
/// (kein Suchlauf, kein OBD-Bereich, außer er steht im Profil).
pub fn uds_targets_mit(be: &Backend, db: &LabelDb, profil: Option<&crate::profil::Profil>) -> Vec<(Option<u16>, u16, u16)> {
    if let Some(p) = profil {
        let mut t: Vec<(Option<u16>, u16, u16)> = db.ecus_with_can().into_iter().map(|(a, tx, rx)| (Some(a), tx, rx)).collect();
        t.extend(p.uds.iter().map(|&(tx, rx)| (None, tx, rx)));
        let mut seen = std::collections::HashSet::new();
        t.retain(|x| seen.insert(x.1));
        return t;
    }
    let mut targets: Vec<(Option<u16>, u16, u16)> =
        db.ecus_with_can().into_iter().map(|(a, tx, rx)| (Some(a), tx, rx)).collect();
    for i in 0..8u16 {
        targets.push((None, 0x7E0 + i, 0x7E8 + i));
    }
    match be {
        Backend::SocketCan { iface } => {
            for (tx, rx) in crate::uds::vag_sweep(iface) {
                targets.push((None, tx, rx));
            }
        }
        // Simulation: entspricht dem Suchlauf am echten Bus
        Backend::Demo => {
            for (_, tx, rx, _) in crate::sim::uds_modules() {
                targets.push((None, tx, rx));
            }
        }
        // ELM327 (seriell/WLAN): kein Mithören des Busses, also kein Suchlauf. Stattdessen
        // die UDS-Steuergeräte aus dem Profil dieses Fahrzeugs (frühere Scans, Fahrgestell-
        // nummer per OBD), sonst die üblichen VAG-Kennungen (am Golf 6, 22.11.2026: ohne das
        // fand autoscan über den ELM327 kein einziges UDS-Steuergerät).
        _ => match crate::profil::fuer_scan(be).filter(|(_, p)| !p.uds.is_empty()) {
            Some((_, p)) => targets.extend(p.uds.iter().map(|&(tx, rx)| (None, tx, rx))),
            None => targets.extend(VAG_UDS_UEBLICH.iter().map(|&tx| (None, tx, crate::uds::antwort_id(tx)))),
        },
    }
    // Jede CAN-ID nur einmal (mehrere Label-Dateien können dieselbe nennen)
    let mut seen = std::collections::HashSet::new();
    targets.retain(|t| seen.insert(t.1));
    targets
}

/// Scan aller Steuergeräte. Ist das Fahrzeug bekannt und hat ein Profil,
/// werden nur dessen Steuergeräte abgefragt (Schnell-Scan), sonst alles.
pub fn scan(be: &Backend, db: &LabelDb, with_tp20: bool, progress: impl FnMut(&str)) -> Result<ScanReport> {
    scan_opts(be, db, with_tp20, false, progress)
}

/// Immer der volle Suchlauf (z. B. Kaufcheck, Nachrüstung — findet auch neu
/// verbaute oder fehlende Steuergeräte). Aktualisiert das Profil.
pub fn scan_voll(be: &Backend, db: &LabelDb, with_tp20: bool, progress: impl FnMut(&str)) -> Result<ScanReport> {
    scan_opts(be, db, with_tp20, true, progress)
}

pub fn scan_opts(be: &Backend, db: &LabelDb, with_tp20: bool, voll: bool, mut progress: impl FnMut(&str)) -> Result<ScanReport> {
    let mut modules = Vec::new();
    let mut vin: Option<String> = None;
    let profil = if voll {
        None
    } else {
        progress(crate::trs!("Fahrzeug erkennen…"));
        crate::profil::fuer_scan(be)
    };
    if let Some((v, _)) = &profil {
        vin = Some(v.clone());
    }

    // --- UDS: Label-ECUs mit CAN-IDs + OBD-Bereich ---
    if profil.is_none() {
        progress("UDS-Suchlauf…");
    }
    let targets = uds_targets_mit(be, db, profil.as_ref().map(|p| &p.1));
    for (addr, tx, rx) in targets {
        progress(&tr!("UDS {tx}…", tx = format!("{:#05x}", tx)));
        type Snap = Vec<Vec<(String, String)>>;
        let probe = (|| -> Result<(Vec<u8>, Vec<u8>, Option<Vec<crate::uds::Dtc>>, Snap)> {
            let tp = be.open(tx, rx)?;
            let uds = Uds::new(&*tp).mit_tx(tx);
            uds.diagnostic_session(crate::uds::SESSION_DEFAULT)?;
            let mut part = uds.read_data_by_id(0xF187).unwrap_or_default();
            if let Ok(sys) = uds.read_data_by_id(0xF197) {
                // Systemname anhängen (für den Namen, z. B. „RNS310“)
                part.push(b'|');
                part.extend_from_slice(&sys);
            }
            let v = uds.read_data_by_id(0xF190).unwrap_or_default();
            let dtcs = uds.read_dtcs(0xFF).ok();
            // Umgebungsbedingungen je Fehler (max. 10 je Steuergerät)
            let snaps = dtcs
                .iter()
                .flatten()
                .take(10)
                .map(|d| uds.read_snapshot(d.raw).map(|raw| snapshot_values(&raw, db)).unwrap_or_default())
                .collect();
            Ok((part, v, dtcs, snaps))
        })();
        let Ok((part, v, dtcs, snaps)) = probe else { continue };
        if vin.is_none() && v.len() == 17 {
            vin = Some(ascii(&v));
        }
        let ident: Vec<String> = ascii(&part).split('|').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        let name = addr
            .and_then(|a| db.ecu_name(a).map(str::to_string))
            .unwrap_or_else(|| kwp::tp20_name(0, &ident));
        let name = if name.is_empty() {
            match tx {
                0x7E0 => "Motor (OBD)".to_string(),
                0x7E1 => "Getriebe (OBD)".to_string(),
                _ => name,
            }
        } else {
            name
        };
        let dtcs = dtcs.map(|list| {
            list.iter()
                .enumerate()
                .map(|(i, d)| {
                    let code = d.code();
                    DtcEntry {
                        text: db.dtc_description(&code).unwrap_or("").to_string(),
                        code,
                        alt: crate::dtc_db::fehlerart_kurz(d.fehlerart()),
                        status: uds_status_text(d.status),
                        umgebung: snaps.get(i).cloned().unwrap_or_default(),
                        frueher: vec![],
                    }
                })
                .collect()
        });
        modules.push(ModuleReport {
            protocol: "UDS".into(),
            address: addr,
            name,
            can: format!("{tx:#05x}/{rx:#05x}"),
            ident,
            vin: Some(ascii(&v)).filter(|x| x.len() == 17),
            dtcs,
        });
    }

    // --- TP 2.0 / KWP2000 ---
    if with_tp20 && be.supports_tp20() {
        // Ohne Profil wie ein Werkstatt-Diagnosegerät: erst die Verbauliste aus
        // dem Gateway, dann nur deren TP-2.0-Kanäle (statt 0x01–0x7F). Der volle
        // Suchlauf probiert weiter alle Adressen — findet auch Geräte, die das
        // Gateway nicht kennt.
        let gateway = if profil.is_none() && !voll {
            progress(crate::trs!("Verbauliste aus dem Gateway…"));
            kwp::gateway_verbauliste(be).ok().map(|l| kwp::kanaele_aus_verbauliste(&l))
        } else {
            None
        };
        let found = match (&profil, gateway) {
            (Some((_, p)), _) => kwp::sweep_addrs(be, &p.tp20, |a| progress(&tr!("TP 2.0 {a}…", a = format!("{:#04x}", a))))?,
            (None, Some(k)) => kwp::sweep_addrs(be, &k, |a| progress(&tr!("TP 2.0 {a}…", a = format!("{:#04x}", a))))?,
            (None, None) => kwp::sweep(be, |a| progress(&tr!("TP 2.0 {a}…", a = format!("{:#04x}", a))))?,
        };
        for f in found {
            let umgebung = f.umgebung;
            let dtcs = f.dtcs.map(|list| {
                list.iter()
                    .map(|d| {
                        let umgebung = umgebung.iter().find(|(c, _)| *c == d.code).map(|(_, env)| kwp::umgebung_werte(env)).unwrap_or_default();
                        let alt = d.obd_code(Some(f.address));
                        let text = alt
                            .as_deref()
                            .and_then(|c| db.dtc_description(c))
                            .or_else(|| db.dtc_description(&d.vag_code()))
                            .unwrap_or("")
                            .to_string();
                        DtcEntry { code: d.vag_code(), alt, text, status: d.status_text(), umgebung, frueher: vec![] }
                    })
                    .collect()
            });
            modules.push(ModuleReport {
                protocol: "TP 2.0".into(),
                address: Some(u16::from(f.address)),
                name: if f.name.is_empty() { f.ident.get(1).cloned().unwrap_or_default() } else { f.name.clone() },
                can: format!("{:#05x}/{:#05x}", f.tx, f.rx),
                vin: kwp_vin(be, f.address),
                ident: f.ident,
                dtcs,
            });
        }
    }

    // OBD-II: Freeze-Frame + Kilometerstand (Motor 0x7E0)
    progress("OBD-II…");
    let (freeze, mut km) = match be.open(crate::obd::DEFAULT_TX, crate::obd::DEFAULT_RX) {
        Ok(tp) => {
            let o = crate::obd::Obd::new(&*tp);
            let ff = o.freeze_frame().ok().flatten().map(|f| (f.code, f.values));
            (ff, o.odometer().ok())
        }
        Err(_) => (None, None),
    };
    // Ältere VAG: Kilometerstand im Kombiinstrument (0x17), Messwertblock 1, Formel 0x24
    let kombi = modules.iter().find(|m| m.protocol == "TP 2.0" && m.name.starts_with("Kombi")).and_then(|m| m.address);
    if let (None, true, Some(a)) = (km, with_tp20, kombi) {
        progress("Kilometerstand (Kombi)…");
        km = kwp_odometer(be, a as u8);
    }
    // Golf 6 / PQ35: UDS-Kombi, Kennung 0x2203 (am Golf 6 2010 geprüft)
    if km.is_none() {
        if let Some((tx, rx)) = modules.iter().find(|m| m.protocol == "UDS" && m.name.starts_with("Kombi")).and_then(|m| crate::profil::parse_can(&m.can)) {
            progress("Kilometerstand (Kombi)…");
            km = uds_kombi_odometer(be, tx, rx);
            // genauer Stand: gelernte Stelle aus dem Profil bzw. 0x2002, mit Gegenprobe
            let gelernt = vin.as_deref().and_then(crate::profil::load).and_then(|p| p.km_quelle);
            if let Some((g, _)) = crate::km::genau(be, gelernt.as_ref(), (tx, rx), km) {
                km = Some(g);
            }
        }
    }

    // Voller Scan mit TP 2.0: Profil fürs nächste Mal merken
    if profil.is_none() && with_tp20 && !modules.is_empty() {
        let v = vin.clone().or_else(|| crate::profil::quick_vin(be));
        if let Some(v) = v {
            crate::profil::save(&v, &crate::profil::aus_modulen(&modules));
        }
    }

    Ok(ScanReport {
        fremd: false,
        readiness: Vec::new(),
        created: now_local(),
        timestamp: unix_now(),
        transport: if profil.is_some() { tr!("{a}, Schnell-Scan (bekannte Steuergeräte)", a = be.describe()) } else { be.describe() },
        vin,
        note: String::new(),
        vehicle: String::new(),
        kennzeichen: String::new(),
        fahrzeug: String::new(),
        km,
        reparatur: String::new(),
        freeze,
        check: Vec::new(),
        pruefwerte: Vec::new(),
        modules,
        comparison: None,
    })
}

/// Fahrgestellnummer eines TP-2.0-Steuergeräts (KWP 1A 90, VAG — TODO(verify)).
pub fn kwp_vin(be: &Backend, address: u8) -> Option<String> {
    let mut ch = kwp::open_session(be, address).ok()?;
    let d = kwp::Kwp::new(&mut ch).read_ecu_id(0x90).ok()?;
    let s: String = d.iter().filter(|b| b.is_ascii_alphanumeric()).map(|&b| b as char).collect();
    (s.len() == 17).then_some(s)
}

/// Kilometerstand aus dem Kombiinstrument (TP 2.0, Block 1, Formel 0x24).
pub fn kwp_odometer(be: &Backend, address: u8) -> Option<u32> {
    let mut ch = kwp::open_session(be, address).ok()?;
    let vals = kwp::Kwp::new(&mut ch).read_block(1).ok()?;
    vals.iter().find(|v| v.formula == 0x24).and_then(|v| v.decode()).map(|(km, _)| km as u32)
}

/// Kennung des Kilometerstands im UDS-Kombi (Golf 6 / PQ35).
pub const KOMBI_KM_DID: u16 = 0x2203;

/// Kilometerstand aus dem UDS-Kombi: Kennung 0x2203, 2 Byte in 10-km-Schritten.
/// Am Golf 6 (2010) geprüft: 5E B8 = 24248 → 242 480 km bei 242 488 km im Tacho.
pub fn uds_kombi_odometer(be: &Backend, tx: u16, rx: u16) -> Option<u32> {
    let tp = be.open(tx, rx).ok()?;
    let uds = crate::uds::Uds::new(&*tp);
    let _ = uds.diagnostic_session(0x01);
    kombi_km_aus(&uds.read_data_by_id(KOMBI_KM_DID).ok()?)
}

/// Rohdaten von 0x2203 → km (nur genau 2 Byte, sonst unbekanntes Format).
pub fn kombi_km_aus(d: &[u8]) -> Option<u32> {
    match d {
        [a, b] => Some(u32::from(u16::from_be_bytes([*a, *b])) * 10).filter(|&km| km > 0),
        _ => None,
    }
}

/// Snapshot-Rohdaten → (Name, Wert) mit Labels bzw. bekannten Kennungen.
fn snapshot_values(raw: &[u8], db: &LabelDb) -> Vec<(String, String)> {
    crate::uds::split_snapshot(raw, crate::uds::snapshot_did_len)
        .into_iter()
        .map(|(did, data)| {
            let v = crate::raw_to_f64(&data);
            match did {
                0x2002 => ("Kilometerstand".into(), format!("{v:.0} km")),
                0xF40C => ("Drehzahl".into(), format!("{:.0} 1/min", v / 4.0)),
                0xF405 => ("Kühlmitteltemperatur".into(), format!("{:.0} °C", v - 40.0)),
                0xF40D => ("Geschwindigkeit".into(), format!("{v:.0} km/h")),
                _ => match db.measuring_any(did) {
                    Some(m) => (m.name.clone(), format!("{:.2} {}", v * m.factor + m.offset, m.unit).trim().to_string()),
                    None => (tr!("Kennung {did}", did = format!("{:#06x}", did)), data.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ")),
                },
            }
        })
        .collect()
}

impl ScanReport {
    /// Fahrzeug zuordnen (VIN → bekanntes Fahrzeug, sonst `active`) und
    /// Kennzeichen/Bezeichnung übernehmen.
    pub fn assign_vehicle(&mut self, active: Option<&str>) -> Result<crate::vehicle::Vehicle> {
        let mut v = crate::vehicle::resolve(self.vin.as_deref(), active)?;
        if let Some(km) = self.km {
            if v.note_km(km) {
                crate::vehicle::save(&v)?;
            }
        }
        self.vehicle = v.key.clone();
        self.kennzeichen = v.kennzeichen.clone();
        self.fahrzeug = v.bezeichnung.clone();
        self.attach_repairs();
        Ok(v)
    }

    /// Zu jedem Fehler frühere Reparaturen (dieses und andere eigene Fahrzeuge) eintragen.
    pub fn attach_repairs(&mut self) {
        let key = self.vehicle.clone();
        for m in &mut self.modules {
            for d in m.dtcs.iter_mut().flatten() {
                let mut f = crate::history::earlier_repairs(&key, &d.code);
                if let Some(a) = &d.alt {
                    f.extend(crate::history::earlier_repairs(&key, a));
                }
                f.extend(crate::history::other_vehicle_repairs(&key, d.alt.as_deref().unwrap_or(&d.code)));
                f.dedup();
                d.frueher = f;
            }
        }
    }

    pub fn total_dtcs(&self) -> usize {
        self.modules.iter().map(ModuleReport::dtc_count).sum()
    }

    /// Klartext-Fassung (wie bisher `report` als .txt).
    pub fn to_text(&self) -> String {
        let mut r = String::new();
        r += "ECUWerkbank — Fehlerspeicher-Protokoll\n";
        r += &tr!("Erstellt:   {a}\n", a = self.created);
        if !self.kennzeichen.is_empty() {
            r += &tr!("Kennzeichen: {a}\n", a = self.kennzeichen);
        }
        if !self.fahrzeug.is_empty() {
            r += &tr!("Fahrzeug:   {a}\n", a = self.fahrzeug);
        }
        r += &tr!("Verbindung: {a}\n", a = self.transport);
        if let Some(v) = &self.vin {
            r += &tr!("VIN:        {v}\n", v = v);
        }
        if let Some(km) = self.km {
            r += &tr!("km-Stand:   {km} km\n", km = km);
        }
        if !self.reparatur.is_empty() {
            r += &tr!("Reparatur:  {a}\n", a = self.reparatur);
        }
        for (st, t) in &self.check {
            r += &format!("[{}] {t}\n", st.text());
        }
        if !self.note.is_empty() {
            r += &tr!("Notiz:      {a}\n", a = self.note);
        }
        r += &"=".repeat(60);
        r += "\n";
        if let Some(c) = &self.comparison {
            r += &c.to_text();
            r += &"-".repeat(60);
            r += "\n";
        }
        for m in &self.modules {
            r += &format!("\n{}  [{}]\n", m.title(), m.protocol);
            r += &tr!("  CAN:         {a}\n", a = m.can);
            if !m.ident.is_empty() {
                r += &tr!("  Kennung:     {a}\n", a = m.ident.join(" | "));
            }
            match &m.dtcs {
                None => r += &format!("  {}\n", crate::tr!("Fehlercodes: nicht lesbar")),
                Some(d) if d.is_empty() => r += &format!("  {}\n", crate::tr!("Fehlercodes: keine")),
                Some(d) => {
                    r += &tr!("  Fehlercodes: {a}\n", a = d.len());
                    for e in d {
                        let alt = e.alt.as_ref().map(|a| format!(" ({a})")).unwrap_or_default();
                        r += &tr!("    {a}{alt}  {b}  [Status {c}]\n", a = e.code, alt = alt, b = e.text, c = e.status);
                        for (k, v) in &e.umgebung {
                            r += &format!("        {k}: {v}\n");
                        }
                        for f in &e.frueher {
                            r += &tr!("        Früher: {f}\n", f = f);
                        }
                    }
                }
            }
        }
        if let Some((code, vals)) = &self.freeze {
            r += &tr!("\nOBD-Freeze-Frame (ausgelöst durch {code}):\n", code = code);
            for (k, v) in vals {
                r += &format!("    {k}: {v}\n");
            }
        }
        r += &"=".repeat(60);
        r += "\n";
        r += &tr!("{a} Steuergerät(e), {b} Fehlercode(s) gesamt.\n", a = self.modules.len(), b = self.total_dtcs());
        r
    }

    /// PDF-Fassung.
    pub fn to_pdf(&self) -> Vec<u8> {
        let mut l = Layout::new();
        let accent = (0.10, 0.32, 0.55);
        let red = (0.75, 0.15, 0.12);
        let green = (0.15, 0.50, 0.25);

        // Kopf
        l.doc.banner(accent);
        l.doc.text_rgb(MARGIN, pdf::A4_H - 42.0, 20.0, Font::Bold, crate::trs!("Fehlerspeicher-Protokoll"), (1.0, 1.0, 1.0));
        l.doc.text_rgb(MARGIN, pdf::A4_H - 58.0, 9.0, Font::Regular, crate::trs!("erstellt mit ECUWerkbank"), (0.85, 0.9, 1.0));
        l.y = pdf::A4_H - 100.0;

        let info: Vec<(&str, String)> = [
            (crate::trs!("Datum"), self.created.clone()),
            (crate::trs!("Kennzeichen"), self.kennzeichen.clone()),
            (crate::trs!("Fahrzeug"), self.fahrzeug.clone()),
            (crate::trs!("Fahrgestellnummer"), self.vin.clone().unwrap_or_else(|| crate::tr!("nicht gelesen"))),
            (crate::trs!("Kilometerstand"), self.km.map(|k| format!("{} km", group_thousands(k))).unwrap_or_default()),
            (crate::trs!("Reparatur"), self.reparatur.clone()),
            (crate::trs!("Verbindung"), self.transport.clone()),
            (crate::trs!("Notiz"), self.note.clone()),
        ]
        .into_iter()
        .filter(|(_, v)| !v.is_empty())
        .collect();
        for (k, v) in info {
            l.doc.text_rgb(MARGIN, l.y, 10.0, Font::Regular, k, (0.4, 0.4, 0.4));
            l.doc.text(MARGIN + 110.0, l.y, 10.0, Font::Bold, &v);
            l.y -= 15.0;
        }
        l.y -= 6.0;
        let total = self.total_dtcs();
        let summary = tr!("{a} Steuergerät(e) erreicht, {total} Fehlercode(s)", a = self.modules.len(), total = total);
        l.doc.text_rgb(MARGIN, l.y, 12.0, Font::Bold, &summary, if total > 0 { red } else { green });
        l.y -= 26.0;
        if !self.check.is_empty() {
            l.heading(crate::trs!("Schnell-Check"));
            for (st, t) in &self.check {
                l.ensure(15.0);
                l.doc.rect(MARGIN, l.y - 1.0, 40.0, 11.0, st.rgb());
                l.doc.text_rgb(MARGIN + 4.0, l.y + 1.0, 7.5, Font::Bold, st.text(), (1.0, 1.0, 1.0));
                l.doc.text(MARGIN + 50.0, l.y, 9.5, Font::Regular, &clip(t, 440.0, 9.5));
                l.y -= 15.0;
            }
            l.y -= 8.0;
        }

        // Übersicht
        let cols = [MARGIN, MARGIN + 50.0, MARGIN + 230.0, MARGIN + 385.0, MARGIN + 445.0];
        l.heading(crate::trs!("Übersicht"));
        l.ensure(16.0);
        for (x, h) in cols.iter().zip([crate::trs!("Adresse"), crate::trs!("Steuergerät"), crate::trs!("Kennung"), crate::trs!("System"), crate::trs!("Fehler")]) {
            l.doc.text_rgb(*x, l.y, 9.0, Font::Bold, h, (0.4, 0.4, 0.4));
        }
        l.y -= 5.0;
        l.doc.line(MARGIN, l.y, pdf::A4_W - MARGIN, l.y, 0.6, 0.6);
        l.y -= 13.0;
        if self.modules.is_empty() {
            l.doc.text(MARGIN, l.y, 10.0, Font::Regular, crate::trs!("Keine Steuergeräte erreichbar."));
            l.y -= 14.0;
        }
        for m in &self.modules {
            l.ensure(14.0);
            let addr = m.addr_text();
            let name = clip(if m.name.is_empty() { "–" } else { &m.name }, 170.0, 9.5);
            let ident = clip(&m.ident.first().cloned().unwrap_or_default(), 150.0, 9.5);
            l.doc.text(cols[0], l.y, 9.5, Font::Regular, &addr);
            l.doc.text(cols[1], l.y, 9.5, Font::Regular, &name);
            l.doc.text(cols[2], l.y, 9.5, Font::Regular, &ident);
            l.doc.text(cols[3], l.y, 9.5, Font::Regular, &m.protocol);
            match &m.dtcs {
                None => l.doc.text_rgb(cols[4], l.y, 9.5, Font::Regular, crate::trs!("nicht lesbar"), (0.5, 0.5, 0.5)),
                Some(d) if d.is_empty() => l.doc.text_rgb(cols[4], l.y, 9.5, Font::Bold, crate::trs!("keine"), green),
                Some(d) => l.doc.text_rgb(cols[4], l.y, 9.5, Font::Bold, &d.len().to_string(), red),
            }
            l.y -= 14.0;
        }

        // Vergleich mit dem letzten Scan
        if let Some(c) = &self.comparison {
            l.y -= 12.0;
            l.heading(&tr!("Vergleich mit Scan vom {a}", a = c.since));
            let groups: [(&str, &Vec<crate::history::DiffItem>, (f32, f32, f32)); 3] = [
                ("Neu", &c.diff.new, red),
                ("Behoben", &c.diff.gone, green),
                ("Weiterhin vorhanden", &c.diff.still, (0.35, 0.35, 0.35)),
            ];
            if c.diff.is_empty() {
                l.ensure(14.0);
                l.doc.text(MARGIN, l.y, 10.0, Font::Regular, crate::trs!("Keine Fehler – weder damals noch heute."));
                l.y -= 14.0;
            }
            for (label, items, color) in groups {
                if items.is_empty() {
                    continue;
                }
                l.ensure(16.0);
                l.doc.text_rgb(MARGIN, l.y, 10.0, Font::Bold, &format!("{label} ({})", items.len()), color);
                l.y -= 14.0;
                for it in items {
                    let mut line = format!("{}  {}  –  {}", it.code, it.text, it.module);
                    if it.recurring {
                        line.push_str("  (wiederkehrend)");
                    }
                    for w in pdf::wrap(&line, pdf::A4_W - 2.0 * MARGIN - 14.0, 9.0, Font::Regular).iter() {
                        l.ensure(12.0);
                        l.doc.text(MARGIN + 14.0, l.y, 9.0, Font::Regular, w);
                        l.y -= 12.0;
                    }
                }
                l.y -= 4.0;
            }
        }

        // Details je Steuergerät mit Fehlern
        let with_faults: Vec<&ModuleReport> = self.modules.iter().filter(|m| m.dtc_count() > 0).collect();
        if !with_faults.is_empty() {
            l.y -= 12.0;
            l.heading(crate::trs!("Fehlercodes im Detail"));
        }
        let dcols = [MARGIN + 6.0, MARGIN + 100.0, MARGIN + 360.0];
        let text_w = dcols[2] - dcols[1] - 12.0;
        let status_w = pdf::A4_W - MARGIN - dcols[2];
        for m in with_faults {
            l.ensure(44.0);
            l.doc.rect(MARGIN, l.y - 5.0, pdf::A4_W - 2.0 * MARGIN, 18.0, (0.92, 0.94, 0.97));
            let mut title = m.title();
            if let Some(i) = m.ident.first() {
                title.push_str(&format!("  —  {i}"));
            }
            l.doc.text(MARGIN + 6.0, l.y, 10.5, Font::Bold, &clip(&title, 400.0, 10.5));
            let sys = format!("{} · {}", m.protocol, m.can);
            let x = pdf::A4_W - MARGIN - 6.0 - pdf::text_width(&sys, 8.5, Font::Regular);
            l.doc.text_rgb(x, l.y, 8.5, Font::Regular, &sys, (0.4, 0.4, 0.4));
            l.y -= 22.0;
            for e in m.dtcs.as_deref().unwrap_or(&[]) {
                let text = if e.text.is_empty() { crate::tr!("(kein Klartext hinterlegt)") } else { e.text.clone() };
                let lines = pdf::wrap(&text, text_w, 9.5, Font::Regular);
                // Zweitcode/Fehlerart: passt er neben den Code (z. B. „(P0300)“), steht er dort,
                // sonst (lange Fehlerart) als graue Zeile unter der Beschreibung
                let alt = e.alt.as_ref().map(|a| format!("({a})"));
                let alt_neben = alt.as_ref().filter(|a| pdf::text_width(a, 8.5, Font::Regular) <= dcols[1] - dcols[0] - 46.0);
                let alt_unten: Vec<String> = match (&alt, alt_neben) {
                    (Some(a), None) => pdf::wrap(a, text_w, 8.5, Font::Regular),
                    _ => Vec::new(),
                };
                let status = pdf::wrap(&e.status, status_w, 8.5, Font::Regular);
                let rows = (lines.len() + alt_unten.len()).max(status.len());
                l.ensure(13.0 * rows as f32 + 6.0);
                l.doc.text(dcols[0], l.y, 10.0, Font::Bold, &e.code);
                if let Some(a) = alt_neben {
                    l.doc.text_rgb(dcols[0] + 40.0, l.y, 8.5, Font::Regular, a, (0.4, 0.4, 0.4));
                }
                let gray = if e.text.is_empty() { (0.5, 0.5, 0.5) } else { (0.0, 0.0, 0.0) };
                for (i, line) in lines.iter().enumerate() {
                    l.doc.text_rgb(dcols[1], l.y - 13.0 * i as f32, 9.5, Font::Regular, line, gray);
                }
                for (i, line) in alt_unten.iter().enumerate() {
                    l.doc.text_rgb(dcols[1], l.y - 13.0 * (lines.len() + i) as f32, 8.5, Font::Regular, line, (0.4, 0.4, 0.4));
                }
                for (i, line) in status.iter().enumerate() {
                    l.doc.text_rgb(dcols[2], l.y - 13.0 * i as f32, 8.5, Font::Regular, line, (0.35, 0.35, 0.35));
                }
                l.y -= 13.0 * rows as f32 + 2.0;
                for fr in &e.frueher {
                    for line in pdf::wrap(&tr!("Früher: {fr}", fr = fr), text_w + status_w, 8.0, Font::Bold) {
                        l.ensure(11.0);
                        l.doc.text_rgb(dcols[1], l.y, 8.0, Font::Bold, &line, (0.55, 0.3, 0.05));
                        l.y -= 11.0;
                    }
                }
                if !e.umgebung.is_empty() {
                    let env: Vec<String> = e.umgebung.iter().map(|(k, v)| format!("{k}: {v}")).collect();
                    for line in pdf::wrap(&tr!("Umgebung: {a}", a = env.join(" · ")), text_w + status_w, 8.0, Font::Regular) {
                        l.ensure(11.0);
                        l.doc.text_rgb(dcols[1], l.y, 8.0, Font::Regular, &line, (0.35, 0.35, 0.45));
                        l.y -= 11.0;
                    }
                }
                l.y -= 3.0;
            }
            l.y -= 8.0;
        }

        if let Some((code, vals)) = &self.freeze {
            l.y -= 8.0;
            l.heading(&tr!("OBD-Freeze-Frame (ausgelöst durch {code})", code = code));
            for chunk in vals.chunks(2) {
                l.ensure(13.0);
                for (i, (k, v)) in chunk.iter().enumerate() {
                    let x = MARGIN + 6.0 + i as f32 * 250.0;
                    l.doc.text_rgb(x, l.y, 9.0, Font::Regular, k, (0.4, 0.4, 0.4));
                    l.doc.text(x + 140.0, l.y, 9.0, Font::Bold, v);
                }
                l.y -= 13.0;
            }
        }

        // Fußzeilen mit Seitenzahl
        let n = l.doc.page_count();
        let vin = self.vin.clone().unwrap_or_default();
        for p in 0..n {
            let who = if self.kennzeichen.is_empty() { vin.clone() } else { format!("{} {vin}", self.kennzeichen) };
            l.doc.text_on_page(p, MARGIN, 28.0, 8.0, Font::Regular, &tr!("ECUWerkbank · Fehlerspeicher-Protokoll · {a} · {who}", a = self.created, who = who));
            l.doc.text_on_page(p, pdf::A4_W - MARGIN - 50.0, 28.0, 8.0, Font::Regular, &tr!("Seite {a} von {n}", a = p + 1, n = n));
        }
        l.doc.to_bytes(crate::trs!("Fehlerspeicher-Protokoll"))
    }

    /// Speichern: `.pdf` → PDF, sonst Text.
    pub fn save(&self, path: &Path) -> Result<()> {
        let is_pdf = path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("pdf"));
        let bytes = if is_pdf { self.to_pdf() } else { self.to_text().into_bytes() };
        std::fs::write(path, bytes).with_context(|| tr!("Report schreiben: {a}", a = path.display()))
    }
}

pub(crate) const MARGIN: f32 = 50.0;

/// 154300 → "154.300"
pub fn group_thousands(n: u32) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push('.');
        }
        out.push(c);
    }
    out
}

/// Text auf eine Breite kürzen („…“).
pub(crate) fn clip(s: &str, width: f32, size: f32) -> String {
    if pdf::text_width(s, size, Font::Regular) <= width {
        return s.to_string();
    }
    let mut out = String::new();
    for c in s.chars() {
        if pdf::text_width(&format!("{out}{c}…"), size, Font::Regular) > width {
            break;
        }
        out.push(c);
    }
    format!("{out}…")
}

pub(crate) struct Layout {
    pub(crate) doc: Doc,
    pub(crate) y: f32,
}

impl Layout {
    pub(crate) fn new() -> Self {
        let mut doc = Doc::new();
        doc.new_page();
        Self { doc, y: pdf::A4_H - MARGIN }
    }

    /// Neue Seite, wenn weniger als `h` Platz bleibt.
    pub(crate) fn ensure(&mut self, h: f32) {
        if self.y - h < 55.0 {
            self.doc.new_page();
            self.y = pdf::A4_H - MARGIN;
        }
    }

    pub(crate) fn heading(&mut self, s: &str) {
        self.ensure(40.0);
        self.doc.text_rgb(MARGIN, self.y, 13.0, Font::Bold, s, (0.10, 0.32, 0.55));
        self.y -= 18.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kombi_km_golf6() {
        // am Golf 6 2010: 5E B8 bei 242 488 km im Tacho
        assert_eq!(kombi_km_aus(&[0x5E, 0xB8]), Some(242_480));
        assert_eq!(kombi_km_aus(&[0x00, 0x00]), None);
        assert_eq!(kombi_km_aus(&[0x01, 0x02, 0x03]), None);
    }

    fn sample(n_modules: usize) -> ScanReport {
        let modules = (0..n_modules)
            .map(|i| ModuleReport {
                protocol: if i % 2 == 0 { "UDS".into() } else { "TP 2.0".into() },
                address: Some(i as u16 + 1),
                name: "Motorsteuergerät".into(),
                can: "0x7e0/0x7e8".into(),
                ident: vec!["03G906016".into()],
                vin: None,
                dtcs: Some(vec![DtcEntry {
                    code: "16684".into(),
                    alt: Some("P0300".into()),
                    text: "Verbrennungsaussetzer erkannt (mehrere Zylinder) — Zündung prüfen, Kerzen & Spulen".into(),
                    status: "0x23".into(),
                    umgebung: vec![("Kilometerstand".into(), "153100 km".into())],
                    frueher: vec!["01.05.2026 bei 141.000 km: Zündspule getauscht".into()],
                }]),
            })
            .collect();
        ScanReport {
            fremd: false,
            readiness: Vec::new(),
            created: "27.10.2026 16:45".into(),
            timestamp: 1,
            transport: "Demo".into(),
            vin: Some("WVWZZZ1KZAW000001".into()),
            note: "Kennzeichen B-XY 123".into(),
            vehicle: String::new(),
            kennzeichen: "B-XY 123".into(),
            fahrzeug: "Golf 6".into(),
            km: Some(154300),
            reparatur: "Zündspule Zyl. 3 getauscht".into(),
            freeze: Some(("P0300".into(), vec![("Drehzahl".into(), "2350 1/min".into())])),
            check: vec![(crate::limits::Stufe::Warnung, "2 neue Fehler".into())],
            pruefwerte: Vec::new(),
            modules,
            comparison: None,
        }
    }

    #[test]
    fn pdf_is_wellformed_and_paginates() {
        let one = sample(2).to_pdf();
        assert!(one.starts_with(b"%PDF-1.4"));
        assert!(one.ends_with(b"%%EOF\n"));
        let many = sample(40).to_pdf();
        let pages = String::from_utf8_lossy(&many).matches("/Type /Page ").count();
        assert!(pages >= 3, "{pages} Seiten");
        // Umlaute als WinAnsi-Byte (ä = 0xE4), nicht UTF-8
        assert!(one.windows(1).any(|w| w == [0xE4]));
    }

    #[test]
    fn status_bits() {
        assert_eq!(uds_status_text(0x09), "aktuell, gespeichert (0x09)");
        assert_eq!(uds_status_text(0x00), "0x00");
        // Polo 6R, Wegfahrsperre B104B: VCDS zeigt ihn nicht
        assert_eq!(uds_status_text(0x11), "aktuell, nicht gespeichert (0x11)");
    }
}
