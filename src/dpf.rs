//! Partikelfilter-Überwachung (Diesel): Rußbeladung, km seit Regeneration,
//! Differenzdruck, Abgastemperatur — mit Ampel und Fahrhinweisen.
//!
//! Die Messwert-Kennungen sind motorspezifisch. Sie stehen im Fahrzeug
//! (`[dpf]` in fahrzeug.toml) — ermittelt aus Labels oder per Lernhilfe aus
//! einem Mitschnitt. Das Demo-Fahrzeug hat eigene Demo-Kennungen.

use crate::tr;
use crate::limits::Stufe;
use crate::transport::Backend;
use crate::uds::Uds;
use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Eine Messwert-Quelle: UDS-DID am Steuergerät (Rohwert × faktor + offset).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Quelle {
    pub tx: u16,
    pub rx: u16,
    pub did: u16,
    #[serde(default = "one")]
    pub faktor: f64,
    #[serde(default)]
    pub offset: f64,
}

fn one() -> f64 {
    1.0
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Config {
    #[serde(default)]
    pub russ: Option<Quelle>,
    #[serde(default)]
    pub km_seit_regeneration: Option<Quelle>,
    #[serde(default)]
    pub differenzdruck: Option<Quelle>,
    #[serde(default)]
    pub abgastemperatur: Option<Quelle>,
    /// Beladung, bei der regeneriert wird (g) — Herstellerwert, Standard 24 g
    #[serde(default)]
    pub russ_grenze_g: Option<f64>,
}

/// Konfiguration des Demo-Fahrzeugs.
pub fn demo_config() -> Config {
    let q = |did: u16, f: f64| Some(Quelle { tx: 0x7E0, rx: 0x7E8, did, faktor: f, offset: 0.0 });
    Config { russ: q(0x1100, 0.01), km_seit_regeneration: q(0x1101, 1.0), differenzdruck: q(0x1102, 1.0), abgastemperatur: q(0x1103, 1.0), russ_grenze_g: Some(24.0) }
}

#[derive(Debug, Clone, Default)]
pub struct Werte {
    pub russ_g: Option<f64>,
    pub km: Option<f64>,
    pub diff_mbar: Option<f64>,
    pub temp_c: Option<f64>,
}

fn read_q(be: &Backend, q: &Option<Quelle>) -> Option<f64> {
    let q = q.as_ref()?;
    let tp = be.open(q.tx, q.rx).ok()?;
    let uds = Uds::new(&*tp);
    let d = uds.read_data_by_id(q.did).ok()?;
    Some(crate::raw_to_f64(&d) * q.faktor + q.offset)
}

pub fn read(be: &Backend, c: &Config) -> Result<Werte> {
    let w = Werte {
        russ_g: read_q(be, &c.russ),
        km: read_q(be, &c.km_seit_regeneration),
        diff_mbar: read_q(be, &c.differenzdruck),
        temp_c: read_q(be, &c.abgastemperatur),
    };
    if w.russ_g.is_none() && w.km.is_none() && w.diff_mbar.is_none() {
        anyhow::bail!(tr!("Keine Partikelfilter-Werte lesbar — Kennungen in der Fahrzeug-Konfiguration prüfen"));
    }
    Ok(w)
}

/// Bewertung und Fahrhinweise.
pub fn bewerten(w: &Werte, c: &Config) -> Vec<(Stufe, String)> {
    let grenze = c.russ_grenze_g.unwrap_or(24.0);
    let mut out = Vec::new();
    if let Some(r) = w.russ_g {
        let pct = r / grenze * 100.0;
        let (st, t) = if pct >= 110.0 {
            (Stufe::Warnung, crate::trs!("über der Regenerationsgrenze — bald Regenerationsfahrt, sonst Notlauf möglich"))
        } else if pct >= 80.0 {
            (Stufe::Hinweis, crate::trs!("Regeneration steht bald an"))
        } else {
            (Stufe::Gut, crate::trs!("in Ordnung"))
        };
        out.push((st, crate::tr!("Rußbeladung {r} g ({pct} % von {grenze} g): {t}", r = format!("{r:.1}"), pct = format!("{pct:.0}"), grenze = format!("{grenze:.0}"), t = t)));
    }
    if let Some(k) = w.km {
        let st = if k > 1000.0 { Stufe::Hinweis } else { Stufe::Gut };
        out.push((st, crate::tr!("{k} km seit der letzten Regeneration{a}", k = format!("{k:.0}"), a = if k > 1000.0 { crate::trs!(" — viel Kurzstrecke? Regeneration wird wohl abgebrochen") } else { "" })));
    }
    if let Some(d) = w.diff_mbar {
        let st = if d > 250.0 { Stufe::Warnung } else { Stufe::Gut };
        out.push((st, tr!("Differenzdruck {d} mbar{a}", d = format!("{:.0}", d), a = if d > 250.0 { crate::trs!(" — hoch: Filter stark beladen/Asche, Schläuche des Drucksensors prüfen") } else { "" })));
    }
    if let Some(t) = w.temp_c {
        out.push((Stufe::Gut, tr!("Abgastemperatur {t} °C{a}", t = format!("{:.0}", t), a = if t > 550.0 { crate::trs!(" (Regeneration läuft)") } else { "" })));
    }
    if out.iter().any(|(s, _)| *s != Stufe::Gut) {
        out.push((Stufe::Hinweis, tr!("Regenerationsfahrt: Motor warm, 15–20 min Landstraße/Autobahn mit 2000–3000 1/min ohne Stopp; Motor dabei nicht abstellen.")));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_values_and_rating() {
        let c = demo_config();
        let w = read(&Backend::Demo, &c).unwrap();
        assert!(w.russ_g.unwrap() > 10.0 && w.km == Some(612.0));
        let r = bewerten(&Werte { russ_g: Some(27.0), km: Some(1500.0), diff_mbar: Some(300.0), temp_c: None }, &c);
        assert!(r.iter().any(|(s, t)| *s == Stufe::Warnung && t.contains("Rußbeladung")));
        assert!(r.iter().any(|(_, t)| t.contains("Regenerationsfahrt")));
    }
}
