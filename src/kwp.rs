//! KWP2000 (ISO 14230-3) services as used by VAG ECUs over TP 2.0.
//!
//! Covers the everyday functions: diagnostic session, ECU identification,
//! fault codes (read/clear) and measuring blocks ("Messwertblöcke").
//!
//! The measuring-block formula table is based on public community
//! documentation of the VAG KW1281/KWP format and is marked TODO(verify):
//! compare values with the instrument cluster or a known-good reading.

use crate::tr;
use anyhow::{bail, Result};

pub const SID_START_SESSION: u8 = 0x10;
pub const SID_CLEAR_DTC: u8 = 0x14;
pub const SID_READ_DTC_BY_STATUS: u8 = 0x18;
pub const SID_READ_ECU_ID: u8 = 0x1A;
pub const SID_READ_BY_LOCAL_ID: u8 = 0x21;

/// VAG diagnostic session used over TP 2.0.
pub const SESSION_VAG_DIAG: u8 = 0x89;
/// ReadEcuIdentification option: VAG part number / software / coding block.
pub const ECU_ID_VAG: u8 = 0x9B;
/// Identifikation 0x9A: Betriebsnummer, Softwarestand und (beim Gateway) die
/// lange Codierung. Am Golf 6 per Mitschnitt gesehen (15.11.2026).
pub const ECU_ID_9A: u8 = 0x9A;
/// Identifikation 0x9F beim Gateway: die Verbauliste (alle bekannten
/// Steuergeräte mit „verbaut“ und „Fehler gespeichert“).
pub const ECU_ID_VERBAULISTE: u8 = 0x9F;
/// TP-2.0-Adresse des Gateways (Golf 6)
pub const GATEWAY: u8 = 0x1F;
pub const SID_READ_DTC_STATUS: u8 = 0x17;

const NEGATIVE_RESPONSE: u8 = 0x7F;
const NRC_RESPONSE_PENDING: u8 = 0x78;

/// Request/response link that carries KWP2000 messages (e.g. a TP 2.0 channel).
pub trait KwpLink {
    fn send(&mut self, req: &[u8]) -> Result<()>;
    fn recv(&mut self) -> Result<Vec<u8>>;
}

impl<I: crate::tp20::CanIo> KwpLink for crate::tp20::Tp20<I> {
    fn send(&mut self, req: &[u8]) -> Result<()> {
        self.keep_alive()?;
        crate::tp20::Tp20::send(self, req)
    }
    fn recv(&mut self) -> Result<Vec<u8>> {
        crate::tp20::Tp20::recv(self)
    }
}

/// Zu diesem Fehlercode gibt es keine (passenden) Umgebungsdaten — kein
/// Grund aufzuhören. Eigene Fehlerart, damit das nicht am Text hängt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UmgebungFehlt {
    Keine,
    PasstNicht,
}

impl std::fmt::Display for UmgebungFehlt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&match self {
            UmgebungFehlt::Keine => tr!("keine Umgebungsdaten gespeichert"),
            UmgebungFehlt::PasstNicht => tr!("Antwort passt nicht zum Fehlercode"),
        })
    }
}

impl std::error::Error for UmgebungFehlt {}

pub struct Kwp<'a> {
    link: &'a mut dyn KwpLink,
}

impl<'a> Kwp<'a> {
    pub fn new(link: &'a mut dyn KwpLink) -> Self {
        Self { link }
    }

    /// Send a request and return the positive response without the SID byte.
    /// 0x78 (responsePending) is waited out; other negative responses are errors.
    pub fn request(&mut self, sid: u8, data: &[u8]) -> Result<Vec<u8>> {
        let mut req = Vec::with_capacity(1 + data.len());
        req.push(sid);
        req.extend_from_slice(data);
        self.link.send(&req)?;
        let mut busy = 0;
        for _ in 0..50 {
            let resp = self.link.recv()?;
            if resp.is_empty() {
                bail!(tr!("leere KWP2000-Antwort"));
            }
            if resp[0] == NEGATIVE_RESPONSE {
                let nrc = resp.get(2).copied();
                if nrc == Some(NRC_RESPONSE_PENDING) {
                    continue;
                }
                // „beschäftigt, bitte wiederholen“: kurz warten, nochmal fragen
                if nrc == Some(0x21) && busy < 5 {
                    busy += 1;
                    std::thread::sleep(std::time::Duration::from_millis(200));
                    self.link.send(&req)?;
                    continue;
                }
                bail!(tr!("Negative Antwort auf SID {sid}: NRC {a} ({b})", sid = format!("{:#04x}", sid), a = nrc.map(|n| format!("{n:#04x}")).unwrap_or_else(|| "?".into()), b = nrc.map(nrc_name).unwrap_or("unbekannt")));
            }
            if resp[0] != sid.wrapping_add(0x40) {
                bail!(tr!("unerwartete Antwort-SID {a} (erwartet {b})", a = format!("{:#04x}", resp[0]), b = format!("{:#04x}", sid + 0x40)));
            }
            return Ok(resp[1..].to_vec());
        }
        bail!(tr!("Steuergerät meldet dauerhaft 'Antwort folgt' (0x78)"))
    }

    pub fn start_session(&mut self, session: u8) -> Result<()> {
        self.request(SID_START_SESSION, &[session]).map(|_| ())
    }

    /// ReadEcuIdentification; returns the data after the echoed option byte.
    pub fn read_ecu_id(&mut self, option: u8) -> Result<Vec<u8>> {
        let r = self.request(SID_READ_ECU_ID, &[option])?;
        Ok(r.get(1..).map(<[u8]>::to_vec).unwrap_or_default())
    }

    /// ReadDiagnosticTroubleCodesByStatus, all groups.
    pub fn read_dtcs(&mut self) -> Result<Vec<KwpDtc>> {
        // Manche Steuergeräte (z. B. MED17) kennen nur Typ 0x00 statt 0x02
        let r = match self.request(SID_READ_DTC_BY_STATUS, &[0x02, 0xFF, 0x00]) {
            Ok(r) => r,
            Err(e) if format!("{e:#}").contains("NRC 0x12") => self.request(SID_READ_DTC_BY_STATUS, &[0x00, 0xFF, 0x00])?,
            Err(e) => return Err(e),
        };
        Ok(parse_dtcs(&r))
    }

    /// Umgebungsbedingungen zu einem Fehler: ReadStatusOfDiagnosticTroubleCodes
    /// (0x17, ISO 14230-3) für genau diesen Code. Liefert die Daten hinter
    /// Code und Status (Aufbau herstellerabhängig, daher roh). Nur lesend.
    pub fn read_umgebung(&mut self, code: u16) -> Result<Vec<u8>> {
        let r = self.request(SID_READ_DTC_STATUS, &code.to_be_bytes())?;
        // Antwort: Anzahl, Code (2), Status, Umgebungsdaten …
        match r.as_slice() {
            [n, hi, lo, _st, env @ ..] if *n >= 1 && u16::from_be_bytes([*hi, *lo]) == code && !env.is_empty() => Ok(env.to_vec()),
            [_, _, _, _] => Err(UmgebungFehlt::Keine.into()),
            _ => Err(UmgebungFehlt::PasstNicht.into()),
        }
    }

    /// Fehlerspeicher plus Umgebungsbedingungen je Fehler (max. 10). Kennt das
    /// Steuergerät 0x17 nicht (oder antwortet nicht), wird nicht weiter gefragt;
    /// nur „zu diesem Code nichts“ geht weiter.
    pub fn read_dtcs_mit_umgebung(&mut self) -> Result<Vec<(KwpDtc, Option<Vec<u8>>)>> {
        let dtcs = self.read_dtcs()?;
        let mut out: Vec<(KwpDtc, Option<Vec<u8>>)> = dtcs.into_iter().map(|d| (d, None)).collect();
        for (d, env) in out.iter_mut().take(10) {
            match self.read_umgebung(d.code) {
                Ok(e) => *env = Some(e),
                // „NRC 0x31“ bleibt in jeder Sprache gleich (siehe i18n-Test)
                Err(e) if e.downcast_ref::<UmgebungFehlt>().is_some() || format!("{e:#}").contains("NRC 0x31") => {}
                Err(_) => break,
            }
        }
        Ok(out)
    }

    /// ClearDiagnosticInformation, all groups.
    pub fn clear_dtcs(&mut self) -> Result<()> {
        self.request(SID_CLEAR_DTC, &[0xFF, 0x00]).map(|_| ())
    }

    /// Read one measuring block (ReadDataByLocalIdentifier).
    pub fn read_block(&mut self, block: u8) -> Result<Vec<BlockValue>> {
        let r = self.request(SID_READ_BY_LOCAL_ID, &[block])?;
        if r.first() != Some(&block) {
            bail!(tr!("Antwort gehört nicht zu Block {block}", block = block));
        }
        Ok(parse_block(&r[1..]))
    }
}

/// VAG fault code: 16-bit number (shown 5-digit decimal) plus status byte.
#[derive(Debug, Clone, PartialEq)]
pub struct KwpDtc {
    pub code: u16,
    pub status: u8,
}

impl KwpDtc {
    /// 5-digit VAG code, e.g. "16684".
    pub fn vag_code(&self) -> String {
        format!("{:05}", self.code)
    }

