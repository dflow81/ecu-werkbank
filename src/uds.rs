//! Minimal UDS (ISO 14229) client on top of ISO-TP.
//!
//! Dienste (ISO 14229-1): 0x10 Sitzung, 0x11 ECUReset, 0x14 Fehler löschen,
//! 0x19 Fehler lesen (01/02/04/06/0A), 0x22/0x2E Kennungen, 0x23 Speicher lesen,
//! 0x27 SecurityAccess, 0x28 CommunicationControl, 0x2F Stellglied,
//! 0x31 Routine, 0x34–0x37 Datenübertragung (Download/Upload), 0x3E
//! TesterPresent, 0x85 ControlDTCSetting.
//!
//! Die Übertragungsdienste (0x34–0x37) sind Bausteine — einen Flash-Ablauf
//! für echte Steuergeräte gibt es bewusst noch nicht (siehe ROADMAP.md).

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
pub const SID_TESTER_PRESENT: u8 = 0x3E;
pub const SID_ECU_RESET: u8 = 0x11;
pub const SID_READ_MEMORY_BY_ADDRESS: u8 = 0x23;
pub const SID_COMMUNICATION_CONTROL: u8 = 0x28;
pub const SID_REQUEST_DOWNLOAD: u8 = 0x34;
pub const SID_REQUEST_UPLOAD: u8 = 0x35;
pub const SID_TRANSFER_DATA: u8 = 0x36;
pub const SID_REQUEST_TRANSFER_EXIT: u8 = 0x37;
pub const SID_CONTROL_DTC_SETTING: u8 = 0x85;

/// ECUReset-Arten (ISO 14229-1, 0x11).
pub const RESET_HARD: u8 = 0x01;
pub const RESET_KEY_OFF_ON: u8 = 0x02;
pub const RESET_SOFT: u8 = 0x03;

/// Höchstens so lange auf „Antwort folgt“ (NRC 0x78) warten.
const MAX_WARTEN_78: std::time::Duration = std::time::Duration::from_secs(60);

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
    /// Anfrage-ID des Steuergeräts, falls bekannt (für die Deutung der Fehlercodes)
    tx: Option<u16>,
}

impl<'a> Uds<'a> {
    pub fn new(tp: &'a dyn Transport) -> Self {
        Self { tp, tx: None }
    }

