//! Drehzahl (oder andere Werte) in einem Bereich halten oder schwanken lassen.
//!
//! - [`Muster`]: Sollverlauf — halten, schwanken (Sinus), Rampe. Wird vom
//!   Fahrer-Assistenten (du hältst mit dem Gaspedal), von der Demo-Simulation
//!   und vom Steuergerät-Regler genutzt.
//! - [`Stellfunktion`]: eine Werkstattfunktion des Steuergeräts, z. B.
//!   „Leerlaufdrehzahl anheben“, als UDS-Stellglieddiagnose (0x2F) oder
//!   Routine (0x31). Die Kennungen sind steuergerätespezifisch und kommen aus
//!   deinen Labels (`[[stellfunktion]]`) — ermittelt aus einem Mitschnitt.
//! - [`Regler`]: führt eine Stellfunktion mit Sicherheitsgrenzen aus.
//!
//! **Sicherheit:** Nur im Stand. Abbruch bei Geschwindigkeit > 0, Überschreiten
//! der Grenze, Kommunikationsfehler, Zeitlimit oder Not-Aus — danach wird die
//! Kontrolle immer ans Steuergerät zurückgegeben.

use crate::tr;
use crate::limits::Stufe;
use crate::transport::{Backend, Transport};
use crate::uds::Uds;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// Sollverlauf über die Zeit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Muster {
    /// Konstanter Wert (Mitte des Bereichs)
    Halten(f64),
    /// Sinus zwischen min und max mit Periode in Sekunden
    Schwanken { min: f64, max: f64, periode: f64 },
    /// Linear von → bis in `dauer` Sekunden, danach halten
    Rampe { von: f64, bis: f64, dauer: f64 },
}

impl Muster {
    /// Sollwert nach `t` Sekunden.
    pub fn ziel(&self, t: f64) -> f64 {
        match *self {
            Muster::Halten(v) => v,
            Muster::Schwanken { min, max, periode } => {
                let mid = (min + max) / 2.0;
                let amp = (max - min) / 2.0;
                mid + amp * (2.0 * std::f64::consts::PI * t / periode.max(0.5)).sin()
            }
            Muster::Rampe { von, bis, dauer } => {
                let f = (t / dauer.max(0.1)).clamp(0.0, 1.0);
                von + (bis - von) * f
            }
        }
    }

    /// Text, z. B. "schwanken 2000–3000 (10 s)".
    pub fn text(&self) -> String {
        let n = crate::csvlog::num;
        match *self {
            Muster::Halten(v) => format!("halten {}", n(v)),
            Muster::Schwanken { min, max, periode } => tr!("schwanken {a}–{b} ({c} s)", a = n(min), b = n(max), c = n(periode)),
            Muster::Rampe { von, bis, dauer } => tr!("Rampe {a}→{b} ({c} s)", a = n(von), b = n(bis), c = n(dauer)),
        }
    }

    /// "halten:2500", "schwanken:2000:3000:10", "rampe:800:3000:20"
    pub fn parse(s: &str) -> Result<Self> {
        let p: Vec<&str> = s.split(':').map(str::trim).collect();
        let f = |i: usize| -> Result<f64> {
            p.get(i).with_context(|| tr!("'{s}': Wert {i} fehlt", s = s, i = i))?.replace(',', ".").parse::<f64>().with_context(|| tr!("'{s}': keine Zahl", s = s))
        };
        Ok(match p[0].to_lowercase().as_str() {
            "halten" | "hold" => Muster::Halten(f(1)?),
            "schwanken" | "sinus" => Muster::Schwanken { min: f(1)?, max: f(2)?, periode: f(3).unwrap_or(10.0) },
            "rampe" => Muster::Rampe { von: f(1)?, bis: f(2)?, dauer: f(3).unwrap_or(20.0) },
            x => bail!(tr!("Unbekanntes Muster '{x}' (halten, schwanken, rampe)", x = x)),
        })
    }
}

