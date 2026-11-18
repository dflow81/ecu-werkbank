//! VW Transport Protocol 2.0 (TP 2.0) over CAN.
//!
//! Used by VAG control units roughly 2004–2010 to carry KWP2000 (ISO 14230-3)
//! application messages over CAN, before the fleet moved to UDS/ISO-TP.
//!
//! ## VERIFY AGAINST A REAL CAPTURE
//!
//! TP 2.0 is not published by VAG. The structure here follows public community
//! documentation (see README sources). Parts marked "gesichert" are well
//! established; parts marked "TODO(verify)" must be confirmed against a real
//! handshake on your vehicle (`candump -L can0` while a channel is opened).
//!
//! Documented example handshake (engine, address 0x01):
//! ```text
//! 200  01 C0 00 10 00 03 01   Tester: Kanal zu 0x01, RX-ID ungültig, TX 0x300
//! 201  00 D0 00 03 40 07 01   ECU:    ECU sendet auf 0x300, Tester auf 0x740
//! 740  A0 0F 8A FF 32 FF      Tester: Kanalparameter (Blockgröße 15, Timing)
//! 300  A1 0F 8A FF 4A FF      ECU:    Kanalparameter
//! 740  10 00 02 10 89         Tester: Daten, Seq 0, Länge 2, KWP "10 89"
//! 300  B1                     ECU:    ACK, nächste erwartete Seq 1
//! ```
//!
//! Data-frame first byte: high nibble = type, low nibble = sequence (0..15,
//! running continuously over the whole channel, not reset per message):
//!   0x0X  weitere folgen, ACK erwartet (Blockende)
//!   0x1X  letztes Paket, ACK erwartet
//!   0x2X  weitere folgen, kein ACK
//!   0x3X  letztes Paket, kein ACK
//! (Passt zum dokumentierten Beispiel oben: `10 00 02 10 89` wird mit `B1`
//! bestätigt. Mit `ecu-werkbank decode-log` gegen eigenen Mitschnitt prüfen.)
//!   0xBX  ACK, bereit (X = nächste erwartete Seq)
//!   0x9X  ACK, nicht bereit

use anyhow::{anyhow, bail, Context, Result};
use crate::can::{CanFrame, CanSocket, EmbeddedFrame, Id, Socket, StandardId};
use std::time::{Duration, Instant};

/// Kanalaufbau vorübergehend nicht möglich — später nochmal versuchen.
/// Als eigene Fehlerart, damit die Wiederholung nicht an der (übersetzten)
/// Meldung hängt: `e.downcast_ref::<Kurzzeitig>()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kurzzeitig {
    /// 0xD7: Anwendungstyp vorübergehend nicht verfügbar
    Voruebergehend,
    /// 0xD8: keine Ressourcen frei
    KeineRessourcen,
    /// Kanal steht, aber keine Parameter-Antwort (0xA1)
    KeineParameterAntwort,
}

impl std::fmt::Display for Kurzzeitig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&match self {
            Kurzzeitig::Voruebergehend => crate::tr!("TP2.0 Setup abgelehnt: Anwendungstyp vorübergehend nicht verfügbar"),
            Kurzzeitig::KeineRessourcen => crate::tr!("TP2.0 Setup abgelehnt: keine Ressourcen frei"),
            Kurzzeitig::KeineParameterAntwort => crate::tr!("keine Parameter-Antwort (0xA1)"),
        })
    }
}

impl std::error::Error for Kurzzeitig {}

/// Ist das ein Fehler, nach dem sich ein neuer Versuch lohnt?
pub fn kurzzeitig(e: &anyhow::Error) -> Option<Kurzzeitig> {
    e.downcast_ref::<Kurzzeitig>().copied()
}

pub const CHANNEL_SETUP_ID: u16 = 0x200;

