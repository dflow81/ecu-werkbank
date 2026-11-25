//! Transport-Abstraktion: UDS/ISO-TP über austauschbare „Drähte".
//!
//! - **SocketCAN** (nativer CAN-Adapter, z. B. CANable) — schnell, voller
//!   Buszugriff, der leistungsstarke Standard.
//! - **ELM327** über Seriell (USB/Bluetooth) oder TCP (WiFi) — billig und
//!   bequem, aber langsamer und ohne TP 2.0. Braucht Hardware-Verifikation.
//!
//! Die UDS-Schicht spricht nur mit diesem Trait und weiß nichts vom Adapter.

use anyhow::Result;

/// Standard-Transport: unter Linux SocketCAN, sonst (Windows) ELM327.
pub const STANDARD_TRANSPORT: &str = if crate::can::VERFUEGBAR { "socketcan" } else { "elm327" };
/// Standard-Port für ELM327 über USB.
/// (macOS: Name hängt vom USB-Chip ab, z. B. /dev/cu.usbserial-110 — im Feld anpassen.)
pub const STANDARD_PORT: &str = if cfg!(windows) {
    "COM3"
} else if cfg!(target_os = "macos") {
    "/dev/cu.usbserial"
} else {
    "/dev/ttyUSB0"
};

/// Eine Punkt-zu-Punkt-ISO-TP-Verbindung zu einem Steuergerät (feste tx/rx).
pub trait Transport {
    /// Vollständige Service-Nutzlast senden (ISO-TP intern).
    fn send(&self, data: &[u8]) -> Result<()>;
    /// Nächste vollständige Antwort-Nutzlast empfangen.
    fn recv(&self) -> Result<Vec<u8>>;
}

/// Welcher Adapter/„Draht" genutzt wird.
#[derive(Clone, Debug)]
pub enum Backend {
    /// Nativer CAN über SocketCAN (iface z. B. "can0", "vcan0").
    SocketCan { iface: String },
    /// ELM327 über seriellen Port (USB /dev/ttyUSB0 oder BT /dev/rfcomm0).
    Elm327Serial { port: String, baud: u32 },
    /// ELM327 über TCP (WiFi-Adapter, "host:port").
    Elm327Tcp { addr: String },
    /// DoIP (Diagnose über Ethernet, ISO 13400) zum Gateway "IP" oder "IP:Port";
    /// die Anfrage-ID ist die logische Adresse des Steuergeräts.
    Doip { addr: String },
    /// Eingebautes Demo-Fahrzeug (ohne Adapter), siehe [`crate::sim`].
    Demo,
}

impl Backend {
    /// Öffnet eine Verbindung zu einem Steuergerät mit den gegebenen 11-Bit-CAN-IDs.
    pub fn open(&self, tx: u16, rx: u16) -> Result<Box<dyn Transport>> {
        self.open_id(u32::from(tx), u32::from(rx))
    }

    /// Wie [`Backend::open`], auch mit 29-Bit-IDs: IDs über 0x7FF werden als
    /// erweiterte Frames gesendet (ISO 15765-4, z. B. 0x18DA10F1 / 0x18DAF110).
    pub fn open_id(&self, tx: u32, rx: u32) -> Result<Box<dyn Transport>> {
        if tx > 0x1FFF_FFFF || rx > 0x1FFF_FFFF {
            anyhow::bail!(crate::tr!("CAN-ID zu groß (höchstens 29 Bit)"));
        }
        match self {
            Backend::SocketCan { iface } => {
                Ok(Box::new(crate::isotp::IsoTp::open(iface, tx, rx)?))
            }
            Backend::Elm327Serial { port, baud } => {
                Ok(Box::new(crate::elm327::Elm327::open_serial(port, *baud, tx, rx)?))
            }
            Backend::Elm327Tcp { addr } => {
                Ok(Box::new(crate::elm327::Elm327::open_tcp(addr, tx, rx)?))
            }
            Backend::Doip { addr } => {
                let ziel = u16::try_from(tx).map_err(|_| anyhow::anyhow!(crate::tr!("DoIP: logische Adresse höchstens 16 Bit")))?;
                Ok(Box::new(crate::doip::DoIp::verbinden(addr, ziel)?))
            }
            Backend::Demo => Ok(Box::new(crate::sim::DemoUds::new(crate::sim::demo_tx(tx)))),
        }
    }

    /// Kann dieses Backend TP 2.0? (ELM327 nicht.)
    pub fn supports_tp20(&self) -> bool {
        matches!(self, Backend::SocketCan { .. } | Backend::Demo)
    }

    /// Öffnet einen TP-2.0-Kanal; `setup_timeout` gilt für den Kanalaufbau,
    /// danach 1 s.
    pub fn open_tp20(&self, address: u8, setup_timeout: std::time::Duration) -> Result<crate::tp20::Channel> {
        use crate::tp20::{CanIo, Tp20};
        let io: Box<dyn CanIo + Send> = match self {
            Backend::SocketCan { iface } => Box::new(crate::tp20::SocketIo::open(iface, setup_timeout)?),
            Backend::Demo => Box::new(crate::sim::DemoCanIo::new()),
            _ => anyhow::bail!(crate::trs!("TP 2.0 braucht SocketCAN (oder Demo) — ELM327 kann es nicht")),
        };
        let ch = Tp20::connect_io(io, address)?;
        ch.set_timeout(std::time::Duration::from_millis(1000))?;
        Ok(ch)
    }

    /// Kurzer Klartext fürs Log.
    pub fn describe(&self) -> String {
        match self {
            Backend::SocketCan { iface } => format!("SocketCAN ({iface})"),
            Backend::Elm327Serial { port, baud } => format!("ELM327 seriell ({port} @ {baud})"),
            Backend::Elm327Tcp { addr } => format!("ELM327 TCP ({addr})"),
            Backend::Doip { addr } => format!("DoIP ({addr})"),
            Backend::Demo => match crate::sim::modell() {
                crate::sim::Modell::Demo => crate::tr!("Demo-Fahrzeug (ohne Adapter)"),
                m => crate::tr!("Simulation {a} (ohne Adapter)", a = m.name()),
            },
        }
    }
}

// SocketCAN-ISO-TP ist bereits ein Transport.
impl Transport for crate::isotp::IsoTp {
    fn send(&self, data: &[u8]) -> Result<()> {
        crate::isotp::IsoTp::send(self, data)
    }
    fn recv(&self) -> Result<Vec<u8>> {
        crate::isotp::IsoTp::recv(self)
    }
}
