//! Login (SecurityAccess) für Steuergeräte, damit das Schreiben von Anpassungen
//! und Codierung auch bei geschützten Geräten klappt.
//!
//! Viele UDS-Steuergeräte geben vor dem Schreiben erst eine Zufallszahl heraus
//! (den „Seed", 0x27 mit ungeradem Level) und erwarten die passende Antwort
//! (den „Key", gerades Level). Erst danach schreiben sie.
//!
//! Bei VAG wird der Key meist über **SA2** berechnet: ein kleines Programm
//! (Bytecode), das Rechenschritte auf den Seed anwendet. Dieses Programm ist
//! pro Steuergerät und Softwarestand verschieden und steckt in den Flash-Daten
//! des Geräts — **es muss vom Nutzer vorgegeben werden** (wir liefern keine
//! fremde Datenbank mit). Die Rechenmaschine hier ist eigener Code nach der
//! öffentlich dokumentierten SA2-Beschreibung; geprüft gegen bekannte Werte
//! (siehe Tests).
//!
//! Ablage je Fahrzeug: `…/fahrzeuge/<VIN>/logins.toml` (ECU-ID → Verfahren).
//!
//! Hinweis: nur für UDS-Steuergeräte. Bei den älteren TP-2.0-Geräten läuft der
//! Login anders (fester Code bzw. anderes Seed/Key-Verfahren) — das ist hier
//! noch nicht eingebaut.

use crate::kwp::Kwp;
use crate::tr;
use crate::uds::Uds;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// SA2-Rechenmaschine
//
// Kleiner Bytecode-Automat mit einem 32-Bit-Register (Startwert = Seed) und
// einem Übertrags-Flag (Carry). Der Key ist der Registerinhalt am Ende.
// Opcodes nach öffentlich dokumentierter Beschreibung des VAG-SA2-Verfahrens.

/// Fehler „Bytecode passt nicht" (zu kurz, unbekannter Opcode, Endlosschleife).
fn sa2_schritt(reg: &mut u32, carry: &mut bool, tape: &[u8], ip: &mut usize, schleifen: &mut Vec<(usize, u32)>) -> Result<bool> {
    let op = tape[*ip];
    let operand = |n: usize| -> Result<u8> { tape.get(*ip + n).copied().context(tr!("SA2-Bytecode zu kurz")) };
    let wort32 = |von: usize| -> Result<u32> {
        let b = tape.get(von..von + 4).context(tr!("SA2-Bytecode zu kurz"))?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    };
    match op {
        // 0x81: links schieben, oberstes Bit in den Übertrag und unten wieder rein (rotate left)
        0x81 => {
            *carry = *reg & 0x8000_0000 != 0;
            *reg = reg.wrapping_shl(1) | u32::from(*carry);
            *ip += 1;
        }
        // 0x82: rechts schieben (rotate right)
        0x82 => {
            *carry = *reg & 1 != 0;
            *reg >>= 1;
            if *carry {
                *reg |= 0x8000_0000;
            }
            *ip += 1;
        }
        // 0x93: 32-Bit-Wert addieren (Übertrag bei Überlauf)
        0x93 => {
            let w = wort32(*ip + 1)?;
            let (r, ov) = reg.overflowing_add(w);
            *reg = r;
            *carry = ov;
            *ip += 5;
        }
        // 0x84: 32-Bit-Wert abziehen (Übertrag bei Unterlauf)
        0x84 => {
            let w = wort32(*ip + 1)?;
            let (r, ov) = reg.overflowing_sub(w);
            *reg = r;
            *carry = ov;
            *ip += 5;
        }
        // 0x87: mit 32-Bit-Wert XOR-verknüpfen
        0x87 => {
            *reg ^= wort32(*ip + 1)?;
            *ip += 5;
        }
        // 0x68: Schleifenanfang (operand = Anzahl Durchläufe)
        0x68 => {
            let n = u32::from(operand(1)?);
            *ip += 2;
            schleifen.push((*ip, n.saturating_sub(1)));
        }
        // 0x49: Schleifenende
        0x49 => match schleifen.last_mut() {
            Some((start, rest)) if *rest > 0 => {
                *rest -= 1;
                *ip = *start;
            }
            Some(_) => {
                schleifen.pop();
                *ip += 1;
            }
            None => bail!(tr!("SA2: Schleifenende ohne Anfang")),
        },
        // 0x4A: Sprung vorwärts, wenn Übertrag = 0 (operand+2 Bytes)
        0x4A => {
            let w = usize::from(operand(1)?) + 2;
            *ip += if !*carry { w } else { 2 };
        }
        // 0x6B: Sprung vorwärts, immer (operand+2 Bytes)
        0x6B => {
            *ip += usize::from(operand(1)?) + 2;
        }
        // 0x4C: Ende
        0x4C => return Ok(true),
        other => bail!(tr!("SA2: unbekannter Opcode {a}", a = format!("{other:#04x}"))),
    }
    Ok(false)
}

