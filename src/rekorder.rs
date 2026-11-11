//! Fahrt-Rekorder: Messwertblöcke des Motors (TP 2.0, Adresse 01) laufen
//! während der Fahrt in einen Ringpuffer. Passiert etwas — neuer Fehler im
//! Motorsteuergerät, ein Wert verlässt seinen Bereich oder du drückst
//! „Merken“ —, werden die letzten Sekunden davor und ein paar danach als CSV
//! gespeichert (`…/fahrzeuge/<VIN>/rekorder/`). So sieht man, was beim Fehler
//! los war, obwohl der Golf 6 keine Umgebungsdaten speichert.
//!
//! Nur lesen.

use crate::kwp::{self, BlockValue, Kwp};
use crate::motorcheck::Feld;
use crate::transport::Backend;
use crate::tr;
use anyhow::{bail, Context, Result};
use std::collections::{BTreeSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Eine Messung: Zeit (s seit Start) und alle Werte in Spaltenreihenfolge.
#[derive(Debug, Clone, PartialEq)]
pub struct Zeile {
    pub t: f64,
    pub werte: Vec<Option<f64>>,
}

/// Was die Aufnahme ausgelöst hat.
#[derive(Debug, Clone, PartialEq)]
pub enum Ausloeser {
    Manuell,
    NeuerFehler(String),
    Grenze(String),
}

impl Ausloeser {
    pub fn text(&self) -> String {
        match self {
            Ausloeser::Manuell => tr!("von Hand gemerkt"),
            Ausloeser::NeuerFehler(c) => tr!("neuer Fehler {a}", a = c),
            Ausloeser::Grenze(t) => tr!("Grenze: {a}", a = t),
        }
    }

    fn datei(&self) -> String {
        match self {
            Ausloeser::Manuell => "manuell".into(),
            Ausloeser::NeuerFehler(c) => format!("fehler-{c}"),
            Ausloeser::Grenze(_) => "grenze".into(),
        }
    }
}

/// Grenzwert für ein Feld: „003/4>40“ oder „001/2<70“.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grenze {
    pub feld: Feld,
    pub groesser: bool,
    pub wert: f64,
}

impl Grenze {
    pub fn parse(s: &str) -> Option<Grenze> {
        let (i, groesser) = s.find('>').map(|i| (i, true)).or_else(|| s.find('<').map(|i| (i, false)))?;
        let feld = Feld::parse(&s[..i])?;
        let wert = s[i + 1..].trim().replace(',', ".").parse().ok()?;
        Some(Grenze { feld, groesser, wert })
    }

    pub fn text(&self) -> String {
        format!("{:03}/{} {} {}", self.feld.block, self.feld.feld, if self.groesser { ">" } else { "<" }, crate::csvlog::num(self.wert))
    }

    fn verletzt(&self, v: f64) -> bool {
        if self.groesser {
            v > self.wert
        } else {
            v < self.wert
        }
    }
}

/// Ein fertiges Ereignis mit den Messungen davor und danach.
#[derive(Debug, Clone, PartialEq)]
pub struct Ereignis {
    pub ausloeser: Ausloeser,
    /// Zeitpunkt des Auslösens (s seit Start)
    pub t: f64,
    pub zeilen: Vec<Zeile>,
}

/// Ringpuffer mit Auslöser-Logik (ohne Hardware, testbar).
pub struct Puffer {
    pub vorlauf: f64,
    pub nachlauf: f64,
    puffer: VecDeque<Zeile>,
    offen: Option<Ereignis>,
}

impl Puffer {
    pub fn new(vorlauf: f64, nachlauf: f64) -> Self {
        Self { vorlauf, nachlauf, puffer: VecDeque::new(), offen: None }
    }

    /// Ereignis auslösen (ein schon laufendes geht vor).
    pub fn ausloesen(&mut self, a: Ausloeser, t: f64) {
        if self.offen.is_none() {
            self.offen = Some(Ereignis { ausloeser: a, t, zeilen: self.puffer.iter().cloned().collect() });
        }
    }

    pub fn laeuft(&self) -> bool {
        self.offen.is_some()
    }

