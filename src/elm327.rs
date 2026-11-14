//! ELM327-Backend: UDS über einen ELM327-kompatiblen OBD-Adapter.
//!
//! Erreichbar über Seriell (USB `/dev/ttyUSB0`, Bluetooth `/dev/rfcomm0`) oder
//! TCP (WiFi-Adapter). Der ELM327 übernimmt ISO-TP selbst (`ATCAF1`), wir
//! schicken also nur die UDS-Nutzlast als Hex und parsen die Hex-Antwort.
//!
//! ## Stand der Prüfung
//!
//! Am 14.11.2026 am Golf 6 (2010) mit einem USB-ELM327 v1.5 (FTDI) geprüft:
//! OBD-II (VIN, Readiness, Fehler, Live-Werte) und UDS-Steuergeräte mit
//! Herstelleradressen (Kombi, Airbag, Climatronic, Radio) lesend. Schreiben
//! über ELM327 ist weiter ungeprüft. TP 2.0 kann ein ELM327 grundsätzlich nicht.

use anyhow::{Context, Result};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::time::Duration;

use crate::transport::Transport;

/// Lese-/Schreib-Strom (Seriell oder TCP).
trait Stream: Read + Write {}
impl<T: Read + Write> Stream for T {}

pub struct Elm327 {
    stream: RefCell<Box<dyn Stream>>,
    queue: RefCell<VecDeque<Vec<u8>>>,
}

impl Elm327 {
    pub fn open_serial(port: &str, baud: u32, tx: u16, rx: u16) -> Result<Self> {
        let mut sp = serial2::SerialPort::open(port, baud)
            .with_context(|| crate::tr!("Seriellen Port '{port}' öffnen", port = port))?;
        let _ = sp.set_read_timeout(Duration::from_secs(5));
        let me = Self::wrap(Box::new(sp));
        me.init(tx, rx)?;
        Ok(me)
    }

    pub fn open_tcp(addr: &str, tx: u16, rx: u16) -> Result<Self> {
        let stream = std::net::TcpStream::connect(addr)
            .with_context(|| format!("TCP-Verbindung zu '{addr}'"))?;
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        let me = Self::wrap(Box::new(stream));
        me.init(tx, rx)?;
        Ok(me)
    }

    fn wrap(stream: Box<dyn Stream>) -> Self {
        Self {
            stream: RefCell::new(stream),
            queue: RefCell::new(VecDeque::new()),
        }
    }

    /// Ein AT-/Hex-Kommando senden und bis zum `>`-Prompt lesen.
    fn cmd(&self, c: &str) -> Result<String> {
        {
            let mut s = self.stream.borrow_mut();
            s.write_all(c.as_bytes())?;
            s.write_all(b"\r")?;
            s.flush()?;
        }
        self.read_until_prompt()
    }

    fn read_until_prompt(&self) -> Result<String> {
        let mut out = String::new();
        let mut buf = [0u8; 256];
        let mut s = self.stream.borrow_mut();
        loop {
            let n = s.read(&mut buf).context("ELM327 Lesen (Timeout?)")?;
            if n == 0 {
                break;
            }
            out.push_str(&String::from_utf8_lossy(&buf[..n]));
            if out.contains('>') {
                break;
            }
        }
        Ok(out)
    }

    fn init(&self, tx: u16, rx: u16) -> Result<()> {
        let _ = self.cmd("ATZ"); // Reset (Antwort variiert, ignorieren)
        self.cmd("ATE0")?; // Echo aus
        self.cmd("ATL0")?; // Linefeeds aus
        self.cmd("ATS0")?; // Leerzeichen aus
        self.cmd("ATH0")?; // Header aus (nur Nutzdaten)
        self.cmd("ATSP6")?; // ISO 15765-4 CAN, 11 bit, 500 kbit/s
        self.cmd("ATCAF1")?; // ISO-TP-Autoformatierung (Chip macht Segmentierung)
        self.cmd(&format!("ATSH{tx:03X}"))?; // Request-Header (tx-ID)
        self.cmd(&format!("ATCRA{rx:03X}"))?; // Empfangsfilter (rx-ID)
        // Flusskontrolle für mehrteilige Antworten selbst setzen: von sich aus
        // schickt der ELM327 das „weiter senden“ nur bei OBD-IDs (0x7E8…) an die
        // richtige Adresse — bei Herstelleradressen (Golf 6: Kombi 0x714 → 0x77E)
        // kam sonst nur der erste Teil an (am Auto geprüft, 14.11.2026).
        self.cmd(&format!("ATFCSH{tx:03X}"))?; // FC an die Anfrage-ID
        self.cmd("ATFCSD300000")?; // FC: weiter senden, keine Blockgrenze, kein Abstand
        self.cmd("ATFCSM1")?; // FC-Kopf und -Daten wie oben
        // ~1 s auf Antworten warten: Airbag und Radio melden erst „bitte warten“
        // (7F xx 78) — mit längerer Wartezeit kommt die echte Antwort gleich mit.
        self.cmd("ATSTFF")?;
        Ok(())
    }
}

impl Transport for Elm327 {
    fn send(&self, data: &[u8]) -> Result<()> {
        let hex: String = data.iter().map(|b| format!("{b:02X}")).collect();
        let resp = self.cmd(&hex)?;
        let mut q = self.queue.borrow_mut();
        q.clear(); // Reste einer früheren Anfrage verwerfen
        q.extend(parse_response(&resp));
        Ok(())
    }

