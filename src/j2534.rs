//! J2534 „Pass-Thru“-Adapter unter Windows (SAE J2534-1, Version 04.04):
//! Werkstatt-Schnittstellen wie Tactrix OpenPort, Drew Mongoose, Bosch/VAS
//! mit Hersteller-DLL. ECUWerkbank lädt die DLL des Adapters und nutzt das
//! Protokoll ISO 15765 (CAN mit ISO-TP im Adapter) — darüber läuft UDS bzw.
//! OBD wie gewohnt.
//!
//! - Adapter finden: die Treiber tragen sich in der Registry ein
//!   (`HKLM\SOFTWARE\PassThruSupport.04.04\<Name>`, Wert `FunctionLibrary`).
//! - Transport `j2534`, `--port` = Pfad zur DLL oder Name aus der Liste.
//! - **Wichtig:** ECUWerkbank ist ein 64-Bit-Programm und kann nur 64-Bit-DLLs
//!   laden. Viele ältere Treiber gibt es nur als 32 Bit.
//!
//! **Nicht an einem echten Adapter geprüft** (kein Windows-Adapter vorhanden).

use crate::tr;
use anyhow::Result;

/// Ein in der Registry eingetragener Adapter: (Name, DLL-Pfad).
pub fn adapter() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for schluessel in ["HKLM\\SOFTWARE\\PassThruSupport.04.04", "HKLM\\SOFTWARE\\WOW6432Node\\PassThruSupport.04.04"] {
        let Ok(o) = std::process::Command::new("reg").args(["query", schluessel, "/s", "/v", "FunctionLibrary"]).output() else { continue };
        let text = String::from_utf8_lossy(&o.stdout).to_string();
        let mut name = String::new();
        for z in text.lines() {
            if z.starts_with("HKEY_") {
                name = z.rsplit('\\').next().unwrap_or("").to_string();
            } else if let Some(p) = z.trim().strip_prefix("FunctionLibrary") {
                let pfad = p.trim().trim_start_matches("REG_SZ").trim().to_string();
                let wow = if schluessel.contains("WOW6432") { tr!(" (32 Bit — nicht nutzbar)") } else { String::new() };
                out.push((format!("{name}{wow}"), pfad));
            }
        }
    }
    out
}

/// DLL-Pfad zu `--port`: ein Pfad oder ein Name aus [`adapter`].
pub fn dll_fuer(port: &str) -> Option<String> {
    if port.to_ascii_lowercase().ends_with(".dll") {
        return Some(port.to_string());
    }
    adapter().into_iter().find(|(n, _)| n.to_ascii_lowercase().contains(&port.to_ascii_lowercase())).map(|(_, p)| p)
}

#[cfg(windows)]
mod echt {
    use super::*;
    use anyhow::{bail, Context};
    use std::sync::{Mutex, OnceLock};

    const ISO15765: u32 = 6;
    const CAN_29BIT_ID: u32 = 0x100;
    const ISO15765_FRAME_PAD: u32 = 0x40;
    const FLOW_CONTROL_FILTER: u32 = 3;
    const CLEAR_RX_BUFFER: u32 = 0x08;
    const TX_MSG_TYPE: u32 = 0x01;
    const START_OF_MESSAGE: u32 = 0x02;
    const ERR_BUFFER_EMPTY: i32 = 0x10;

    #[repr(C)]
    pub struct Msg {
        protocol: u32,
        rx_status: u32,
        tx_flags: u32,
        timestamp: u32,
        data_size: u32,
        extra_data_index: u32,
        data: [u8; 4128],
    }

    impl Msg {
        fn neu(protocol: u32, tx_flags: u32, data: &[u8]) -> Box<Self> {
            let mut m = Box::new(Msg { protocol, rx_status: 0, tx_flags, timestamp: 0, data_size: data.len() as u32, extra_data_index: 0, data: [0; 4128] });
            m.data[..data.len()].copy_from_slice(data);
            m
        }
    }

