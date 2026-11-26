//! ISO 15765-2 (ISO-TP) transport over raw CAN frames.
//!
//! Implemented in user space on top of SocketCAN so it does not depend on the
//! `can-isotp` kernel module being present. Supports classic 8-byte CAN frames
//! with 11-bit identifiers (VAG) and 29-bit identifiers (ISO 15765-4, e.g.
//! 0x18DA10F1) — an ID above 0x7FF is sent as an extended frame.
//!
//! **CAN FD** (ISO 15765-2:2016): Frames bis 64 Byte, Einzel-Frame mit
//! Längen-Escape (`00 LL`), Erst-Frame mit 32-Bit-Länge (`10 00 LLLLLLLL`)
//! für Nutzlasten über 4095 Byte. Eingeschaltet nur ausdrücklich
//! (`VAG_CANFD=1`) und nur, wenn die Schnittstelle CAN FD kann (MTU 72) —
//! ältere Autos wie der Golf 6 sprechen kein FD. Empfangen werden immer beide
//! Arten.
//!
//! **Erweiterte Adressierung** (ISO 15765-2, „extended addressing“): Das erste
//! Datenbyte jedes Frames ist die Zieladresse, für ISO-TP bleiben 7 Byte.
//! So spricht z. B. BMW/Mini: Tester sendet auf 0x6F1 mit der Adresse des
//! Steuergeräts im ersten Byte, das Steuergerät antwortet auf 0x600 + Adresse
//! mit 0xF1 (Tester) im ersten Byte. Im Programm wird das in der Kennung
//! mitgeführt: Bit 31 gesetzt, Adressbyte in Bit 16–23 (siehe [`mit_adresse`]).

use anyhow::{anyhow, bail, Context, Result};
use crate::can::{CanFrame, CanSocket, EmbeddedFrame, ExtendedId, Id, Socket, StandardId};
use std::time::Duration;

/// Padding byte for frames shorter than 8 data bytes. VAG expects full 8-byte
/// frames; the exact pad value is not significant for the ECUs but must be
/// present. Change here if a controller is observed to be picky.
const PAD: u8 = 0xAA;

/// Merker in einer Kennung: erweiterte Adressierung (Adressbyte in Bit 16–23).
pub const ERW_ADR: u32 = 0x8000_0000;

/// CAN-ID (11 Bit) mit Adressbyte im ersten Datenbyte.
pub fn mit_adresse(can_id: u16, adresse: u8) -> u32 {
    ERW_ADR | (u32::from(adresse) << 16) | u32::from(can_id & 0x7FF)
}

/// Kennung zerlegen: (CAN-ID, Adressbyte bei erweiterter Adressierung).
pub fn zerlegen(id: u32) -> (u32, Option<u8>) {
    if id & ERW_ADR != 0 {
        (id & 0x7FF, Some((id >> 16) as u8))
    } else {
        (id, None)
    }
}

/// BMW/Mini: (Anfrage, Antwort) für das Steuergerät mit Adresse `ziel`
/// (Tester 0x6F1 → Ziel; Antwort 0x600 + Ziel → Tester 0xF1).
pub fn bmw_paar(ziel: u8) -> (u32, u32) {
    (mit_adresse(0x6F1, ziel), mit_adresse(0x600 + u16::from(ziel), 0xF1))
}

/// Gültige CAN-FD-Nutzlängen (DLC 9–15 = 12 … 64 Byte).
const FD_LAENGEN: [usize; 8] = [8, 12, 16, 20, 24, 32, 48, 64];

/// Auf die nächste gültige Frame-Länge auffüllen (klassisch immer 8).
fn auffuellen(mut f: Vec<u8>, dl: usize) -> Vec<u8> {
    let ziel = if dl <= 8 { dl } else { FD_LAENGEN.iter().copied().find(|l| *l >= f.len()).unwrap_or(64) };
    f.resize(ziel, PAD);
    f
}