/// Key aus Seed und SA2-Bytecode berechnen.
pub fn sa2(bytecode: &[u8], seed: u32) -> Result<u32> {
    let mut reg = seed;
    let mut carry = false;
    let mut ip = 0usize;
    let mut schleifen: Vec<(usize, u32)> = Vec::new();
    // Obergrenze gegen Endlosschleifen (fehlerhafter Bytecode)
    for _ in 0..1_000_000 {
        if ip >= bytecode.len() {
            return Ok(reg);
        }
        if sa2_schritt(&mut reg, &mut carry, bytecode, &mut ip, &mut schleifen)? {
            return Ok(reg);
        }
    }
    bail!(tr!("SA2: Bytecode läuft nicht zu Ende (fehlerhaft?)"))
}

// ---------------------------------------------------------------------------
// KWP2000-Login (TP 2.0) — fester Seed/Key-Algorithmus „XorKey"
//
// Ältere VAG-Steuergeräte (Bordnetz, ABS, Türen …) geben über 0x27 einen
// 4-Byte-Seed heraus; der Key wird mit diesem festen Verfahren berechnet. Kein
// gerätespezifischer Bytecode nötig — nur eine feste Tabelle und ein Index
// (0–63). In der Praxis nutzen die Geräte Index 63 (am Golf bestätigt).
//
// Eigener Code nach öffentlich dokumentierter Beschreibung. Die Tabelle sind
// zurückentwickelte Konstanten zur Kommunikation mit VAG-Steuergeräten; sie
// tauchen gleich in bri3d/kwp2000 (XorSeedKeyLogin.java) und im
// nefariousmotorsports-Forum (MED9.1-Thread) auf. Siehe CREDITS.md.
// Geprüft gegen bekannte Werte (siehe Tests).

const XOR_SEED: [u32; 64] = [
    0x0A221289, 0x144890A1, 0x24212491, 0x290A0285, 0x42145091, 0x504822C1, 0x0A24C4C1, 0x14252229,
    0x24250525, 0x2510A491, 0x28488863, 0x29148885, 0x422184A5, 0x49128521, 0x50844A85, 0x620CC211,
    0x124452A9, 0x18932251, 0x2424A459, 0x29149521, 0x42352621, 0x4A512289, 0x52A48911, 0x11891475,
    0x22346523, 0x4A3118D1, 0x64497111, 0x0AE34529, 0x15398989, 0x22324A67, 0x2D12B489, 0x132A4A75,
    0x19B13469, 0x25D2C453, 0x4949349B, 0x524E9259, 0x1964CA6B, 0x24F5249B, 0x28979175, 0x352A5959,
    0x3A391749, 0x51D44EA9, 0x564A4F25, 0x6AD52649, 0x76493925, 0x25DE52C9, 0x332E9333, 0x68D64997,
    0x494947FB, 0x33749ACF, 0x5AD55B5D, 0x7F272A4F, 0x35BD5B75, 0x3F5AD55D, 0x5B5B6DAD, 0x6B5DAD6B,
    0x75B57AD5, 0x5DBAD56F, 0x6DBF6AAD, 0x75775EB5, 0x5AEDFED5, 0x6B5F7DD5, 0x6F757B6B, 0x5FBD5DBD,
];