    /// Neue Messung; liefert ein Ereignis, sobald der Nachlauf um ist.
    pub fn push(&mut self, z: Zeile) -> Option<Ereignis> {
        let t = z.t;
        self.puffer.push_back(z.clone());
        while self.puffer.front().is_some_and(|f| f.t < t - self.vorlauf) {
            self.puffer.pop_front();
        }
        if let Some(e) = &mut self.offen {
            e.zeilen.push(z);
            if t >= e.t + self.nachlauf {
                return self.offen.take();
            }
        }
        None
    }
}

/// Spaltennamen: „Block/Feld Name“ (eigener Text, sonst Art).
pub fn spalten(bloecke: &[(u8, Vec<BlockValue>)]) -> Vec<(Feld, String)> {
    let texte = crate::messwerttexte::Texte::laden();
    let mut v = Vec::new();
    for (b, werte) in bloecke {
        for (i, w) in werte.iter().enumerate() {
            if w.is_leer() {
                continue;
            }
            let feld = i as u8 + 1;
            let einheit = w.decode().map(|x| x.1).filter(|u| !u.is_empty()).map(|u| format!(" [{u}]")).unwrap_or_default();
            v.push((Feld { block: *b, feld }, format!("{b:03}/{feld} {}{einheit}", texte.feld(0x01, *b, feld, w))));
        }
    }
    v
}

/// Ereignis als CSV (Trennzeichen ;, Dezimalkomma). Zeit relativ zum Auslösen.
pub fn csv(e: &Ereignis, spalten: &[(Feld, String)]) -> String {
    let mut o = format!("# ECUWerkbank Fahrt-Rekorder: {}\n", e.ausloeser.text());
    o += "t [s]";
    for (_, n) in spalten {
        o += ";";
        o += &n.replace(';', ",");
    }
    o += "\n";
    for z in &e.zeilen {
        o += &crate::csvlog::num(z.t - e.t);
        for w in &z.werte {
            o += ";";
            if let Some(x) = w {
                o += &crate::csvlog::num(*x);
            }
        }
        o += "\n";
    }
    o
}

/// Meldungen an die Oberfläche.
#[derive(Debug, Clone)]
pub enum Meldung {
    /// Läuft: Anzahl Messungen, Spaltennamen beim ersten Mal
    Laeuft { messungen: usize, spalten: Option<Vec<String>>, letzte: Vec<Option<f64>> },
    Ausgeloest(String),
    Gespeichert(std::path::PathBuf, String),
}