/// Erster Frame einer Nachricht: (Frame, verbrauchte Nutzbytes, Einzel-Frame?).
/// `dl` = Frame-Größe (8 klassisch, 7 mit Adressbyte, 64 bei CAN FD).
pub fn erster_frame(data: &[u8], dl: usize) -> Result<(Vec<u8>, usize, bool)> {
    let n = data.len();
    if n == 0 {
        bail!("leere ISO-TP-Nutzlast");
    }
    if n < dl.min(8) {
        let mut f = vec![n as u8];
        f.extend_from_slice(data);
        return Ok((auffuellen(f, dl), n, true));
    }
    if dl > 8 && n <= dl - 2 {
        // FD-Einzel-Frame mit Längen-Escape
        let mut f = vec![0x00, n as u8];
        f.extend_from_slice(data);
        return Ok((auffuellen(f, dl), n, true));
    }
    if n <= 0xFFF {
        let mut f = vec![0x10 | ((n >> 8) & 0x0F) as u8, (n & 0xFF) as u8];
        let k = dl - 2;
        f.extend_from_slice(&data[..k]);
        return Ok((f, k, false));
    }
    if dl <= 8 {
        bail!(crate::tr!("Nutzlast {len} B überschreitet ISO-TP-Klassik-Limit (4095 B)", len = n));
    }
    // Erst-Frame mit 32-Bit-Länge (nur sinnvoll mit FD)
    let mut f = vec![0x10, 0x00];
    f.extend_from_slice(&(n as u32).to_be_bytes());
    let k = dl - 6;
    f.extend_from_slice(&data[..k]);
    Ok((f, k, false))
}

/// Folge-Frame ab `offset` mit Sequenznummer `sn`: (Frame, neuer Offset).
pub fn folge_frame(data: &[u8], offset: usize, sn: u8, dl: usize) -> (Vec<u8>, usize) {
    let ende = (offset + dl - 1).min(data.len());
    let mut f = vec![0x20 | (sn & 0x0F)];
    f.extend_from_slice(&data[offset..ende]);
    // letzter Folge-Frame bei FD: nur bis zur nächsten gültigen Länge auffüllen
    (auffuellen(f, dl), ende)
}

/// Was der erste empfangene Frame sagt.
#[derive(Debug, PartialEq)]
pub enum Anfang {
    Einzel(Vec<u8>),
    /// Gesamtlänge, schon enthaltene Daten
    Erster(usize, Vec<u8>),
}

pub fn anfang_lesen(f: &[u8]) -> Result<Anfang> {
    let b = |i: usize| f.get(i).copied().ok_or_else(|| anyhow!("ISO-TP-Frame zu kurz"));
    match b(0)? & 0xF0 {
        0x00 => {
            let l = usize::from(b(0)? & 0x0F);
            if l == 0 && f.len() > 8 {
                // FD-Einzel-Frame mit Escape
                let l = usize::from(b(1)?);
                Ok(Anfang::Einzel(f.get(2..2 + l).ok_or_else(|| anyhow!("ISO-TP-Einzel-Frame zu kurz"))?.to_vec()))
            } else {
                Ok(Anfang::Einzel(f.get(1..1 + l).ok_or_else(|| anyhow!("ISO-TP-Einzel-Frame zu kurz"))?.to_vec()))
            }
        }
        0x10 => {
            let l12 = (usize::from(b(0)? & 0x0F) << 8) | usize::from(b(1)?);
            if l12 == 0 {
                let l = u32::from_be_bytes([b(2)?, b(3)?, b(4)?, b(5)?]) as usize;
                Ok(Anfang::Erster(l, f[6..].to_vec()))
            } else {
                Ok(Anfang::Erster(l12, f[2..].to_vec()))
            }
        }
        other => bail!("unerwarteter ISO-TP PCI-Typ {other:#04x}"),
    }
}

enum Sock {
    Klassisch(CanSocket),
    #[cfg(target_os = "linux")]
    Fd(socketcan::CanFdSocket),
}

pub struct IsoTp {
    sock: Sock,
    tx_id: u32,
    rx_id: u32,
    /// erweiterte Adressierung: Adressbyte beim Senden / erwartet beim Empfang
    tx_adr: Option<u8>,
    rx_adr: Option<u8>,
    /// Frame-Größe beim Senden: 8 (klassisch) oder 64 (CAN FD)
    dl: usize,
    _belegt: crate::bus::Belegt,
}

