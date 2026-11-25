//! DoIP — Diagnose über Ethernet (ISO 13400-2), für neuere Fahrzeuge
//! (VW-Konzern ab etwa MQB-evo/Golf 8, BMW ab F-Reihe) über ein
//! „OBD-Ethernet“-Kabel oder das Fahrzeug-WLAN einer Werkstatt-Schnittstelle.
//!
//! Ablauf:
//! 1. **Fahrzeug finden** (optional): UDP-Rundruf „Vehicle Identification
//!    Request“ an Port 13400 → das Gateway meldet VIN und logische Adresse.
//! 2. **TCP-Verbindung** zum Gateway (Port 13400), „Routing Activation“ mit
//!    der Tester-Adresse (0x0E00).
//! 3. **Diagnose-Nachrichten** (Typ 0x8001): Quelle = Tester, Ziel =
//!    logische Adresse des Steuergeräts, darin die UDS-Nutzlast. Das Gateway
//!    bestätigt jede Nachricht (0x8002), dann kommt die Antwort (0x8001).
//!
//! Nur die Transportschicht — darüber läuft das gewohnte UDS. In der
//! Oberfläche/CLI: Transport `doip`, Adresse `192.168.0.10` (Port 13400).
//! Die „Anfrage-ID“ eines Steuergeräts ist bei DoIP seine logische Adresse
//! (z. B. 0x4010 statt CAN-ID 0x7E0).

use crate::tr;
use anyhow::{bail, Context, Result};
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs, UdpSocket};
use std::sync::Mutex;
use std::time::Duration;

pub const PORT: u16 = 13400;
/// Logische Adresse des Testers (ISO 13400: 0x0E00–0x0FFF für externe Tester).
pub const TESTER: u16 = 0x0E00;
const VERSION: u8 = 0x02;

/// Nachrichtentypen (Payload-Typ im Kopf).
pub mod typ {
    pub const NEGATIVE_ACK: u16 = 0x0000;
    pub const FAHRZEUG_ANFRAGE: u16 = 0x0001;
    pub const FAHRZEUG_ANTWORT: u16 = 0x0004;
    pub const ROUTING_ANFRAGE: u16 = 0x0005;
    pub const ROUTING_ANTWORT: u16 = 0x0006;
    pub const ALIVE_ANFRAGE: u16 = 0x0007;
    pub const ALIVE_ANTWORT: u16 = 0x0008;
    pub const DIAGNOSE: u16 = 0x8001;
    pub const DIAGNOSE_ACK: u16 = 0x8002;
    pub const DIAGNOSE_NACK: u16 = 0x8003;
}

/// Kopf + Nutzlast bauen.
pub fn nachricht(typ: u16, nutz: &[u8]) -> Vec<u8> {
    let mut v = vec![VERSION, !VERSION];
    v.extend_from_slice(&typ.to_be_bytes());
    v.extend_from_slice(&(nutz.len() as u32).to_be_bytes());
    v.extend_from_slice(nutz);
    v
}

/// Kopf prüfen: (Typ, Länge).
pub fn kopf(k: &[u8; 8]) -> Result<(u16, usize)> {
    if k[1] != !k[0] {
        bail!(tr!("DoIP: ungültiger Kopf (Version {a})", a = format!("{:#04x}", k[0])));
    }
    let typ = u16::from_be_bytes([k[2], k[3]]);
    let len = u32::from_be_bytes([k[4], k[5], k[6], k[7]]) as usize;
    if len > 1 << 20 {
        bail!(tr!("DoIP: Nachricht zu groß ({a} Byte)", a = len));
    }
    Ok((typ, len))
}

/// Ein gefundenes Fahrzeug (Antwort auf den Rundruf).
#[derive(Debug, Clone, PartialEq)]
pub struct Fahrzeug {
    pub adresse: String,
    pub vin: String,
    pub logisch: u16,
}

/// Fahrzeug-Ansage zerlegen (VIN 17, logische Adresse 2, EID 6, GID 6, …).
pub fn fahrzeug_lesen(nutz: &[u8], adresse: &str) -> Option<Fahrzeug> {
    let vin = String::from_utf8_lossy(nutz.get(0..17)?).trim().to_string();
    let logisch = u16::from_be_bytes([*nutz.get(17)?, *nutz.get(18)?]);
    Some(Fahrzeug { adresse: adresse.to_string(), vin, logisch })
}

