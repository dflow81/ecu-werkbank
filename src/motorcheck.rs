//! Motor-Checks über Messwertblöcke des Motorsteuergeräts (TP 2.0, Adresse 01):
//!
//! - **Steuerkette:** Phasenlage der Nockenwelle betriebswarm im Leerlauf,
//!   als Trend. Wird die Kette länger, wandert der Wert über Monate weg.
//!   Dazu die genormten Fehler P0016/P0017/P0011/P0014 (Kurbel-/Nockenwelle).
//! - **Ladedruck / Kraftstoffdruck:** Soll und Ist bei Volllast mitschreiben
//!   und vergleichen.
//!
//! Welcher Block und welches Feld es ist, unterscheidet sich je Software-Stand.
//! Du legst es einmal je Fahrzeug fest (Blockscan hilft), gespeichert in
//! `…/fahrzeuge/<VIN>/motorcheck.toml`.
//!
//! TODO(verify): Die Grenzen unten sind eigene, vorsichtige Schätzungen, keine
//! Herstellerwerte. Die Kette wird deshalb vor allem über die Veränderung
//! gegenüber der ersten Messung bewertet.

use crate::kwp::{self, BlockValue, Kwp};
use crate::limits::Stufe;
use crate::transport::Backend;
use crate::tr;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

const MOTOR: u8 = 0x01;

/// Ein Feld eines Messwertblocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Feld {
    pub block: u8,
    /// Feldnummer ab 1
    pub feld: u8,
}

impl Feld {
    pub fn text(&self) -> String {
        tr!("Block {a} Feld {b}", a = format!("{:03}", self.block), b = self.feld)
    }

    /// Umgerechneter Wert dieses Felds aus einem gelesenen Block.
    pub fn wert(&self, werte: &[BlockValue]) -> Option<(f64, &'static str)> {
        werte.get(usize::from(self.feld).checked_sub(1)?).filter(|v| !v.is_leer())?.decode()
    }

    /// „003/2“ oder „3/2“ → Feld
    pub fn parse(s: &str) -> Option<Feld> {
        let (b, f) = s.trim().split_once('/')?;
        let feld: u8 = f.trim().parse().ok()?;
        (1..=8).contains(&feld).then_some(())?;
        Some(Feld { block: b.trim().parse().ok()?, feld })
    }
}

/// Einstellungen je Fahrzeug. `[soll, ist]` bei den Drücken.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Einstellung {
    #[serde(default)]
    pub kette: Option<Feld>,
    #[serde(default)]
    pub ladedruck: Option<[Feld; 2]>,
    #[serde(default)]
    pub kraftstoff: Option<[Feld; 2]>,
}

fn pfad(vehicle: &str) -> std::path::PathBuf {
    crate::vehicle::dir(vehicle).join("motorcheck.toml")
}

pub fn laden(vehicle: &str) -> Einstellung {
    std::fs::read_to_string(pfad(vehicle)).ok().and_then(|t| toml::from_str(&t).ok()).unwrap_or_default()
}

pub fn speichern(vehicle: &str, e: &Einstellung) -> Result<()> {
    let p = pfad(vehicle);
    std::fs::create_dir_all(p.parent().expect("hat Eltern"))?;
    std::fs::write(&p, toml::to_string(e)?).with_context(|| tr!("Schreiben: {a}", a = p.display()))
}

/// Felder, deren Art zu einer Winkelangabe passt (Kandidaten für die
/// Phasenlage), aus einem Blockscan.
pub fn winkel_kandidaten(bloecke: &[(u8, Vec<BlockValue>)]) -> Vec<Feld> {
    bloecke
        .iter()
        .flat_map(|(b, w)| w.iter().enumerate().filter(|(_, v)| matches!(v.formula, 0x03 | 0x1B) && !v.is_leer()).map(move |(i, _)| Feld { block: *b, feld: i as u8 + 1 }))
        .collect()
}

/// Druck-Kandidaten (Formel 0x12, mbar) als Paare nebeneinander (Soll, Ist).
pub fn druck_kandidaten(bloecke: &[(u8, Vec<BlockValue>)]) -> Vec<[Feld; 2]> {
    let mut v = Vec::new();
    for (b, w) in bloecke {
        for i in 0..w.len().saturating_sub(1) {
            if w[i].formula == 0x12 && w[i + 1].formula == 0x12 {
                v.push([Feld { block: *b, feld: i as u8 + 1 }, Feld { block: *b, feld: i as u8 + 2 }]);
            }
        }
    }
    v
}