/// Fahrer-Hinweis zum Zielbereich.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Richtung {
    Mehr,
    Weniger,
    Passt,
}

/// Wert gegen Ziel ± Toleranz bewerten.
pub fn bewerten(ist: f64, ziel: f64, toleranz: f64) -> (Richtung, Stufe) {
    let d = ist - ziel;
    if d.abs() <= toleranz {
        (Richtung::Passt, Stufe::Gut)
    } else {
        let s = if d.abs() <= 2.0 * toleranz { Stufe::Hinweis } else { Stufe::Warnung };
        (if d < 0.0 { Richtung::Mehr } else { Richtung::Weniger }, s)
    }
}

/// Werkstattfunktion eines Steuergeräts (aus den Labels).
///
/// ```toml
/// [[stellfunktion]]
/// name = "Leerlaufdrehzahl anheben"
/// art = "uds-io"        # uds-io (0x2F) | uds-routine (0x31)
/// tx = 0x7E0
/// rx = 0x7E8
/// id = 0x1234           # DID bzw. Routine-ID (aus Mitschnitt!)
/// bytes = 2             # Länge des Sollwerts
/// faktor = 1.0          # Rohwert = (Soll - offset) / faktor
/// offset = 0.0
/// min = 800.0           # Sicherheitsgrenzen für den Sollwert
/// max = 1500.0
/// rueckmeldung_pid = 0x0C   # OBD-PID für den Istwert (Drehzahl)
/// max_dauer_s = 300
/// ```
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Stellfunktion {
    pub name: String,
    pub art: String,
    pub tx: u16,
    pub rx: u16,
    pub id: u16,
    #[serde(default = "two")]
    pub bytes: u8,
    #[serde(default = "one")]
    pub faktor: f64,
    #[serde(default)]
    pub offset: f64,
    pub min: f64,
    pub max: f64,
    #[serde(default = "rpm_pid")]
    pub rueckmeldung_pid: u8,
    #[serde(default = "five_min")]
    pub max_dauer_s: u64,
    #[serde(default)]
    pub einheit: String,
}

fn two() -> u8 {
    2
}
fn one() -> f64 {
    1.0
}
fn rpm_pid() -> u8 {
    0x0C
}
fn five_min() -> u64 {
    300
}

/// Stellfunktion des Demo-Fahrzeugs (DID 0xF1C0 = Leerlauf-Solldrehzahl, nur Simulation).
pub fn demo_stellfunktion() -> Stellfunktion {
    Stellfunktion {
        name: "Demo: Leerlaufdrehzahl anheben".into(),
        art: "uds-io".into(),
        tx: 0x7E0,
        rx: 0x7E8,
        id: crate::sim::DEMO_IDLE_DID,
        bytes: 2,
        faktor: 1.0,
        offset: 0.0,
        min: 700.0,
        max: 3500.0,
        rueckmeldung_pid: 0x0C,
        max_dauer_s: 300,
        einheit: "1/min".into(),
    }
}

impl Stellfunktion {
    fn encode(&self, soll: f64) -> Vec<u8> {
        let raw = ((soll - self.offset) / self.faktor.max(1e-9)).round().max(0.0) as u64;
        let n = self.bytes.clamp(1, 4) as usize;
        raw.to_be_bytes()[8 - n..].to_vec()
    }
}

/// Eine Regler-Messung.
#[derive(Debug, Clone, Copy)]
pub struct Messung {
    pub soll: f64,
    pub ist: Option<f64>,
    pub geschwindigkeit: Option<f64>,
}

/// Der Regler hat aus Sicherheitsgründen abgebrochen und die Kontrolle
/// zurückgegeben. Eigene Fehlerart, damit die Schleife das unabhängig von der
/// Sprache der Meldung erkennt: [`ist_abbruch`].
#[derive(Debug, Clone, PartialEq)]
pub struct Abbruch(pub String);

impl std::fmt::Display for Abbruch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Abbruch {}