const OP_SETUP_REQUEST: u8 = 0xC0; // gesichert
const OP_SETUP_RESPONSE: u8 = 0xD0; // gesichert
const OP_PARAMS_REQUEST: u8 = 0xA0;
const OP_PARAMS_RESPONSE: u8 = 0xA1;
const OP_CHANNEL_TEST: u8 = 0xA3; // Keep-Alive
const OP_DISCONNECT: u8 = 0xA8;

const T_MORE_ACK: u8 = 0x00;
const T_LAST_ACK: u8 = 0x10;
const T_MORE_NOACK: u8 = 0x20;
const T_LAST_NOACK: u8 = 0x30;
const T_ACK_READY: u8 = 0xB0;
const T_ACK_NOT_READY: u8 = 0x90;

/// Application type requested in channel setup. 0x01 = KWP2000 diagnostics.
const APP_TYPE_KWP2000: u8 = 0x01;

/// Parameters we propose: block size 15, T1 = 0x8A (100 ms), T3 = 0x32 (5 ms).
const OUR_PARAMS: [u8; 6] = [OP_PARAMS_REQUEST, 0x0F, 0x8A, 0xFF, 0x32, 0xFF];

/// Raw CAN frame access, so TP 2.0 can run on SocketCAN or in-memory in tests.
pub trait CanIo {
    fn write(&self, id: u16, data: &[u8]) -> Result<()>;
    /// Next frame on `id` (other traffic is skipped). Err on timeout.
    fn read(&self, id: u16) -> Result<Vec<u8>>;
    /// Change the read timeout (no-op where it does not apply).
    fn set_timeout(&self, _timeout: Duration) -> Result<()> {
        Ok(())
    }
}

impl<T: CanIo + ?Sized> CanIo for &T {
    fn write(&self, id: u16, data: &[u8]) -> Result<()> {
        (**self).write(id, data)
    }
    fn read(&self, id: u16) -> Result<Vec<u8>> {
        (**self).read(id)
    }
    fn set_timeout(&self, timeout: Duration) -> Result<()> {
        (**self).set_timeout(timeout)
    }
}

impl CanIo for Box<dyn CanIo + Send> {
    fn write(&self, id: u16, data: &[u8]) -> Result<()> {
        (**self).write(id, data)
    }
    fn read(&self, id: u16) -> Result<Vec<u8>> {
        (**self).read(id)
    }
    fn set_timeout(&self, timeout: Duration) -> Result<()> {
        (**self).set_timeout(timeout)
    }
}

/// A TP 2.0 channel over any frame source (SocketCAN or demo vehicle).
pub type Channel = Tp20<Box<dyn CanIo + Send>>;

/// SocketCAN implementation of [`CanIo`].
pub struct SocketIo {
    sock: CanSocket,
    _belegt: crate::bus::Belegt,
}

impl SocketIo {
    pub fn open(iface: &str, timeout: Duration) -> Result<Self> {
        let sock = CanSocket::open(iface)
            .with_context(|| crate::tr!("CAN-Interface '{iface}' öffnen fehlgeschlagen", iface = iface))?;
        crate::connect::fix_hw_loopback(iface, &sock);
        sock.set_read_timeout(timeout)?;
        sock.set_write_timeout(Duration::from_millis(1000))?;
        Ok(Self { sock, _belegt: crate::bus::Belegt::neu() })
    }

    /// Next standard-ID frame of any ID (for the ECU simulator).
    pub fn read_any(&self) -> Result<(u16, Vec<u8>)> {
        loop {
            let frame = self.sock.read_frame().context("Timeout beim CAN-Empfang")?;
            if let Id::Standard(s) = frame.id() {
                return Ok((s.as_raw(), frame.data().to_vec()));
            }
        }
    }

}

impl CanIo for SocketIo {
    fn set_timeout(&self, timeout: Duration) -> Result<()> {
        self.sock.set_read_timeout(timeout)?;
        Ok(())
    }