/// CAN FD gewünscht (`VAG_CANFD=1`) und kann die Schnittstelle es (MTU 72)?
pub fn fd_aktiv(iface: &str) -> bool {
    let gewuenscht = std::env::var("VAG_CANFD").is_ok_and(|v| v == "1");
    gewuenscht && std::fs::read_to_string(format!("/sys/class/net/{iface}/mtu")).is_ok_and(|m| m.trim() == "72")
}

impl IsoTp {
    /// Kennungen über 0x7FF = 29 Bit; mit [`ERW_ADR`] = erweiterte Adressierung.
    pub fn open(iface: &str, tx: u32, rx: u32) -> Result<Self> {
        let fehler = || crate::tr!("CAN-Interface '{iface}' konnte nicht geöffnet werden", iface = iface);
        let ((tx_id, tx_adr), (rx_id, rx_adr)) = (zerlegen(tx), zerlegen(rx));
        #[cfg(target_os = "linux")]
        if fd_aktiv(iface) && tx_adr.is_none() {
            let sock = socketcan::CanFdSocket::open(iface).with_context(fehler)?;
            sock.set_read_timeout(Duration::from_millis(1000))?;
            sock.set_write_timeout(Duration::from_millis(1000))?;
            return Ok(Self { sock: Sock::Fd(sock), tx_id, rx_id, tx_adr, rx_adr, dl: 64, _belegt: crate::bus::Belegt::neu() });
        }
        let sock = CanSocket::open(iface).with_context(fehler)?;
        crate::connect::fix_hw_loopback(iface, &sock);
        sock.set_read_timeout(Duration::from_millis(1000))?;
        sock.set_write_timeout(Duration::from_millis(1000))?;
        let dl = if tx_adr.is_some() { 7 } else { 8 };
        Ok(Self { sock: Sock::Klassisch(sock), tx_id, rx_id, tx_adr, rx_adr, dl, _belegt: crate::bus::Belegt::neu() })
    }

    /// Sendet über CAN FD?
    pub fn ist_fd(&self) -> bool {
        self.dl > 8
    }

    /// (ID, 29 Bit?) eines empfangenen Frames.
    fn raw_id(id: Id) -> (u32, bool) {
        match id {
            Id::Standard(s) => (u32::from(s.as_raw()), false),
            Id::Extended(e) => (e.as_raw(), true),
        }
    }

    fn id(id: u32) -> Result<Id> {
        let ungueltig = || anyhow!(crate::tr!("ungültige CAN-ID {id}", id = format!("{id:#x}")));
        Ok(if id > 0x7FF { Id::Extended(ExtendedId::new(id).ok_or_else(ungueltig)?) } else { Id::Standard(StandardId::new(id as u16).ok_or_else(ungueltig)?) })
    }

    fn write(&self, id: u32, payload: &[u8]) -> Result<()> {
        let mut data = auffuellen(payload.to_vec(), self.dl);
        if let Some(a) = self.tx_adr {
            data.insert(0, a);
        }
        match &self.sock {
            Sock::Klassisch(s) => {
                let frame = CanFrame::new(Self::id(id)?, &data).ok_or_else(|| anyhow!(crate::trs!("Frame-Bau fehlgeschlagen")))?;
                s.write_frame(&frame).context(crate::trs!("CAN-Senden fehlgeschlagen"))?;
            }
            #[cfg(target_os = "linux")]
            Sock::Fd(s) => {
                let frame = socketcan::CanFdFrame::new(Self::id(id)?, &data).ok_or_else(|| anyhow!(crate::trs!("Frame-Bau fehlgeschlagen")))?;
                s.write_frame(&frame).context(crate::trs!("CAN-Senden fehlgeschlagen"))?;
            }
        }
        Ok(())
    }

