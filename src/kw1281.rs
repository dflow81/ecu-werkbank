//! KW1281 — VAG-Diagnose über die K-Leitung (Fahrzeuge bis etwa 2004), über
//! ein KKL-Kabel (USB-Seriell, z. B. FTDI) am OBD-Stecker. **Nur lesen**:
//! Identifikation, Fehlerspeicher, Messwertgruppen.
//!
//! Ablauf (öffentlich beschriebenes Protokoll):
//! 1. **5-Baud-Anmeldung**: die Steuergeräte-Adresse (z. B. 0x01 Motor) wird
//!    mit 5 Bit/s gesendet (7 Datenbits, ungerade Parität), indem die Leitung
//!    per „Break“ auf Low gezogen wird.
//! 2. Das Steuergerät antwortet mit 10 400 Baud: Sync `55`, Schlüsselwort
//!    `01 8A`; der Tester schickt das Komplement des letzten Bytes (`75`).
//! 3. Danach Blöcke: `Länge Zähler Titel Daten… 03`. Jedes Byte außer dem
//!    Endbyte wird vom Empfänger mit seinem Komplement quittiert. Der Zähler
//!    läuft über beide Seiten weiter.
//! 4. Das Steuergerät schickt zuerst seine Identifikation (Titel `F6`, Text),
//!    jeder Block wird mit `ACK` (Titel `09`) beantwortet, bis es selbst `ACK`
//!    schickt — dann darf gefragt werden.
//!
//! Die K-Leitung ist eine einzige Leitung: das KKL-Kabel liefert jedes
//! gesendete Byte als Echo zurück; das wird gelesen und verworfen.

use crate::kwp::BlockValue;
use crate::tr;
use anyhow::{bail, Context, Result};
use std::time::Duration;

/// Block-Titel.
pub mod titel {
    pub const ACK: u8 = 0x09;
    pub const NAK: u8 = 0x0A;
    pub const ENDE: u8 = 0x06;
    pub const FEHLER_LESEN: u8 = 0x07;
    pub const FEHLER_ANTWORT: u8 = 0xFC;
    pub const GRUPPE_LESEN: u8 = 0x29;
    pub const GRUPPE_ANTWORT: u8 = 0xE7;
    pub const TEXT: u8 = 0xF6;
}

/// Die K-Leitung: Bytes senden/empfangen und die 5-Baud-Anmeldung.
pub trait Leitung {
    /// Ein Byte senden (ein Echo der Leitung liest die Umsetzung selbst weg).
    fn senden(&mut self, b: u8) -> Result<()>;
    /// Ein Byte empfangen (Fehler bei Zeitüberschreitung).
    fn empfangen(&mut self, timeout: Duration) -> Result<u8>;
    /// Adresse mit 5 Bit/s senden (7O1) und danach auf 10 400 Baud bereit sein.
    fn fuenf_baud(&mut self, adresse: u8) -> Result<()>;
}

/// Bits der 5-Baud-Anmeldung: Start (0), 7 Datenbits LSB zuerst, ungerade
/// Parität, Stopp (1). `true` = High (Ruhe), `false` = Low (Break).
pub fn fuenf_baud_bits(adresse: u8) -> Vec<bool> {
    let mut v = vec![false];
    let mut einsen = 0;
    for i in 0..7 {
        let bit = (adresse >> i) & 1 == 1;
        einsen += usize::from(bit);
        v.push(bit);
    }
    v.push(einsen % 2 == 0); // ungerade Parität
    v.push(true);
    v
}

/// Verbindung zu einem Steuergerät über KW1281.
pub struct Kw1281<L: Leitung> {
    leitung: L,
    zaehler: u8,
    /// Identifikation (Teilenummer, Bezeichnung …) aus der Anmeldung
    pub ident: Vec<String>,
}

const T_BYTE: Duration = Duration::from_millis(1000);

