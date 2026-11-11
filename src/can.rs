//! Roher CAN-Zugang. Unter Linux SocketCAN; auf anderen Systemen (Windows, macOS)
//! gleichnamige Platzhalter, deren `open` sauber mit einer Erklärung
//! scheitert. So läuft dort alles andere (ELM327, Demo, Auswertungen), und
//! ein echter Windows-CAN-Treiber kann später hier eingehängt werden.

#[cfg(target_os = "linux")]
pub use socketcan::{CanFrame, CanSocket, EmbeddedFrame, Id, Socket, StandardId};

#[cfg(not(target_os = "linux"))]
pub use ersatz::*;

/// Gibt es auf diesem System direkten CAN-Zugang (SocketCAN)?
pub const VERFUEGBAR: bool = cfg!(target_os = "linux");

#[cfg(not(target_os = "linux"))]
mod ersatz {
    use std::io;
    use std::time::Duration;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct StandardId(u16);
    impl StandardId {
        pub fn new(raw: u16) -> Option<Self> {
            (raw <= 0x7FF).then_some(Self(raw))
        }
        pub fn as_raw(&self) -> u16 {
            self.0
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct ExtendedId(u32);
    impl ExtendedId {
        pub fn as_raw(&self) -> u32 {
            self.0
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Id {
        Standard(StandardId),
        Extended(ExtendedId),
    }
    impl From<StandardId> for Id {
        fn from(s: StandardId) -> Self {
            Id::Standard(s)
        }
    }

    #[derive(Debug, Clone)]
    pub struct CanFrame {
        id: Id,
        data: Vec<u8>,
    }

    pub trait EmbeddedFrame: Sized {
        fn new(id: impl Into<Id>, data: &[u8]) -> Option<Self>;
        fn id(&self) -> Id;
        fn data(&self) -> &[u8];
    }
    impl EmbeddedFrame for CanFrame {
        fn new(id: impl Into<Id>, data: &[u8]) -> Option<Self> {
            (data.len() <= 8).then(|| CanFrame { id: id.into(), data: data.to_vec() })
        }
        fn id(&self) -> Id {
            self.id
        }
        fn data(&self) -> &[u8] {
            &self.data
        }
    }

    /// Platzhalter: lässt sich nie öffnen.
    pub struct CanSocket(());

    // wie das echte Socket (schließt beim Freigeben) — der Code gibt es ausdrücklich frei
    impl Drop for CanSocket {
        fn drop(&mut self) {}
    }

    fn fehlt() -> io::Error {
        io::Error::new(
            io::ErrorKind::Unsupported,
            crate::trs!("Direkter CAN-Zugang (SocketCAN) gibt es nur unter Linux. Unter Windows und macOS: ELM327 oder Demo-Fahrzeug wählen."),
        )
    }

    pub trait Socket: Sized {
        fn open(iface: &str) -> io::Result<Self>;
        fn read_frame(&self) -> io::Result<CanFrame>;
        fn write_frame(&self, frame: &CanFrame) -> io::Result<()>;
        fn set_read_timeout(&self, t: Duration) -> io::Result<()>;
        fn set_write_timeout(&self, t: Duration) -> io::Result<()>;
        fn set_nonblocking(&self, an: bool) -> io::Result<()>;
    }
    impl Socket for CanSocket {
        fn open(_iface: &str) -> io::Result<Self> {
            Err(fehlt())
        }
        fn read_frame(&self) -> io::Result<CanFrame> {
            Err(fehlt())
        }
        fn write_frame(&self, _frame: &CanFrame) -> io::Result<()> {
            Err(fehlt())
        }
        fn set_read_timeout(&self, _t: Duration) -> io::Result<()> {
            Err(fehlt())
        }
        fn set_write_timeout(&self, _t: Duration) -> io::Result<()> {
            Err(fehlt())
        }
        fn set_nonblocking(&self, _an: bool) -> io::Result<()> {
            Err(fehlt())
        }
    }
}