    /// Read the next CAN frame addressed to our rx_id, ignoring unrelated traffic.
    fn read_rx(&self) -> Result<Vec<u8>> {
        let gesucht = (self.rx_id, self.rx_id > 0x7FF);
        loop {
            match &self.sock {
                Sock::Klassisch(s) => {
                    let frame = s.read_frame().context(crate::trs!("Keine Antwort (Timeout) vom Steuergerät"))?;
                    if Self::raw_id(frame.id()) == gesucht {
                        let mut d = frame.data().to_vec();
                        match self.rx_adr {
                            // erweiterte Adressierung: nur Frames an uns, Adressbyte weg
                            Some(a) if d.first() != Some(&a) => continue,
                            Some(_) => {
                                d.remove(0);
                                d.resize(7, 0);
                            }
                            None => d.resize(8, 0),
                        }
                        return Ok(d);
                    }
                }
                #[cfg(target_os = "linux")]
                Sock::Fd(s) => {
                    use socketcan::CanAnyFrame;
                    let frame = s.read_frame().context(crate::trs!("Keine Antwort (Timeout) vom Steuergerät"))?;
                    let (id, d) = match &frame {
                        CanAnyFrame::Normal(f) => (f.id(), f.data().to_vec()),
                        CanAnyFrame::Fd(f) => (f.id(), f.data().to_vec()),
                        _ => continue,
                    };
                    if Self::raw_id(id) == gesucht {
                        let mut d = d;
                        if d.len() < 8 {
                            d.resize(8, 0);
                        }
                        return Ok(d);
                    }
                }
            }
        }
    }

    /// Send a complete service payload (SF or FF+CF with flow control).
    pub fn send(&self, data: &[u8]) -> Result<()> {
        let (erster, mut offset, einzel) = erster_frame(data, self.dl)?;
        self.write(self.tx_id, &erster)?;
        if einzel {
            return Ok(());
        }
        // Wait for Flow Control from the ECU
        let fc = self.read_rx()?;
        if fc[0] & 0xF0 != 0x30 {
            bail!("Flow Control erwartet, {:#04x} erhalten", fc[0]);
        }
        if fc[0] & 0x0F == 0x01 {
            bail!(crate::trs!("Flow Control WAIT (noch nicht implementiert)"));
        }
        let block_size = fc[1];
        let stmin = decode_stmin(fc[2]);

        // Consecutive Frames
        let mut sn = 1u8;
        let mut sent_in_block = 0u8;
        while offset < data.len() {
            let (cf, neu) = folge_frame(data, offset, sn, self.dl);
            self.write(self.tx_id, &cf)?;
            offset = neu;
            sn = sn.wrapping_add(1) & 0x0F;
            sent_in_block += 1;
            if block_size != 0 && sent_in_block == block_size && offset < data.len() {
                let fc = self.read_rx()?;
                if fc[0] & 0xF0 != 0x30 {
                    bail!("Flow Control erwartet, {:#04x} erhalten", fc[0]);
                }
                sent_in_block = 0;
            }
            std::thread::sleep(stmin);
        }
        Ok(())
    }

    /// Receive a complete service payload (SF or FF+CF), sending Flow Control.
    pub fn recv(&self) -> Result<Vec<u8>> {
        let first = self.read_rx()?;
        match anfang_lesen(&first)? {
            Anfang::Einzel(d) => Ok(d),
            Anfang::Erster(len, schon) => {
                let mut out = Vec::with_capacity(len);
                out.extend_from_slice(&schon[..schon.len().min(len)]);
                // Send Flow Control: CTS, BS=0 (all at once), STmin=0
                self.write(self.tx_id, &[0x30, 0x00, 0x00])?;
                let mut expect_sn = 1u8;
                while out.len() < len {
                    let cf = self.read_rx()?;
                    if cf[0] & 0xF0 != 0x20 {
                        bail!("Consecutive Frame erwartet, {:#04x} erhalten", cf[0]);
                    }
                    let sn = cf[0] & 0x0F;
                    if sn != expect_sn {
                        bail!("Sequenzfehler: {sn} erwartet {expect_sn}");
                    }
                    let take = (len - out.len()).min(cf.len() - 1);
                    out.extend_from_slice(&cf[1..1 + take]);
                    expect_sn = (expect_sn + 1) & 0x0F;
                }
                Ok(out)
            }
        }
    }
}