/// Sicherheitsabbruch des Reglers (dann nicht wiederholen, sondern beenden)?
pub fn ist_abbruch(e: &anyhow::Error) -> bool {
    e.downcast_ref::<Abbruch>().is_some()
}

/// Führt eine Stellfunktion mit Sicherheitsgrenzen aus.
pub struct Regler {
    tp: Box<dyn Transport>,
    pub f: Stellfunktion,
    start: Instant,
    aktiv: bool,
    fehler_in_folge: u32,
}

impl Regler {
    /// Erweiterte Sitzung öffnen und prüfen, dass das Fahrzeug steht.
    pub fn start(be: &Backend, f: Stellfunktion) -> Result<Self> {
        if !matches!(f.art.as_str(), "uds-io" | "uds-routine") {
            bail!(tr!("Art '{a}' wird nicht unterstützt (uds-io, uds-routine)", a = f.art));
        }
        if f.min >= f.max {
            bail!(tr!("Sicherheitsgrenzen ungültig (min ≥ max)"));
        }
        let tp = be.open(f.tx, f.rx)?;
        {
            let uds = Uds::new(&*tp);
            let speed = uds.request(0x01, &[0x0D]).ok().and_then(|r| r.get(1).copied());
            match speed {
                Some(0) => {}
                Some(v) => return Err(Abbruch(tr!("Abbruch: Fahrzeug fährt ({v} km/h) — nur im Stand erlaubt", v = v)).into()),
                None => return Err(Abbruch(tr!("Abbruch: Geschwindigkeit nicht lesbar — Stillstand kann nicht geprüft werden")).into()),
            }
            uds.diagnostic_session(crate::uds::SESSION_EXTENDED)
                .context(tr!("Erweiterte Sitzung abgelehnt — evtl. Zugriffsberechtigung nötig"))?;
        }
        Ok(Self { tp, f, start: Instant::now(), aktiv: true, fehler_in_folge: 0 })
    }

    pub fn laufzeit(&self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }

    /// Einen Sollwert senden und Ist-/Geschwindigkeitswert lesen (alle ~250 ms aufrufen).
    pub fn schritt(&mut self, soll: f64) -> Result<Messung> {
        if !self.aktiv {
            return Err(Abbruch(tr!("Regler ist gestoppt")).into());
        }
        if self.start.elapsed() > Duration::from_secs(self.f.max_dauer_s) {
            self.stop();
            return Err(Abbruch(tr!("Zeitlimit ({a} s) erreicht — Kontrolle zurückgegeben", a = self.f.max_dauer_s)).into());
        }
        let soll = soll.clamp(self.f.min, self.f.max);
        let res = (|| -> Result<Messung> {
            let uds = Uds::new(&*self.tp);
            let data = self.f.encode(soll);
            match self.f.art.as_str() {
                "uds-io" => {
                    uds.io_control(self.f.id, 0x03, &data)?;
                }
                _ => {
                    uds.routine_control(crate::uds::ROUTINE_START, self.f.id, &data)?;
                }
            }
            let o = crate::obd::Obd::new(&*self.tp);
            let ist = o.read_value(self.f.rueckmeldung_pid).ok();
            let geschwindigkeit = o.read_value(0x0D).ok();
            Ok(Messung { soll, ist, geschwindigkeit })
        })();
        match res {
            Ok(m) => {
                self.fehler_in_folge = 0;
                if m.geschwindigkeit.is_some_and(|v| v > 0.0) {
                    self.stop();
                    return Err(Abbruch(tr!("Abbruch: Fahrzeug bewegt sich — Kontrolle zurückgegeben")).into());
                }
                if let Some(ist) = m.ist {
                    let grenze = self.f.max + (self.f.max - self.f.min) * 0.15;
                    if ist > grenze {
                        self.stop();
                        return Err(Abbruch(tr!("Abbruch: Istwert {ist} über Sicherheitsgrenze {grenze}", ist = format!("{:.0}", ist), grenze = format!("{:.0}", grenze))).into());
                    }
                }
                Ok(m)
            }
            Err(e) => {
                self.fehler_in_folge += 1;
                if self.fehler_in_folge >= 3 {
                    self.stop();
                    return Err(e.context(Abbruch(tr!("3 Fehler in Folge — Abbruch, Kontrolle zurückgegeben"))));
                }
                Err(e)
            }
        }
    }