    fn write(&self, id: u16, payload: &[u8]) -> Result<()> {
        let sid = StandardId::new(id).ok_or_else(|| anyhow!(crate::tr!("ungültige CAN-ID {id}", id = format!("{id:#x}"))))?;
        let frame =
            CanFrame::new(sid, payload).ok_or_else(|| anyhow!("Frame-Bau fehlgeschlagen"))?;
        self.sock.write_frame(&frame).context("CAN-Senden fehlgeschlagen")?;
        Ok(())
    }

    fn read(&self, want_id: u16) -> Result<Vec<u8>> {
        loop {
            let frame = self.sock.read_frame().context("Timeout beim CAN-Empfang")?;
            if let Id::Standard(s) = frame.id() {
                if s.as_raw() == want_id {
                    return Ok(frame.data().to_vec());
                }
            }
        }
    }
}

/// 11-bit CAN ID from a little-endian TP 2.0 ID field. The high byte carries a
/// validity flag in bit 4 (0x10 = "ungültig/frei wählbar").
fn id_field(lo: u8, hi: u8) -> u16 {
    u16::from(lo) | (u16::from(hi & 0x07) << 8)
}

/// An open TP 2.0 channel to one ECU.
pub struct Tp20<I: CanIo = SocketIo> {
    io: I,
    /// CAN ID we transmit on (negotiated).
    tx_id: u16,
    /// CAN ID the ECU transmits on (negotiated).
    rx_id: u16,
    tx_seq: u8,
    rx_seq: u8,
    block_size: u8,
    last_activity: Instant,
    open: bool,
    /// Mindestabstand zwischen zwei eigenen Frames (T3, vom Steuergerät vorgegeben)
    t3: Duration,
    last_tx: Instant,
    /// Zuletzt gesendeter Daten-Frame (für Wiederholung nach „nicht bereit“)
    last_frame: Vec<u8>,
}

/// T3-Byte dekodieren: Bits 7-6 = Einheit (0,1 ms / 1 ms / 10 ms / 100 ms), Bits 5-0 = Wert.
fn decode_time(b: u8) -> Duration {
    let v = u64::from(b & 0x3F);
    match b >> 6 {
        0 => Duration::from_micros(v * 100),
        1 => Duration::from_millis(v),
        2 => Duration::from_millis(v * 10),
        _ => Duration::from_millis(v * 100),
    }
}

impl Tp20<SocketIo> {
    /// Open a channel to the ECU at the given TP 2.0 logical address
    /// (e.g. 0x01 engine) with a 1 s timeout.
    pub fn connect(iface: &str, ecu_address: u8) -> Result<Self> {
        Self::connect_timeout(iface, ecu_address, Duration::from_millis(1000))
    }

    /// Like [`connect`](Self::connect), with a custom setup timeout (for fast
    /// bus scans). Data transfer afterwards uses a 1 s timeout.
    pub fn connect_timeout(iface: &str, ecu_address: u8, setup_timeout: Duration) -> Result<Self> {
        let io = SocketIo::open(iface, setup_timeout)?;
        let ch = Self::connect_io(io, ecu_address)?;
        ch.io.set_timeout(Duration::from_millis(1000))?;
        Ok(ch)
    }
}

