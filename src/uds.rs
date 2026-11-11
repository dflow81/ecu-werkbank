//! Minimal UDS (ISO 14229) client on top of ISO-TP.
//!
//! Covers the services needed to replicate the everyday VCDS workflow:
//! session control, tester-present keep-alive, reading data identifiers,
//! reading and clearing diagnostic trouble codes.

use crate::tr;
use anyhow::{bail, Result};
use crate::transport::Transport;

// Service IDs
pub const SID_DIAGNOSTIC_SESSION_CONTROL: u8 = 0x10;
pub const SID_CLEAR_DTC: u8 = 0x14;
pub const SID_READ_DTC: u8 = 0x19;
pub const SID_READ_DATA_BY_ID: u8 = 0x22;
pub const SID_SECURITY_ACCESS: u8 = 0x27;
pub const SID_WRITE_DATA_BY_ID: u8 = 0x2E;
pub const SID_IO_CONTROL: u8 = 0x2F;
pub const SID_ROUTINE_CONTROL: u8 = 0x31;
#[allow(dead_code)]
pub const SID_TESTER_PRESENT: u8 = 0x3E;

// Häufige Sessions
pub const SESSION_DEFAULT: u8 = 0x01;
pub const SESSION_EXTENDED: u8 = 0x03;

// RoutineControl Sub-Funktionen
pub const ROUTINE_START: u8 = 0x01;
pub const ROUTINE_STOP: u8 = 0x02;
pub const ROUTINE_RESULTS: u8 = 0x03;

/// Sub-Funktion für RoutineControl prüfen: nur 1 (Start), 2 (Stopp), 3 (Ergebnis).
/// Alles andere ist ein Eingabefehler — nie still auf „Start“ ausweichen.
pub fn routine_sub_pruefen(sub: u16) -> Result<u8> {
    match sub {
        1..=3 => Ok(sub as u8),
        _ => bail!(tr!("Sub {a} gibt es nicht — nur 1 (Start), 2 (Stopp) oder 3 (Ergebnis).", a = sub)),
    }
}

/// Klartext einer RoutineControl-Sub-Funktion.
pub fn routine_sub_text(sub: u8) -> &'static str {
    match sub {
        ROUTINE_START => crate::trs!("starten"),
        ROUTINE_STOP => crate::trs!("stoppen"),
        ROUTINE_RESULTS => crate::trs!("Ergebnis abfragen"),
        _ => "?",
    }
}

const NEGATIVE_RESPONSE: u8 = 0x7F;
const NRC_RESPONSE_PENDING: u8 = 0x78;
const NRC_BUSY_REPEAT: u8 = 0x21;

pub struct Uds<'a> {
    tp: &'a dyn Transport,
}

impl<'a> Uds<'a> {
    pub fn new(tp: &'a dyn Transport) -> Self {
        Self { tp }
    }

    /// Send a request and return the positive-response payload (without the
    /// echoed service id). Negative responses become errors; 0x78
    /// (responsePending) is awaited transparently.
    pub fn request(&self, sid: u8, data: &[u8]) -> Result<Vec<u8>> {
        let mut req = Vec::with_capacity(1 + data.len());
        req.push(sid);
        req.extend_from_slice(data);
        self.tp.send(&req)?;
        // „besetzt, bitte wiederholen“ (0x21): die Anfrage wurde NICHT ausgeführt,
        // nach kurzer Pause erneut senden (ISO 14229-1). Golf 6: RNS 310 nach dem Einschalten.
        let mut besetzt = 0;
        let mut fremd = 0;

        loop {
            let resp = self.tp.recv()?;
            if resp.is_empty() {
                bail!(tr!("leere UDS-Antwort"));
            }
            if resp[0] == NEGATIVE_RESPONSE {
                let (req_sid, nrc) = (resp.get(1).copied(), resp.get(2).copied());
                if nrc == Some(NRC_RESPONSE_PENDING) {
                    continue; // ECU asks us to wait; read the next frame
                }
                if nrc == Some(NRC_BUSY_REPEAT) && req_sid == Some(sid) && besetzt < 5 {
                    besetzt += 1;
                    std::thread::sleep(std::time::Duration::from_millis(200 * besetzt));
                    self.tp.send(&req)?;
                    continue;
                }
                bail!(tr!("Negative Response auf SID {a}: NRC {b} ({c})", a = format!("{:#04x}", req_sid.unwrap_or(0)), b = nrc.map(|n| format!("{n:#04x}")).unwrap_or_else(|| "?".into()), c = nrc.map(nrc_name).unwrap_or("unbekannt")));
            }
            // verspätete Antwort auf eine frühere Anfrage (Golf 6: RNS 310 nach
            // dem Anlassen): überspringen und auf die eigene warten
            if resp[0] != sid + 0x40 && resp[0] & 0x40 != 0 && fremd < 3 {
                fremd += 1;
                continue;
            }
            if resp[0] != sid + 0x40 {
                bail!(tr!("unerwartete Antwort-SID {a} (erwartet {b})", a = format!("{:#04x}", resp[0]), b = format!("{:#04x}", sid + 0x40)));
            }
            return Ok(resp[1..].to_vec());
        }
    }