    /// Kontrolle ans Steuergerät zurückgeben (auch automatisch beim Beenden).
    pub fn stop(&mut self) {
        if !self.aktiv {
            return;
        }
        self.aktiv = false;
        let uds = Uds::new(&*self.tp);
        match self.f.art.as_str() {
            "uds-io" => {
                let _ = uds.io_control(self.f.id, 0x00, &[]);
            }
            _ => {
                let _ = uds.routine_control(crate::uds::ROUTINE_STOP, self.f.id, &[]);
            }
        }
        let _ = uds.diagnostic_session(crate::uds::SESSION_DEFAULT);
    }
}

impl Drop for Regler {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abbruch_unabhaengig_vom_text() {
        use anyhow::Context;
        let e: anyhow::Error = Abbruch("egal welche Sprache".into()).into();
        assert!(ist_abbruch(&e));
        // auch unter weiterem Kontext (z. B. „3 Fehler in Folge“ oder Aufrufer)
        let e2 = Err::<(), _>(anyhow::anyhow!("Timeout")).context(Abbruch("x".into())).context("außen").unwrap_err();
        assert!(ist_abbruch(&e2));
        assert!(!ist_abbruch(&anyhow::anyhow!("Abbruch im Text, aber keine Fehlerart")));
    }

    #[test]
    fn muster_values() {
        let s = Muster::Schwanken { min: 2000.0, max: 3000.0, periode: 10.0 };
        assert!((s.ziel(0.0) - 2500.0).abs() < 1e-6);
        assert!((s.ziel(2.5) - 3000.0).abs() < 1e-6);
        let r = Muster::Rampe { von: 800.0, bis: 3000.0, dauer: 20.0 };
        assert_eq!(r.ziel(10.0), 1900.0);
        assert_eq!(r.ziel(99.0), 3000.0);
        assert_eq!(Muster::parse("schwanken:2000:3000:8").unwrap(), Muster::Schwanken { min: 2000.0, max: 3000.0, periode: 8.0 });
        assert!(Muster::parse("quatsch:1").is_err());
    }

    #[test]
    fn direction() {
        assert_eq!(bewerten(2500.0, 2500.0, 100.0).0, Richtung::Passt);
        assert_eq!(bewerten(2300.0, 2500.0, 100.0), (Richtung::Mehr, Stufe::Hinweis));
        assert_eq!(bewerten(3000.0, 2500.0, 100.0), (Richtung::Weniger, Stufe::Warnung));
    }

    #[test]
    fn encode_raw() {
        let f = demo_stellfunktion();
        assert_eq!(f.encode(1200.0), vec![0x04, 0xB0]);
    }

    #[test]
    fn demo_regler_holds_and_releases() {
        let _g = crate::sim::TEST_LOCK.lock().unwrap();
        crate::sim::set_muster(crate::sim::Param::Geschwindigkeit, Some(Muster::Halten(0.0)));
        let mut r = Regler::start(&Backend::Demo, demo_stellfunktion()).unwrap();
        let mut last = None;
        for _ in 0..30 {
            last = r.schritt(1500.0).unwrap().ist;
            std::thread::sleep(Duration::from_millis(60));
        }
        assert!((last.unwrap() - 1500.0).abs() < 120.0, "{last:?}");
        r.stop();
        assert!(crate::sim::idle_target().is_none());
        // Fahrzeug fährt → Start verweigert
        crate::sim::set_muster(crate::sim::Param::Geschwindigkeit, Some(Muster::Halten(30.0)));
        assert!(Regler::start(&Backend::Demo, demo_stellfunktion()).is_err());
        crate::sim::clear_muster();
    }
}