impl<I: CanIo> Tp20<I> {
    /// Channel setup + parameter exchange over any [`CanIo`].
    pub fn connect_io(io: I, ecu_address: u8) -> Result<Self> {
        let setup = [
            ecu_address,      // Ziel (gesichert)
            OP_SETUP_REQUEST, // 0xC0 (gesichert)
            0x00, 0x10,       // RX-ID: ungültig → ECU wählt
            0x00, 0x03,       // TX-ID-Vorschlag 0x300 (wie im dokumentierten Beispiel)
            APP_TYPE_KWP2000,
        ];
        io.write(CHANNEL_SETUP_ID, &setup)?;

        let resp_id = CHANNEL_SETUP_ID + u16::from(ecu_address); // gesichert
        let resp = io
            .read(resp_id)
            .context(crate::trs!("keine TP2.0 Setup-Antwort (0xD0) vom Steuergerät"))?;
        if resp.len() < 6 {
            bail!(crate::tr!("TP2.0 Setup-Antwort zu kurz ({n} Byte)", n = resp.len()));
        }
        match resp[1] {
            OP_SETUP_RESPONSE => {}
            0xD6 => bail!(crate::trs!("TP2.0 Setup abgelehnt: Anwendungstyp nicht unterstützt")),
            0xD7 => return Err(Kurzzeitig::Voruebergehend.into()),
            0xD8 => return Err(Kurzzeitig::KeineRessourcen.into()),
            op => bail!(crate::tr!("TP2.0 Setup fehlgeschlagen: Opcode {op} statt 0xD0", op = format!("{op:#04x}"))),
        }

        // Response: bytes 2-3 = ID the ECU transmits on, bytes 4-5 = ID the
        // tester must transmit on (per documented example: 0x300 / 0x740).
        // TODO(verify) against your own capture.
        // Ab jetzt läuft die Verbindung: Steuergeräte hinter dem Gateway
        // brauchen für die Parameter länger als für die Setup-Antwort.
        io.set_timeout(Duration::from_millis(1000))?;
        let rx_id = id_field(resp[2], resp[3]);
        let tx_id = id_field(resp[4], resp[5]);
        if tx_id == 0 || rx_id == 0 || tx_id == rx_id {
            bail!(
                "TP2.0: ausgehandelte CAN-IDs unplausibel (tx={tx_id:#05x} rx={rx_id:#05x}); \
                 Byte-Offsets gegen echten Mitschnitt prüfen"
            );
        }

        let mut ch = Self {
            io,
            tx_id,
            rx_id,
            tx_seq: 0,
            rx_seq: 0,
            block_size: 0x0F,
            last_activity: Instant::now(),
            open: true,
            // bis zur Parameter-Antwort vorsichtig 10 ms annehmen
            t3: Duration::from_millis(10),
            last_tx: Instant::now(),
            last_frame: Vec::new(),
        };

        ch.write_paced(&OUR_PARAMS)?;
        let pr = ch.read_control(OP_PARAMS_RESPONSE).context(Kurzzeitig::KeineParameterAntwort)?;
        if pr.len() >= 2 {
            ch.block_size = pr[1].clamp(1, 15);
        }
        if pr.len() >= 5 {
            // nicht kürzer als 5 ms, nicht länger als 100 ms
            ch.t3 = decode_time(pr[4]).clamp(Duration::from_millis(5), Duration::from_millis(100));
        }
        ch.last_activity = Instant::now();
        Ok(ch)
    }

    /// Change the read timeout of the underlying frame source.
    pub fn set_timeout(&self, timeout: Duration) -> Result<()> {
        self.io.set_timeout(timeout)
    }

    /// Frame senden und dabei den Mindestabstand T3 einhalten.
    fn write_paced(&mut self, data: &[u8]) -> Result<()> {
        let since = self.last_tx.elapsed();
        if since < self.t3 {
            std::thread::sleep(self.t3 - since);
        }
        self.io.write(self.tx_id, data)?;
        self.last_tx = Instant::now();
        Ok(())
    }

    /// Kanaltest des Steuergeräts mit unseren Parametern beantworten.
    fn answer_channel_test(&mut self) -> Result<()> {
        let mut p = OUR_PARAMS;
        p[0] = OP_PARAMS_RESPONSE;
        self.write_paced(&p)
    }

    /// Read on rx_id until a frame with the given opcode arrives.
    fn read_control(&self, op: u8) -> Result<Vec<u8>> {
        for _ in 0..16 {
            let f = self.io.read(self.rx_id)?;
            if f.first() == Some(&op) {
                return Ok(f);
            }
        }
        bail!(crate::tr!("TP2.0: Kontroll-Opcode {op} nicht erhalten", op = format!("{op:#04x}")))
    }