    type Open = unsafe extern "system" fn(*const std::ffi::c_void, *mut u32) -> i32;
    type Close = unsafe extern "system" fn(u32) -> i32;
    type Connect = unsafe extern "system" fn(u32, u32, u32, u32, *mut u32) -> i32;
    type Disconnect = unsafe extern "system" fn(u32) -> i32;
    type ReadMsgs = unsafe extern "system" fn(u32, *mut Msg, *mut u32, u32) -> i32;
    type WriteMsgs = unsafe extern "system" fn(u32, *const Msg, *mut u32, u32) -> i32;
    type Filter = unsafe extern "system" fn(u32, u32, *const Msg, *const Msg, *const Msg, *mut u32) -> i32;
    type Ioctl = unsafe extern "system" fn(u32, u32, *const std::ffi::c_void, *mut std::ffi::c_void) -> i32;
    type LastError = unsafe extern "system" fn(*mut u8) -> i32;

    struct Geraet {
        _lib: libloading::Library,
        dll: String,
        id: u32,
        kanal: Option<u32>,
        connect: Connect,
        disconnect: Disconnect,
        read: ReadMsgs,
        write: WriteMsgs,
        filter: Filter,
        ioctl: Ioctl,
        last_error: LastError,
        _close: Close,
    }

    // Ein Adapter kann je Programm nur einmal geöffnet werden.
    static GERAET: OnceLock<Mutex<Option<Geraet>>> = OnceLock::new();

    fn fehler(g: &Geraet, code: i32, was: &str) -> anyhow::Error {
        let mut buf = [0u8; 80];
        unsafe { (g.last_error)(buf.as_mut_ptr()) };
        let t = String::from_utf8_lossy(&buf).trim_end_matches('\0').to_string();
        anyhow::anyhow!(tr!("J2534 {a}: Fehler {b} {c}", a = was, b = format!("{code:#04x}"), c = t))
    }

    fn laden(dll: &str) -> Result<Geraet> {
        unsafe {
            let lib = libloading::Library::new(dll).with_context(|| tr!("J2534-DLL „{a}“ laden (64-Bit-Treiber nötig)", a = dll))?;
            macro_rules! f {
                ($n:literal, $t:ty) => {
                    *lib.get::<$t>($n).with_context(|| tr!("J2534-DLL ohne {a}", a = String::from_utf8_lossy($n)))?
                };
            }
            let open: Open = f!(b"PassThruOpen", Open);
            let g = Geraet {
                connect: f!(b"PassThruConnect", Connect),
                disconnect: f!(b"PassThruDisconnect", Disconnect),
                read: f!(b"PassThruReadMsgs", ReadMsgs),
                write: f!(b"PassThruWriteMsgs", WriteMsgs),
                filter: f!(b"PassThruStartMsgFilter", Filter),
                ioctl: f!(b"PassThruIoctl", Ioctl),
                last_error: f!(b"PassThruGetLastError", LastError),
                _close: f!(b"PassThruClose", Close),
                id: 0,
                kanal: None,
                dll: dll.to_string(),
                _lib: lib,
            };
            let mut id = 0u32;
            let r = open(std::ptr::null(), &mut id);
            if r != 0 {
                return Err(fehler(&g, r, "PassThruOpen"));
            }
            Ok(Geraet { id, ..g })
        }
    }

    /// Ein ISO-15765-Kanal zu einem Steuergerät (tx/rx).
    pub struct J2534 {
        tx: u32,
        flags: u32,
    }

    impl J2534 {
        pub fn oeffnen(port: &str, tx: u32, rx: u32) -> Result<Self> {
            let dll = dll_fuer(port).ok_or_else(|| anyhow::anyhow!(tr!("Kein J2534-Adapter „{a}“ gefunden — DLL-Pfad angeben oder Treiber installieren", a = port)))?;
            let m = GERAET.get_or_init(|| Mutex::new(None));
            let mut g = m.lock().unwrap_or_else(|e| e.into_inner());
            if g.as_ref().is_none_or(|x| x.dll != dll) {
                *g = Some(laden(&dll)?);
            }
            let g = g.as_mut().expect("geladen");
            if let Some(k) = g.kanal.take() {
                unsafe { (g.disconnect)(k) };
            }
            let flags = if tx > 0x7FF { CAN_29BIT_ID } else { 0 };
            let mut kanal = 0u32;
            let r = unsafe { (g.connect)(g.id, ISO15765, flags, 500_000, &mut kanal) };
            if r != 0 {
                return Err(fehler(g, r, "PassThruConnect"));
            }
            // Flow-Control-Filter: Antworten von rx annehmen, Flusssteuerung an tx
            let id = |v: u32| v.to_be_bytes();
            let maske = Msg::neu(ISO15765, ISO15765_FRAME_PAD | flags, &[0xFF, 0xFF, 0xFF, 0xFF]);
            let muster = Msg::neu(ISO15765, ISO15765_FRAME_PAD | flags, &id(rx));
            let fc = Msg::neu(ISO15765, ISO15765_FRAME_PAD | flags, &id(tx));
            let mut fid = 0u32;
            let r = unsafe { (g.filter)(kanal, FLOW_CONTROL_FILTER, &*maske, &*muster, &*fc, &mut fid) };
            if r != 0 {
                return Err(fehler(g, r, "PassThruStartMsgFilter"));
            }
            unsafe { (g.ioctl)(kanal, CLEAR_RX_BUFFER, std::ptr::null(), std::ptr::null_mut()) };
            g.kanal = Some(kanal);
            Ok(Self { tx, flags })
        }
    }