/// Nur Demo: passende Fahrsituation einstellen (Leerlauf warm bzw. Volllast).
pub fn demo_vorbereiten(be: &Backend, volllast: bool) {
    use crate::control::Muster;
    use crate::sim::{set_muster, Param};
    if !matches!(be, Backend::Demo) {
        return;
    }
    set_muster(Param::Kuehlmittel, Some(Muster::Halten(90.0)));
    if volllast {
        set_muster(Param::Drehzahl, Some(Muster::Rampe { von: 1500.0, bis: 5500.0, dauer: 6.0 }));
        set_muster(Param::Last, Some(Muster::Rampe { von: 20.0, bis: 100.0, dauer: 1.5 }));
    } else {
        set_muster(Param::Drehzahl, Some(Muster::Halten(780.0)));
    }
}

/// Nur Demo: Fahrsituation wieder freigeben.
pub fn demo_ende(be: &Backend) {
    if matches!(be, Backend::Demo) {
        crate::sim::clear_muster();
    }
}

// ---------------------------------------------------------------------------
// Steuerkette

/// Genormte Fehler, die auf Steuerzeiten/Kette hindeuten (Texte eigene Worte).
pub const KETTE_FEHLER: &[(&str, &str)] = &[
    ("P0016", crate::trn!("Kurbelwelle und Nockenwelle (Bank 1, Sensor A) passen nicht zusammen")),
    ("P0017", crate::trn!("Kurbelwelle und Nockenwelle (Bank 1, Sensor B) passen nicht zusammen")),
    ("P0011", crate::trn!("Nockenwelle (Bank 1, A) zu weit verstellt / Sollwert nicht erreicht")),
    ("P0014", crate::trn!("Nockenwelle (Bank 1, B) zu weit verstellt / Sollwert nicht erreicht")),
];

pub const TREND_KETTE: &str = "Steuerkette Phasenlage";

#[derive(Debug, Clone, PartialEq)]
pub struct KetteMessung {
    pub feld: Feld,
    /// Mittelwert der Phasenlage
    pub wert: f64,
    pub einheit: String,
    /// Spanne max − min der Proben
    pub streuung: f64,
    pub drehzahl: f64,
    pub kuehlmittel: Option<f64>,
    pub proben: usize,
    /// Gefundene Ketten-Fehler (Code, Text)
    pub fehler: Vec<(String, String)>,
}

/// Phasenlage messen: Motor betriebswarm im Leerlauf. Liest Block 1
/// (Drehzahl, Kühlmittel) und das gewählte Feld `proben`-mal.
pub fn kette_messen(be: &Backend, feld: Feld, proben: usize, mut fortschritt: impl FnMut(usize)) -> Result<KetteMessung> {
    let mut ch = kwp::open_session(be, MOTOR)?;
    let fehler: Vec<(String, String)> = Kwp::new(&mut ch)
        .read_dtcs()
        .unwrap_or_default()
        .iter()
        .filter_map(|d| d.obd_code(Some(MOTOR)))
        .filter_map(|c| KETTE_FEHLER.iter().find(|(k, _)| *k == c).map(|(k, t)| (k.to_string(), crate::i18n::t(t))))
        .collect();
    let (mut werte, mut rpm, mut kuehl) = (Vec::new(), Vec::new(), Vec::new());
    let mut einheit = "";
    for i in 0..proben.max(1) {
        fortschritt(i);
        let b1 = Kwp::new(&mut ch).read_block(1)?;
        if let Some((r, _)) = b1.first().filter(|v| v.formula == 0x01).and_then(BlockValue::decode) {
            rpm.push(r);
        }
        if let Some((t, _)) = b1.get(1).filter(|v| v.formula == 0x05).and_then(BlockValue::decode) {
            kuehl.push(t);
        }
        let b = if feld.block == 1 { b1 } else { Kwp::new(&mut ch).read_block(feld.block)? };
        let (w, e) = feld.wert(&b).with_context(|| tr!("{a}: kein Zahlenwert (Formel unbekannt oder Feld leer)", a = feld.text()))?;
        werte.push(w);
        einheit = e;
        if matches!(be, Backend::SocketCan { .. }) {
            std::thread::sleep(Duration::from_millis(250));
        }
    }
    let mittel = |v: &[f64]| if v.is_empty() { None } else { Some(v.iter().sum::<f64>() / v.len() as f64) };
    let drehzahl = mittel(&rpm).unwrap_or(0.0);
    if !(450.0..=1300.0).contains(&drehzahl) {
        bail!(tr!("Motor muss im Leerlauf laufen (gemessen {a} 1/min).", a = format!("{drehzahl:.0}")));
    }
    let max = werte.iter().cloned().fold(f64::MIN, f64::max);
    let min = werte.iter().cloned().fold(f64::MAX, f64::min);
    Ok(KetteMessung { feld, wert: mittel(&werte).expect("≥1 Probe"), einheit: einheit.to_string(), streuung: max - min, drehzahl, kuehlmittel: mittel(&kuehl), proben: werte.len(), fehler })
}