    /// Send a channel test if the channel has been idle for ~900 ms.
    /// Call this regularly when holding a channel open without requests.
    pub fn keep_alive(&mut self) -> Result<()> {
        if self.last_activity.elapsed() >= Duration::from_millis(900) {
            self.write_paced(&[OP_CHANNEL_TEST])?;
            self.read_control(OP_PARAMS_RESPONSE)?;
            self.last_activity = Instant::now();
        }
        Ok(())
    }

    /// Send one KWP2000 message and return the reassembled response.
    pub fn request(&mut self, kwp: &[u8]) -> Result<Vec<u8>> {
        self.keep_alive()?;
        self.send(kwp)?;
        self.recv()
    }

    /// Send one KWP2000 message (length-prefixed, segmented into 7-byte packets).
    pub fn send(&mut self, data: &[u8]) -> Result<()> {
        if data.is_empty() || data.len() > 0x7FFF {
            bail!(crate::tr!("TP2.0: ungültige Nachrichtenlänge {n}", n = data.len()));
        }
        let mut body = Vec::with_capacity(2 + data.len());
        body.push((data.len() >> 8) as u8);
        body.push(data.len() as u8);
        body.extend_from_slice(data);

        let chunks: Vec<&[u8]> = body.chunks(7).collect();
        let mut in_block = 0u8;
        for (i, chunk) in chunks.iter().enumerate() {
            let last = i + 1 == chunks.len();
            in_block += 1;
            let block_end = in_block >= self.block_size;
            let kind = match (last, block_end) {
                (true, _) => T_LAST_ACK,
                (false, true) => T_MORE_ACK,
                (false, false) => T_MORE_NOACK,
            };
            let mut frame = Vec::with_capacity(8);
            frame.push(kind | self.tx_seq);
            frame.extend_from_slice(chunk);
            self.write_paced(&frame)?;
            self.last_frame = frame;
            self.tx_seq = (self.tx_seq + 1) & 0x0F;

            if kind == T_LAST_ACK || kind == T_MORE_ACK {
                self.wait_ack()?;
                in_block = 0;
            }
        }
        self.last_activity = Instant::now();
        Ok(())
    }