/// In der Praxis verwendeter Tabellen-Index (am Golf 6 bestätigt).
pub const KWP_ECU_INDEX_STANDARD: u8 = 63;

/// KWP2000-Key aus Seed und Tabellen-Index (0–63) berechnen.
pub fn kwp_xor_key(ecu_index: u8, seed: u32) -> u32 {
    let faktor = XOR_SEED[usize::from(ecu_index) & 63];
    let mut s = seed;
    for _ in 0..5 {
        let msb = s & 0x8000_0000 != 0;
        s = s.rotate_left(1);
        if msb {
            s ^= faktor;
        }
    }
    s
}

// ---------------------------------------------------------------------------
// Login-Verfahren je Steuergerät

/// Wie sich das Programm bei einem Steuergerät anmeldet (nur UDS).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "art")]
pub enum Methode {
    /// SecurityAccess mit SA2: Seed über Level anfordern, Key per Bytecode rechnen.
    Sa2 {
        /// Seed-Level (ungerade, z. B. 3). Key wird mit Level+1 gesendet.
        level: u8,
        /// SA2-Bytecode (hex), aus den Flash-Daten des Geräts.
        bytecode: String,
    },
    /// SecurityAccess mit festem Key (manche Geräte/Level).
    FesterKey {
        level: u8,
        /// Key als Hex.
        key: String,
    },
    /// TP-2.0-Login (KWP2000) mit festem Seed/Key-Verfahren (XorKey).
    KwpXor {
        /// Tabellen-Index 0–63 (Standard 63, am Golf bestätigt).
        #[serde(default = "kwp_index_standard")]
        ecu_index: u8,
    },
}

fn kwp_index_standard() -> u8 {
    KWP_ECU_INDEX_STANDARD
}

impl Methode {
    pub fn beschreibung(&self) -> String {
        match self {
            Methode::Sa2 { level, bytecode } => {
                let n = crate::backup::parse_hex(bytecode).map(|b| b.len()).unwrap_or(0);
                tr!("SA2, Level {a}, Bytecode {b} Byte", a = level, b = n)
            }
            Methode::FesterKey { level, .. } => tr!("fester Key, Level {a}", a = level),
            Methode::KwpXor { ecu_index } => tr!("TP 2.0 (KWP), XorKey, Index {a}", a = ecu_index),
        }
    }

    /// Passt das Verfahren zu einem TP-2.0-Gerät (sonst UDS)?
    pub fn ist_tp20(&self) -> bool {
        matches!(self, Methode::KwpXor { .. })
    }
}

/// Alle gespeicherten Logins eines Fahrzeugs (ECU-ID als Hex-String → Verfahren).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Logins {
    #[serde(default)]
    pub geraete: BTreeMap<String, Methode>,
}

fn datei(vehicle: &str) -> std::path::PathBuf {
    crate::vehicle::dir(vehicle).join("logins.toml")
}

fn schluessel(tx: u16) -> String {
    format!("{tx:#05x}")
}

pub fn laden(vehicle: &str) -> Logins {
    crate::ablage::lesen(datei(vehicle)).ok().and_then(|t| toml::from_str(&t).ok()).unwrap_or_default()
}

/// Login für ein Steuergerät (UDS-Anfrage-ID) merken.
pub fn setzen(vehicle: &str, tx: u16, m: Methode) -> Result<()> {
    let mut l = laden(vehicle);
    l.geraete.insert(schluessel(tx), m);
    let d = datei(vehicle);
    if let Some(p) = d.parent() {
        std::fs::create_dir_all(p).ok();
    }
    crate::ablage::schreiben(&d, toml::to_string(&l)?).with_context(|| tr!("Schreiben: {a}", a = d.display()))?;
    Ok(())
}

/// Login für ein Steuergerät nachschlagen.
pub fn fuer(vehicle: &str, tx: u16) -> Option<Methode> {
    laden(vehicle).geraete.remove(&schluessel(tx))
}