    /// Passender OBD-Code (Pxxxx), soweit sich das sicher sagen lässt.
    ///
    /// * 16384..=17383 → P0000..P0999 und 17408..=18407 → P1000..P1999
    ///   (ältere VAG-Motorsteuergeräte, z. B. 16684 = P0300, 17544 = P1136).
    /// * Neuere Motor-/Getriebesteuergeräte (Adresse 0x01/0x02, z. B. MED17)
    ///   senden den OBD-Code direkt als Zahl unter 0x4000: 08851 = 0x2293 = P2293.
    ///   Bei anderen Steuergeräten sind kleine Zahlen VAG-Bauteilcodes.
    pub fn obd_code(&self, address: Option<u8>) -> Option<String> {
        let c = self.code;
        match c {
            16384..=17383 => Some(format!("P{:04}", c - 16384)),
            17408..=18407 => Some(format!("P{:04}", c - 17408 + 1000)),
            1..=0x3FFF if matches!(address, Some(0x01 | 0x02)) => Some(format!("P{c:04X}")),
            _ => None,
        }
    }

    /// Statusbyte im Klartext nach ISO 14230-3 („statusOfDTC“):
    /// Bits 6-5 = Speicherzustand, Bit 7 = Warnlampe, Bits 3-0 = Fehlerart
    /// (Nummer; die Bedeutung legt der Hersteller fest). Bit 4 gehört nicht
    /// zur Fehlerart: am Golf 6 zeigt ein Werkstatt-Diagnosegerät bei Status
    /// 0x38 Fehlerart 8 (nicht 24) — verglichen am 15.11.2026.
    pub fn status_text(&self) -> String {
        let s = self.status;
        let zustand = match (s >> 5) & 0x03 {
            0 => "kein Fehler erkannt",
            1 => "sporadisch (gespeichert, gerade nicht vorhanden)",
            2 => "wird geprüft (noch nicht bestätigt)",
            _ => "aktuell vorhanden",
        };
        let mut t = zustand.to_string();
        if s & 0x80 != 0 {
            t += ", Warnlampe an";
        }
        let art = s & 0x0F;
        if art != 0 {
            match crate::dtc_db::tp20_fehlerart(art) {
                Some(txt) => t += &tr!(", Fehlerart {art}: {txt}", art = art, txt = txt),
                None => t += &tr!(", Fehlerart {art}", art = art),
            }
        }
        format!("{t} ({s:#04x})")
    }
}

/// Response body: [count][hi lo status]*count. A single 0xFFFF entry means
/// "no faults" on some ECUs.
pub fn parse_dtcs(r: &[u8]) -> Vec<KwpDtc> {
    let Some((&count, rest)) = r.split_first() else { return vec![] };
    rest.as_chunks::<3>().0.iter()
        .take(count as usize)
        .map(|c| KwpDtc { code: u16::from_be_bytes([c[0], c[1]]), status: c[2] })
        .filter(|d| d.code != 0xFFFF)
        .collect()
}

/// One value of a measuring block: formula id + two raw bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockValue {
    pub formula: u8,
    pub a: u8,
    pub b: u8,
    /// Textfeld (Formel 0x5F): a = Länge, danach so viele Zeichen
    pub text: Option<String>,
}

/// Formel 0x5F: Text mit Längenbyte statt zwei Rohbytes (am Golf 6 gesehen:
/// Motorkennung, Fahrgestellnummer, Seriennummern in Block 80–82 und 250).
const FORMEL_TEXT: u8 = 0x5F;

/// Ein Block hat bis zu 8 Felder (MED17 schickt immer 8). Leere Felder am Ende
/// („25 00 00“) werden weggelassen; leere Felder in der Mitte bleiben, damit
/// die Feldnummern stimmen.
pub fn parse_block(r: &[u8]) -> Vec<BlockValue> {
    let mut v = Vec::new();
    let mut i = 0;
    while i + 3 <= r.len() {
        let (formula, a, b) = (r[i], r[i + 1], r[i + 2]);
        if formula == FORMEL_TEXT && i + 2 + a as usize <= r.len() {
            let t: String = r[i + 2..i + 2 + a as usize].iter().map(|&c| if (0x20..0x7F).contains(&c) { c as char } else { ' ' }).collect();
            v.push(BlockValue { formula, a, b, text: Some(t.trim().to_string()) });
            i += 2 + a as usize;
            continue;
        }
        v.push(BlockValue { formula, a, b, text: None });
        i += 3;
    }
    while v.last().is_some_and(BlockValue::is_leer) {
        v.pop();
    }
    v
}

impl BlockValue {
    /// Leeres Feld (Formel 0x25 mit Wert 0), am Golf 6 / MED17 gesehen.
    pub fn is_leer(&self) -> bool {
        self.formula == 0x25 && self.a == 0 && self.b == 0
    }

    /// Decoded value and unit. Unknown formulas return None — show raw then.
    pub fn decode(&self) -> Option<(f64, &'static str)> {
        let (a, b) = (f64::from(self.a), f64::from(self.b));
        // Formeln aus öffentlicher Community-Beschreibung. Am Golf 6 mit einem
        // Werkstatt-Diagnosegerät verglichen (15.11.2026) und bestätigt:
        // 01 06 07 08 12 14 15 16 17 1B 21 22 36 5E; dazu die neuen weiter unten.
        Some(match self.formula {
            0x01 => (0.2 * a * b, "1/min"),
            0x02 => (a * 0.002 * b, "%"),
            0x03 => (0.002 * a * b, "°"),
            0x05 => (a * (b - 100.0) * 0.1, "°C"),
            0x06 => (0.001 * a * b, "V"),
            0x07 => (0.01 * a * b, "km/h"),
            0x08 => (0.1 * a * b, ""),
            0x0F => (0.01 * a * b, "ms"),
            0x12 => (0.04 * a * b, "mbar"),
            0x14 => (a * (b - 128.0) / 128.0, "%"),
            0x15 => (0.001 * a * b, "V"),
            0x16 => (0.001 * a * b, "ms"),
            0x17 => (b / 256.0 * a, "%"),
            0x1B => ((b - 128.0) * 0.01 * a, "°"),
            0x21 => (if self.a == 0 { 100.0 * b } else { 100.0 * b / a }, "%"),
            0x22 => ((b - 128.0) * 0.01 * a, "kW"),
            0x23 => (0.01 * a * b, "l/h"),
            0x24 => (a * 2560.0 + b * 10.0, "km"),
            0x36 => (a * 256.0 + b, ""),
            // TODO(verify): weitere Formeln aus der Community (u. a. PyVCDS), nicht am Auto geprüft
            0x04 => ((b - 127.0) * 0.01 * a, "°"),
            0x18 => (0.001 * a * b, "A"),
            0x19 => (b * 1.421 + a / 182.0, "g/s"),
            0x1A => (b - a, "°C"),
            0x27 => (a * b / 256.0, "mg/H"),
            0x31 => (a * b / 40.0, "mg/H"),
            0x33 => ((b - 128.0) / 255.0 * a, "mg/H"),
            0x37 => (a * b / 200.0, "s"),
            0x5E => (a * (b / 50.0 - 1.0), "Nm"),
            // Am Golf 6 (MED17) mit der Anzeige eines Werkstatt-Diagnosegeräts
            // abgeleitet (15.11.2026), je mit den gesehenen Werten:
            0x1F => (a * b / 2560.0, "λ"), // 20·128 → 1,00; 40·53 → 0,83; 80·35 → 1,09
            0x34 => ((b - 50.0) * a / 50.0, "Nm"), // 73,254 → 297,8; 73,50 → 0
            0x3C => ((a * 256.0 + b) / 100.0, "s"), // 255,255 → 655,35
            0x41 => ((b - 127.0) * a / 100.0, "mm"), // 101,177 → 50,5; 0,177 → 0
            0x42 => (a * b / 512.0, "V"), // 10,76 → 1,48; 10,23 → 0,45
            0x60 => (a * b / 10.0, "mbar"), // 100,99 → 990
            // vorzeichenbehaftet, 16 Bit: Lenkwinkel −545,6° (CF 49) / +541,6°
            // (30 5B), −4,59° (FF 97), Motor 116,68° (0A 6B)
            0x51 => (f64::from(i16::from_be_bytes([self.a, self.b])) * 0.04375, "°"),
            0x30 | 0x4B => (a * 256.0 + b, ""), // Zähler/Zahl: 0,5 → 5; 0,69 → 69
            _ => return None,
        })
    }

    /// Human-readable value, falling back to raw bytes.
    pub fn display(&self) -> String {
        if self.is_leer() {
            return "—".into();
        }
        if let Some(t) = &self.text {
            return t.clone();
        }
        // Bitfeld (z. B. Grundeinstell-Bedingungen): a = Maske, b = Bits
        // TODO(verify): Zuordnung Maske/Bits aus der Community-Beschreibung
        if self.formula == 0x10 {
            return format!("{:04b} {:04b}", (self.b & self.a) >> 4, self.b & self.a & 0x0F);
        }
        // Hex-Anzeige (Golf 6 Lenkung: 90 03 → 0x9003)
        if self.formula == 0x6B {
            return format!("0x{:02X}{:02X}", self.a, self.b);
        }
        // 16 einzelne Bits
        if self.formula == 0xA1 {
            return format!("{:08b} {:08b}", self.a, self.b);
        }
        // Textnummer (F25): eigener Text aus der Wertetext-Tabelle des Nutzers
        if self.formula == 0x25 {
            if let Some(t) = crate::wertetexte::text(self.formula, self.a, self.b) {
                return t;
            }
        }
        // zwei Zeichen Text
        if self.formula == 0x11 {
            return [self.a, self.b].iter().map(|&c| if (0x20..0x7F).contains(&c) { c as char } else { '·' }).collect();
        }
        match self.decode() {
            Some((v, "")) => format!("{v:.2}"),
            Some((v, unit)) => format!("{v:.2} {unit}"),
            None => tr!("roh F{a} {b} {c}", a = format!("{:02X}", self.formula), b = format!("{:02X}", self.a), c = format!("{:02X}", self.b)),
        }
    }
}

