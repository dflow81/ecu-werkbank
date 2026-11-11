//! Langzeit-Trends je Fahrzeug: Messwerte aus Schnell-Check, Batterietest,
//! Gemisch-Assistent und Abgas-Prüfergebnissen über Monate verfolgen und
//! schleichende Verschlechterung melden.
//!
//! Ablage: `…/fahrzeuge/<VIN>/trends.toml`.

use crate::tr;
use crate::limits::Stufe;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Punkt {
    pub ts: u64,
    pub datum: String,
    #[serde(default)]
    pub km: Option<u32>,
    pub name: String,
    pub wert: f64,
    #[serde(default)]
    pub einheit: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Datei {
    #[serde(default)]
    punkt: Vec<Punkt>,
}

fn path(vehicle: &str) -> std::path::PathBuf {
    crate::vehicle::dir(vehicle).join("trends.toml")
}

pub fn load(vehicle: &str) -> Vec<Punkt> {
    std::fs::read_to_string(path(vehicle)).ok().and_then(|t| toml::from_str::<Datei>(&t).ok()).map(|d| d.punkt).unwrap_or_default()
}

/// Mehrere Werte mit gleichem Zeitstempel speichern: (Name, Wert, Einheit).
pub fn add(vehicle: &str, km: Option<u32>, werte: &[(String, f64, String)]) -> Result<()> {
    if werte.is_empty() || vehicle.is_empty() {
        return Ok(());
    }
    let mut d = Datei { punkt: load(vehicle) };
    let ts = crate::report::unix_now();
    let datum = crate::report::now_local();
    for (name, wert, einheit) in werte {
        if wert.is_finite() {
            d.punkt.push(Punkt { ts, datum: datum.clone(), km, name: name.clone(), wert: *wert, einheit: einheit.clone() });
        }
    }
    let p = path(vehicle);
    std::fs::create_dir_all(p.parent().expect("hat Eltern"))?;
    std::fs::write(&p, toml::to_string(&d)?).with_context(|| tr!("Schreiben: {a}", a = p.display()))
}

/// Verläufe je Messwert (zeitlich sortiert).
pub fn series(vehicle: &str) -> BTreeMap<String, Vec<Punkt>> {
    let mut m: BTreeMap<String, Vec<Punkt>> = BTreeMap::new();
    for p in load(vehicle) {
        m.entry(p.name.clone()).or_default().push(p);
    }
    for v in m.values_mut() {
        v.sort_by_key(|p| p.ts);
    }
    m
}

/// Steigung pro Messung (lineare Regression über die letzten Punkte).
fn slope(v: &[f64]) -> f64 {
    let n = v.len() as f64;
    if n < 2.0 {
        return 0.0;
    }
    let mx = (n - 1.0) / 2.0;
    let my = v.iter().sum::<f64>() / n;
    let (mut num, mut den) = (0.0, 0.0);
    for (i, y) in v.iter().enumerate() {
        let dx = i as f64 - mx;
        num += dx * (y - my);
        den += dx * dx;
    }
    if den == 0.0 { 0.0 } else { num / den }
}

/// Schleichende Veränderungen erkennen (mind. 3 Messungen je Wert).
pub fn analyze(s: &BTreeMap<String, Vec<Punkt>>) -> Vec<(Stufe, String)> {
    let mut out = Vec::new();
    for (name, pts) in s {
        let last: Vec<f64> = pts.iter().rev().take(6).rev().map(|p| p.wert).collect();
        if last.len() < 3 {
            continue;
        }
        let (first, now) = (last[0], *last.last().expect("≥3"));
        let k = slope(&last);
        let since = &pts[pts.len() - last.len()].datum;
        let n = crate::csvlog::num;
        let unit = pts.last().map(|p| p.einheit.as_str()).unwrap_or("");
        let msg = |what: &str| tr!("{name}: {what} (von {a} auf {b} {unit} seit {since})", name = name, what = what, a = n(first), b = n(now), unit = unit, since = since);
        if name.starts_with("Ruhespannung") || name.starts_with("Startspannung") {
            if now < first - 0.2 && k < 0.0 {
                out.push((Stufe::Hinweis, msg("sinkt — Batterie altert")));
            }
        } else if name.starts_with("Gemisch") {
            if now.abs() > first.abs() + 4.0 && k.abs() > 0.5 {
                out.push((Stufe::Hinweis, msg(crate::trs!("Korrektur wird größer — Ursache suchen, bevor ein Fehler kommt"))));
            }
        } else if name.starts_with("Aussetzer") {
            if now > first + 3.0 && k > 0.0 {
                out.push((Stufe::Hinweis, msg(crate::trs!("steigt — Zündkerze/Zündspule beobachten"))));
            }
        } else if name.starts_with("Reserve") {
            if now < first - 15.0 && k < 0.0 {
                let st = if now < 25.0 { Stufe::Warnung } else { Stufe::Hinweis };
                out.push((st, msg(crate::trs!("nähert sich der Grenze"))));
            }
        } else if name == crate::motorcheck::TREND_KETTE {
            if (now - first).abs() >= 2.0 && k.abs() > 0.2 {
                let st = if (now - first).abs() >= 4.0 { Stufe::Warnung } else { Stufe::Hinweis };
                out.push((st, msg("wandert — Steuerkette/Nockenwellenversteller beobachten")));
            }
        } else if name.starts_with("Ladedruck") && name.ends_with("Lücke") {
            if now > first + 100.0 && k > 0.0 {
                out.push((Stufe::Hinweis, msg(crate::trs!("Ladedruck erreicht das Soll immer schlechter"))));
            }
        } else if name == "Fehler gesamt" && now > first && k > 0.0 {
            out.push((Stufe::Hinweis, msg("Anzahl steigt")));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pts(name: &str, v: &[f64]) -> Vec<Punkt> {
        v.iter()
            .enumerate()
            .map(|(i, w)| Punkt { ts: i as u64, datum: format!("0{}.01.2026", i + 1), km: None, name: name.into(), wert: *w, einheit: "V".into() })
            .collect()
    }

    #[test]
    fn detects_aging_battery_and_kat() {
        let mut m = BTreeMap::new();
        m.insert("Ruhespannung".to_string(), pts("Ruhespannung", &[12.7, 12.6, 12.45, 12.35]));
        m.insert("Reserve Katalysator Bank 1".to_string(), pts("Reserve Katalysator Bank 1", &[60.0, 48.0, 35.0, 22.0]));
        m.insert("Gemisch Leerlauf".to_string(), pts("Gemisch Leerlauf", &[2.0, 2.5, 1.8]));
        let r = analyze(&m);
        assert_eq!(r.len(), 2, "{r:?}");
        assert!(r.iter().any(|(s, t)| *s == Stufe::Warnung && t.contains("Katalysator")));
    }

    #[test]
    fn store_and_load() {
        let _g = crate::history::TEST_LOCK.lock().unwrap();
        let d = std::env::temp_dir().join(format!("vag-trend-{}", std::process::id()));
        std::env::set_var("VAG_DATA_DIR", &d);
        add("WVW", Some(1000), &[("Ruhespannung".into(), 12.6, "V".into())]).unwrap();
        add("WVW", Some(1100), &[("Ruhespannung".into(), 12.5, "V".into())]).unwrap();
        assert_eq!(series("WVW")["Ruhespannung"].len(), 2);
        let _ = std::fs::remove_dir_all(d);
    }
}
