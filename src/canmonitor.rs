//! Live-CAN-Monitor: zeigt je CAN-ID die letzten Daten, Anzahl, Rate und
//! welche Bits sich ändern („Sniffer“).
//!
//! **Nur zuhören** — es wird nie etwas auf den Bus gesendet.
//!
//! Signale finden: Erst „Rauschen merken“ (alle Bits, die sich von selbst
//! ändern, z. B. Zähler und Prüfsummen, werden ab dann ignoriert), dann am
//! Auto etwas betätigen (Blinker, Fensterheber). Hervorgehoben werden danach
//! nur noch die Bits, die sich *dadurch* ändern, und IDs, die neu dazukommen.

use anyhow::Result;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Zustand einer CAN-ID.
#[derive(Debug, Clone, PartialEq)]
pub struct Eintrag {
    pub id: u32,
    pub extended: bool,
    pub daten: Vec<u8>,
    pub anzahl: u64,
    /// Zeitpunkt (s) des ersten und letzten Frames
    pub erste: f64,
    pub letzte: f64,
    /// gleitender Mittelwert des Abstands zwischen zwei Frames (s)
    abstand: f64,
    /// je Byte: Bits, die sich seit dem Start/„Rauschen merken“ geändert haben (ohne Rauschen)
    pub geaendert: Vec<u8>,
    /// je Byte: Zeitpunkt der letzten Änderung (ohne Rauschen), `None` = nie
    pub geaendert_zeit: Vec<Option<f64>>,
    /// je Byte: alle Bits, die sich je geändert haben (für „Rauschen merken“)
    alle_bits: Vec<u8>,
    /// je Byte: Bits, die als Rauschen gelten
    pub rauschen: Vec<u8>,
}

impl Eintrag {
    fn neu(id: u32, extended: bool, t: f64, daten: &[u8]) -> Self {
        let n = daten.len();
        Eintrag {
            id,
            extended,
            daten: daten.to_vec(),
            anzahl: 1,
            erste: t,
            letzte: t,
            abstand: 0.0,
            geaendert: vec![0; n],
            geaendert_zeit: vec![None; n],
            alle_bits: vec![0; n],
            rauschen: vec![0; n],
        }
    }

    fn frame(&mut self, t: f64, daten: &[u8]) {
        let dt = t - self.letzte;
        if dt > 0.0 {
            self.abstand = if self.abstand == 0.0 { dt } else { self.abstand * 0.9 + dt * 0.1 };
        }
        self.anzahl += 1;
        self.letzte = t;
        let n = daten.len().max(self.daten.len());
        for v in [&mut self.geaendert, &mut self.alle_bits, &mut self.rauschen] {
            v.resize(n, 0);
        }
        self.geaendert_zeit.resize(n, None);
        for i in 0..n {
            let alt = self.daten.get(i).copied().unwrap_or(0);
            let neu = daten.get(i).copied().unwrap_or(0);
            let diff = alt ^ neu;
            self.alle_bits[i] |= diff;
            let wichtig = diff & !self.rauschen[i];
            if wichtig != 0 {
                self.geaendert[i] |= wichtig;
                self.geaendert_zeit[i] = Some(t);
            }
        }
        self.daten = daten.to_vec();
    }

    /// Frames pro Sekunde (0, solange erst ein Frame da ist).
    pub fn rate_hz(&self) -> f64 {
        if self.abstand > 0.0 {
            1.0 / self.abstand
        } else {
            0.0
        }
    }

    /// Byte `i` hat sich innerhalb von `dauer` Sekunden vor `jetzt` geändert (ohne Rauschen).
    pub fn frisch(&self, i: usize, jetzt: f64, dauer: f64) -> bool {
        self.geaendert_zeit.get(i).copied().flatten().is_some_and(|z| jetzt - z <= dauer)
    }

    /// Daten als Hex, z. B. `01 A0 FF`.
    pub fn hex(&self) -> String {
        self.daten.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ")
    }
}