/// Fahrzeuge im Netz suchen (UDP-Rundruf, wartet `warten`).
pub fn suchen(ziel: &str, warten: Duration) -> Result<Vec<Fahrzeug>> {
    let sock = UdpSocket::bind(("0.0.0.0", 0))?;
    sock.set_broadcast(true)?;
    sock.set_read_timeout(Some(Duration::from_millis(200)))?;
    let ziel = if ziel.contains(':') { ziel.to_string() } else { format!("{ziel}:{PORT}") };
    sock.send_to(&nachricht(typ::FAHRZEUG_ANFRAGE, &[]), &ziel).with_context(|| tr!("DoIP-Rundruf an {a}", a = ziel))?;
    let ende = std::time::Instant::now() + warten;
    let mut out = Vec::new();
    let mut buf = [0u8; 512];
    while std::time::Instant::now() < ende {
        if let Ok((n, von)) = sock.recv_from(&mut buf) {
            if n >= 8 {
                let k: [u8; 8] = buf[..8].try_into().unwrap_or_default();
                if let Ok((typ::FAHRZEUG_ANTWORT, len)) = kopf(&k) {
                    if let Some(f) = fahrzeug_lesen(&buf[8..(8 + len).min(n)], &von.ip().to_string()) {
                        if !out.contains(&f) {
                            out.push(f);
                        }
                    }
                }
            }
        }
    }
    Ok(out)
}

/// Verbindung zu einem Steuergerät über das DoIP-Gateway.
pub struct DoIp {
    strom: Mutex<TcpStream>,
    ziel: u16,
}

impl DoIp {
    /// TCP aufbauen und Routing aktivieren. `adresse` = "IP" oder "IP:Port".
    pub fn verbinden(adresse: &str, ziel: u16) -> Result<Self> {
        let a = if adresse.contains(':') { adresse.to_string() } else { format!("{adresse}:{PORT}") };
        let sa = a.to_socket_addrs().with_context(|| tr!("DoIP-Adresse „{a}“ ungültig", a = a))?.next().context("keine Adresse")?;
        let strom = TcpStream::connect_timeout(&sa, Duration::from_secs(3)).with_context(|| tr!("DoIP-Gateway {a} nicht erreichbar", a = a))?;
        strom.set_read_timeout(Some(Duration::from_secs(2)))?;
        strom.set_nodelay(true)?;
        let d = DoIp { strom: Mutex::new(strom), ziel };
        // Routing Activation: Tester-Adresse, Art 0x00 (Standard), 4 Byte reserviert
        let mut n = TESTER.to_be_bytes().to_vec();
        n.extend_from_slice(&[0x00, 0, 0, 0, 0]);
        d.schreiben(typ::ROUTING_ANFRAGE, &n)?;
        let (t, a) = d.lesen()?;
        if t != typ::ROUTING_ANTWORT {
            bail!(tr!("DoIP: Routing-Antwort erwartet, Typ {a} erhalten", a = format!("{t:#06x}")));
        }
        match a.get(4) {
            Some(0x10) => Ok(d),
            Some(code) => bail!(tr!("DoIP: Routing abgelehnt (Code {a})", a = format!("{code:#04x}"))),
            None => bail!(tr!("DoIP: Routing-Antwort zu kurz")),
        }
    }

    fn schreiben(&self, typ: u16, nutz: &[u8]) -> Result<()> {
        let mut s = self.strom.lock().unwrap_or_else(|e| e.into_inner());
        s.write_all(&nachricht(typ, nutz)).context(tr!("DoIP: Senden fehlgeschlagen"))
    }

    fn lesen(&self) -> Result<(u16, Vec<u8>)> {
        let mut s = self.strom.lock().unwrap_or_else(|e| e.into_inner());
        let mut k = [0u8; 8];
        s.read_exact(&mut k).context(tr!("Keine Antwort (Timeout) vom DoIP-Gateway"))?;
        let (t, len) = kopf(&k)?;
        let mut n = vec![0u8; len];
        s.read_exact(&mut n)?;
        Ok((t, n))
    }
}

impl crate::transport::Transport for DoIp {
    fn send(&self, data: &[u8]) -> Result<()> {
        let mut n = TESTER.to_be_bytes().to_vec();
        n.extend_from_slice(&self.ziel.to_be_bytes());
        n.extend_from_slice(data);
        self.schreiben(typ::DIAGNOSE, &n)?;
        // auf die Bestätigung des Gateways warten (Alive-Checks beantworten)
        loop {
            let (t, a) = self.lesen()?;
            match t {
                typ::DIAGNOSE_ACK => return Ok(()),
                typ::DIAGNOSE_NACK => bail!(tr!("DoIP: Nachricht abgelehnt (Code {a}) — Steuergerät {b} unbekannt?", a = format!("{:#04x}", a.get(4).copied().unwrap_or(0)), b = format!("{:#06x}", self.ziel))),
                typ::ALIVE_ANFRAGE => self.schreiben(typ::ALIVE_ANTWORT, &TESTER.to_be_bytes())?,
                typ::NEGATIVE_ACK => bail!(tr!("DoIP: Gateway lehnt ab (Code {a})", a = format!("{:#04x}", a.first().copied().unwrap_or(0)))),
                _ => {}
            }
        }
    }

