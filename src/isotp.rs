//! ISO 15765-2 (ISO-TP) transport over raw CAN frames.
//!
//! Implemented in user space on top of SocketCAN so it does not depend on the
//! `can-isotp` kernel module being present. Supports classic 8-byte CAN frames
//! with 11-bit identifiers (VAG) and 29-bit identifiers (ISO 15765-4, e.g.
//! 0x18DA10F1) — an ID above 0x7FF is sent as an extended frame.

use anyhow::{anyhow, bail, Context, Result};
use crate::can::{CanFrame, CanSocket, EmbeddedFrame, ExtendedId, Id, Socket, StandardId};
use std::time::Duration;

/// Padding byte for frames shorter than 8 data bytes. VAG expects full 8-byte
/// frames; the exact pad value is not significant for the ECUs but must be
/// present. Change here if a controller is observed to be picky.
const PAD: u8 = 0xAA;

pub struct IsoTp {
    sock: CanSocket,
    tx_id: u32,
    rx_id: u32,
    _belegt: crate::bus::Belegt,
}

impl IsoTp {
    pub fn open(iface: &str, tx_id: u32, rx_id: u32) -> Result<Self> {
        let sock = CanSocket::open(iface)
            .with_context(|| crate::tr!("CAN-Interface '{iface}' konnte nicht geöffnet werden", iface = iface))?;
        crate::connect::fix_hw_loopback(iface, &sock);
        sock.set_read_timeout(Duration::from_millis(1000))?;
        sock.set_write_timeout(Duration::from_millis(1000))?;
        Ok(Self { sock, tx_id, rx_id, _belegt: crate::bus::Belegt::neu() })
    }

    /// (ID, 29 Bit?) eines empfangenen Frames.
    fn raw_id(frame: &CanFrame) -> (u32, bool) {
        match frame.id() {
            Id::Standard(s) => (u32::from(s.as_raw()), false),
            Id::Extended(e) => (e.as_raw(), true),
        }
    }

    fn write(&self, id: u32, payload: &[u8]) -> Result<()> {
        let mut data = [PAD; 8];
        data[..payload.len()].copy_from_slice(payload);
        let ungueltig = || anyhow!(crate::tr!("ungültige CAN-ID {id}", id = format!("{id:#x}")));
        let frame = if id > 0x7FF {
            CanFrame::new(ExtendedId::new(id).ok_or_else(ungueltig)?, &data)
        } else {
            CanFrame::new(StandardId::new(id as u16).ok_or_else(ungueltig)?, &data)
        };
        let frame = frame.ok_or_else(|| anyhow!(crate::trs!("Frame-Bau fehlgeschlagen")))?;
        self.sock.write_frame(&frame).context(crate::trs!("CAN-Senden fehlgeschlagen"))?;
        Ok(())
    }

    /// Read the next CAN frame addressed to our rx_id, ignoring unrelated traffic.
    fn read_rx(&self) -> Result<[u8; 8]> {
        loop {
            let frame = self
                .sock
                .read_frame()
                .context(crate::trs!("Keine Antwort (Timeout) vom Steuergerät"))?;
            if Self::raw_id(&frame) == (self.rx_id, self.rx_id > 0x7FF) {
                let mut buf = [0u8; 8];
                let d = frame.data();
                buf[..d.len()].copy_from_slice(d);
                return Ok(buf);
            }
        }
    }

    /// Send a complete service payload (SF or FF+CF with flow control).
    pub fn send(&self, data: &[u8]) -> Result<()> {
        if data.is_empty() {
            bail!("leere ISO-TP-Nutzlast");
        }
        if data.len() <= 7 {
            // Single Frame
            let mut sf = Vec::with_capacity(8);
            sf.push(data.len() as u8); // PCI: 0x0L
            sf.extend_from_slice(data);
            self.write(self.tx_id, &sf)?;
            return Ok(());
        }

        // First Frame
        let len = data.len();
        if len > 0xFFF {
            bail!(crate::tr!("Nutzlast {len} B überschreitet ISO-TP-Klassik-Limit (4095 B)", len = len));
        }
        let mut ff = Vec::with_capacity(8);
        ff.push(0x10 | ((len >> 8) & 0x0F) as u8);
        ff.push((len & 0xFF) as u8);
        ff.extend_from_slice(&data[..6]);
        self.write(self.tx_id, &ff)?;

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
        let mut offset = 6usize;
        let mut sent_in_block = 0u8;
        while offset < len {
            let end = (offset + 7).min(len);
            let mut cf = Vec::with_capacity(8);
            cf.push(0x20 | (sn & 0x0F));
            cf.extend_from_slice(&data[offset..end]);
            self.write(self.tx_id, &cf)?;
            offset = end;
            sn = sn.wrapping_add(1) & 0x0F;
            if sn == 0 {
                sn = 0; // SN wraps 0..15, but after 15 goes to 0 per spec
            }
            sent_in_block += 1;
            if block_size != 0 && sent_in_block == block_size && offset < len {
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
        match first[0] & 0xF0 {
            0x00 => {
                // Single Frame
                let len = (first[0] & 0x0F) as usize;
                Ok(first[1..1 + len].to_vec())
            }
            0x10 => {
                // First Frame
                let len = (((first[0] & 0x0F) as usize) << 8) | first[1] as usize;
                let mut out = Vec::with_capacity(len);
                out.extend_from_slice(&first[2..8]);
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
                    let take = (len - out.len()).min(7);
                    out.extend_from_slice(&cf[1..1 + take]);
                    expect_sn = (expect_sn + 1) & 0x0F;
                }
                Ok(out)
            }
            other => bail!("unerwarteter ISO-TP PCI-Typ {other:#04x}"),
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