/// Alle IDs, die gesehen wurden.
#[derive(Debug, Clone, Default)]
pub struct Monitor {
    eintraege: BTreeMap<u32, Eintrag>,
    pub frames: u64,
    /// Zeitpunkt von „Rauschen merken“
    pub gemerkt: Option<f64>,
    /// Zeitpunkt des letzten Frames
    pub zuletzt: f64,
}

impl Monitor {
    pub fn frame(&mut self, t: f64, id: u32, extended: bool, daten: &[u8]) {
        self.frames += 1;
        self.zuletzt = self.zuletzt.max(t);
        match self.eintraege.get_mut(&id) {
            Some(e) => e.frame(t, daten),
            None => {
                self.eintraege.insert(id, Eintrag::neu(id, extended, t, daten));
            }
        }
    }

    /// Alle bisher wechselnden Bits gelten ab jetzt als Rauschen; Markierungen zurücksetzen.
    pub fn rauschen_merken(&mut self) {
        for e in self.eintraege.values_mut() {
            for (r, a) in e.rauschen.iter_mut().zip(&e.alle_bits) {
                *r |= a;
            }
            e.geaendert.iter_mut().for_each(|g| *g = 0);
            e.geaendert_zeit.iter_mut().for_each(|z| *z = None);
        }
        self.gemerkt = Some(self.zuletzt);
    }

    pub fn rauschen_vergessen(&mut self) {
        for e in self.eintraege.values_mut() {
            e.rauschen.iter_mut().for_each(|r| *r = 0);
        }
        self.gemerkt = None;
    }

    /// Ist die ID erst nach „Rauschen merken“ aufgetaucht?
    pub fn ist_neu(&self, e: &Eintrag) -> bool {
        self.gemerkt.is_some_and(|g| e.erste > g)
    }

    pub fn eintraege(&self) -> impl Iterator<Item = &Eintrag> {
        self.eintraege.values()
    }

    pub fn anzahl_ids(&self) -> usize {
        self.eintraege.len()
    }
}

/// Woher die Frames kommen.
#[derive(Debug, Clone, PartialEq)]
pub enum Quelle {
    /// SocketCAN-Interface, nur lesen
    SocketCan(String),
    /// erfundener Grundverkehr zum Ausprobieren (ohne Adapter)
    Demo,
}

/// Ein laufender Monitor: Lese-Thread, gemeinsamer Zustand, optionaler Mitschnitt.
pub struct Lauf {
    pub monitor: Arc<Mutex<Monitor>>,
    stopp: Arc<AtomicBool>,
    /// Demo: Blinker-Bit einschalten (zum Ausprobieren von „Rauschen merken“)
    pub demo_aktion: Arc<AtomicBool>,
    /// laufender Mitschnitt: Pfad, Schreiber, Anzahl Frames
    mitschnitt: Arc<Mutex<Option<(PathBuf, std::io::BufWriter<std::fs::File>, u64)>>>,
    /// letzter Fehler des Lese-Threads (z. B. Interface weg)
    pub fehler: Arc<Mutex<Option<String>>>,
}