impl<L: Leitung> Kw1281<L> {
    /// Anmelden und Identifikation lesen.
    pub fn verbinden(mut leitung: L, adresse: u8) -> Result<Self> {
        leitung.fuenf_baud(adresse)?;
        let sync = leitung.empfangen(Duration::from_millis(2500)).context(tr!("K-Leitung: keine Antwort auf die Anmeldung (Zündung an? Adresse richtig?)"))?;
        if sync != 0x55 {
            bail!(tr!("K-Leitung: Sync {a} statt 55 — falsche Baudrate?", a = format!("{sync:02X}")));
        }
        let _kw1 = leitung.empfangen(T_BYTE)?;
        let kw2 = leitung.empfangen(T_BYTE)?;
        std::thread::sleep(Duration::from_millis(25));
        leitung.senden(!kw2)?;
        let mut k = Self { leitung, zaehler: 0, ident: Vec::new() };
        // Identifikation, bis das Steuergerät ACK schickt
        for _ in 0..10 {
            let (t, d) = k.block_empfangen()?;
            match t {
                titel::ACK => return Ok(k),
                titel::TEXT => {
                    let s = String::from_utf8_lossy(&d).trim().to_string();
                    if !s.is_empty() {
                        k.ident.push(s);
                    }
                    k.block_senden(titel::ACK, &[])?;
                }
                _ => k.block_senden(titel::ACK, &[])?,
            }
        }
        bail!(tr!("K-Leitung: Identifikation endet nicht"))
    }

    fn naechster_zaehler(&mut self) -> u8 {
        self.zaehler = self.zaehler.wrapping_add(1);
        self.zaehler
    }

    /// Block senden: Länge, Zähler, Titel, Daten (jedes Byte quittiert), Ende 03.
    pub fn block_senden(&mut self, t: u8, daten: &[u8]) -> Result<()> {
        let z = self.naechster_zaehler();
        let mut b = vec![(daten.len() + 3) as u8, z, t];
        b.extend_from_slice(daten);
        for &x in &b {
            self.leitung.senden(x)?;
            let q = self.leitung.empfangen(T_BYTE).context(tr!("K-Leitung: Quittung fehlt"))?;
            if q != !x {
                bail!(tr!("K-Leitung: falsche Quittung {a} für {b}", a = format!("{q:02X}"), b = format!("{x:02X}")));
            }
        }
        self.leitung.senden(0x03)
    }

    /// Block empfangen (jedes Byte außer dem Ende quittieren): (Titel, Daten).
    pub fn block_empfangen(&mut self) -> Result<(u8, Vec<u8>)> {
        let lies = |l: &mut L| -> Result<u8> {
            let x = l.empfangen(T_BYTE)?;
            l.senden(!x)?;
            Ok(x)
        };
        let len = lies(&mut self.leitung)? as usize;
        if len < 3 {
            bail!(tr!("K-Leitung: Blocklänge {a} ungültig", a = len));
        }
        let z = lies(&mut self.leitung)?;
        self.zaehler = z;
        let t = lies(&mut self.leitung)?;
        let mut d = Vec::with_capacity(len - 3);
        for _ in 0..len - 3 {
            d.push(lies(&mut self.leitung)?);
        }
        let ende = self.leitung.empfangen(T_BYTE)?;
        if ende != 0x03 {
            bail!(tr!("K-Leitung: Blockende {a} statt 03", a = format!("{ende:02X}")));
        }
        Ok((t, d))
    }

    /// Anfrage senden und Antwortblöcke sammeln, bis das Steuergerät ACK schickt.
    fn anfrage(&mut self, t: u8, daten: &[u8]) -> Result<Vec<(u8, Vec<u8>)>> {
        self.block_senden(t, daten)?;
        let mut out = Vec::new();
        for _ in 0..20 {
            let (rt, d) = self.block_empfangen()?;
            if rt == titel::ACK {
                return Ok(out);
            }
            if rt == titel::NAK {
                bail!(tr!("K-Leitung: Steuergerät lehnt ab (NAK)"));
            }
            out.push((rt, d));
            self.block_senden(titel::ACK, &[])?;
        }
        bail!(tr!("K-Leitung: Antwort endet nicht"))
    }

    /// Fehlerspeicher: (Code, Status). Status Bit 7 = sporadisch, Bits 0–6 = Fehlerart.
    pub fn fehler_lesen(&mut self) -> Result<Vec<(u16, u8)>> {
        let mut out = Vec::new();
        for (t, d) in self.anfrage(titel::FEHLER_LESEN, &[])? {
            if t != titel::FEHLER_ANTWORT {
                continue;
            }
            for c in d.chunks(3) {
                if let [hi, lo, st] = c {
                    // FF FF 88 = kein Fehler gespeichert
                    if [*hi, *lo] != [0xFF, 0xFF] {
                        out.push((u16::from_be_bytes([*hi, *lo]), *st));
                    }
                }
            }
        }
        Ok(out)
    }