    fn wait_ack(&mut self) -> Result<()> {
        let mut resent = 0;
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            let f = self.io.read(self.rx_id)?;
            let Some(&b0) = f.first() else { continue };
            if b0 == OP_DISCONNECT {
                self.open = false;
                bail!(crate::trs!("TP2.0: Steuergerät hat den Kanal geschlossen"));
            }
            if b0 == OP_CHANNEL_TEST {
                self.answer_channel_test()?;
                continue;
            }
            let kind = b0 & 0xF0;
            if kind != T_ACK_READY && kind != T_ACK_NOT_READY {
                continue;
            }
            let seq = b0 & 0x0F;
            if seq == self.tx_seq {
                if kind == T_ACK_READY {
                    return Ok(());
                }
                continue; // angenommen, Steuergerät noch beschäftigt — auf 0xBX warten
            }
            // Steuergerät erwartet noch unseren letzten Frame: nochmal senden
            if seq == self.tx_seq.wrapping_sub(1) & 0x0F && !self.last_frame.is_empty() && resent < 3 {
                resent += 1;
                std::thread::sleep(Duration::from_millis(if kind == T_ACK_NOT_READY { 50 } else { 0 }));
                let f = self.last_frame.clone();
                self.write_paced(&f)?;
                continue;
            }
            bail!(crate::tr!("TP2.0: ACK für Seq {a} erwartet, {seq} erhalten", a = self.tx_seq, seq = seq));
        }
        bail!(crate::trs!("TP2.0: kein ACK erhalten"))
    }

    /// Receive one complete KWP2000 message.
    pub fn recv(&mut self) -> Result<Vec<u8>> {
        let mut assembled: Vec<u8> = Vec::new();
        loop {
            let frame = self.io.read(self.rx_id)?;
            let Some(&b0) = frame.first() else { continue };
            match b0 {
                OP_DISCONNECT => {
                    self.open = false;
                    bail!(crate::trs!("TP2.0: Steuergerät hat den Kanal geschlossen"));
                }
                OP_CHANNEL_TEST => {
                    // ECU-initiated channel test: answer with our parameters.
                    self.answer_channel_test()?;
                    continue;
                }
                0xA0..=0xAF => continue,
                _ => {}
            }
            let kind = b0 & 0xF0;
            let seq = b0 & 0x0F;
            if kind == T_ACK_READY || kind == T_ACK_NOT_READY {
                continue; // stray ACK
            }
            if !matches!(kind, T_MORE_ACK | T_MORE_NOACK | T_LAST_ACK | T_LAST_NOACK) {
                continue;
            }
            if seq != self.rx_seq {
                bail!("TP2.0 Sequenzfehler: {seq} erhalten, {} erwartet", self.rx_seq);
            }
            assembled.extend_from_slice(&frame[1..]);
            self.rx_seq = (self.rx_seq + 1) & 0x0F;

            if kind == T_MORE_ACK || kind == T_LAST_ACK {
                let ack = [T_ACK_READY | self.rx_seq];
                self.write_paced(&ack)?;
            }
            if kind == T_LAST_ACK || kind == T_LAST_NOACK {
                break;
            }
        }
        self.last_activity = Instant::now();
        if assembled.len() < 2 {
            bail!(crate::trs!("TP2.0: Nachricht ohne Längenfeld"));
        }
        let len = ((usize::from(assembled[0]) << 8) | usize::from(assembled[1])) & 0x7FFF;
        let mut payload = assembled.split_off(2);
        if payload.len() < len {
            bail!(crate::tr!("TP2.0: Nachricht unvollständig ({a} von {len} Byte)", a = payload.len(), len = len));
        }
        payload.truncate(len);
        Ok(payload)
    }

    /// Close the channel (also done automatically on drop).
    pub fn disconnect(&mut self) {
        if self.open {
            let _ = self.write_paced(&[OP_DISCONNECT]);
            self.open = false;
        }
    }

    pub fn tx_id(&self) -> u16 {
        self.tx_id
    }
    pub fn rx_id(&self) -> u16 {
        self.rx_id
    }
}

impl<I: CanIo> Drop for Tp20<I> {
    fn drop(&mut self) {
        self.disconnect();
    }
}

/// ECU side of TP 2.0, for the mock ECU and tests. Feed it every frame seen on
/// the bus; it returns the frames the ECU would send.
/// Antwortet auf eine Anfrage mit null oder mehr Antworten (simuliertes Steuergerät).
pub type EcuHandler = Box<dyn FnMut(&[u8]) -> Vec<Vec<u8>> + Send>;

pub struct EcuSim {
    pub address: u8,
    /// ID the ECU transmits on.
    pub ecu_tx: u16,
    /// ID the tester must transmit on.
    pub tester_tx: u16,
    /// Returns zero or more response messages for one request.
    handler: EcuHandler,
    rx_seq: u8,
    tx_seq: u8,
    assembled: Vec<u8>,
    connected: bool,
    /// Antwortet nur auf den Kanalaufbau, danach nichts mehr (wie Adresse 0x29 im Golf 6)
    pub nur_setup: bool,
    /// Liefert true, solange der Kanalaufbau mit „vorübergehend nicht
    /// verfügbar“ (0xD7) abgelehnt wird (Golf-6-Motor kurz nach OBD)
    pub gesperrt: Option<Box<dyn Fn() -> bool + Send>>,
}