    pub fn diagnostic_session(&self, session: u8) -> Result<Vec<u8>> {
        self.request(SID_DIAGNOSTIC_SESSION_CONTROL, &[session])
    }

    #[allow(dead_code)] // keep-alive für längere Sitzungen, Nutzung folgt
    pub fn tester_present(&self) -> Result<()> {
        // sub-function 0x00, suppress-positive-response bit NOT set so we can confirm
        self.request(SID_TESTER_PRESENT, &[0x00]).map(|_| ())
    }

    pub fn read_data_by_id(&self, did: u16) -> Result<Vec<u8>> {
        let resp = self.request(SID_READ_DATA_BY_ID, &[(did >> 8) as u8, did as u8])?;
        // Antwort ist [DID_hi, DID_lo, Daten…] — das echo-DID abschneiden.
        if resp.len() >= 2 {
            Ok(resp[2..].to_vec())
        } else {
            Ok(resp)
        }
    }

    /// WriteDataByIdentifier (0x2E) — für Codierung und Anpassung.
    /// Schreibt `data` unter den Data Identifier `did`.
    pub fn write_data_by_id(&self, did: u16, data: &[u8]) -> Result<()> {
        let mut payload = Vec::with_capacity(2 + data.len());
        payload.push((did >> 8) as u8);
        payload.push(did as u8);
        payload.extend_from_slice(data);
        self.request(SID_WRITE_DATA_BY_ID, &payload).map(|_| ())
    }

    /// RoutineControl (0x31) — für Grundeinstellung / Service-Routinen.
    /// `sub` = ROUTINE_START/STOP/RESULTS, `routine_id` = 16-bit Routine.
    pub fn routine_control(&self, sub: u8, routine_id: u16, data: &[u8]) -> Result<Vec<u8>> {
        let mut payload = Vec::with_capacity(3 + data.len());
        payload.push(sub);
        payload.push((routine_id >> 8) as u8);
        payload.push(routine_id as u8);
        payload.extend_from_slice(data);
        self.request(SID_ROUTINE_CONTROL, &payload)
    }

    /// SecurityAccess (0x27) — Seed anfordern (ungerade Sub-Funktion, z. B. 0x01).
    /// Gibt den Seed zurück; die Key-Berechnung ist steuergerätespezifisch.
    pub fn security_request_seed(&self, level: u8) -> Result<Vec<u8>> {
        let resp = self.request(SID_SECURITY_ACCESS, &[level])?;
        // Antwort: [echo level][seed…] — Seed ohne das Level-Echo zurückgeben.
        Ok(if resp.is_empty() { resp } else { resp[1..].to_vec() })
    }

    /// SecurityAccess (0x27) — berechneten Key senden (gerade Sub-Funktion,
    /// z. B. 0x02 zu Seed-Level 0x01).
    pub fn security_send_key(&self, level: u8, key: &[u8]) -> Result<()> {
        let mut payload = Vec::with_capacity(1 + key.len());
        payload.push(level);
        payload.extend_from_slice(key);
        self.request(SID_SECURITY_ACCESS, &payload).map(|_| ())
    }

    /// InputOutputControlByIdentifier (0x2F) — Stellglieddiagnose (Output Tests).
    /// `control` = Parameter (z. B. 0x03 shortTermAdjustment, 0x00 returnControl),
    /// `state` = gewünschte Stellgröße (ECU-abhängig).
    pub fn io_control(&self, did: u16, control: u8, state: &[u8]) -> Result<Vec<u8>> {
        let mut payload = Vec::with_capacity(4 + state.len());
        payload.push((did >> 8) as u8);
        payload.push(did as u8);
        payload.push(control);
        payload.extend_from_slice(state);
        self.request(SID_IO_CONTROL, &payload)
    }

    /// ReadDTCInformation, sub-function 0x02 (reportDTCByStatusMask).
    pub fn read_dtcs(&self, status_mask: u8) -> Result<Vec<Dtc>> {
        let resp = self.request(SID_READ_DTC, &[0x02, status_mask])?;
        // Viele Steuergeräte melden ALLE bekannten Codes, auch mit Status
        // 0x10/0x40 („Test noch nicht gelaufen“) — das sind keine Fehler.
        Ok(dtc_liste(&resp).into_iter().filter(|d| d.status & status_mask & !0x50 != 0).collect())
    }