/// VAG-Identifikation (0x1A 0x9B) mit festen Feldern, wie am Golf 6 gelesen:
/// Byte 0–11 Teilenummer, 12–15 Softwarestand, 16–19 Codierung, 20–25
/// Betriebsnummer, ab 26 Name. Liefert ["Teilenummer  SW", "Name"] oder None bei anderem Aufbau.
/// Bit 7 im Softwarestand (Türen, Lenksäule: B1 34 30 31) wird ausgeblendet —
/// daraus wird „1401“ (am Golf 6 mit einem Werkstatt-Diagnosegerät verglichen).
pub fn ident_9b(raw: &[u8]) -> Option<Vec<String>> {
    if raw.len() < 26 || !raw[..12].iter().all(|b| (0x20..0x7F).contains(b)) {
        return None;
    }
    let txt = |d: &[u8]| -> String {
        d.iter().map(|&b| b & 0x7F).map(|c| if (0x20..0x7F).contains(&c) { c as char } else { ' ' }).collect::<String>().trim().to_string()
    };
    let teil = txt(&raw[..12]);
    if teil.len() < 9 || teil.contains("  ") {
        return None;
    }
    let sw = txt(&raw[12..16]);
    let mut out = vec![format!("{teil}  {sw}").trim().to_string()];
    // Byte 20–25: Betriebsnummer der letzten Codierung (6 Byte wie 0xF1A5),
    // Name erst ab Byte 26 — am Golf 6 mit dem Werkstattgerät verglichen
    // (Tür: 00 00 00 00 00 42 = WSC 00066). Früher galt 0x42 als „B“ des Namens.
    let name = ident_strings(&raw[26.min(raw.len())..]).join(" ");
    if !name.is_empty() {
        out.push(name);
    }
    Some(out)
}

/// Printable text runs (≥ 3 chars) from an identification response, e.g.
/// part number, component name, software version.
pub fn ident_strings(data: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for &b in data {
        if (0x20..=0x7E).contains(&b) {
            cur.push(b as char);
        } else {
            push_run(&mut out, &mut cur);
        }
    }
    push_run(&mut out, &mut cur);
    out
}

fn push_run(out: &mut Vec<String>, cur: &mut String) {
    let t = cur.trim();
    if t.len() >= 3 {
        out.push(t.to_string());
    }
    cur.clear();
}

/// Open a TP 2.0 channel and start the VAG diagnostic session.
pub fn open_session(be: &crate::transport::Backend, address: u8) -> Result<crate::tp20::Channel> {
    use anyhow::Context;
    // Nach einer OBD-Abfrage sperrt der Golf-6-Motor TP 2.0 für ~3 s
    // („vorübergehend nicht verfügbar“, am Auto gemessen): warten, nochmal
    let mut opened = be.open_tp20(address, std::time::Duration::from_millis(1000));
    for warte_ms in [700, 1500, 3000] {
        match &opened {
            Err(e) if matches!(crate::tp20::kurzzeitig(e), Some(crate::tp20::Kurzzeitig::Voruebergehend | crate::tp20::Kurzzeitig::KeineRessourcen)) => {
                std::thread::sleep(std::time::Duration::from_millis(warte_ms));
                opened = be.open_tp20(address, std::time::Duration::from_millis(1000));
            }
            _ => break,
        }
    }
    let mut ch = opened.with_context(|| tr!("TP2.0-Kanal zu {address}", address = format!("{:#04x}", address)))?;
    Kwp::new(&mut ch).start_session(SESSION_VAG_DIAG)?;
    Ok(ch)
}

/// Alle Messwertblöcke `von..=bis` lesen (nur lesen). Blöcke, die das
/// Steuergerät ablehnt (negative Antwort), werden übersprungen;
/// Verbindungsfehler brechen ab. `progress` bekommt die aktuelle Blocknummer.
pub fn blockscan(be: &crate::transport::Backend, address: u8, von: u8, bis: u8, mut progress: impl FnMut(u8)) -> Result<Vec<(u8, Vec<BlockValue>)>> {
    let mut ch = open_session(be, address)?;
    let mut out = Vec::new();
    for block in von..=bis {
        progress(block);
        match Kwp::new(&mut ch).read_block(block) {
            Ok(v) if !v.is_empty() => out.push((block, v)),
            Ok(_) => {}
            Err(e) => {
                let t = format!("{e:#}");
                if !t.contains("NRC 0x") {
                    bail!(tr!("Block {a}: {e}", a = block, e = t));
                }
            }
        }
    }
    Ok(out)
}

/// One ECU found by [`sweep`].
#[derive(Debug, Clone)]
pub struct Tp20Found {
    pub address: u8,
    /// Name aus der Antwort des Steuergeräts (z. B. „Gateway (J533)“)
    pub name: String,
    pub tx: u16,
    pub rx: u16,
    pub ident: Vec<String>,
    /// Rohdaten der Identifikation (enthält bei VAG auch die Codierung)
    pub ident_raw: Vec<u8>,
    /// None = Fehlerspeicher nicht lesbar
    pub dtcs: Option<Vec<KwpDtc>>,
    /// Umgebungsbedingungen je Fehlercode (roh, siehe [`Kwp::read_umgebung`])
    pub umgebung: Vec<(u16, Vec<u8>)>,
    /// Codierung, wenn lesbar (siehe [`codierung_lesen`])
    pub codierung: Option<TpCodierung>,
}

/// Kennung der langen Codierung — wie bei UDS. Neuere TP-2.0-Steuergeräte
/// (Golf 6: Bordnetz, ABS, Einparkhilfe, Anhänger) verstehen 0x22 mit Kennungen.
/// Am Golf 6 bestätigt (Mitschnitt 14.11.2026): Bordnetz 0x20 und ABS 0x03
/// liefern über 0x22 0x0600 dieselbe Codierung, die ein Werkstatt-Diagnosegerät
/// liest (Sitzung 0x89 vorher, wie hier).
pub const CODING_DID: u16 = 0x0600;

/// Codierung eines TP-2.0-Steuergeräts.
#[derive(Debug, Clone, PartialEq)]
pub enum TpCodierung {
    /// Lange Codierung über 0x22 0x0600 (neuere Steuergeräte)
    Lang(Vec<u8>),
    /// Kurzcodierung aus der Identifikation 0x1A 0x9B (ältere Steuergeräte).
    /// `roh` = Byte 16–25 der Identifikation (Codierung + Betriebsnummer), `wert` = Byte 17–19 als Zahl.
    /// Am Golf 6 gesehen bei Motor, Türen und Lenksäule (Byte 16 = 03).
    Kurz { wert: u32, roh: Vec<u8> },
}

impl TpCodierung {
    pub fn text(&self) -> String {
        match self {
            TpCodierung::Lang(b) => b.iter().map(|x| format!("{x:02X}")).collect(),
            TpCodierung::Kurz { wert, .. } => format!("{wert:07}"),
        }
    }
}

/// Kurzcodierung aus der 0x9B-Identifikation — nur beim Aufbau „03 xx xx xx“
/// in Byte 16–19 (am Golf 6: Motor, Türen, Lenksäule). Gateway (10 …) und
/// Servolenkung sind anders aufgebaut und bleiben außen vor.
/// Wert = Byte 17–19: am Golf 6 mit der Anzeige eines Werkstatt-Diagnosegeräts
/// verglichen (15.11.2026) — Motor 72, Türen 180/436/144, Lenksäule 34 stimmen.
pub fn kurzcodierung_9b(raw: &[u8]) -> Option<TpCodierung> {
    ident_9b(raw)?;
    let roh = raw.get(16..26).or_else(|| raw.get(16..25))?.to_vec();
    if roh[0] != 0x03 {
        return None;
    }
    let wert = u32::from_be_bytes([0, roh[1], roh[2], roh[3]]);
    Some(TpCodierung::Kurz { wert, roh })
}

/// Daten für „Kurzcodierung schreiben“ (KWP 0x3B, Kennung 0x9A) aus der
/// gelesenen Identifikation 0x9B: Betriebsnummer (Byte 20–25) und
/// Softwarestand (Byte 12–15) bleiben unverändert, dahinter 03 + Codierung
/// (3 Byte). Genau so hat ein Werkstatt-Diagnosegerät an der Golf-6-Tür
/// geschrieben (Mitschnitt 17.11.2026). None, wenn das Gerät keine
/// Kurzcodierung im Aufbau „03 xx xx xx“ hat oder der Wert zu groß ist.
pub fn kurzcodierung_daten(raw_9b: &[u8], neu: u32) -> Option<Vec<u8>> {
    kurzcodierung_9b(raw_9b)?;
    if neu > 0xFF_FFFF || raw_9b.len() < 26 {
        return None;
    }
    let mut d = vec![ECU_ID_9A];
    d.extend_from_slice(&raw_9b[20..26]);
    d.extend_from_slice(&raw_9b[12..16]);
    d.push(0x03);
    d.extend_from_slice(&neu.to_be_bytes()[1..]);
    Some(d)
}