    fn recv(&self) -> Result<Vec<u8>> {
        loop {
            let (t, a) = self.lesen()?;
            match t {
                // Antwort vom Steuergerät an uns
                typ::DIAGNOSE if a.len() >= 4 && u16::from_be_bytes([a[0], a[1]]) == self.ziel => return Ok(a[4..].to_vec()),
                typ::ALIVE_ANFRAGE => self.schreiben(typ::ALIVE_ANTWORT, &TESTER.to_be_bytes())?,
                _ => {}
            }
        }
    }
}

/// Simuliertes DoIP-Gateway für Tests: beantwortet Routing und leitet
/// Diagnose-Nachrichten an das Demo-Fahrzeug (UDS über `sim`) weiter.
/// Gibt die Adresse "127.0.0.1:port" zurück; läuft, bis der Test endet.
#[cfg(test)]
pub fn test_gateway() -> String {
    use std::net::TcpListener;
    let l = TcpListener::bind("127.0.0.1:0").expect("bind");
    let adr = l.local_addr().expect("addr").to_string();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { continue };
            std::thread::spawn(move || {
                let mut k = [0u8; 8];
                while s.read_exact(&mut k).is_ok() {
                    let Ok((t, len)) = kopf(&k) else { return };
                    let mut n = vec![0u8; len];
                    if s.read_exact(&mut n).is_err() {
                        return;
                    }
                    match t {
                        typ::ROUTING_ANFRAGE => {
                            let mut a = n[..2].to_vec();
                            a.extend_from_slice(&0x1000u16.to_be_bytes());
                            a.extend_from_slice(&[0x10, 0, 0, 0, 0]);
                            let _ = s.write_all(&nachricht(typ::ROUTING_ANTWORT, &a));
                        }
                        typ::DIAGNOSE => {
                            let (quelle, ziel) = (u16::from_be_bytes([n[0], n[1]]), u16::from_be_bytes([n[2], n[3]]));
                            let mut ack = n[..4].to_vec();
                            if ziel != 0x4010 {
                                ack.push(0x03); // unbekanntes Ziel
                                let _ = s.write_all(&nachricht(typ::DIAGNOSE_NACK, &ack));
                                continue;
                            }
                            ack.push(0x00);
                            let _ = s.write_all(&nachricht(typ::DIAGNOSE_ACK, &ack));
                            // Demo-Motor (CAN 0x7E0) beantwortet die UDS-Anfrage
                            let tp = crate::sim::DemoUds::new(0x7E0);
                            use crate::transport::Transport as _;
                            if tp.send(&n[4..]).is_ok() {
                                if let Ok(r) = tp.recv() {
                                    let mut a = ziel.to_be_bytes().to_vec();
                                    a.extend_from_slice(&quelle.to_be_bytes());
                                    a.extend_from_slice(&r);
                                    let _ = s.write_all(&nachricht(typ::DIAGNOSE, &a));
                                }
                            }
                        }
                        _ => {}
                    }
                }
            });
        }
    });
    adr
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kopf_und_fahrzeug() {
        let n = nachricht(typ::DIAGNOSE, &[1, 2, 3]);
        assert_eq!(&n[..8], &[0x02, 0xFD, 0x80, 0x01, 0, 0, 0, 3]);
        assert_eq!(kopf(&n[..8].try_into().unwrap()).unwrap(), (0x8001, 3));
        assert!(kopf(&[0x02, 0x02, 0, 0, 0, 0, 0, 0]).is_err(), "Inverse passt nicht");
        let mut ansage = b"WVWZZZ1KZAM000017".to_vec();
        ansage.extend_from_slice(&[0x10, 0x00, 1, 2, 3, 4, 5, 6, 0, 0, 0, 0, 0, 0, 0]);
        let f = fahrzeug_lesen(&ansage, "192.168.0.10").unwrap();
        assert_eq!((f.vin.as_str(), f.logisch), ("WVWZZZ1KZAM000017", 0x1000));
    }

    #[test]
    fn uds_ueber_simuliertes_gateway() {
        let _g = crate::sim::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::sim::set_modell(crate::sim::Modell::Demo);
        let adr = test_gateway();
        let d = DoIp::verbinden(&adr, 0x4010).unwrap();
        let uds = crate::uds::Uds::new(&d);
        let vin = uds.read_data_by_id(0xF190).unwrap();
        assert_eq!(vin.len(), 17, "{vin:?}");
        // unbekanntes Steuergerät: Gateway lehnt ab
        let falsch = DoIp::verbinden(&adr, 0x4099).unwrap();
        let e = crate::uds::Uds::new(&falsch).read_data_by_id(0xF190).unwrap_err().to_string();
        assert!(e.contains("abgelehnt"), "{e}");
    }
}