    impl crate::transport::Transport for J2534 {
        fn send(&self, data: &[u8]) -> Result<()> {
            let m = GERAET.get().context("J2534 nicht geöffnet")?;
            let g = m.lock().unwrap_or_else(|e| e.into_inner());
            let g = g.as_ref().context("J2534 nicht geöffnet")?;
            let kanal = g.kanal.context("J2534: kein Kanal")?;
            let mut d = self.tx.to_be_bytes().to_vec();
            d.extend_from_slice(data);
            let msg = Msg::neu(ISO15765, ISO15765_FRAME_PAD | self.flags, &d);
            let mut n = 1u32;
            let r = unsafe { (g.write)(kanal, &*msg, &mut n, 1000) };
            if r != 0 {
                return Err(fehler(g, r, "PassThruWriteMsgs"));
            }
            Ok(())
        }

        fn recv(&self) -> Result<Vec<u8>> {
            let m = GERAET.get().context("J2534 nicht geöffnet")?;
            let ende = std::time::Instant::now() + std::time::Duration::from_millis(1500);
            while std::time::Instant::now() < ende {
                let g = m.lock().unwrap_or_else(|e| e.into_inner());
                let g = g.as_ref().context("J2534 nicht geöffnet")?;
                let kanal = g.kanal.context("J2534: kein Kanal")?;
                let mut msg = Msg::neu(ISO15765, 0, &[]);
                let mut n = 1u32;
                let r = unsafe { (g.read)(kanal, &mut *msg, &mut n, 100) };
                if r == ERR_BUFFER_EMPTY || n == 0 {
                    continue;
                }
                if r != 0 {
                    return Err(fehler(g, r, "PassThruReadMsgs"));
                }
                // Echo der eigenen Nachricht und Beginn-Meldungen überspringen
                if msg.rx_status & (TX_MSG_TYPE | START_OF_MESSAGE) != 0 || msg.data_size <= 4 {
                    continue;
                }
                return Ok(msg.data[4..msg.data_size as usize].to_vec());
            }
            bail!(tr!("Keine Antwort (Timeout) vom Steuergerät"))
        }
    }
}

#[cfg(windows)]
pub use echt::J2534;

/// Unter Linux/macOS gibt es keine J2534-Treiber.
#[cfg(not(windows))]
pub struct J2534;

#[cfg(not(windows))]
impl J2534 {
    pub fn oeffnen(_port: &str, _tx: u32, _rx: u32) -> Result<Self> {
        anyhow::bail!(tr!("J2534-Adapter gibt es nur unter Windows — unter Linux SocketCAN oder ELM327 nutzen."))
    }
}

#[cfg(not(windows))]
impl crate::transport::Transport for J2534 {
    fn send(&self, _data: &[u8]) -> Result<()> {
        anyhow::bail!("J2534 nur unter Windows")
    }
    fn recv(&self) -> Result<Vec<u8>> {
        anyhow::bail!("J2534 nur unter Windows")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dll_pfad_direkt() {
        assert_eq!(dll_fuer("C:\\Treiber\\op20pt64.dll").as_deref(), Some("C:\\Treiber\\op20pt64.dll"));
        #[cfg(not(windows))]
        assert!(J2534::oeffnen("x.dll", 0x7E0, 0x7E8).is_err());
    }
}