/// Kurzcodierung eines TP-2.0-Steuergeräts schreiben und zur Kontrolle neu
/// lesen. Gibt (alter Wert, danach gelesener Wert) zurück.
pub fn kurzcodierung_schreiben(be: &crate::transport::Backend, adresse: u8, neu: u32) -> Result<(u32, u32)> {
    // vor dem TP-2.0-Kanal (OBD-Abfrage sperrt den Golf-6-Motor kurz für TP 2.0)
    crate::spannung::vor_dem_schreiben(be)?;
    let mut ch = open_session(be, adresse)?;
    let mut k = Kwp::new(&mut ch);
    let raw = k.read_ecu_id(ECU_ID_VAG)?;
    let alt = match kurzcodierung_9b(&raw) {
        Some(TpCodierung::Kurz { wert, .. }) => wert,
        _ => bail!(tr!("Steuergerät {a} hat keine Kurzcodierung (Aufbau 03 xx xx xx).", a = format!("{adresse:02X}"))),
    };
    let daten = kurzcodierung_daten(&raw, neu).ok_or_else(|| anyhow::anyhow!(tr!("Codierung {a} passt nicht (höchstens 7 Stellen).", a = neu)))?;
    k.request(0x3B, &daten)?;
    let danach = match kurzcodierung_9b(&k.read_ecu_id(ECU_ID_VAG)?) {
        Some(TpCodierung::Kurz { wert, .. }) => wert,
        _ => bail!(tr!("Kontrolle: Codierung danach nicht lesbar.")),
    };
    Ok((alt, danach))
}

/// Lange Codierung über 0x0600 schreiben — am Auto mit einem Mitschnitt
/// bestätigt? Solange `false`, macht [`langcodierung_schreiben`] am echten
/// Auto nur einen Probelauf (zeigt die Anfrage, sendet nichts).
/// Annahme: KWP 0x2E (WriteDataByCommonIdentifier) mit derselben Kennung,
/// über die das Steuergerät die Codierung mit 0x22 liefert.
pub const LANGCODIERUNG_GEPRUEFT: bool = false;
/// Lange Codierung des Gateways (0x3B 0x9A) — siehe [`LANGCODIERUNG_GEPRUEFT`].
/// Annahme: gleicher Aufbau wie die am Golf 6 mitgeschnittene Kurzcodierung
/// (0x3B 0x9A [Betriebsnummer][Softwarestand][Codierblock]).
pub const GATEWAY_CODIERUNG_GEPRUEFT: bool = false;
/// Anpassung schreiben (Routine 0x0103) — siehe [`anpass_schreib_anfrage`].
pub const ANPASSUNG_SCHREIBEN_GEPRUEFT: bool = false;

/// Wo die lange Codierung eines TP-2.0-Steuergeräts steht.
#[derive(Debug, Clone, PartialEq)]
pub enum LangArt {
    /// 0x22/0x2E mit Kennung 0x0600 (Bordnetz, ABS, Einparkhilfe …)
    Kennung0600,
    /// Identifikation 0x9A wie beim Gateway (Rohdaten ohne SID/Option)
    Gateway9A(Vec<u8>),
}

/// Ergebnis eines Schreibversuchs.
#[derive(Debug, Clone, PartialEq)]
pub enum Schreiben {
    /// Am echten Auto noch nicht freigegeben: diese Anfrage (mit SID) würde gesendet.
    Probelauf { anfrage: Vec<u8>, alt: Vec<u8> },
    /// Geschrieben; danach neu gelesen.
    Geschrieben { alt: Vec<u8>, danach: Vec<u8> },
}

/// Darf ein ungeprüfter Schreibweg wirklich senden? Nur in der Simulation.
pub fn darf_senden(be: &crate::transport::Backend, geprueft: bool) -> bool {
    geprueft || matches!(be, crate::transport::Backend::Demo)
}

/// Anfrage (mit SID) für eine neue lange Codierung. Die Länge muss gleich bleiben.
pub fn langcodierung_anfrage(art: &LangArt, alt: &[u8], neu: &[u8]) -> Result<Vec<u8>> {
    if neu.len() != alt.len() {
        bail!(tr!("Codierung muss {a} Byte lang sein (eingegeben: {b}).", a = alt.len(), b = neu.len()));
    }
    Ok(match art {
        LangArt::Kennung0600 => [&[0x2E][..], &CODING_DID.to_be_bytes(), neu].concat(),
        LangArt::Gateway9A(raw) => {
            // Rohdaten übernehmen, nur die Codierbytes tauschen (Betriebsnummer,
            // Softwarestand, Längenbyte und Abschluss bleiben)
            let mut d = raw.clone();
            d.get_mut(12..12 + neu.len()).ok_or_else(|| anyhow::anyhow!(tr!("Gateway-Codierung unvollständig gelesen")))?.copy_from_slice(neu);
            [&[0x3B, ECU_ID_9A][..], &d].concat()
        }
    })
}

/// Lange Codierung und ihren Ort lesen.
pub fn langcodierung_lesen(k: &mut Kwp) -> Option<(LangArt, Vec<u8>)> {
    if let Some(c) = codierung_22(k) {
        return Some((LangArt::Kennung0600, c));
    }
    let raw = k.read_ecu_id(ECU_ID_9A).ok()?;
    let c = codierung_9a(&raw)?;
    Some((LangArt::Gateway9A(raw), c))
}

/// Lange Codierung eines TP-2.0-Steuergeräts schreiben und zur Kontrolle neu
/// lesen. Am echten Auto nur ein Probelauf, solange der Weg nicht mit einem
/// Mitschnitt bestätigt ist ([`LANGCODIERUNG_GEPRUEFT`]).
pub fn langcodierung_schreiben(be: &crate::transport::Backend, adresse: u8, neu: &[u8]) -> Result<Schreiben> {
    if darf_senden(be, LANGCODIERUNG_GEPRUEFT || GATEWAY_CODIERUNG_GEPRUEFT) {
        crate::spannung::vor_dem_schreiben(be)?;
    }
    let mut ch = open_session(be, adresse)?;
    let mut k = Kwp::new(&mut ch);
    let (art, alt) = langcodierung_lesen(&mut k).ok_or_else(|| anyhow::anyhow!(tr!("Steuergerät {a} hat keine lange Codierung.", a = format!("{adresse:02X}"))))?;
    let anfrage = langcodierung_anfrage(&art, &alt, neu)?;
    let geprueft = match art {
        LangArt::Kennung0600 => LANGCODIERUNG_GEPRUEFT,
        LangArt::Gateway9A(_) => GATEWAY_CODIERUNG_GEPRUEFT,
    };
    if !darf_senden(be, geprueft) {
        return Ok(Schreiben::Probelauf { anfrage, alt });
    }
    k.request(anfrage[0], &anfrage[1..])?;
    let danach = langcodierung_lesen(&mut k).map(|(_, c)| c).ok_or_else(|| anyhow::anyhow!(tr!("Kontrolle: Codierung danach nicht lesbar.")))?;
    Ok(Schreiben::Geschrieben { alt, danach })
}

/// Lange Codierung über 0x22 0x0600; None, wenn das Steuergerät sie nicht kennt.
pub fn codierung_22(k: &mut Kwp) -> Option<Vec<u8>> {
    k.request(0x22, &CODING_DID.to_be_bytes()).ok().and_then(|r| r.get(2..).map(<[u8]>::to_vec)).filter(|d| !d.is_empty())
}

/// Codierung lesen: ältere Steuergeräte aus der Identifikation, neuere über 0x0600.
pub fn codierung_lesen(k: &mut Kwp, ident_raw: &[u8]) -> Option<TpCodierung> {
    kurzcodierung_9b(ident_raw)
        .or_else(|| codierung_22(k).map(TpCodierung::Lang))
        // Gateway: lange Codierung steckt in der Identifikation 0x9A
        .or_else(|| k.read_ecu_id(ECU_ID_9A).ok().and_then(|r| codierung_9a(&r)).map(TpCodierung::Lang))
}

/// Try every TP 2.0 logical address 0x01..=0x7F with a short setup timeout
/// (~8 s in total on a real bus). `progress` is called before each address.
pub fn sweep(be: &crate::transport::Backend, progress: impl FnMut(u8)) -> Result<Vec<Tp20Found>> {
    sweep_addrs(be, &(0x01..=0x7Fu8).collect::<Vec<_>>(), progress)
}

