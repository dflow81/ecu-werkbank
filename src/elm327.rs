//! ELM327-Backend: UDS über einen ELM327-kompatiblen OBD-Adapter.
//!
//! Erreichbar über Seriell (USB `/dev/ttyUSB0`, Bluetooth `/dev/rfcomm0`) oder
//! TCP (WiFi-Adapter). Der ELM327 übernimmt ISO-TP selbst (`ATCAF1`), wir
//! schicken also nur die UDS-Nutzlast als Hex und parsen die Hex-Antwort.
//!
//! ## Honesty-Hinweis — HARDWARE-VERIFIKATION NÖTIG
//!
//! Diese Implementierung folgt dem dokumentierten ELM327-AT-Befehlssatz, konnte
//! aber hier ohne echten Adapter nicht getestet werden. Timings, Antwort-Parsing
//! und Protokollwahl (`ATSP6` = ISO 15765-4 CAN 11bit/500k) sind gegen echte
//! Hardware zu prüfen, bevor man sich darauf verlässt. SocketCAN bleibt der
//! verifizierte, empfohlene Weg.

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
        Ok(())
    }
}

impl Transport for Elm327 {
    fn send(&self, data: &[u8]) -> Result<()> {
        let hex: String = data.iter().map(|b| format!("{b:02X}")).collect();
        let resp = self.cmd(&hex)?;
        let mut q = self.queue.borrow_mut();
        for line in resp.lines() {
            let line = line.trim().replace('>', "");
            if line.is_empty() {
                continue;
            }
            let up = line.to_uppercase();
            if up.contains("NO DATA")
                || up.contains("CAN ERROR")
                || up.contains("UNABLE")
                || up.contains("BUFFER")
                || up.contains("SEARCHING")
                || up.contains("STOPPED")
                || up == "?"
                || up == "OK"
            {
                continue;
            }
            if let Some(bytes) = parse_hex_line(&line) {
                if !bytes.is_empty() {
                    q.push_back(bytes);
                }
            }
        }
        Ok(())
    }

    fn recv(&self) -> Result<Vec<u8>> {
        self.queue
            .borrow_mut()
            .pop_front()
            .ok_or_else(|| anyhow::anyhow!(crate::trs!("keine ELM327-Antwort (NO DATA / Timeout?)")))
    }
}

/// Eine ELM327-Antwortzeile (Hex, evtl. mit Leerzeichen/Längenpräfix) in Bytes.
fn parse_hex_line(s: &str) -> Option<Vec<u8>> {
    let clean: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if clean.len() < 2 || !clean.len().is_multiple_of(2) {
        return None;
    }
    (0..clean.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&clean[i..i + 2], 16).ok())
        .collect()
}