impl Lauf {
    /// Startet den Lese-Thread. Sendet nie etwas.
    pub fn starten(quelle: Quelle) -> Result<Lauf> {
        let lauf = Lauf {
            monitor: Arc::default(),
            stopp: Arc::default(),
            demo_aktion: Arc::default(),
            mitschnitt: Arc::default(),
            fehler: Arc::default(),
        };
        let (m, stopp, aktion, mit, fehler) = (lauf.monitor.clone(), lauf.stopp.clone(), lauf.demo_aktion.clone(), lauf.mitschnitt.clone(), lauf.fehler.clone());
        let iface = match &quelle {
            Quelle::SocketCan(i) => i.clone(),
            Quelle::Demo => "demo".into(),
        };
        let eintragen = move |t: f64, id: u32, ext: bool, d: &[u8]| {
            if let Ok(mut g) = mit.lock() {
                if let Some((_, w, n)) = g.as_mut() {
                    let _ = writeln!(w, "{}", crate::canlog::format_line(t, &iface, id, d));
                    *n += 1;
                }
            }
            if let Ok(mut g) = m.lock() {
                g.frame(t, id, ext, d);
            }
        };
        match quelle {
            Quelle::Demo => {
                std::thread::Builder::new().name("can-monitor".into()).spawn(move || {
                    let start = Instant::now();
                    let mut demo = Demo::default();
                    while !stopp.load(Ordering::Relaxed) {
                        let t = start.elapsed().as_secs_f64();
                        for (id, d) in demo.takt(t, aktion.load(Ordering::Relaxed)) {
                            eintragen(unix_jetzt(), id, false, &d);
                        }
                        std::thread::sleep(Duration::from_millis(10));
                    }
                })?;
            }
            Quelle::SocketCan(iface) => {
                use crate::can::{CanSocket, EmbeddedFrame, Id, Socket};
                let sock = CanSocket::open(&iface).map_err(|e| anyhow::anyhow!(crate::tr!("CAN-Interface '{iface}' öffnen: {e}", iface = iface, e = e)))?;
                sock.set_read_timeout(Duration::from_millis(200))?;
                std::thread::Builder::new().name("can-monitor".into()).spawn(move || {
                    while !stopp.load(Ordering::Relaxed) {
                        match sock.read_frame() {
                            Ok(f) => {
                                let (id, ext) = match f.id() {
                                    Id::Standard(s) => (u32::from(s.as_raw()), false),
                                    Id::Extended(e) => (e.as_raw(), true),
                                };
                                eintragen(unix_jetzt(), id, ext, f.data());
                            }
                            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
                            Err(e) => {
                                if let Ok(mut g) = fehler.lock() {
                                    *g = Some(e.to_string());
                                }
                                std::thread::sleep(Duration::from_millis(500));
                            }
                        }
                    }
                })?;
            }
        }
        Ok(lauf)
    }

    /// Mitschnitt (candump-Format) starten; läuft, bis [`Lauf::mitschnitt_ende`].
    pub fn mitschnitt_start(&self, pfad: PathBuf) -> Result<()> {
        let w = std::io::BufWriter::new(std::fs::File::create(&pfad)?);
        *self.mitschnitt.lock().map_err(|_| anyhow::anyhow!("Sperre"))? = Some((pfad, w, 0));
        Ok(())
    }

    /// Mitschnitt beenden: (Pfad, Anzahl Frames).
    pub fn mitschnitt_ende(&self) -> Option<(PathBuf, u64)> {
        let (p, mut w, n) = self.mitschnitt.lock().ok()?.take()?;
        let _ = w.flush();
        Some((p, n))
    }

    pub fn schneidet_mit(&self) -> Option<u64> {
        self.mitschnitt.lock().ok()?.as_ref().map(|(_, _, n)| *n)
    }
}

impl Drop for Lauf {
    fn drop(&mut self) {
        self.stopp.store(true, Ordering::Relaxed);
        let _ = self.mitschnitt_ende();
    }
}

fn unix_jetzt() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

/// Erfundener Grundverkehr (keine echten Fahrzeugdaten): ein Zähler, der sich
/// ständig ändert (= Rauschen), Drehzahl, Geschwindigkeit und ein Blinker-Bit.
#[derive(Default)]
struct Demo {
    naechste: [f64; 4],
    zaehler: u8,
}