/// Wie [`sweep`], aber nur die angegebenen Adressen (Schnell-Scan mit Profil).
pub fn sweep_addrs(be: &crate::transport::Backend, addrs: &[u8], mut progress: impl FnMut(u8)) -> Result<Vec<Tp20Found>> {
    use crate::transport::Backend;
    match be {
        // Fail early (and clearly) if the interface does not exist.
        Backend::SocketCan { iface } => {
            crate::tp20::SocketIo::open(iface, std::time::Duration::from_millis(10))?;
        }
        Backend::Demo => {}
        _ => bail!(tr!("TP 2.0 braucht SocketCAN (oder Demo) — ELM327 kann es nicht")),
    }
    let mut found = Vec::new();
    for &address in addrs {
        progress(address);
        // „vorübergehend nicht verfügbar“ (z. B. Türsteuergeräte, während das
        // Gateway den letzten Kanal noch abbaut; Golf-6-Motor bis ~3 s nach
        // einer OBD-Abfrage): warten, nochmal versuchen — jedes Mal länger
        let mut opened = be.open_tp20(address, std::time::Duration::from_millis(60));
        for warte_ms in [300, 1200, 3000] {
            match &opened {
                Err(e) if crate::tp20::kurzzeitig(e).is_some() => {
                    // halb offenen Kanal beim Steuergerät auslaufen lassen
                    std::thread::sleep(std::time::Duration::from_millis(if crate::tp20::kurzzeitig(e) == Some(crate::tp20::Kurzzeitig::KeineParameterAntwort) { warte_ms.max(1200) } else { warte_ms }));
                    opened = be.open_tp20(address, std::time::Duration::from_millis(60));
                }
                _ => break,
            }
        }
        let Ok(mut ch) = opened else {
            continue;
        };
        let (tx, rx) = (ch.tx_id(), ch.rx_id());
        let mut k = Kwp::new(&mut ch);
        if k.start_session(SESSION_VAG_DIAG).is_err() {
            continue;
        }
        let ident_raw = k.read_ecu_id(ECU_ID_VAG).unwrap_or_default();
        let mut ident = ident_9b(&ident_raw).unwrap_or_else(|| ident_strings(&ident_raw));
        if ident.is_empty() {
            // Neuere Steuergeräte kennen 0x1A nicht, aber 0x22 mit F1xx-Kennungen
            ident = ident_22(&mut k);
        }
        let (dtcs, umgebung) = match k.read_dtcs_mit_umgebung() {
            Ok(l) => {
                let env = l.iter().filter_map(|(d, e)| e.clone().map(|e| (d.code, e))).collect();
                (Some(l.into_iter().map(|(d, _)| d).collect::<Vec<_>>()), env)
            }
            Err(_) => (None, Vec::new()),
        };
        let codierung = codierung_lesen(&mut k, &ident_raw);
        let name = tp20_name(address, &ident);
        found.push(Tp20Found { address, name, tx, rx, ident, ident_raw, dtcs, umgebung, codierung });
        drop(ch);
        // dem Gateway Zeit geben, den Kanal abzubauen
        if matches!(be, Backend::SocketCan { .. }) {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
    Ok(found)
}

/// Umgebungsdaten als Anzeige. Der Aufbau ist je Steuergerät verschieden und
/// für VAG-KWP nicht offen dokumentiert — darum vorerst als Rohdaten.
pub fn umgebung_werte(env: &[u8]) -> Vec<(String, String)> {
    vec![("Umgebungsdaten (roh)".into(), umgebung_hex(env))]
}

pub fn umgebung_hex(env: &[u8]) -> String {
    env.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ")
}

/// Identifikation über ReadDataByIdentifier (0x22): Teilenummer + Software,
/// Systemname, Hardware — im selben Aufbau wie [`ident_strings`].
pub fn ident_22(k: &mut Kwp) -> Vec<String> {
    let mut read = |did: u16| -> String {
        k.request(0x22, &did.to_be_bytes())
            .ok()
            .and_then(|r| r.get(2..).map(|d| String::from_utf8_lossy(d).trim().to_string()))
            .filter(|s| !s.is_empty())
            .unwrap_or_default()
    };
    let teil = read(0xF187);
    let sw = read(0xF189);
    let system = read(0xF197);
    let hw = read(0xF191);
    let mut out = Vec::new();
    if !teil.is_empty() || !sw.is_empty() {
        out.push(format!("{teil}  {sw}").trim().to_string());
    }
    if !system.is_empty() {
        out.push(system);
    }
    if !hw.is_empty() {
        out.push(format!("HW {hw}"));
    }
    out
}

/// Name eines TP-2.0-Steuergeräts. Die TP-2.0-Logikadressen sind NICHT die
/// gewohnten Diagnoseadressen (beim Golf 6: 0x09 = Servolenkung, 0x1F = Gateway,
/// 0x20 = Bordnetz) — darum zuerst aus der Antwort (VW-Bauteilnummer J…,
/// Stichwörter), nur für 0x01–0x03 aus der Adresse.
pub fn tp20_name(address: u8, ident: &[String]) -> String {
    let text = ident.join(" ").to_uppercase();
    // VW-Bauteilbezeichnung, z. B. „BJ533“, „BJ0527“
    let j = text
        .match_indices('J')
        .filter_map(|(i, _)| {
            let d: String = text[i + 1..].chars().take_while(|c| c.is_ascii_digit()).collect();
            let d = d.trim_start_matches('0');
            (d.len() == 3).then(|| d.to_string())
        })
        .next();
    let by_j = j.as_deref().and_then(|n| {
        Some(match n {
            "623" | "220" | "248" | "361" => "Motorsteuergerät",
            "217" | "743" => "Getriebe",
            "104" => "ABS/ESP",
            "234" => "Airbag",
            "285" => "Kombiinstrument",
            "519" => "Bordnetzsteuergerät",
            "533" => "Gateway",
            "386" => "Tür Fahrer",
            "387" => "Tür Beifahrer",
            "388" => "Tür hinten links",
            "389" => "Tür hinten rechts",
            "527" => "Lenksäulenelektronik",
            "500" => "Servolenkung",
            "255" | "301" => "Klimaanlage",
            "446" => "Einparkhilfe",
            "345" => "Anhängererkennung",
            "393" => "Komfortsystem",
            "540" => "Parkbremse",
            "587" => "Wählhebel",
            "367" => "Batterieüberwachung",
            _ => return None,
        })
    });
    let by_word = || -> Option<&'static str> {
        let has = |w: &str| text.contains(w);
        Some(if has("IMMO") {
            "Wegfahrsperre"
        } else if has("PARKHILFE") || has("PDC") {
            "Einparkhilfe"
        } else if has("TRAILER") || has("ANHAENG") {
            "Anhängersteuergerät"
        } else if has("RNS") || has("RCD") || has("NAVI") || has("RADIO") {
            "Radio/Navigation"
        } else if has("GATEWAY") || has("GW-") {
            // Polo 6R: „GW-K-CAN TP20“ (am Auto gesehen, 19.11.2026)
            "Gateway"
        } else if has("TUER") || has("TÜR") {
            "Türsteuergerät"
        } else if has("EPS") || has("LENKHILFE") {
            "Servolenkung"
        } else if has("KOMBI") {
            "Kombiinstrument"
        } else if has("AIRBAG") {
            "Airbag"
        } else if has("BCM") || has("BORDNETZ") {
            "Bordnetzsteuergerät"
        } else if has("ABS") || has("ESP") {
            "ABS/ESP"
        } else if has("KLIMA") || has("CLIMA") || has("MAN AC") || has("AUTO AC") {
            // Polo 6R: „Man AC“ (manuelle Klimaanlage, am Auto gesehen, 19.11.2026)
            "Klimaanlage"
        } else if has("MED17") || has("EDC") || has("MOTOR") || has("SIMOS") || has("ME7") {
            "Motorsteuergerät"
        } else if has("GETRIEBE") || has("DSG") {
            "Getriebe"
        } else {
            return None;
        })
    };
    let by_addr = || match address {
        0x01 => Some("Motorsteuergerät"),
        0x02 => Some("Getriebe"),
        0x03 => Some("ABS/ESP"),
        _ => None,
    };
    let name = by_j.or_else(by_word).or_else(by_addr).unwrap_or("");
    match (&j, name.is_empty()) {
        (Some(n), false) => format!("{name} (J{n})"),
        (_, false) => name.to_string(),
        _ => String::new(),
    }
}

fn nrc_name(nrc: u8) -> &'static str {
    match nrc {
        0x10 => "generalReject",
        0x11 => "serviceNotSupported",
        0x12 => "subFunctionNotSupported / invalidFormat",
        0x21 => "busyRepeatRequest",
        0x22 => "conditionsNotCorrect",
        0x31 => "requestOutOfRange",
        0x33 => "securityAccessDenied",
        0x78 => "responsePending",
        0x80 => "serviceNotSupportedInActiveDiagnosticSession",
        _ => "siehe ISO 14230-3",
    }
}

/// Ein Anpassungskanal (Funktion 10) eines TP-2.0-Steuergeräts.
#[derive(Debug, Clone, PartialEq)]
pub struct AnpassKanal {
    pub kanal: u8,
    /// Gespeicherter Anpassungswert
    pub wert: u16,
    /// Anzeigefelder wie bei Messwertblöcken
    pub felder: Vec<BlockValue>,
}

/// Ergebnis der Anpassungs-Routine (0x31 0xBA 01 03, ohne SID) zerlegen:
/// [BA 01 03][Status 0x82][0x03][Wert hi lo][Länge][Felder (Länge − 1)][FF].
/// Status 0x81 = noch nicht fertig → None. Am Golf 6 (Bordnetz 5K0 937 086)
/// mitgeschnitten: Kanal 1 = 0, 3 = 0, 17 = 1, 23 = 10 — wie im
/// Werkstatt-Diagnosegerät angezeigt.
pub fn anpass_ergebnis(r: &[u8]) -> Option<(u16, Vec<BlockValue>)> {
    if r.get(..3) != Some(&[0xBA, 0x01, 0x03]) || r.get(3) != Some(&0x82) {
        return None;
    }
    let p = r.get(4..)?;
    let wert = u16::from_be_bytes([*p.get(1)?, *p.get(2)?]);
    let n = usize::from(*p.get(3)?);
    let felder = p.get(4..4 + n.saturating_sub(1)).map(parse_block).unwrap_or_default();
    Some((wert, felder))
}

/// Anpassungskanäle eines TP-2.0-Steuergeräts lesen (nur lesen), so wie ein
/// Werkstatt-Diagnosegerät es tut: Routine 0x0103 starten (31 B8 01 03),
/// Kennung als Textzeilen: 1A 9B (fester VW-Aufbau, sonst Textstücke), bei
/// neueren Steuergeräten 0x22 (Golf-6-Bordnetz kennt 1A 9B nicht).
pub fn ident_zeilen(k: &mut Kwp) -> Vec<String> {
    match k.read_ecu_id(ECU_ID_VAG) {
        Ok(d) => ident_9b(&d).unwrap_or_else(|| ident_strings(&d)),
        Err(_) => ident_22(k),
    }
}