    /// Anfrage-ID mitgeben: Herstelleradressen (nicht 0x7E0–0x7E7) melden bei
    /// VW Fehlernummern, auch wenn sie das Format nicht nennen.
    pub fn mit_tx(mut self, tx: u16) -> Self {
        self.tx = Some(tx);
        self
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
        let start = std::time::Instant::now();

        loop {
            let resp = self.tp.recv()?;
            if resp.is_empty() {
                bail!(tr!("leere UDS-Antwort"));
            }
            if resp[0] == NEGATIVE_RESPONSE {
                let (req_sid, nrc) = (resp.get(1).copied(), resp.get(2).copied());
                if nrc == Some(NRC_RESPONSE_PENDING) {
                    if start.elapsed() > MAX_WARTEN_78 {
                        bail!(tr!("Steuergerät meldet seit {a} s „Antwort folgt“ — abgebrochen", a = MAX_WARTEN_78.as_secs()));
                    }
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
        let l: Vec<Dtc> = dtc_liste(&resp).into_iter().filter(|d| d.status & status_mask & !0x50 != 0).collect();
        Ok(self.mit_format(l))
    }

    /// Fehlercode-Format des Steuergeräts (ReadDTCInformation 0x01, nur lesen):
    /// 0x00 = ISO 15031-6 (P-Code + Fehlerart), 0x01 = ISO 14229-1 (Hersteller-
    /// nummer ohne genormte Bedeutung), 0x04 = SAE J2012-DA. None = nicht gemeldet.
    pub fn dtc_format(&self) -> Option<u8> {
        let r = self.request(SID_READ_DTC, &[0x01, 0xFF]).ok()?;
        // [01][Verfügbarkeitsmaske][Format][Anzahl hi lo]
        (r.first() == Some(&0x01) && r.len() >= 3).then(|| r[2])
    }

    /// Codes als Herstellernummer kennzeichnen, wenn das Steuergerät dieses
    /// Format meldet — oder das Format gar nicht nennt und auf einer
    /// Herstelleradresse sitzt (Golf 6 RNS 310 lehnt 0x19 0x01 ab, zeigt im
    /// Werkstatt-Diagnosegerät aber 0x100006 als „1048582“). Genormte P-Codes
    /// bleiben bei gemeldetem Format 0x00/0x04 und auf den OBD-Adressen.
    /// Nur gefragt, wenn es überhaupt Codes gibt.
    fn mit_format(&self, mut l: Vec<Dtc>) -> Vec<Dtc> {
        if l.is_empty() {
            return l;
        }
        let hersteller = match self.dtc_format() {
            Some(f) => f == DTC_FORMAT_ISO14229,
            None => self.tx.is_some_and(|t| !(0x7E0..=0x7E7).contains(&t)),
        };
        if hersteller {
            for d in &mut l {
                d.hersteller = true;
            }
        }
        l
    }

    /// Alle Fehlercodes, die das Steuergerät kennt (nicht nur die gespeicherten):
    /// ReadDTCInformation 0x0A (reportSupportedDTC). Kennt das Gerät das nicht,
    /// die ungefilterte Antwort auf 0x02 0xFF — viele VAG-Geräte melden dort
    /// ohnehin alle Codes mit „Test noch nicht gelaufen“. Nur lesen.
    pub fn read_supported_dtcs(&self) -> Result<(Vec<Dtc>, bool)> {
        match self.request(SID_READ_DTC, &[0x0A]) {
            Ok(r) => Ok((self.mit_format(dtc_liste(&r)), true)),
            Err(_) => Ok((self.mit_format(dtc_liste(&self.request(SID_READ_DTC, &[0x02, 0xFF])?)), false)),
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

    /// ReadDTCInformation 0x01 (reportNumberOfDTCByStatusMask): Anzahl der
    /// Fehler mit passendem Status, ohne die Liste zu holen.
    pub fn dtc_anzahl(&self, status_mask: u8) -> Result<u16> {
        let r = self.request(SID_READ_DTC, &[0x01, status_mask])?;
        // [01][Verfügbarkeitsmaske][Format][Anzahl hi lo]
        match r.get(3..5) {
            Some(&[hi, lo]) => Ok(u16::from_be_bytes([hi, lo])),
            _ => bail!(tr!("Antwort auf 0x19 0x01 zu kurz")),
        }
    }

    /// ReadDTCInformation 0x06 (reportDTCExtDataRecordByDTCNumber): erweiterte
    /// Daten eines Fehlers (bei VW u. a. Häufigkeit, Verlernzähler). Rohdaten
    /// nach [06][DTC 3][Status].
    pub fn read_ext_data(&self, dtc: u32, record: u8) -> Result<Vec<u8>> {
        let r = self.request(SID_READ_DTC, &[0x06, (dtc >> 16) as u8, (dtc >> 8) as u8, dtc as u8, record])?;
        Ok(r.get(5..).map(<[u8]>::to_vec).unwrap_or_default())
    }

    /// ECUReset (0x11). Startet das Steuergerät neu — nur am stehenden Auto.
    pub fn ecu_reset(&self, art: u8) -> Result<()> {
        self.request(SID_ECU_RESET, &[art]).map(|_| ())
    }

    /// CommunicationControl (0x28): `steuerung` 0x00 = alles an, 0x03 = alles
    /// aus; `art` 0x01 normale Botschaften, 0x03 normale + Netzwerkmanagement.
    pub fn communication_control(&self, steuerung: u8, art: u8) -> Result<()> {
        self.request(SID_COMMUNICATION_CONTROL, &[steuerung, art]).map(|_| ())
    }

    /// ControlDTCSetting (0x85): 0x01 = Fehler speichern an, 0x02 = aus.
    pub fn dtc_speichern(&self, an: bool) -> Result<()> {
        self.request(SID_CONTROL_DTC_SETTING, &[if an { 0x01 } else { 0x02 }]).map(|_| ())
    }

    /// ReadMemoryByAddress (0x23). Nur lesen.
    pub fn read_memory(&self, adresse: u32, laenge: u16) -> Result<Vec<u8>> {
        let mut d = vec![0x24]; // 2 Byte Länge, 4 Byte Adresse
        d.extend_from_slice(&adresse.to_be_bytes());
        d.extend_from_slice(&laenge.to_be_bytes());
        self.request(SID_READ_MEMORY_BY_ADDRESS, &d)
    }

    /// RequestDownload (0x34, zum Steuergerät) bzw. RequestUpload (0x35, vom
    /// Steuergerät). Gibt die höchste Blocklänge für TransferData zurück
    /// (inklusive SID und Blockzähler, wie in ISO 14229-1).
    pub fn request_transfer(&self, download: bool, adresse: u32, laenge: u32, format: u8) -> Result<usize> {
        let mut d = vec![format, 0x44]; // 4 Byte Länge, 4 Byte Adresse
        d.extend_from_slice(&adresse.to_be_bytes());
        d.extend_from_slice(&laenge.to_be_bytes());
        let r = self.request(if download { SID_REQUEST_DOWNLOAD } else { SID_REQUEST_UPLOAD }, &d)?;
        max_block_laenge(&r)
    }

    /// TransferData (0x36): ein Block mit Zähler (1, 2 … 255, 0, 1 …).
    pub fn transfer_data(&self, zaehler: u8, daten: &[u8]) -> Result<Vec<u8>> {
        let r = self.request(SID_TRANSFER_DATA, &[&[zaehler][..], daten].concat())?;
        if r.first() != Some(&zaehler) {
            bail!(tr!("TransferData: falscher Blockzähler in der Antwort"));
        }
        Ok(r[1..].to_vec())
    }

    /// RequestTransferExit (0x37).
    pub fn transfer_exit(&self) -> Result<Vec<u8>> {
        self.request(SID_REQUEST_TRANSFER_EXIT, &[])
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
        .map(|c| Dtc { raw: ((c[0] as u32) << 16) | ((c[1] as u32) << 8) | c[2] as u32, status: c[3], hersteller: false })
        .collect()
}

/// DTC-Format „ISO 14229-1“: die 3 Byte sind eine Herstellernummer.
pub const DTC_FORMAT_ISO14229: u8 = 0x01;

#[derive(Debug, Clone)]
pub struct Dtc {
    pub raw: u32,
    pub status: u8,
    /// Herstellernummer statt P-Code (Steuergerät meldet Format ISO 14229-1)
    pub hersteller: bool,
}

impl Dtc {
    /// Fehlerart (drittes Byte, „failure type“), z. B. 0x29.
    /// Bei Herstellernummern gibt es keine genormte Fehlerart (0).
    pub fn fehlerart(&self) -> u8 {
        if self.hersteller {
            return 0;
        }
        (self.raw & 0xFF) as u8
    }

    /// Anzeige-Code: genormt Pxxxx/Cxxxx/Bxxxx/Uxxxx, bei Herstellernummern
    /// die ganze Zahl dezimal (so zeigen es auch Werkstatt-Diagnosegeräte).
    pub fn code(&self) -> String {
        if self.hersteller {
            return self.raw.to_string();
        }
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

/// Antwort auf RequestDownload/-Upload (ohne SID): [lengthFormat][maxNumberOfBlockLength].
pub fn max_block_laenge(r: &[u8]) -> Result<usize> {
    let n = usize::from(r.first().copied().unwrap_or(0) >> 4);
    match r.get(1..1 + n) {
        Some(b) if (1..=4).contains(&n) => Ok(b.iter().fold(0usize, |a, x| a << 8 | usize::from(*x))),
        _ => bail!(tr!("Antwort auf RequestDownload/-Upload unvollständig")),
    }
}

pub fn nrc_name(nrc: u8) -> &'static str {
    match nrc {
        0x10 => "generalReject",
        0x11 => "serviceNotSupported",
        0x12 => "subFunctionNotSupported",
        0x13 => "incorrectMessageLengthOrInvalidFormat",
        0x14 => "responseTooLong",
        0x21 => "busyRepeatRequest (Steuergerät besetzt)",
        0x22 => "conditionsNotCorrect",
        0x24 => "requestSequenceError",
        0x25 => "noResponseFromSubnetComponent",
        0x26 => "failurePreventsExecutionOfRequestedAction",
        0x31 => "requestOutOfRange",
        0x33 => "securityAccessDenied",
        0x35 => "invalidKey",
        0x36 => "exceedNumberOfAttempts",
        0x37 => "requiredTimeDelayNotExpired",
        0x70 => "uploadDownloadNotAccepted",
        0x71 => "transferDataSuspended",
        0x72 => "generalProgrammingFailure",
        0x73 => "wrongBlockSequenceCounter",
        0x78 => "requestCorrectlyReceived-ResponsePending",
        0x7E => "subFunctionNotSupportedInActiveSession",
        0x7F => "serviceNotSupportedInActiveSession",
        0x81 => "rpmTooHigh",
        0x82 => "rpmTooLow",
        0x83 => "engineIsRunning",
        0x84 => "engineIsNotRunning",
        0x85 => "engineRunTimeTooLow",
        0x86 => "temperatureTooHigh",
        0x87 => "temperatureTooLow",
        0x88 => "vehicleSpeedTooHigh",
        0x89 => "vehicleSpeedTooLow",
        0x8A => "throttle/PedalTooHigh",
        0x8B => "throttle/PedalTooLow",
        0x8C => "transmissionRangeNotInNeutral",
        0x8D => "transmissionRangeNotInGear",
        0x8F => "brakeSwitch(es)NotClosed",
        0x90 => "shifterLeverNotInPark",
        0x91 => "torqueConverterClutchLocked",
        0x92 => "voltageTooHigh",
        0x93 => "voltageTooLow",
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

    #[test]
    fn hersteller_nummer_statt_p_code() {
        let d = Dtc { raw: 0x100006, status: 0x09, hersteller: false };
        assert_eq!((d.code(), d.fehlerart()), ("P1000".to_string(), 0x06));
        let d = Dtc { hersteller: true, ..d };
        assert_eq!((d.code(), d.fehlerart()), ("1048582".to_string(), 0));
    }

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

#[cfg(test)]
mod dienst_tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;

    /// Prüf-Transport: merkt sich die Anfragen, liefert vorgegebene Antworten.
    struct Skript {
        gesendet: RefCell<Vec<Vec<u8>>>,
        antworten: RefCell<VecDeque<Vec<u8>>>,
    }

    impl Skript {
        fn neu(antworten: &[&[u8]]) -> Self {
            Skript { gesendet: RefCell::new(Vec::new()), antworten: RefCell::new(antworten.iter().map(|a| a.to_vec()).collect()) }
        }
    }

    impl Transport for Skript {
        fn send(&self, data: &[u8]) -> Result<()> {
            self.gesendet.borrow_mut().push(data.to_vec());
            Ok(())
        }
        fn recv(&self) -> Result<Vec<u8>> {
            self.antworten.borrow_mut().pop_front().ok_or_else(|| anyhow::anyhow!("Timeout"))
        }
    }

    #[test]
    fn reset_kommunikation_dtc_setting() {
        let t = Skript::neu(&[&[0x51, 0x01], &[0x68, 0x03], &[0xC5, 0x02]]);
        let u = Uds::new(&t);
        u.ecu_reset(RESET_HARD).unwrap();
        u.communication_control(0x03, 0x01).unwrap();
        u.dtc_speichern(false).unwrap();
        assert_eq!(*t.gesendet.borrow(), vec![vec![0x11, 0x01], vec![0x28, 0x03, 0x01], vec![0x85, 0x02]]);
    }

    #[test]
    fn download_ablauf() {
        // 0x78 zwischendurch, maxNumberOfBlockLength 0x0402 (2 Byte)
        let t = Skript::neu(&[&[0x7F, 0x34, 0x78], &[0x74, 0x20, 0x04, 0x02], &[0x76, 0x01], &[0x76, 0x02], &[0x77]]);
        let u = Uds::new(&t);
        assert_eq!(u.request_transfer(true, 0x0001_0000, 0x200, 0x00).unwrap(), 0x402);
        u.transfer_data(1, &[0xAA, 0xBB]).unwrap();
        assert!(u.transfer_data(1, &[0xCC]).is_err(), "Zähler 2 statt 1 in der Antwort");
        u.transfer_exit().unwrap();
        let g = t.gesendet.borrow();
        assert_eq!(g[0], vec![0x34, 0x00, 0x44, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00]);
        assert_eq!(g[1], vec![0x36, 0x01, 0xAA, 0xBB]);
    }

    #[test]
    fn dtc_anzahl_und_speicher() {
        let t = Skript::neu(&[&[0x59, 0x01, 0xFF, 0x01, 0x00, 0x03], &[0x63, 0xDE, 0xAD], &[0x7F, 0x23, 0x31]]);
        let u = Uds::new(&t);
        assert_eq!(u.dtc_anzahl(0x08).unwrap(), 3);
        assert_eq!(u.read_memory(0x1000, 2).unwrap(), vec![0xDE, 0xAD]);
        let e = u.read_memory(0x1000, 2).unwrap_err().to_string();
        assert!(e.contains("requestOutOfRange"), "{e}");
        assert_eq!(nrc_name(0x93), "voltageTooLow");
    }
}