    /// Alle Fehlercodes, die das Steuergerät kennt (nicht nur die gespeicherten):
    /// ReadDTCInformation 0x0A (reportSupportedDTC). Kennt das Gerät das nicht,
    /// die ungefilterte Antwort auf 0x02 0xFF — viele VAG-Geräte melden dort
    /// ohnehin alle Codes mit „Test noch nicht gelaufen“. Nur lesen.
    pub fn read_supported_dtcs(&self) -> Result<(Vec<Dtc>, bool)> {
        match self.request(SID_READ_DTC, &[0x0A]) {
            Ok(r) => Ok((dtc_liste(&r), true)),
            Err(_) => Ok((dtc_liste(&self.request(SID_READ_DTC, &[0x02, 0xFF])?), false)),
        }
    }

    /// ReadDTCInformation 0x04 (reportDTCSnapshotRecordByDTCNumber): Umgebungs-
    /// bedingungen eines Fehlers. Liefert je Datensatz die Rohdaten nach der
    /// Anzahl der Kennungen — die Aufteilung auf einzelne DIDs hängt von deren
    /// (steuergerätespezifischer) Länge ab, siehe [`split_snapshot`].
    pub fn read_snapshot(&self, dtc: u32) -> Result<Vec<u8>> {
        let r = self.request(SID_READ_DTC, &[0x04, (dtc >> 16) as u8, (dtc >> 8) as u8, dtc as u8, 0xFF])?;
        // [04][DTC 3][Status][Datensatz-Nr][Anzahl][DID…]
        Ok(r.get(5..).map(<[u8]>::to_vec).unwrap_or_default())
    }

    /// ClearDiagnosticInformation for all groups (0xFFFFFF).
    pub fn clear_dtcs(&self) -> Result<()> {
        self.request(SID_CLEAR_DTC, &[0xFF, 0xFF, 0xFF]).map(|_| ())
    }
}

/// Antwort auf 0x19 0x02/0x0A zerlegen:
/// [Unterfunktion][Verfügbarkeitsmaske][ (3 Byte Code + 1 Byte Status) × N ]
fn dtc_liste(resp: &[u8]) -> Vec<Dtc> {
    resp.get(2..)
        .unwrap_or_default()
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| Dtc { raw: ((c[0] as u32) << 16) | ((c[1] as u32) << 8) | c[2] as u32, status: c[3] })
        .collect()
}

#[derive(Debug, Clone)]
pub struct Dtc {
    pub raw: u32,
    pub status: u8,
}

impl Dtc {
    /// Render as the standard Pxxxx/Cxxxx/Bxxxx/Uxxxx code.
    /// Fehlerart (drittes Byte, „failure type“), z. B. 0x29.
    pub fn fehlerart(&self) -> u8 {
        (self.raw & 0xFF) as u8
    }

    pub fn code(&self) -> String {
        let b = ((self.raw >> 16) & 0xFF) as u8;
        let letter = match b >> 6 {
            0 => 'P',
            1 => 'C',
            2 => 'B',
            _ => 'U',
        };
        let d1 = (b >> 4) & 0x03;
        let d2 = b & 0x0F;
        let d3 = (self.raw >> 8) & 0xFF;
        format!("{letter}{d1}{d2:X}{:02X}", d3)
    }
}

fn nrc_name(nrc: u8) -> &'static str {
    match nrc {
        0x10 => "generalReject",
        0x11 => "serviceNotSupported",
        0x12 => "subFunctionNotSupported",
        0x13 => "incorrectMessageLengthOrInvalidFormat",
        0x21 => "busyRepeatRequest (Steuergerät besetzt)",
        0x22 => "conditionsNotCorrect",
        0x31 => "requestOutOfRange",
        0x33 => "securityAccessDenied",
        0x35 => "invalidKey",
        0x78 => "requestCorrectlyReceived-ResponsePending",
        0x7F => "serviceNotSupportedInActiveSession",
        _ => "siehe ISO 14229-1 Tabelle",
    }
}

/// Snapshot-Rohdaten in (DID, Daten) zerlegen. `len_of` liefert die Datenlänge
/// bekannter DIDs; bei unbekannter Länge bekommt die letzte Kennung den Rest,
/// sonst bricht die Zerlegung ab (Rest als DID 0 = „Rohdaten“).
pub fn split_snapshot(raw: &[u8], len_of: impl Fn(u16) -> Option<usize>) -> Vec<(u16, Vec<u8>)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 2 <= raw.len() {
        let rec_count = raw[i + 1] as usize; // [Datensatz-Nr][Anzahl]
        i += 2;
        for k in 0..rec_count {
            if i + 2 > raw.len() {
                return out;
            }
            let did = u16::from_be_bytes([raw[i], raw[i + 1]]);
            i += 2;
            let last = k + 1 == rec_count;
            let n = match len_of(did) {
                Some(n) => n,
                None if last => raw.len() - i,
                None => {
                    out.push((did, raw[i..].to_vec()));
                    return out;
                }
            };
            let end = (i + n).min(raw.len());
            out.push((did, raw[i..end].to_vec()));
            i = end;
        }
    }
    out
}