/// Bewertung: Fehler, Bedingungen und Veränderung gegenüber der ersten Messung.
/// `frueher` = bisherige Trendwerte (älteste zuerst), ohne diese Messung.
pub fn kette_bewerten(m: &KetteMessung, frueher: &[f64]) -> Vec<(Stufe, String)> {
    let mut out = Vec::new();
    for (code, text) in &m.fehler {
        out.push((Stufe::Warnung, tr!("{a}: {b} — typisch bei gelängter Steuerkette oder Nockenwellenversteller", a = code, b = text)));
    }
    if m.kuehlmittel.is_some_and(|t| t < 80.0) {
        out.push((Stufe::Hinweis, tr!("Motor noch nicht warm ({a} °C) — der Wert ist kalt nicht vergleichbar, warm wiederholen", a = format!("{:.0}", m.kuehlmittel.unwrap_or(0.0)))));
    }
    if m.streuung > 3.0 {
        out.push((Stufe::Hinweis, tr!("Wert schwankt stark ({a} {u}) — Leerlauf unruhig? Messung wiederholen", a = format!("{:.1}", m.streuung), u = m.einheit)));
    }
    match frueher.first() {
        None => out.push((Stufe::Gut, tr!("Erste Messung: {a} {u} — das ist ab jetzt der Bezugswert. Alle paar tausend km wiederholen.", a = format!("{:.1}", m.wert), u = m.einheit))),
        Some(erster) => {
            let d = m.wert - erster;
            let txt = tr!("{a} {u}, seit der ersten Messung {b} {u}", a = format!("{:.1}", m.wert), b = format!("{d:+.1}"), u = m.einheit);
            if d.abs() >= 4.0 {
                out.push((Stufe::Warnung, tr!("{a} — deutlich verändert, Steuerkette prüfen lassen", a = txt)));
            } else if d.abs() >= 2.0 {
                out.push((Stufe::Hinweis, tr!("{a} — verändert sich, weiter beobachten", a = txt)));
            } else {
                out.push((Stufe::Gut, tr!("{a} — unverändert", a = txt)));
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Ladedruck / Kraftstoffdruck

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Druck {
    Ladedruck,
    Kraftstoff,
}

impl Druck {
    pub fn name(self) -> String {
        match self {
            Druck::Ladedruck => tr!("Ladedruck"),
            Druck::Kraftstoff => tr!("Kraftstoffdruck"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Probe {
    /// Sekunden seit Start
    pub t: f64,
    pub drehzahl: Option<f64>,
    pub soll: f64,
    pub ist: f64,
}

/// Soll/Ist aufzeichnen, bis `sekunden` um sind oder `weiter` false liefert.
/// Die Drehzahl kommt aus einem Drehzahl-Feld im selben Block, falls vorhanden.
pub fn druck_aufzeichnen(be: &Backend, paar: [Feld; 2], sekunden: f64, mut weiter: impl FnMut(&Probe) -> bool) -> Result<(Vec<Probe>, String)> {
    let mut ch = kwp::open_session(be, MOTOR)?;
    let start = Instant::now();
    let mut proben = Vec::new();
    let mut einheit = String::new();
    while start.elapsed().as_secs_f64() < sekunden {
        let a = Kwp::new(&mut ch).read_block(paar[0].block)?;
        let b = if paar[1].block == paar[0].block { a.clone() } else { Kwp::new(&mut ch).read_block(paar[1].block)? };
        let (Some((soll, e)), Some((ist, _))) = (paar[0].wert(&a), paar[1].wert(&b)) else {
            bail!(tr!("{a} / {b}: kein Zahlenwert", a = paar[0].text(), b = paar[1].text()));
        };
        einheit = e.to_string();
        let drehzahl = a.iter().find(|v| v.formula == 0x01).and_then(BlockValue::decode).map(|x| x.0);
        let p = Probe { t: start.elapsed().as_secs_f64(), drehzahl, soll, ist };
        proben.push(p);
        if !weiter(&p) {
            break;
        }
        if !matches!(be, Backend::SocketCan { .. }) {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    Ok((proben, einheit))
}

/// Ergebnis der Volllast-Prüfung.
#[derive(Debug, Clone, PartialEq)]
pub struct DruckErgebnis {
    pub max_soll: f64,
    pub max_ist: f64,
    /// größte Lücke Soll − Ist im Volllastbereich (positiv = zu wenig)
    pub luecke: f64,
    /// Sekunden, in denen die Lücke über der Warnschwelle lag
    pub dauer_zu_wenig: f64,
    /// größter Überschuss Ist − Soll
    pub ueberschuss: f64,
    /// Proben im Volllastbereich
    pub volllast: usize,
}

/// Auswerten: Volllastbereich = Soll über Grundwert + 60 % der Spanne, ohne
/// die erste Sekunde (Ansprechzeit des Turbos bzw. der Pumpe).
/// Schwellen (TODO(verify), eigene Schätzung): Ladedruck 100/200 mbar,
/// Kraftstoffdruck 7 %/15 % unter Soll.
pub fn druck_bewerten(art: Druck, proben: &[Probe], einheit: &str) -> (Option<DruckErgebnis>, Vec<(Stufe, String)>) {
    let mut out = Vec::new();
    if proben.len() < 5 {
        out.push((Stufe::Hinweis, tr!("Zu wenige Messwerte — länger aufzeichnen.")));
        return (None, out);
    }
    let min_soll = proben.iter().map(|p| p.soll).fold(f64::MAX, f64::min);
    let max_soll = proben.iter().map(|p| p.soll).fold(f64::MIN, f64::max);
    let max_ist = proben.iter().map(|p| p.ist).fold(f64::MIN, f64::max);
    let grenze = min_soll + 0.6 * (max_soll - min_soll);
    let spanne_ok = match art {
        Druck::Ladedruck => max_soll - min_soll >= 300.0,
        Druck::Kraftstoff => max_soll - min_soll >= 0.2 * max_soll.max(1.0),
    };
    if !spanne_ok {
        out.push((Stufe::Hinweis, tr!("Kein Volllastbereich erreicht — im 3. Gang ab ca. 1500 1/min voll beschleunigen.")));
        return (None, out);
    }
    let schwelle = |soll: f64| -> (f64, f64) {
        match art {
            Druck::Ladedruck => (100.0, 200.0),
            Druck::Kraftstoff => (0.07 * soll, 0.15 * soll),
        }
    };
    let mut luecke = f64::MIN;
    let mut ueberschuss = f64::MIN;
    let mut dauer = 0.0;
    let mut volllast = 0;
    // Ansprechzeit: die erste Sekunde nach Beginn der Volllast hinkt Ist normal hinterher
    let mut beginn: Option<f64> = None;
    for w in proben.windows(2) {
        let p = w[1];
        if p.soll < grenze {
            beginn = None;
            continue;
        }
        let b = *beginn.get_or_insert(p.t);
        if p.t - b < 1.0 {
            continue;
        }
        volllast += 1;
        let d = p.soll - p.ist;
        luecke = luecke.max(d);
        ueberschuss = ueberschuss.max(-d);
        if d > schwelle(p.soll).1 {
            dauer += p.t - w[0].t;
        }
    }
    let e = DruckErgebnis { max_soll, max_ist, luecke: luecke.max(0.0), dauer_zu_wenig: dauer, ueberschuss: ueberschuss.max(0.0), volllast };
    let n = |x: f64| if einheit == "mbar" { format!("{x:.0} {einheit}") } else { format!("{x:.1} {einheit}") };
    let (s1, s2) = schwelle(max_soll);
    let name = art.name();
    if e.dauer_zu_wenig >= 1.0 {
        out.push((Stufe::Warnung, tr!("{a} bleibt deutlich unter Soll (bis {b} zu wenig, {c} s lang)", a = name, b = n(e.luecke), c = format!("{:.1}", e.dauer_zu_wenig))));
        out.push((Stufe::Hinweis, match art {
            Druck::Ladedruck => tr!("Mögliche Ursachen: Undichtigkeit im Ladeluftweg (Schläuche, Ladeluftkühler), Wastegate/Schubumluftventil, Turbolader."),
            Druck::Kraftstoff => tr!("Mögliche Ursachen: Hochdruckpumpe (Nocken/Stößel), Niederdruck zu gering, Druckregelventil."),
        }));
    } else if e.luecke > s1 {
        out.push((Stufe::Hinweis, tr!("{a} kurz unter Soll (bis {b}) — knapp, beobachten", a = name, b = n(e.luecke))));
    } else {
        out.push((Stufe::Gut, tr!("{a} folgt dem Soll (max. Soll {b}, max. Ist {c})", a = name, b = n(e.max_soll), c = n(e.max_ist))));
    }
    if e.ueberschuss > s2 {
        out.push((Stufe::Hinweis, tr!("{a} schießt über Soll (bis {b} zu viel) — Regelung/Wastegate prüfen", a = name, b = n(e.ueberschuss))));
    }
    (Some(e), out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(formula: u8, a: u8, b: u8) -> BlockValue {
        BlockValue { formula, a, b }
    }

    #[test]
    fn feld_und_kandidaten() {
        assert_eq!(Feld::parse("093/2"), Some(Feld { block: 93, feld: 2 }));
        assert_eq!(Feld::parse("115/9"), None);
        let bloecke = vec![(93u8, vec![v(0x01, 200, 20), v(0x1B, 75, 120)]), (115, vec![v(0x01, 200, 20), v(0x21, 133, 50), v(0x12, 250, 100), v(0x12, 250, 98)])];
        assert_eq!(winkel_kandidaten(&bloecke), vec![Feld { block: 93, feld: 2 }]);
        assert_eq!(druck_kandidaten(&bloecke), vec![[Feld { block: 115, feld: 3 }, Feld { block: 115, feld: 4 }]]);
    }

    #[test]
    fn kette_trend() {
        let m = KetteMessung { feld: Feld { block: 93, feld: 2 }, wert: -3.0, einheit: "°".into(), streuung: 0.5, drehzahl: 750.0, kuehlmittel: Some(90.0), proben: 10, fehler: vec![] };
        assert_eq!(kette_bewerten(&m, &[])[0].0, Stufe::Gut);
        assert_eq!(kette_bewerten(&m, &[-2.5])[0].0, Stufe::Gut);
        assert_eq!(kette_bewerten(&m, &[-0.5])[0].0, Stufe::Hinweis);
        assert_eq!(kette_bewerten(&m, &[1.5])[0].0, Stufe::Warnung);
        let mut f = m.clone();
        f.fehler.push(("P0016".into(), "x".into()));
        assert_eq!(kette_bewerten(&f, &[])[0].0, Stufe::Warnung);
    }

    #[test]
    fn ladedruck_bewertung() {
        // 10 s Vollgas: Soll steigt auf 2000 mbar, Ist folgt (gut) bzw. bleibt bei 1700 (schlecht)
        let lauf = |verlust: f64| -> Vec<Probe> {
            (0..100)
                .map(|i| {
                    let t = i as f64 * 0.1;
                    let soll = if t < 1.0 { 1000.0 } else { 2000.0 };
                    let ist = if t < 1.5 { 1000.0 + (t * 600.0).min(1000.0) } else { soll - verlust };
                    Probe { t, drehzahl: Some(2000.0 + t * 300.0), soll, ist }
                })
                .collect()
        };
        let (e, st) = druck_bewerten(Druck::Ladedruck, &lauf(30.0), "mbar");
        assert_eq!(st[0].0, Stufe::Gut, "{st:?}");
        assert!(e.unwrap().volllast > 50);
        let (_, st) = druck_bewerten(Druck::Ladedruck, &lauf(300.0), "mbar");
        assert_eq!(st[0].0, Stufe::Warnung, "{st:?}");
        // Leerlauf ohne Volllast
        let leer: Vec<Probe> = (0..20).map(|i| Probe { t: i as f64 * 0.1, drehzahl: None, soll: 1000.0, ist: 990.0 }).collect();
        assert_eq!(druck_bewerten(Druck::Ladedruck, &leer, "mbar").0, None);
    }
}