// ---------------------------------------------------------------------------
// Anmelden am Steuergerät (UDS)

/// Am UDS-Steuergerät anmelden. Setzt voraus, dass schon eine erweiterte
/// Sitzung (0x03) offen ist. Gibt bei Erfolg Ok(()).
pub fn anmelden_uds(uds: &Uds, m: &Methode) -> Result<()> {
    match m {
        Methode::Sa2 { level, bytecode } => {
            let bc = crate::backup::parse_hex(bytecode).context(tr!("SA2-Bytecode ist kein gültiges Hex"))?;
            if bc.is_empty() {
                bail!(tr!("Kein SA2-Bytecode hinterlegt"));
            }
            let seed = uds.security_request_seed(*level).context(tr!("Seed anfordern (0x27) abgelehnt"))?;
            if seed.iter().all(|&b| b == 0) {
                return Ok(()); // Steuergerät meldet „schon freigeschaltet"
            }
            if seed.len() != 4 {
                bail!(tr!("Seed hat {a} Byte, SA2 erwartet 4 — anderes Verfahren?", a = seed.len()));
            }
            let key = sa2(&bc, u32::from_be_bytes([seed[0], seed[1], seed[2], seed[3]]))?;
            uds.security_send_key(level + 1, &key.to_be_bytes()).context(tr!("Key abgelehnt — falscher Bytecode/Level?"))?;
            Ok(())
        }
        Methode::FesterKey { level, key } => {
            let k = crate::backup::parse_hex(key).context(tr!("Key ist kein gültiges Hex"))?;
            let seed = uds.security_request_seed(*level).context(tr!("Seed anfordern (0x27) abgelehnt"))?;
            if seed.iter().all(|&b| b == 0) {
                return Ok(());
            }
            uds.security_send_key(level + 1, &k).context(tr!("Key abgelehnt"))?;
            Ok(())
        }
        Methode::KwpXor { .. } => bail!(tr!("KWP-Login (XorKey) ist nur für TP-2.0-Steuergeräte")),
    }
}

/// Am TP-2.0-Steuergerät anmelden (KWP2000 SecurityAccess 0x27). Setzt voraus,
/// dass der KWP-Kanal offen und die Diagnose-Sitzung gestartet ist.
pub fn anmelden_kwp(k: &mut Kwp, m: &Methode) -> Result<()> {
    let Methode::KwpXor { ecu_index } = m else {
        bail!(tr!("Für TP 2.0 ist nur der KWP-Login (XorKey) vorgesehen"));
    };
    // Seed anfordern: 0x27 0x01 → Antwort [0x01, s0, s1, s2, s3]
    let r = k.request(0x27, &[0x01]).context(tr!("Seed anfordern (0x27) abgelehnt"))?;
    let seed = r.get(1..5).context(tr!("Seed zu kurz (anderes Verfahren?)"))?;
    if seed.iter().all(|&b| b == 0) {
        return Ok(()); // schon freigeschaltet
    }
    let key = kwp_xor_key(*ecu_index, u32::from_be_bytes([seed[0], seed[1], seed[2], seed[3]]));
    // Key senden: 0x27 0x02 <key(4)>
    let mut payload = vec![0x02];
    payload.extend_from_slice(&key.to_be_bytes());
    k.request(0x27, &payload).context(tr!("Key abgelehnt — evtl. anderer Index (Standard 63)?"))?;
    Ok(())
}