    /// Messwertgruppe lesen (4 Felder je Formel, a, b — gleiche Formeln wie TP 2.0).
    pub fn gruppe_lesen(&mut self, gruppe: u8) -> Result<Vec<BlockValue>> {
        for (t, d) in self.anfrage(titel::GRUPPE_LESEN, &[gruppe])? {
            if t == titel::GRUPPE_ANTWORT {
                return Ok(d.chunks(3).filter(|c| c.len() == 3).map(|c| BlockValue { formula: c[0], a: c[1], b: c[2], text: None }).collect());
            }
        }
        bail!(tr!("K-Leitung: Gruppe {a} nicht lesbar", a = gruppe))
    }

    /// Verbindung beenden.
    pub fn beenden(mut self) {
        let _ = self.block_senden(titel::ENDE, &[]);
    }
}

/// Fehlerart in Worten (eigene Texte aus der Datenbank, Protokoll `KW1281`).
pub fn fehlerart(status: u8) -> String {
    let nr = status & 0x7F;
    let sporadisch = if status & 0x80 != 0 { tr!(" — sporadisch") } else { String::new() };
    match crate::dtc_db::eigene_fehlerart("KW1281", nr.into()) {
        Some(t) => format!("{nr:03} {t}{sporadisch}"),
        None => tr!("Fehlerart {a}{b}", a = format!("{nr:03}"), b = sporadisch),
    }
}

/// Echtes KKL-Kabel an einer seriellen Schnittstelle (10 400 Baud, 8N1).
pub struct Kkl {
    port: serial2::SerialPort,
}

impl Kkl {
    pub fn oeffnen(pfad: &str) -> Result<Self> {
        let mut port = serial2::SerialPort::open(pfad, 10400).with_context(|| tr!("KKL-Kabel an {a} öffnen", a = pfad))?;
        port.set_read_timeout(Duration::from_millis(50))?;
        Ok(Self { port })
    }

    fn ein_byte(&mut self, timeout: Duration) -> Result<u8> {
        let ende = std::time::Instant::now() + timeout;
        let mut b = [0u8; 1];
        while std::time::Instant::now() < ende {
            if let Ok(1) = self.port.read(&mut b) {
                return Ok(b[0]);
            }
        }
        bail!(tr!("K-Leitung: Zeitüberschreitung"))
    }
}

impl Leitung for Kkl {
    fn senden(&mut self, b: u8) -> Result<()> {
        std::thread::sleep(Duration::from_millis(2)); // Abstand zwischen Bytes
        self.port.write_all(&[b])?;
        // Echo der eigenen Leitung wegwerfen
        let e = self.ein_byte(Duration::from_millis(100))?;
        if e != b {
            bail!(tr!("K-Leitung: Echo {a} statt {b} — Kabel ohne Echo oder Störung", a = format!("{e:02X}"), b = format!("{b:02X}")));
        }
        Ok(())
    }

    fn empfangen(&mut self, timeout: Duration) -> Result<u8> {
        self.ein_byte(timeout)
    }

    fn fuenf_baud(&mut self, adresse: u8) -> Result<()> {
        let _ = self.port.discard_buffers();
        std::thread::sleep(Duration::from_millis(300)); // Ruhe vor der Anmeldung
        for bit in fuenf_baud_bits(adresse) {
            self.port.set_break(!bit)?;
            std::thread::sleep(Duration::from_millis(200));
        }
        self.port.set_break(false)?;
        let _ = self.port.discard_input_buffer();
        Ok(())
    }
}

/// Simuliertes KW1281-Steuergerät (für Tests und `-t demo`): Motor mit einem
/// Fehler und Messwertgruppe 1.
#[derive(Default)]
pub struct DemoLeitung {
    /// was das Steuergerät als Nächstes sendet
    raus: std::collections::VecDeque<u8>,
    /// Bytes eines gerade empfangenen Blocks
    rein: Vec<u8>,
    erwartet_quittung: Option<u8>,
    zaehler: u8,
    phase: u8,
}

impl DemoLeitung {
    /// Block des Steuergeräts in die Ausgabe stellen (mit Quittungs-Handshake).
    fn block(&mut self, t: u8, d: &[u8]) {
        self.zaehler = self.zaehler.wrapping_add(1);
        let mut b = vec![(d.len() + 3) as u8, self.zaehler, t];
        b.extend_from_slice(d);
        b.push(0x03);
        self.raus.extend(b);
    }