/// STmin encoding per ISO 15765-2.
fn decode_stmin(b: u8) -> Duration {
    match b {
        0x00..=0x7F => Duration::from_millis(b as u64),
        0xF1..=0xF9 => Duration::from_micros(((b - 0xF0) as u64) * 100),
        _ => Duration::from_millis(1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Nachricht in Frames zerlegen und wieder zusammensetzen (ohne Bus).
    fn hin_und_zurueck(data: &[u8], dl: usize) -> (Vec<Vec<u8>>, Vec<u8>) {
        let (erster, mut off, einzel) = erster_frame(data, dl).unwrap();
        let mut frames = vec![erster];
        let mut sn = 1u8;
        while !einzel && off < data.len() {
            let (f, n) = folge_frame(data, off, sn, dl);
            frames.push(f);
            off = n;
            sn = (sn + 1) & 0x0F;
        }
        let zurueck = match anfang_lesen(&frames[0]).unwrap() {
            Anfang::Einzel(d) => d,
            Anfang::Erster(len, schon) => {
                let mut out = schon[..schon.len().min(len)].to_vec();
                for f in &frames[1..] {
                    let take = (len - out.len()).min(f.len() - 1);
                    out.extend_from_slice(&f[1..1 + take]);
                }
                out
            }
        };
        (frames, zurueck)
    }

    #[test]
    fn klassisch_und_fd_rund() {
        for dl in [8usize, 64] {
            for n in [1usize, 7, 8, 20, 62, 63, 64, 100, 300, 4095] {
                let data: Vec<u8> = (0..n).map(|i| (i * 7 + 3) as u8).collect();
                let (frames, zurueck) = hin_und_zurueck(&data, dl);
                assert_eq!(zurueck, data, "dl {dl}, {n} Byte");
                for f in &frames {
                    if dl == 8 {
                        assert_eq!(f.len(), 8);
                    } else {
                        assert!(FD_LAENGEN.contains(&f.len()), "ungültige FD-Länge {}", f.len());
                    }
                }
            }
        }
        // klassisch: 20 Byte = Erst-Frame (6) + 2 Folge-Frames
        assert_eq!(hin_und_zurueck(&[1; 20], 8).0.len(), 3);
        // FD: 20 Byte passen in einen Einzel-Frame mit Escape (00 14 …), auf 24 aufgefüllt
        let (f, _) = hin_und_zurueck(&[1; 20], 64);
        assert_eq!((f.len(), f[0][0], f[0][1], f[0].len()), (1, 0x00, 20, 24));
        // bis 7 Byte bleibt es auch bei FD der klassische Einzel-Frame
        assert_eq!(hin_und_zurueck(&[9; 5], 64).0[0][0], 0x05);
    }

    #[test]
    fn grosse_nachricht_nur_mit_fd() {
        let data = vec![0x55; 5000];
        assert!(erster_frame(&data, 8).is_err(), "klassisch max. 4095");
        let (frames, zurueck) = hin_und_zurueck(&data, 64);
        assert_eq!(zurueck, data);
        assert_eq!(&frames[0][..6], &[0x10, 0x00, 0x00, 0x00, 0x13, 0x88], "32-Bit-Länge");
    }

    #[test]
    fn erweiterte_adressierung() {
        // 7 Byte je Frame: Einzel-Frame bis 6 Byte, Erst-Frame trägt 5, Folge-Frames 6
        for n in [1usize, 6, 7, 11, 12, 13, 100] {
            let data: Vec<u8> = (0..n).map(|i| i as u8).collect();
            let (frames, zurueck) = hin_und_zurueck(&data, 7);
            assert_eq!(zurueck, data, "{n} Byte");
            assert!(frames.iter().all(|f| f.len() == 7));
        }
        assert_eq!(hin_und_zurueck(&[1; 6], 7).0.len(), 1);
        assert_eq!(hin_und_zurueck(&[1; 7], 7).0.len(), 2, "7 Byte brauchen Erst- + Folge-Frame");
        assert_eq!(hin_und_zurueck(&[1; 11], 7).0.len(), 2);
        assert_eq!(hin_und_zurueck(&[1; 12], 7).0.len(), 3);
        // BMW: DME 0x12
        let (tx, rx) = bmw_paar(0x12);
        assert_eq!(zerlegen(tx), (0x6F1, Some(0x12)));
        assert_eq!(zerlegen(rx), (0x612, Some(0xF1)));
        assert_eq!(zerlegen(0x7E0), (0x7E0, None));
        assert_eq!(zerlegen(0x18DA_10F1), (0x18DA_10F1, None));
    }

    #[test]
    fn fd_nur_auf_wunsch() {
        std::env::remove_var("VAG_CANFD");
        assert!(!fd_aktiv("lo"), "ohne VAG_CANFD nie");
    }
}