/// Ist der Fehler ein „Zugriff verweigert" oder „Reihenfolge-Fehler"?
/// (beides deutet bei VAG darauf hin, dass vorher ein Login nötig ist)
/// - 0x33 securityAccessDenied, 0x22 conditionsNotCorrect
/// - 0x24 requestSequenceError (bei VAG: SecurityAccess fehlt vor dem Schreiben)
pub fn ist_zugriff_verweigert(fehler: &str) -> bool {
    ["NRC 0x33", "NRC 0x22", "NRC 0x24", "securityAccess"].iter().any(|n| fehler.contains(n))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sa2_bekannte_werte() {
        // Prüfwerte aus bri3d/sa2_seed_key (MIT, © 2022 Brian Ledbetter),
        // Datei tests/test_sa2_seed_key.py — siehe CREDITS.md. Eigene Umsetzung
        // (Funktion `sa2`) gegen diese bekannten Werte geprüft.
        let bc1 = [0x68u8, 0x02, 0x81, 0x49, 0x93, 0xa5, 0x5a, 0x55, 0xaa, 0x4a, 0x05, 0x87, 0x81, 0x05, 0x95, 0x26, 0x68, 0x05, 0x82, 0x49, 0x84, 0x5a, 0xa5, 0xaa, 0x55, 0x87, 0x03, 0xf7, 0x80, 0x6a, 0x4c];
        assert_eq!(sa2(&bc1, 0x1a1b_1c1d).unwrap(), 0x6a37_f02e);
        let bc2 = [0x68u8, 0x02, 0x81, 0x4A, 0x10, 0x68, 0x04, 0x93, 0x08, 0x08, 0x20, 0x09, 0x4A, 0x05, 0x87, 0x22, 0x12, 0x19, 0x54, 0x82, 0x49, 0x93, 0x07, 0x12, 0x20, 0x11, 0x82, 0x4A, 0x05, 0x87, 0x03, 0x11, 0x20, 0x10, 0x82, 0x4A, 0x01, 0x81, 0x49, 0x4C];
        assert_eq!(sa2(&bc2, 0xa04e_b1ed).unwrap(), 0x3C78_76D8);
    }

    #[test]
    fn kwp_xor_bekannte_werte() {
        // Prüfwerte aus bri3d/kwp2000, Datei XorSeedKeyLogin.java (LoginTest) —
        // ohne Lizenz veröffentlicht; nur die Zahlenkonstanten zum Abgleich
        // genutzt (siehe CREDITS.md). Plus eigener Integrationstest (echter Seed/Key).
        assert_eq!(kwp_xor_key(63, 0x0CF4_233D), 0xC139_3A1C);
        assert_eq!(kwp_xor_key(63, 0x0332_952C), 0x6652_A580);
        assert_eq!(kwp_xor_key(63, 0x1066_7401), 0xB3B4_3B58);
        assert_eq!(kwp_xor_key(63, 0x01AA_20C4), 0x3544_1880);
    }

    #[test]
    fn sa2_fehlerhaft() {
        assert!(sa2(&[0x93, 0x00], 0).is_err()); // Addition ohne 4 Operanden-Bytes
        assert!(sa2(&[0xFF], 0).is_err()); // unbekannter Opcode
    }

    #[test]
    fn kwp_anmelden_demo() {
        let _s = crate::sim::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::sim::set_modell(crate::sim::Modell::Golf6);
        let be = crate::transport::Backend::Demo;
        // ein vorhandenes TP-2.0-Gerät (Bordnetz 0x20)
        let mut ch = crate::kwp::open_session(&be, 0x20).expect("KWP-Kanal");
        let ok = anmelden_kwp(&mut crate::kwp::Kwp::new(&mut ch), &Methode::KwpXor { ecu_index: 63 });
        assert!(ok.is_ok(), "{ok:?}");
        drop(ch);
        let mut ch = crate::kwp::open_session(&be, 0x20).expect("KWP-Kanal");
        let bad = anmelden_kwp(&mut crate::kwp::Kwp::new(&mut ch), &Methode::KwpXor { ecu_index: 10 });
        assert!(bad.is_err(), "falscher Index müsste scheitern");
        crate::sim::reset();
    }

    #[test]
    fn speichern_laden() {
        let _g = crate::history::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("vag-login-{}", std::process::id()));
        std::env::set_var("VAG_DATA_DIR", &dir);
        let m = Methode::Sa2 { level: 3, bytecode: "68 02 4C".into() };
        setzen("TEST", 0x7E0, m.clone()).unwrap();
        assert_eq!(fuer("TEST", 0x7E0), Some(m));
        assert_eq!(fuer("TEST", 0x714), None);
        std::env::remove_var("VAG_DATA_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