impl EcuSim {
    pub fn new(address: u8, handler: EcuHandler) -> Self {
        Self {
            address,
            ecu_tx: 0x300 + u16::from(address),
            tester_tx: 0x740 + u16::from(address),
            handler,
            rx_seq: 0,
            tx_seq: 0,
            assembled: Vec::new(),
            connected: false,
            nur_setup: false,
            gesperrt: None,
        }
    }

    pub fn on_frame(&mut self, id: u16, data: &[u8]) -> Vec<(u16, Vec<u8>)> {
        let mut out = Vec::new();
        if id == CHANNEL_SETUP_ID && data.len() >= 7 && data[0] == self.address && data[1] == OP_SETUP_REQUEST {
            if self.gesperrt.as_ref().is_some_and(|f| f()) {
                self.connected = false;
                out.push((CHANNEL_SETUP_ID + u16::from(self.address), vec![0x00, 0xD7, 0x00, 0x00, 0x00, 0x00, APP_TYPE_KWP2000]));
                return out;
            }
            self.rx_seq = 0;
            self.tx_seq = 0;
            self.assembled.clear();
            self.connected = true;
            out.push((
                CHANNEL_SETUP_ID + u16::from(self.address),
                vec![
                    0x00,
                    OP_SETUP_RESPONSE,
                    self.ecu_tx as u8,
                    (self.ecu_tx >> 8) as u8,
                    self.tester_tx as u8,
                    (self.tester_tx >> 8) as u8,
                    APP_TYPE_KWP2000,
                ],
            ));
            return out;
        }
        if id != self.tester_tx || !self.connected || self.nur_setup {
            return out;
        }
        let Some(&b0) = data.first() else { return out };
        match b0 {
            OP_PARAMS_REQUEST | OP_CHANNEL_TEST => {
                out.push((self.ecu_tx, vec![OP_PARAMS_RESPONSE, 0x0F, 0x8A, 0xFF, 0x4A, 0xFF]));
                return out;
            }
            OP_DISCONNECT => {
                self.connected = false;
                out.push((self.ecu_tx, vec![OP_DISCONNECT]));
                return out;
            }
            _ => {}
        }
        let kind = b0 & 0xF0;
        if !matches!(kind, T_MORE_ACK | T_MORE_NOACK | T_LAST_ACK | T_LAST_NOACK) {
            return out; // tester ACKs etc.
        }
        self.assembled.extend_from_slice(&data[1..]);
        self.rx_seq = ((b0 & 0x0F) + 1) & 0x0F;
        if kind == T_MORE_ACK || kind == T_LAST_ACK {
            out.push((self.ecu_tx, vec![T_ACK_READY | self.rx_seq]));
        }
        if kind == T_LAST_ACK || kind == T_LAST_NOACK {
            let msg = std::mem::take(&mut self.assembled);
            if msg.len() >= 2 {
                let len = ((usize::from(msg[0]) << 8) | usize::from(msg[1])).min(msg.len() - 2);
                let responses = (self.handler)(&msg[2..2 + len]);
                for resp in responses.into_iter().filter(|r| !r.is_empty()) {
                    let mut body = vec![(resp.len() >> 8) as u8, resp.len() as u8];
                    body.extend_from_slice(&resp);
                    let chunks: Vec<&[u8]> = body.chunks(7).collect();
                    for (i, c) in chunks.iter().enumerate() {
                        let kind = if i + 1 == chunks.len() { T_LAST_ACK } else { T_MORE_NOACK };
                        let mut f = vec![kind | self.tx_seq];
                        f.extend_from_slice(c);
                        out.push((self.ecu_tx, f));
                        self.tx_seq = (self.tx_seq + 1) & 0x0F;
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
pub(crate) mod test_bus {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;

    /// In-memory bus: frames written by the tester go to the simulated ECUs;
    /// their replies queue up for the tester to read.
    pub struct SimBus {
        pub ecus: RefCell<Vec<EcuSim>>,
        inbox: RefCell<VecDeque<(u16, Vec<u8>)>>,
        pub log: RefCell<Vec<(u16, Vec<u8>)>>,
    }

    impl SimBus {
        pub fn new(ecus: Vec<EcuSim>) -> Self {
            Self { ecus: RefCell::new(ecus), inbox: RefCell::default(), log: RefCell::default() }
        }
    }

    impl CanIo for SimBus {
        fn write(&self, id: u16, data: &[u8]) -> Result<()> {
            self.log.borrow_mut().push((id, data.to_vec()));
            for ecu in self.ecus.borrow_mut().iter_mut() {
                for f in ecu.on_frame(id, data) {
                    self.log.borrow_mut().push(f.clone());
                    self.inbox.borrow_mut().push_back(f);
                }
            }
            Ok(())
        }
        fn read(&self, id: u16) -> Result<Vec<u8>> {
            let mut inbox = self.inbox.borrow_mut();
            while let Some((fid, data)) = inbox.pop_front() {
                if fid == id {
                    return Ok(data);
                }
            }
            bail!("Timeout (Sim)")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_bus::SimBus;
    use super::*;

    fn echo_ecu(addr: u8) -> EcuSim {
        // Positive response: SID + 0x40, then echo the rest.
        EcuSim::new(addr, Box::new(|req| {
            let mut r = vec![req[0] + 0x40];
            r.extend_from_slice(&req[1..]);
            vec![r]
        }))
    }

    #[test]
    fn setup_uses_documented_id_order() {
        let bus = SimBus::new(vec![echo_ecu(0x01)]);
        let ch = Tp20::connect_io(&bus, 0x01).unwrap();
        assert_eq!(ch.tx_id(), 0x741); // tester → ECU
        assert_eq!(ch.rx_id(), 0x301); // ECU → tester
        assert_eq!(bus.log.borrow()[0], (0x200, vec![0x01, 0xC0, 0x00, 0x10, 0x00, 0x03, 0x01]));
    }

    #[test]
    fn parses_documented_setup_reply() {
        assert_eq!(id_field(0x00, 0x03), 0x300);
        assert_eq!(id_field(0x40, 0x07), 0x740);
        assert_eq!(id_field(0x00, 0x10), 0x000); // validity flag masked off
    }

    #[test]
    fn short_and_long_messages_roundtrip() {
        let bus = SimBus::new(vec![echo_ecu(0x01)]);
        let mut ch = Tp20::connect_io(&bus, 0x01).unwrap();
        assert_eq!(ch.request(&[0x10, 0x89]).unwrap(), vec![0x50, 0x89]);
        let long: Vec<u8> = std::iter::once(0x21).chain(0..40u8).collect();
        let resp = ch.request(&long).unwrap();
        assert_eq!(resp[0], 0x61);
        assert_eq!(&resp[1..], &long[1..]);
        // Sequence numbers continue across messages: a third request still works.
        assert_eq!(ch.request(&[0x1A, 0x9B]).unwrap(), vec![0x5A, 0x9B]);
    }

    #[test]
    fn block_size_requests_ack_mid_message() {
        let bus = SimBus::new(vec![echo_ecu(0x01)]);
        let mut ch = Tp20::connect_io(&bus, 0x01).unwrap();
        ch.block_size = 2;
        let msg: Vec<u8> = std::iter::once(0x21).chain(0..30u8).collect();
        ch.request(&msg).unwrap();
        let kinds: Vec<u8> = bus
            .log
            .borrow()
            .iter()
            .filter(|(id, d)| *id == 0x741 && d[0] < 0x40)
            .map(|(_, d)| d[0] & 0xF0)
            .collect();
        assert_eq!(kinds, vec![0x20, 0x00, 0x20, 0x00, 0x10]);
    }

    #[test]
    fn missing_ecu_times_out() {
        let bus = SimBus::new(vec![echo_ecu(0x01)]);
        assert!(Tp20::connect_io(&bus, 0x17).is_err());
    }
}