impl Demo {
    /// Frames, die bis `t` fällig sind. `blinker`: Blinker an (Bit 0x04 in Byte 1 der 0x3A0 blinkt).
    fn takt(&mut self, t: f64, blinker: bool) -> Vec<(u32, Vec<u8>)> {
        const TAKT: [(u32, f64); 4] = [(0x100, 0.02), (0x1A0, 0.05), (0x3A0, 0.1), (0x620, 1.0)];
        let mut out = Vec::new();
        for (i, (id, periode)) in TAKT.iter().enumerate() {
            if t < self.naechste[i] {
                continue;
            }
            self.naechste[i] = t + periode;
            let d = match id {
                0x100 => {
                    self.zaehler = (self.zaehler + 1) & 0x0F;
                    let rpm = (800.0 + 150.0 * (t * 0.7).sin()) as u16 * 4;
                    let [hi, lo] = rpm.to_be_bytes();
                    vec![self.zaehler, hi, lo, 0x00, 0x00, 0x00, 0x00, 0x00]
                }
                0x1A0 => vec![0x00, 0x00, (t * 3.0) as u8 % 50, 0x00, 0x10, 0x00, 0x00, 0x00],
                0x3A0 => {
                    let an = blinker && ((t * 1.5) as u64).is_multiple_of(2);
                    vec![0x00, if an { 0x04 } else { 0x00 }, 0x00, 0x00]
                }
                _ => vec![0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08],
            };
            out.push((*id, d));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_und_aenderungen() {
        let mut m = Monitor::default();
        for i in 0..11 {
            m.frame(i as f64 * 0.1, 0x100, false, &[i as u8, 0x55]);
        }
        let e = m.eintraege().next().unwrap();
        assert_eq!(e.anzahl, 11);
        assert!((e.rate_hz() - 10.0).abs() < 0.01, "{}", e.rate_hz());
        assert_eq!(e.geaendert[1], 0, "Byte 1 ist fest");
        assert_eq!(e.geaendert[0], 0x0F, "Zähler 0..10 ändert die unteren 4 Bits");
        assert!(e.frisch(0, 1.0, 0.5));
        assert!(!e.frisch(1, 1.0, 0.5));
    }

    #[test]
    fn rauschen_merken_zeigt_nur_neue_bits_und_ids() {
        let mut m = Monitor::default();
        // Zähler in Byte 0 läuft, Byte 1 fest
        for i in 0..20u8 {
            m.frame(f64::from(i) * 0.1, 0x100, false, &[i & 0x0F, 0x00]);
        }
        m.rauschen_merken();
        assert_eq!(m.eintraege().next().unwrap().geaendert, vec![0, 0]);
        // jetzt: Zähler läuft weiter (Rauschen), Byte 1 Bit 2 geht an (Aktion)
        for i in 20..30u8 {
            m.frame(f64::from(i) * 0.1, 0x100, false, &[i & 0x0F, 0x04]);
        }
        m.frame(3.0, 0x3A0, false, &[0x01]);
        let e = m.eintraege().find(|e| e.id == 0x100).unwrap();
        assert_eq!(e.geaendert, vec![0, 0x04], "nur das Aktions-Bit");
        assert!(!e.frisch(0, 3.0, 1.0), "Zähler gilt als Rauschen");
        let neu = m.eintraege().find(|e| e.id == 0x3A0).unwrap();
        assert!(m.ist_neu(neu));
        assert!(!m.ist_neu(e));
        m.rauschen_vergessen();
        m.frame(3.1, 0x100, false, &[0x0F, 0x04]);
        let e = m.eintraege().find(|e| e.id == 0x100).unwrap();
        assert_ne!(e.geaendert[0], 0, "ohne Rauschfilter zählt der Zähler wieder");
    }

    #[test]
    fn laenge_aendert_sich() {
        let mut m = Monitor::default();
        m.frame(0.0, 0x10, false, &[1]);
        m.frame(0.1, 0x10, false, &[1, 2, 3]);
        let e = m.eintraege().next().unwrap();
        assert_eq!(e.daten, vec![1, 2, 3]);
        assert_eq!(e.geaendert, vec![0, 2, 3]);
    }

    #[test]
    fn demo_lauf_mit_mitschnitt() {
        let dir = std::env::temp_dir().join(format!("canmon-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let lauf = Lauf::starten(Quelle::Demo).unwrap();
        lauf.mitschnitt_start(dir.join("m.log")).unwrap();
        lauf.demo_aktion.store(true, Ordering::Relaxed);
        std::thread::sleep(Duration::from_millis(1500));
        let (p, n) = lauf.mitschnitt_ende().unwrap();
        assert!(n > 20, "{n}");
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.lines().filter_map(crate::canlog::parse_line).any(|f| f.id == 0x3A0));
        let m = lauf.monitor.lock().unwrap();
        assert!(m.anzahl_ids() >= 4);
        let blinker = m.eintraege().find(|e| e.id == 0x3A0).unwrap();
        assert_eq!(blinker.geaendert[1], 0x04, "Blinker-Bit hat geblinkt");
        drop(m);
        drop(lauf);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