/// Rekorder laufen lassen, bis `stop` gesetzt wird. `merken` = Auslöser von
/// Hand (wird zurückgesetzt). Neue Fehler werden alle 15 s geprüft.
#[allow(clippy::too_many_arguments)]
pub fn laufen(
    be: &Backend,
    bloecke: &[u8],
    grenzen: &[Grenze],
    vorlauf: f64,
    nachlauf: f64,
    ordner: &std::path::Path,
    stop: &AtomicBool,
    merken: &AtomicBool,
    mut melden: impl FnMut(Meldung),
) -> Result<()> {
    if bloecke.is_empty() {
        bail!(tr!("Keine Blöcke gewählt."));
    }
    let mut ch = kwp::open_session(be, 0x01)?;
    let lesen = |ch: &mut crate::tp20::Channel| -> Result<Vec<(u8, Vec<BlockValue>)>> {
        bloecke.iter().map(|&b| Ok((b, Kwp::new(ch).read_block(b).with_context(|| tr!("Block {a}", a = b))?))).collect()
    };
    let erste = lesen(&mut ch)?;
    let sp = spalten(&erste);
    let codes = |ch: &mut crate::tp20::Channel| -> BTreeSet<String> {
        Kwp::new(ch).read_dtcs().unwrap_or_default().iter().map(|d| d.obd_code(Some(0x01)).unwrap_or_else(|| d.vag_code())).collect()
    };
    let mut bekannt = codes(&mut ch);
    let mut puf = Puffer::new(vorlauf, nachlauf);
    let start = Instant::now();
    let mut letzte_dtc = Instant::now();
    let mut n = 0usize;
    let mut erste_meldung = true;
    let mut daten = Some(erste);
    while !stop.load(Ordering::Relaxed) {
        let bl = match daten.take() {
            Some(d) => d,
            None => lesen(&mut ch)?,
        };
        let t = start.elapsed().as_secs_f64();
        let werte: Vec<Option<f64>> = sp
            .iter()
            .map(|(f, _)| bl.iter().find(|(b, _)| *b == f.block).and_then(|(_, w)| f.wert(w)).map(|x| x.0))
            .collect();
        // Grenzen
        if !puf.laeuft() {
            for g in grenzen {
                let v = sp.iter().position(|(f, _)| *f == g.feld).and_then(|i| werte[i]);
                if v.is_some_and(|x| g.verletzt(x)) {
                    puf.ausloesen(Ausloeser::Grenze(g.text()), t);
                    melden(Meldung::Ausgeloest(Ausloeser::Grenze(g.text()).text()));
                    break;
                }
            }
        }
        if merken.swap(false, Ordering::Relaxed) {
            puf.ausloesen(Ausloeser::Manuell, t);
            melden(Meldung::Ausgeloest(Ausloeser::Manuell.text()));
        }
        // neue Fehler
        if letzte_dtc.elapsed() >= Duration::from_secs(15) {
            letzte_dtc = Instant::now();
            let jetzt = codes(&mut ch);
            if let Some(neu) = jetzt.difference(&bekannt).next().cloned() {
                puf.ausloesen(Ausloeser::NeuerFehler(neu.clone()), t);
                melden(Meldung::Ausgeloest(Ausloeser::NeuerFehler(neu).text()));
            }
            bekannt.extend(jetzt);
        }
        n += 1;
        if let Some(e) = puf.push(Zeile { t, werte: werte.clone() }) {
            let name = format!("{}-{}.csv", crate::report::unix_now(), e.ausloeser.datei());
            std::fs::create_dir_all(ordner)?;
            let p = ordner.join(name);
            std::fs::write(&p, csv(&e, &sp)).with_context(|| tr!("Schreiben: {a}", a = p.display()))?;
            melden(Meldung::Gespeichert(p, e.ausloeser.text()));
        }
        melden(Meldung::Laeuft { messungen: n, spalten: erste_meldung.then(|| sp.iter().map(|x| x.1.clone()).collect()), letzte: werte });
        erste_meldung = false;
        if !matches!(be, Backend::SocketCan { .. }) {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grenze_parsen() {
        let g = Grenze::parse("003/4>40").unwrap();
        assert_eq!((g.feld, g.groesser, g.wert), (Feld { block: 3, feld: 4 }, true, 40.0));
        assert!(g.verletzt(41.0) && !g.verletzt(39.0));
        assert_eq!(Grenze::parse("1/2<70,5").unwrap().wert, 70.5);
        assert_eq!(Grenze::parse("1/2=3"), None);
    }

    #[test]
    fn puffer_vor_und_nachlauf() {
        let mut p = Puffer::new(3.0, 2.0);
        let z = |t: f64| Zeile { t, werte: vec![Some(t)] };
        for i in 0..10 {
            assert!(p.push(z(i as f64 * 0.5)).is_none());
        }
        // t = 4.5 zuletzt; Auslösen bei 4.5 → Vorlauf 1.5..4.5
        p.ausloesen(Ausloeser::Manuell, 4.5);
        p.ausloesen(Ausloeser::Grenze("x".into()), 4.5); // zweites zählt nicht
        assert!(p.push(z(5.0)).is_none());
        assert!(p.push(z(6.0)).is_none());
        let e = p.push(z(6.5)).expect("Nachlauf um");
        assert_eq!(e.ausloeser, Ausloeser::Manuell);
        assert_eq!(e.zeilen.first().unwrap().t, 1.5);
        assert_eq!(e.zeilen.last().unwrap().t, 6.5);
        assert!(!p.laeuft());
        let c = csv(&e, &[(Feld { block: 1, feld: 1 }, "001/1 Drehzahl".into())]);
        assert!(c.contains("t [s];001/1 Drehzahl\n-3;1,5\n"), "{c}");
    }
}