    fn antworten(&mut self, t: u8, d: &[u8]) {
        match (self.phase, t) {
            // Identifikation: zwei Textblöcke, dann ACK
            (1, titel::ACK) => {
                self.phase = 2;
                self.block(titel::TEXT, b"MOTRONIC M5.9   ");
            }
            (2, titel::ACK) => {
                self.phase = 3;
                self.block(titel::ACK, &[]);
            }
            (3, titel::FEHLER_LESEN) => {
                self.phase = 4;
                self.block(titel::FEHLER_ANTWORT, &[0x02, 0x26, 0x23]); // 00550, Fehlerart 35
            }
            (3, titel::GRUPPE_LESEN) => {
                self.phase = 4;
                let _ = d;
                self.block(titel::GRUPPE_ANTWORT, &[0x01, 0xC8, 0x14, 0x05, 0x0A, 0x96, 0x15, 0x96, 0x89, 0x10, 0x00, 0x00]);
            }
            (4, titel::ACK) => {
                self.phase = 3;
                self.block(titel::ACK, &[]);
            }
            (_, titel::ENDE) => self.phase = 9,
            _ => self.block(titel::NAK, &[]),
        }
    }
}

impl Leitung for DemoLeitung {
    fn senden(&mut self, b: u8) -> Result<()> {
        if let Some(q) = self.erwartet_quittung.take() {
            // Quittung des Testers für ein Byte des Steuergeräts
            if b != !q {
                bail!("Demo: falsche Quittung");
            }
            return Ok(());
        }
        if self.phase == 0 {
            // Komplement des Schlüsselworts → erster Identifikationsblock
            self.phase = 1;
            self.block(titel::TEXT, b"036906032   ");
            return Ok(());
        }
        // Tester sendet einen Block: jedes Byte außer 03 quittieren
        if b == 0x03 && self.rein.len() >= 3 && self.rein.len() == usize::from(self.rein[0]) {
            let t = self.rein[2];
            let d = self.rein[3..].to_vec();
            self.zaehler = self.rein[1];
            self.rein.clear();
            self.antworten(t, &d);
            return Ok(());
        }
        self.rein.push(b);
        self.raus.push_front(!b);
        Ok(())
    }

    fn empfangen(&mut self, _timeout: Duration) -> Result<u8> {
        let b = self.raus.pop_front().ok_or_else(|| anyhow::anyhow!("Demo: nichts zu senden"))?;
        // Bytes des Steuergeräts (nicht Quittungen, nicht Ende) will es quittiert haben
        let ist_quittung = !self.rein.is_empty() || self.erwartet_quittung.is_some();
        if !ist_quittung && self.phase > 0 {
            let im_block_ende = b == 0x03 && self.raus.is_empty();
            if !im_block_ende {
                self.erwartet_quittung = Some(b);
            }
        }
        Ok(b)
    }

    fn fuenf_baud(&mut self, adresse: u8) -> Result<()> {
        if adresse != 0x01 {
            bail!("Demo: nur Motor (0x01)");
        }
        self.raus.extend([0x55, 0x01, 0x8A]);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuenf_baud_kodierung() {
        // 0x01: Start 0, Daten 1000000, Parität (eine 1 → ungerade → 0), Stopp 1
        assert_eq!(fuenf_baud_bits(0x01), vec![false, true, false, false, false, false, false, false, false, true]);
        // 0x03: zwei Einsen → Paritätsbit 1
        assert!(fuenf_baud_bits(0x03)[8]);
        assert_eq!(fuenf_baud_bits(0x17).len(), 10);
    }

    #[test]
    fn demo_motor_ident_fehler_gruppe() {
        let mut k = Kw1281::verbinden(DemoLeitung::default(), 0x01).unwrap();
        assert_eq!(k.ident, vec!["036906032", "MOTRONIC M5.9"]);
        let f = k.fehler_lesen().unwrap();
        assert_eq!(f, vec![(550, 0x23)]);
        let g = k.gruppe_lesen(1).unwrap();
        assert_eq!(g.len(), 4);
        assert_eq!(g[0].decode().map(|x| x.0.round()), Some(800.0), "Drehzahl 0.2·200·20");
        k.beenden();
    }

    #[test]
    fn fehlerart_text() {
        assert!(fehlerart(0x23).starts_with("035") || fehlerart(0x23).contains("035"));
        assert!(fehlerart(0xA3).contains("sporadisch"));
    }
}