    fn recv(&self) -> Result<Vec<u8>> {
        self.queue
            .borrow_mut()
            .pop_front()
            .ok_or_else(|| anyhow::anyhow!(crate::trs!("keine ELM327-Antwort (NO DATA / Timeout?)")))
    }
}

/// Statuszeilen des ELM327, die keine Nutzdaten sind.
fn ist_status(up: &str) -> bool {
    up.contains("NO DATA")
        || up.contains("CAN ERROR")
        || up.contains("UNABLE")
        || up.contains("BUFFER")
        || up.contains("SEARCHING")
        || up.contains("STOPPED")
        || up.contains("ELM327")
        || up == "?"
        || up == "OK"
}

/// Ganze ELM327-Antwort (bis zum `>`) in einzelne Nachrichten zerlegen.
///
/// Mit `ATL0` trennt der ELM327 Zeilen nur mit `\r`. Mehrteilige Antworten
/// (ISO-TP, z. B. die VIN) kommen mit `ATCAF1` so:
/// ```text
/// 014            ← Gesamtlänge (hex)
/// 0:490201575657 ← Teil 0
/// 1:5A5A5A314B5A41
/// 2:57303030303031
/// ```
/// Die Teile werden zusammengesetzt und auf die Länge gekürzt. Jede andere
/// Zeile ist eine eigene Nachricht (z. B. wenn mehrere Steuergeräte antworten).
fn parse_response(resp: &str) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    // laufende mehrteilige Nachricht: (Länge falls bekannt, Bytes)
    let mut teil: Option<(Option<usize>, Vec<u8>)> = None;
    let fertig = |t: Option<(Option<usize>, Vec<u8>)>, out: &mut Vec<Vec<u8>>| {
        if let Some((len, mut b)) = t {
            if let Some(n) = len {
                b.truncate(n);
            }
            if !b.is_empty() {
                out.push(b);
            }
        }
    };
    for zeile in resp.split(['\r', '\n']) {
        let zeile = zeile.replace('>', "");
        let zeile = zeile.trim();
        if zeile.is_empty() || ist_status(&zeile.to_uppercase()) {
            continue;
        }
        if let Some((nr, daten)) = zeile.split_once(':') {
            // Teil N einer mehrteiligen Nachricht
            let nr = nr.trim();
            let Some(bytes) = parse_hex_line(daten) else { continue };
            if nr == "0" && teil.as_ref().is_some_and(|(_, b)| !b.is_empty()) {
                // neuer Anfang ohne Längenzeile davor
                fertig(teil.take(), &mut out);
            }
            teil.get_or_insert((None, Vec::new())).1.extend(bytes);
            continue;
        }
        let hex: String = zeile.chars().filter(|c| !c.is_whitespace()).collect();
        if hex.len() == 3 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
            // Längenzeile vor einer mehrteiligen Nachricht
            fertig(teil.take(), &mut out);
            teil = Some((usize::from_str_radix(&hex, 16).ok(), Vec::new()));
            continue;
        }
        fertig(teil.take(), &mut out);
        if let Some(bytes) = parse_hex_line(zeile) {
            if !bytes.is_empty() {
                out.push(bytes);
            }
        }
    }
    fertig(teil, &mut out);
    out
}

/// Eine ELM327-Antwortzeile (Hex, evtl. mit Leerzeichen) in Bytes.
fn parse_hex_line(s: &str) -> Option<Vec<u8>> {
    let clean: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    if clean.len() < 2 || !clean.len().is_multiple_of(2) || !clean.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    (0..clean.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&clean[i..i + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIN: &[u8] = b"WVWZZZ1KZAW000001";

    fn vin_antwort() -> Vec<u8> {
        let mut v = vec![0x49, 0x02, 0x01];
        v.extend_from_slice(VIN);
        v
    }

    #[test]
    fn vin_mehrteilig_ohne_leerzeichen() {
        let r = "014\r0:490201575657\r1:5A5A5A314B5A41\r2:57303030303031\r\r>";
        assert_eq!(parse_response(r), vec![vin_antwort()]);
    }

    #[test]
    fn vin_mehrteilig_mit_leerzeichen_und_zeilenvorschub() {
        let r = "014\r\n0: 49 02 01 57 56 57\r\n1: 5A 5A 5A 31 4B 5A 41\r\n2: 57 30 30 30 30 30 31\r\n\r\n>";
        assert_eq!(parse_response(r), vec![vin_antwort()]);
    }

    #[test]
    fn einzelne_und_mehrere_antworten() {
        assert_eq!(parse_response("4100BE3FA813\r\r>"), vec![vec![0x41, 0x00, 0xBE, 0x3F, 0xA8, 0x13]]);
        // zwei Steuergeräte antworten auf dieselbe Anfrage
        let r = "4100BE3FA813\r410080000001\r\r>";
        assert_eq!(parse_response(r).len(), 2);
    }

    #[test]
    fn statuszeilen_werden_ignoriert() {
        assert!(parse_response("SEARCHING...\rNO DATA\r\r>").is_empty());
        assert_eq!(parse_response("SEARCHING...\r4101000E8000\r\r>"), vec![vec![0x41, 0x01, 0x00, 0x0E, 0x80, 0x00]]);
        assert!(parse_response("?\r\r>").is_empty());
    }
}