/// Bekannte Längen gängiger Snapshot-Kennungen (VAG-üblich, TODO(verify)).
pub fn snapshot_did_len(did: u16) -> Option<usize> {
    match did {
        0x2002 => Some(3), // Kilometerstand
        0xF40C => Some(2), // Drehzahl
        0xF405 => Some(1), // Kühlmittel
        0xF40D => Some(1), // Geschwindigkeit
        _ => None,
    }
}

/// Antwort-ID zu einer UDS-Anfrage-ID: OBD-Adressen (0x7E0–0x7E7) antworten auf
/// +8, VAG-Diagnose-IDs (0x700–0x795) auf +0x6A (z. B. Kombi 0x714 → 0x77E,
/// am Golf 6 gemessen). Sonst +8.
pub fn antwort_id(tx: u16) -> u16 {
    match tx {
        0x7E0..=0x7E7 => tx + 8,
        0x700..=0x795 => tx + 0x6A,
        _ => tx.wrapping_add(8),
    }
}

/// Schneller Suchlauf über die VAG-UDS-Diagnose-IDs (0x700–0x7DF, Antwort auf
/// Anfrage-ID + 0x6A). Fragt nur die Teilenummer (0x22 F187) ab, etwa 3 s.
/// Liefert (Anfrage-ID, Antwort-ID) der Steuergeräte, die antworten.
pub fn vag_sweep(iface: &str) -> Vec<(u16, u16)> {
    use crate::can::{CanFrame, CanSocket, EmbeddedFrame, Id, Socket, StandardId};
    let Ok(sock) = CanSocket::open(iface) else { return vec![] };
    let _belegt = crate::bus::Belegt::neu();
    crate::connect::fix_hw_loopback(iface, &sock);
    let _ = sock.set_nonblocking(true);
    let mut found = std::collections::BTreeSet::new();
    let drain = |found: &mut std::collections::BTreeSet<(u16, u16)>| {
        while let Ok(f) = sock.read_frame() {
            if let Id::Standard(id) = f.id() {
                let rx = id.as_raw();
                // positive (0x62) oder negative (0x7F) Antwort, Einzel- oder Erst-Frame
                let d = f.data();
                let pci = d.first().copied().unwrap_or(0) >> 4;
                let sid = if pci == 1 { d.get(2) } else { d.get(1) };
                if (0x76A..=0x7FF).contains(&rx) && matches!(sid, Some(0x62) | Some(0x7F)) {
                    found.insert((rx - 0x6A, rx));
                }
            }
        }
    };
    for tx in 0x700u16..0x7E0 {
        let Some(id) = StandardId::new(tx) else { continue };
        if let Some(f) = CanFrame::new(id, &[0x03, 0x22, 0xF1, 0x87, 0xAA, 0xAA, 0xAA, 0xAA]) {
            let _ = sock.write_frame(&f);
        }
        std::thread::sleep(std::time::Duration::from_millis(12));
        drain(&mut found);
    }
    let end = std::time::Instant::now() + std::time::Duration::from_millis(300);
    while std::time::Instant::now() < end {
        drain(&mut found);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    found.into_iter().collect()
}

#[cfg(test)]
mod tests_routine {
    use super::*;

    #[test]
    fn antwort_ids() {
        assert_eq!(antwort_id(0x7E0), 0x7E8); // OBD Motor
        assert_eq!(antwort_id(0x714), 0x77E); // Kombi Golf 6 (gemessen)
        assert_eq!(antwort_id(0x711), 0x77B); // Wegfahrsperre
        assert_eq!(antwort_id(0x746), 0x7B0); // Klima
        assert_eq!(antwort_id(0x76C), 0x7D6); // Radio
    }

    #[test]
    fn routine_sub_nur_1_bis_3() {
        assert_eq!(routine_sub_pruefen(1).unwrap(), ROUTINE_START);
        assert_eq!(routine_sub_pruefen(2).unwrap(), ROUTINE_STOP);
        assert_eq!(routine_sub_pruefen(3).unwrap(), ROUTINE_RESULTS);
        // früher wurde aus ungültigen Eingaben still „Start“
        assert!(routine_sub_pruefen(0).is_err());
        assert!(routine_sub_pruefen(4).is_err());
        assert!(routine_sub_pruefen(0x102).is_err());
    }
}