/// Teilenummer eines TP-2.0-Steuergeräts aus seiner Kennung lesen (nur lesen).
pub fn teilenummer_lesen(be: &crate::transport::Backend, adresse: u8) -> Option<String> {
    let mut ch = open_session(be, adresse).ok()?;
    let z = ident_zeilen(&mut Kwp::new(&mut ch));
    Some(crate::verbauliste::teilenummer(&z)).filter(|t| !t.is_empty())
}

/// Kanal wählen (31 B9 01 03 Kanal), Ergebnis holen (31 BA 01 03), Routine
/// beenden (32 B8 01 03). Es wird nichts gespeichert oder geändert.
pub fn anpassung_lesen(be: &crate::transport::Backend, adresse: u8, kanaele: &[u8]) -> Result<Vec<std::result::Result<AnpassKanal, String>>> {
    let mut ch = open_session(be, adresse)?;
    let mut k = Kwp::new(&mut ch);
    let mut out = Vec::new();
    for &kanal in kanaele {
        let r = (|| -> Result<AnpassKanal> {
            k.request(0x31, &[0xB8, 0x01, 0x03])?;
            let _ = k.request(0x31, &[0xBA, 0x01, 0x03]);
            k.request(0x31, &[0xB9, 0x01, 0x03, kanal])?;
            for _ in 0..20 {
                let r = k.request(0x31, &[0xBA, 0x01, 0x03])?;
                if let Some((wert, felder)) = anpass_ergebnis(&r) {
                    return Ok(AnpassKanal { kanal, wert, felder });
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            bail!(tr!("Kanal {a}: keine fertige Antwort", a = kanal))
        })();
        // Routine immer beenden (32 B8 01 03), wie das Werkstattgerät — sonst
        // lehnt das Steuergerät den nächsten Kanal ab (NRC 0x22).
        let _ = k.request(0x32, &[0xB8, 0x01, 0x03]);
        out.push(r.map_err(|e| {
            let t = format!("{e:#}");
            if t.contains("NRC 0x31") {
                tr!("Kanal {a}: nicht vorhanden", a = format!("{kanal:03}"))
            } else if t.contains("NRC 0x11") {
                tr!("Kanal {a}: dieses Steuergerät liest Anpassungen auf ältere Art — noch nicht eingebaut", a = format!("{kanal:03}"))
            } else {
                tr!("Kanal {a}: {b}", a = format!("{kanal:03}"), b = t)
            }
        }));
    }
    Ok(out)
}

/// Anfrage (mit SID) zum Speichern eines Anpassungswerts, nachdem der Kanal
/// mit 31 B9 01 03 gewählt ist. ANNAHME, noch nicht mitgeschnitten: Routine
/// 0x0103 mit Unterfunktion 0xBB, Kanal und Wert. Erst nach einem Mitschnitt
/// (Werkstattgerät schreibt einen Kanal) hier anpassen und
/// [`ANPASSUNG_SCHREIBEN_GEPRUEFT`] auf `true` setzen.
pub fn anpass_schreib_anfrage(kanal: u8, wert: u16) -> Vec<u8> {
    let [hi, lo] = wert.to_be_bytes();
    vec![0x31, 0xBB, 0x01, 0x03, kanal, hi, lo]
}

/// Einen Anpassungskanal ändern: Kanal lesen, neuen Wert speichern, zur
/// Kontrolle neu lesen. Am echten Auto nur ein Probelauf (liest den alten
/// Wert, zeigt die Anfrage), solange [`ANPASSUNG_SCHREIBEN_GEPRUEFT`] `false` ist.
pub fn anpassung_schreiben(be: &crate::transport::Backend, adresse: u8, kanal: u8, wert: u16) -> Result<Schreiben> {
    let alt = match anpassung_lesen(be, adresse, &[kanal])?.pop() {
        Some(Ok(a)) => a.wert,
        Some(Err(e)) => bail!(e),
        None => bail!(tr!("Kanal {a}: keine Antwort", a = kanal)),
    };
    let anfrage = anpass_schreib_anfrage(kanal, wert);
    if !darf_senden(be, ANPASSUNG_SCHREIBEN_GEPRUEFT) {
        return Ok(Schreiben::Probelauf { anfrage, alt: alt.to_be_bytes().to_vec() });
    }
    crate::spannung::vor_dem_schreiben(be)?;
    {
        let mut ch = open_session(be, adresse)?;
        let mut k = Kwp::new(&mut ch);
        let r = (|| -> Result<()> {
            k.request(0x31, &[0xB8, 0x01, 0x03])?;
            k.request(0x31, &[0xB9, 0x01, 0x03, kanal])?;
            k.request(anfrage[0], &anfrage[1..])?;
            Ok(())
        })();
        let _ = k.request(0x32, &[0xB8, 0x01, 0x03]);
        r?;
    }
    let danach = match anpassung_lesen(be, adresse, &[kanal])?.pop() {
        Some(Ok(a)) => a.wert,
        _ => bail!(tr!("Kontrolle: Kanal {a} danach nicht lesbar.", a = kanal)),
    };
    Ok(Schreiben::Geschrieben { alt: alt.to_be_bytes().to_vec(), danach: danach.to_be_bytes().to_vec() })
}

/// Ein Schritt der Stellglieddiagnose (Funktion 03), wie das Steuergerät ihn meldet.
#[derive(Debug, Clone, PartialEq)]
pub enum StellSchritt {
    /// Gerade angesteuertes Bauteil (Bauteilnummer, wie bei Fehlercodes 5-stellig)
    Bauteil(u16),
    /// Zwischenstand/Rückmeldung als Messwert
    Wert(BlockValue),
    /// Unbekannter Aufbau (roh)
    Roh(Vec<u8>),
}

impl StellSchritt {
    pub fn text(&self) -> String {
        match self {
            StellSchritt::Bauteil(n) => tr!("Bauteil {a} wird angesteuert", a = format!("{n:05}")),
            StellSchritt::Wert(v) => tr!("Rückmeldung: {a}", a = v.display()),
            StellSchritt::Roh(r) => tr!("Antwort: {a}", a = r.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ")),
        }
    }
}

/// Antwort auf 31 BA 01 02 (ohne SID) zerlegen. Am Golf 6 (Tür 1K0 959 701 AE)
/// mitgeschnitten: [BA 01 02][03 03 hi lo FF] = Bauteil, [BA 01 02][02 F a b] = Wert.
pub fn stell_ergebnis(r: &[u8]) -> Option<StellSchritt> {
    if r.get(..3) != Some(&[0xBA, 0x01, 0x02]) {
        return None;
    }
    let p = &r[3..];
    Some(match p {
        [0x03, 0x03, hi, lo, ..] => StellSchritt::Bauteil(u16::from_be_bytes([*hi, *lo])),
        [0x02, f, a, b, ..] => StellSchritt::Wert(BlockValue { formula: *f, a: *a, b: *b, text: None }),
        _ => StellSchritt::Roh(p.to_vec()),
    })
}

/// Was der Aufrufer nach jeder Abfrage möchte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StellBefehl {
    /// beim aktuellen Bauteil bleiben (weiter abfragen)
    Bleiben,
    /// nächstes Bauteil ansteuern
    Weiter,
    /// Stellglieddiagnose beenden
    Ende,
}

/// Stellglieddiagnose (Funktion 03) eines TP-2.0-Steuergeräts, wie ein
/// Werkstatt-Diagnosegerät sie macht: Routine 0x0102 starten (31 B8 01 02),
/// laufend den Stand abfragen (31 BA 01 02), auf Wunsch zum nächsten Bauteil
/// (31 B9 01 02), am Ende immer beenden (32 B8 01 02).
/// ACHTUNG: dabei werden Bauteile am Auto wirklich angesteuert.
/// `steuer` bekommt jeden neuen Stand und sagt, wie es weitergeht; es wird
/// etwa alle 150 ms gefragt (hält zugleich die Verbindung offen).
pub fn stellglieddiagnose(be: &crate::transport::Backend, adresse: u8, mut steuer: impl FnMut(Option<&StellSchritt>) -> StellBefehl) -> Result<()> {
    let mut ch = open_session(be, adresse)?;
    let mut k = Kwp::new(&mut ch);
    k.request(0x31, &[0xB8, 0x01, 0x02])?;
    let r = (|| -> Result<()> {
        let mut letzter: Option<StellSchritt> = None;
        loop {
            let r = k.request(0x31, &[0xBA, 0x01, 0x02])?;
            let s = stell_ergebnis(&r);
            let neu = s.is_some() && s != letzter;
            let befehl = steuer(if neu { s.as_ref() } else { None });
            if neu {
                letzter = s;
            }
            match befehl {
                StellBefehl::Bleiben => std::thread::sleep(std::time::Duration::from_millis(150)),
                StellBefehl::Weiter => {
                    k.request(0x31, &[0xB9, 0x01, 0x02])?;
                }
                StellBefehl::Ende => return Ok(()),
            }
        }
    })();
    // immer beenden — auch nach einem Fehler
    let _ = k.request(0x32, &[0xB8, 0x01, 0x02]);
    r
}

/// Ein Eintrag der Gateway-Verbauliste (4 Byte je Steuergerät).
#[derive(Debug, Clone, PartialEq)]
pub struct VerbauEintrag {
    /// Diagnoseadresse, z. B. 0x17 = Kombiinstrument
    pub adresse: u8,
    /// TP-2.0-Kanal (0 = keiner)
    pub tp20: u8,
    /// Bus-Kennung (am Golf 6: 0x00 Antrieb, 0x02 Komfort, 0x04 Infotainment …)
    pub bus: u8,
    /// Bit 0 = verbaut, Bit 2 = Fehler gespeichert (wie im Auto-Scan
    /// eines Werkstatt-Diagnosegeräts „i.O. 0000“ / „Fehler 0010“)
    pub status: u8,
}

impl VerbauEintrag {
    pub fn verbaut(&self) -> bool {
        self.status & 0x01 != 0
    }
    pub fn fehler(&self) -> bool {
        self.status & 0x04 != 0
    }
}

/// Antwort auf 0x1A 0x9F (ohne SID und Option) zerlegen:
/// [Kopfbyte][Adresse, TP-2.0-Kanal, Bus, Status] × n, am Ende 0xFF.
/// Am Golf 6 (1K0 907 530 AA) mit dem Auto-Scan eines Werkstatt-
/// Diagnosegeräts verglichen: dieselben 20 Steuergeräte, dieselben 4 mit Fehler.
pub fn verbauliste_9f(raw: &[u8]) -> Vec<VerbauEintrag> {
    let mut v = Vec::new();
    let mut i = 1;
    while i + 4 <= raw.len() {
        let r = &raw[i..i + 4];
        v.push(VerbauEintrag { adresse: r[0], tp20: r[1], bus: r[2], status: r[3] });
        i += 4;
    }
    v
}

/// Lange Codierung aus 0x1A 0x9A (ohne SID und Option), wie beim Gateway:
/// [Betriebsnummer 6][Softwarestand 4][0x10][Länge][Codierung (Länge − 1)]…
/// Golf 6 Gateway: E9 A0 7F 46 40 02 02 00 20 02 — gleich der Anzeige eines
/// Werkstatt-Diagnosegeräts.
pub fn codierung_9a(raw: &[u8]) -> Option<Vec<u8>> {
    if raw.get(10) != Some(&0x10) {
        return None;
    }
    let n = usize::from(*raw.get(11)?);
    if n < 2 {
        return None;
    }
    raw.get(12..12 + n - 1).map(<[u8]>::to_vec)
}

/// TP-2.0-Kanäle der laut Gateway verbauten Steuergeräte (plus das Gateway
/// selbst), sortiert — für einen schnellen Suchlauf wie beim Werkstattgerät.
pub fn kanaele_aus_verbauliste(l: &[VerbauEintrag]) -> Vec<u8> {
    let mut k: Vec<u8> = l.iter().filter(|e| e.verbaut() && e.tp20 != 0).map(|e| e.tp20).collect();
    k.push(GATEWAY);
    k.sort_unstable();
    k.dedup();
    k
}

/// Verbauliste direkt aus dem Gateway lesen (nur lesen).
pub fn gateway_verbauliste(be: &crate::transport::Backend) -> Result<Vec<VerbauEintrag>> {
    let mut ch = open_session(be, GATEWAY)?;
    let mut k = Kwp::new(&mut ch);
    let raw = k.read_ecu_id(ECU_ID_VERBAULISTE).map_err(|e| {
        let t = format!("{e:#}");
        if t.contains("NRC 0x11") || t.contains("NRC 0x12") {
            // Polo 6R (Gateway 6N0 909 901): kennt keine Verbauliste (am Auto gesehen, 19.11.2026)
            anyhow::anyhow!(tr!("Dieses Gateway liefert keine Verbauliste (ältere Bauart). Die Steuergeräte findet der normale Scan."))
        } else {
            e
        }
    })?;
    let l = verbauliste_9f(&raw);
    if l.is_empty() {
        anyhow::bail!(tr!("Das Gateway hat keine Verbauliste geliefert."));
    }
    Ok(l)
}

#[cfg(test)]
mod tests {
    #[test]
    fn namen_polo_6r() {
        // Kennungen vom Polo 6R (2012), 19.11.2026
        assert_eq!(super::tp20_name(0x1F, &["6N0909901  0254".into(), "GW-K-CAN TP20".into()]), "Gateway");
        assert_eq!(super::tp20_name(0x2C, &["6R0820045M  0202".into(), "Man AC".into()]), "Klimaanlage");
    }

    #[test]
    fn stellglied_antworten_wie_am_golf6() {
        assert_eq!(stell_ergebnis(&[0xBA, 0x01, 0x02, 0x03, 0x03, 0x08, 0x8C, 0xFF]), Some(StellSchritt::Bauteil(2188)));
        let w = stell_ergebnis(&[0xBA, 0x01, 0x02, 0x02, 0x21, 0x64, 0x64]).unwrap();
        assert_eq!(w.text(), "Rückmeldung: 100.00 %");
        assert_eq!(stell_ergebnis(&[0xBA, 0x01, 0x03, 0x82]), None);
    }

    #[test]
    fn anpassung_wie_am_golf6() {
        // Bordnetz, Kanal 23: Wert 10, Feld 3 = 10 s (Formel 0x37)
        let r = [0xBA, 0x01, 0x03, 0x82, 0x03, 0x00, 0x0A, 0x0D, 0x25, 0x05, 0xA8, 0x25, 0x00, 0x00, 0x37, 0xC8, 0x0A, 0x25, 0x00, 0x6B, 0xFF];
        let (wert, felder) = anpass_ergebnis(&r).unwrap();
        assert_eq!((wert, felder.len(), felder[2].display().as_str()), (10, 4, "10.00 s"));
        // noch nicht fertig
        assert_eq!(anpass_ergebnis(&[0xBA, 0x01, 0x03, 0x81]), None);
    }

    #[test]
    fn gateway_verbauliste_und_codierung_wie_am_golf6() {
        // gekürzt aus dem echten Mitschnitt: Motor (Fehler), Getriebe (nicht verbaut), Kombi, Tür BF (Fehler)
        let raw = [0xF1, 0x01, 0x01, 0x00, 0x05, 0x02, 0x02, 0x00, 0x00, 0x17, 0x07, 0x06, 0x01, 0x52, 0x23, 0x02, 0x05, 0xFF];
        let l = verbauliste_9f(&raw);
        let verbaut: Vec<(u8, bool)> = l.iter().filter(|e| e.verbaut()).map(|e| (e.adresse, e.fehler())).collect();
        assert_eq!(verbaut, vec![(0x01, true), (0x17, false), (0x52, true)]);
        // Suchlauf-Kanäle: Motor 0x01, Kombi 0x07, Tür BF 0x23, Gateway 0x1F
        assert_eq!(kanaele_aus_verbauliste(&l), vec![0x01, 0x07, 0x1F, 0x23]);
        let r9a = [0, 0, 0, 0, 0, 0x42, b'0', b'0', b'8', b'1', 0x10, 0x0B, 0xE9, 0xA0, 0x7F, 0x46, 0x40, 0x02, 0x02, 0x00, 0x20, 0x02, 0xFF];
        assert_eq!(codierung_9a(&r9a), Some(vec![0xE9, 0xA0, 0x7F, 0x46, 0x40, 0x02, 0x02, 0x00, 0x20, 0x02]));
        assert_eq!(codierung_9a(&r9a[..5]), None);
    }

    use super::*;

    #[test]
    fn umgebung_fehlt_als_fehlerart() {
        use anyhow::Context;
        let e = Err::<(), _>(anyhow::Error::from(UmgebungFehlt::Keine)).context("außen").unwrap_err();
        assert_eq!(e.downcast_ref::<UmgebungFehlt>(), Some(&UmgebungFehlt::Keine));
    }

    #[test]
    fn weitere_formeln() {
        let w = |formula, a, b| BlockValue { formula, a, b, text: None };
        assert_eq!(w(0x1A, 40, 130).display(), "90.00 °C");
        assert_eq!(w(0x11, b'O', b'K').display(), "OK");
        assert_eq!(w(0x37, 200, 10).display(), "10.00 s");
    }

    #[test]
    fn kurzcodierung_wie_golf6() {
        // Byte 0–15 Teilenummer + SW, 16–24 Daten, ab 25 Name (Tür Beifahrer, echt)
        let mut raw = b"1K0959702AE 1519".to_vec();
        raw.extend_from_slice(&[0x03, 0x00, 0x01, 0xB4, 0, 0, 0, 0, 0]);
        raw.extend_from_slice(b"BJ387  TUER-SG BT   ");
        assert_eq!(kurzcodierung_9b(&raw).map(|c| c.text()), Some("0000436".to_string()));
        // Schreiben wie das Werkstattgerät an der Fahrertür: 3B 9A | Betriebsnr. | SW | 03 Codierung
        let mut tuer = b"1K0959701AE 1519".to_vec();
        tuer.extend_from_slice(&[0x03, 0x00, 0x00, 0xB4, 0, 0, 0, 0, 0, 0x42]);
        tuer.extend_from_slice(b"J386  TUER-SG FT");
        assert_eq!(
            kurzcodierung_daten(&tuer, 180),
            Some(vec![0x9A, 0, 0, 0, 0, 0, 0x42, b'1', b'5', b'1', b'9', 0x03, 0x00, 0x00, 0xB4])
        );
        assert_eq!(kurzcodierung_daten(&tuer, 0x100_0000), None);
        // Gateway: anderer Aufbau (10 …) → keine Kurzcodierung
        raw[16] = 0x10;
        assert_eq!(kurzcodierung_9b(&raw), None);
    }

    #[test]
    fn ident_9b_golf6() {
        // Tür hinten links, am Golf 6 gelesen (gekürzt auf das Wesentliche)
        let mut r = b"1K0959703AG ".to_vec();
        r.extend_from_slice(&[0xB1, 0x34, 0x30, 0x31, 0x03, 0x00, 0x00, 0x90, 0, 0, 0, 0, 0]);
        r.extend_from_slice(b"BJ388   TUER-SG HL   ");
        assert_eq!(ident_9b(&r).unwrap(), vec!["1K0959703AG  1401".to_string(), "J388   TUER-SG HL".to_string()]);
        // Motor: Name direkt ab Byte 25
        let mut r = b"03C906016   5049".to_vec();
        r.extend_from_slice(&[0x03, 0x00, 0x00, 0x48, 0, 0, 0, 0, 0]);
        r.extend_from_slice(b"BMED17.5.5       G   ");
        assert_eq!(ident_9b(&r).unwrap(), vec!["03C906016  5049".to_string(), "MED17.5.5       G".to_string()]);
        // anderer Aufbau (Demo: Teil, 00 01, Name) → None
        let mut demo = b"1K0920851  ".to_vec();
        demo.extend_from_slice(&[0x00, 0x01]);
        demo.extend_from_slice(b"KOMBIINSTRUMENT");
        assert!(ident_9b(&demo).is_none());
    }

    #[test]
    fn block_med17_leere_felder() {
        // Golf 6 / MED17, Block 1 bei Zündung an (29.10.2026)
        let r = [0x01, 0xC8, 0x00, 0x05, 0x0A, 0xB5, 0x14, 0x32, 0x80, 0x10, 0xFF, 0x33, 0x25, 0, 0, 0x25, 0, 0, 0x25, 0, 0, 0x25, 0, 0];
        let v = parse_block(&r);
        assert_eq!(v.len(), 4);
        assert_eq!(v[1].display(), "81.00 °C");
        // Leeres Feld in der Mitte bleibt (Feldnummern), wird als „—“ gezeigt
        let w = |formula, a, b| BlockValue { formula, a, b, text: None }.display();
        assert_eq!((w(0x51, 0xCF, 0x49), w(0x51, 0x30, 0x5B), w(0x6B, 0x90, 0x03)), ("-545.61 °".into(), "541.58 °".into(), "0x9003".to_string()));
        let t = parse_block(&[0x01, 0x32, 0x00, 0x5F, 0x04, b'C', b'A', b'X', b' ', 0x05, 0x0A, 0x6D]);
        assert_eq!((t.len(), t[1].display().as_str(), t[2].display().as_str()), (3, "CAX", "9.00 °C"));
        let v = parse_block(&[0x01, 0xC8, 0x00, 0x25, 0, 0, 0x05, 0x0A, 0x6D, 0x25, 0, 0]);
        assert_eq!(v.len(), 3);
        assert_eq!(v[1].display(), "—");
    }
    use crate::tp20::{test_bus::SimBus, EcuSim, Tp20};

    fn vag_ecu() -> EcuSim {
        EcuSim::new(0x01, Box::new(|req| match req[0] {
            0x10 => vec![vec![0x50, req[1]]],
            0x1A => {
                let mut r = vec![0x5A, 0x9B];
                r.extend_from_slice(b"03G906016  \x00\x01MOTORELEKTRONIK");
                vec![r]
            }
            0x18 => vec![vec![0x58, 0x02, 0x41, 0x2C, 0x23, 0x46, 0xB5, 0x60]],
            // "Antwort folgt" (0x78), dann die eigentliche Antwort
            0x14 => vec![vec![0x7F, 0x14, 0x78], vec![0x54, 0xFF, 0x00]],
            0x21 if req[1] == 1 => vec![vec![0x61, 0x01, 0x01, 0xC8, 0x14, 0x05, 0x0A, 0xBE]],
            _ => vec![vec![0x7F, req[0], 0x11]],
        }))
    }

    #[test]
    fn full_kwp_session_over_tp20() {
        let bus = SimBus::new(vec![vag_ecu()]);
        let mut ch = Tp20::connect_io(&bus, 0x01).unwrap();
        let mut kwp = Kwp::new(&mut ch);
        kwp.start_session(SESSION_VAG_DIAG).unwrap();

        let id = kwp.read_ecu_id(ECU_ID_VAG).unwrap();
        assert_eq!(ident_strings(&id), vec!["03G906016", "MOTORELEKTRONIK"]);

        let dtcs = kwp.read_dtcs().unwrap();
        assert_eq!(dtcs.len(), 2);
        assert_eq!(dtcs[0].vag_code(), "16684");
        assert_eq!(dtcs[0].obd_code(None).as_deref(), Some("P0300"));
        assert_eq!(dtcs[1].obd_code(None).as_deref(), Some("P1693")); // 18101 = 17408 + 693

        kwp.clear_dtcs().unwrap(); // first answer is 0x78 pending

        let blk = kwp.read_block(1).unwrap();
        assert_eq!(blk[0].display(), "800.00 1/min"); // 0.2*200*20
        assert_eq!(blk[1].display(), "90.00 °C"); // 10*(190-100)*0.1

        assert!(kwp.read_block(99).is_err()); // NRC 0x11
    }

    #[test]
    fn obd_code_bereiche() {
        let d = |code| KwpDtc { code, status: 0 };
        assert_eq!(d(17544).obd_code(None).as_deref(), Some("P1136"));
        // MED17 (Golf 6): Zahl ist der OBD-Code selbst
        assert_eq!(d(8851).obd_code(Some(0x01)).as_deref(), Some("P2293"));
        assert_eq!(d(4880).obd_code(Some(0x01)).as_deref(), Some("P1310"));
        // Einparkhilfe: VAG-Bauteilcode, kein OBD-Code
        assert_eq!(d(1627).obd_code(Some(0x10)), None);
        assert_eq!(d(8851).obd_code(None), None);
    }

    #[test]
    fn no_fault_marker_is_filtered() {
        assert!(parse_dtcs(&[0x01, 0xFF, 0xFF, 0x00]).is_empty());
        assert!(parse_dtcs(&[]).is_empty());
    }

    #[test]
    fn unknown_formula_shows_raw() {
        let v = BlockValue { formula: 0xEE, a: 1, b: 2, text: None };
        assert_eq!(v.display(), "roh FEE 01 02");
    }

    #[test]
    fn umgebung_lesen_und_ohne_dienst_weiter() {
        let be = crate::transport::Backend::Demo;
        let mut ch = open_session(&be, 0x01).unwrap();
        let l = Kwp::new(&mut ch).read_dtcs_mit_umgebung().unwrap();
        assert_eq!(l.len(), 1);
        assert!(l[0].1.as_ref().is_some_and(|e| e.len() == 6), "{l:?}");
        // Steuergerät ohne Dienst 0x17: Fehler trotzdem lesbar, nur ohne Umgebung
        let bus = SimBus::new(vec![EcuSim::new(
            0x01,
            Box::new(|req: &[u8]| match req[0] {
                0x10 => vec![vec![0x50, 0x89]],
                0x18 => vec![vec![0x58, 0x01, 0x22, 0x93, 0x20]],
                _ => vec![vec![0x7F, req[0], 0x11]],
            }),
        )]);
        let mut tp = Tp20::connect_io(&bus, 0x01).unwrap();
        let mut k = Kwp::new(&mut tp);
        k.start_session(SESSION_VAG_DIAG).unwrap();
        let l = k.read_dtcs_mit_umgebung().unwrap();
        assert_eq!((l.len(), l[0].0.code, l[0].1.is_none()), (1, 8851, true));
    }

    #[test]
    fn kwp_status_im_klartext() {
        let d = |s| KwpDtc { code: 1627, status: s }.status_text();
        assert_eq!(d(0x2E), "sporadisch (gespeichert, gerade nicht vorhanden), Fehlerart 14: Bauteil defekt (0x2e)");
        assert_eq!(d(0x6C), "aktuell vorhanden, Fehlerart 12: elektrischer Fehler im Stromkreis (0x6c)");
        assert_eq!(d(0x20), "sporadisch (gespeichert, gerade nicht vorhanden) (0x20)");
        assert_eq!(d(0xE3), "aktuell vorhanden, Warnlampe an, Fehlerart 3 (0xe3)");
        // Golf 6, Tür Beifahrer 00929: Werkstatt-Diagnosegerät zeigt „008 … sporadisch“
        assert_eq!(d(0x38), "sporadisch (gespeichert, gerade nicht vorhanden), Fehlerart 8: Signal unplausibel (0x38)");
    }

    #[test]
    fn tp20_names_from_golf6_idents() {
        let n = |a, v: &[&str]| tp20_name(a, &v.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(n(0x1F, &["1K0907530AA 0081", "BJ533  Gateway   H07"]), "Gateway (J533)");
        assert_eq!(n(0x22, &["1K0959701AE 1519", "BJ386  TUER-SG FT"]), "Tür Fahrer (J386)");
        assert_eq!(n(0x2A, &["1K0953549CQ", "111", "BJ0527           053"]), "Lenksäulenelektronik (J527)");
        assert_eq!(n(0x09, &["1K0909144H  2901", "EPS_ZFLS Kl. 69"]), "Servolenkung");
        assert_eq!(n(0x20, &["5K0937086  0019", "BCM PQ35 M"]), "Bordnetzsteuergerät");
        assert_eq!(n(0x01, &["03C906016   5049", "BMED17.5.5       G"]), "Motorsteuergerät");
        assert_eq!(n(0x03, &[]), "ABS/ESP");
        assert_eq!(n(0x1D, &[]), "");
    }
}
