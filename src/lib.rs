//! ECUWerkbank — gemeinsame Bibliothek für CLI und GUI.

// Verschachtelte Typen (z. B. Vec<(u16, Vec<u8>)>) sind hier gewollt und lesbar.
#![allow(clippy::type_complexity)]

pub mod akte;
pub mod can;
pub mod anpassung;
pub mod autotest;
pub mod backup;
pub mod blick;
pub mod battery;
pub mod bild;
pub mod canlog;
pub mod chart;
pub mod check;
pub mod codiertexte;
pub mod control;
pub mod connect;
pub mod csvlog;
pub mod datensicherung;
pub mod ablage;
pub mod ansichten;
pub mod auffaelligkeiten;
pub mod canmonitor;
pub mod db;
pub mod dbc;
pub mod firmenkopf;
pub mod dpf;
pub mod doip;
pub mod dtc_db;
pub mod elm327;
pub mod favoriten;
pub mod fehlergeruest;
pub mod fehlertexte;
pub mod grundeinstellung;
pub mod guide;
pub mod history;
pub mod i18n;
pub mod isotp;
pub mod j2534;
pub mod kalender;
pub mod kennungstexte;
pub mod km;
pub mod kosten;
pub mod kw1281;
pub mod ktunnel;
pub mod kwp;
pub mod marken;
pub mod kwp_grund;
pub mod konfigvergleich;
pub mod labels;
pub mod learn;
pub mod limits;
pub mod messwerttexte;
pub mod mixture;
pub mod motorcheck;
pub mod mwvorschlag;
pub mod obd;
pub mod pdf;
pub mod readiness;
pub mod repair;
pub mod replay;
pub mod skript;
pub mod spannung;
pub mod report;
pub mod profil;
pub mod rekorder;
pub mod retrofit;
pub mod service;
pub mod speicher;
pub mod bus;
pub mod erkennung;
pub mod fremd;
pub mod login;
pub mod notfall;
pub mod sim;
pub mod spurexport;
pub mod swinfo;
pub mod systems;
pub mod tp20;
pub mod transport;
pub mod trend;
pub mod uds;
pub mod usedcar;
pub mod vcds;
pub mod vehicle;
pub mod verbauliste;
pub mod web;
pub mod wertetexte;
pub mod wissen;
pub mod zip;

use labels::LabelDb;

/// Dekodiert Codier-Bytes anhand deiner `[[coding]]`-Labels (offline).
/// Liefert je Eintrag (Name, Wert) — Bit → „ein/aus", ganzes Byte → Hex.
pub fn decode_coding(entries: &[labels::Coding], bytes: &[u8]) -> Vec<(String, String)> {
    entries
        .iter()
        .map(|c| {
            let b = bytes.get(c.byte as usize).copied();
            let val = match (b, c.bit) {
                (Some(v), Some(bit)) if bit < 8 => {
                    if (v >> bit) & 1 == 1 { tr!("ein") } else { tr!("aus") }
                }
                (Some(v), _) => format!("{v:#04x}"),
                (None, _) => tr!("— (Byte fehlt)"),
            };
            let label = if c.name.is_empty() {
                format!("Byte {}{}", c.byte, c.bit.map(|b| format!(".{b}")).unwrap_or_default())
            } else {
                c.name.clone()
            };
            (label, val)
        })
        .collect()
}

/// Setzt in `bytes` ein einzelnes Codier-Bit (für Codier-Schreiben).
pub fn set_coding_bit(bytes: &mut [u8], byte: usize, bit: u8, on: bool) -> bool {
    if byte >= bytes.len() || bit >= 8 {
        return false;
    }
    if on {
        bytes[byte] |= 1 << bit;
    } else {
        bytes[byte] &= !(1 << bit);
    }
    true
}

/// Rohbytes als big-endian vorzeichenlose Ganzzahl (bis 8 Byte).
pub fn raw_to_f64(data: &[u8]) -> f64 {
    let mut v: u64 = 0;
    for &b in data.iter().take(8) {
        v = (v << 8) | b as u64;
    }
    v as f64
}

/// Einen Messwert anhand der Labels formatieren: skaliert mit Name+Einheit,
/// sonst ASCII-Text oder Hex. Wird von CLI (`decode`/`rdid`) und GUI genutzt.
pub fn format_value(db: &LabelDb, did: u16, data: &[u8]) -> String {
    if let Some(m) = db.measuring_any(did) {
        let scaled = raw_to_f64(data) * m.factor + m.offset;
        let unit = if m.unit.is_empty() { String::new() } else { format!(" {}", m.unit) };
        return format!("{} (DID {did:#06x}): {:.2}{}", m.name, scaled, unit);
    }
    if !data.is_empty() && data.iter().all(|&b| (0x20..=0x7E).contains(&b)) {
        format!("DID {did:#06x}: \"{}\"", String::from_utf8_lossy(data))
    } else {
        let hex: Vec<String> = data.iter().map(|b| format!("{b:02X}")).collect();
        format!("DID {did:#06x}: {}", hex.join(" "))
    }
}
