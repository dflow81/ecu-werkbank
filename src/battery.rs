//! Batterie- und Starttest über die Steuergerätespannung (OBD-II PID 0x42).
//!
//! Ablauf: Zündung an (Motor aus) → Messung starten → Motor starten →
//! laufen lassen. Ausgewertet werden Ruhespannung, Einbruch beim Anlassen und
//! Ladespannung. Richtwerte für 12-V-Bleibatterien; AGM/EFB ähnlich.
//! Ruhestrom (Entladung über Nacht) lässt sich so nicht messen — dafür braucht
//! es ein Strommessgerät.

use crate::tr;
use crate::limits::Stufe;
use crate::obd::Obd;
use crate::transport::Backend;
use anyhow::Result;

#[derive(Debug, Clone, Default)]
pub struct BatteryResult {
    pub ruhe: Option<f64>,
    pub minimum: Option<f64>,
    pub laden: Option<f64>,
    pub bewertung: Vec<(Stufe, String)>,
}

impl BatteryResult {
    pub fn gesamt(&self) -> Stufe {
        self.bewertung.iter().map(|(s, _)| *s).max().unwrap_or(Stufe::Hinweis)
    }
}

/// Eine Spannungsmessung.
pub fn sample(o: &Obd) -> Result<f64> {
    o.read_value(0x42)
}

fn median(v: &mut [f64]) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(f64::total_cmp);
    Some(v[v.len() / 2])
}

/// Messreihe (t [s], U [V]) auswerten.
pub fn analyze(points: &[[f64; 2]]) -> BatteryResult {
    let mut r = BatteryResult::default();
    if points.len() < 5 {
        r.bewertung.push((Stufe::Hinweis, tr!("Zu wenige Messwerte")));
        return r;
    }
    let t0 = points[0][0];
    let t1 = points[points.len() - 1][0];
    let (min_idx, min_p) = points.iter().enumerate().min_by(|a, b| a.1[1].total_cmp(&b.1[1])).expect("nicht leer");
    let start_dip = min_p[1] < 11.8;

    // Ruhe: erste Sekunde vor dem Einbruch; Laden: letzte 2 Sekunden
    let mut rest: Vec<f64> = points.iter().take_while(|p| p[0] < t0 + 1.0).map(|p| p[1]).collect();
    if start_dip {
        rest.retain(|v| *v > 11.8);
    }
    let mut end: Vec<f64> = points[min_idx..].iter().filter(|p| p[0] > t1 - 2.0).map(|p| p[1]).collect();
    r.ruhe = median(&mut rest);
    r.minimum = Some(min_p[1]);
    r.laden = if start_dip { median(&mut end) } else { None };

    if let Some(u) = r.ruhe {
        let (s, t) = if u >= 13.2 {
            (Stufe::Hinweis, tr!("Motor lief schon — für die Ruhespannung bei Zündung an, Motor aus beginnen"))
        } else if u >= 12.5 {
            (Stufe::Gut, tr!("Batterie gut geladen"))
        } else if u >= 12.2 {
            (Stufe::Hinweis, tr!("Batterie nur teilweise geladen — länger fahren oder nachladen"))
        } else {
            (Stufe::Warnung, tr!("Batterie entladen oder schwach"))
        };
        r.bewertung.push((s, tr!("Ruhespannung {u} V: {t}", u = format!("{:.2}", u), t = t)));
    }
    if start_dip {
        let u = min_p[1];
        let (s, t) = if u >= 10.0 {
            (Stufe::Gut, tr!("Startleistung gut"))
        } else if u >= 9.6 {
            (Stufe::Hinweis, tr!("Startleistung grenzwertig"))
        } else {
            (Stufe::Warnung, tr!("Spannung bricht stark ein — Batterie schwach"))
        };
        r.bewertung.push((s, tr!("Beim Anlassen min. {u} V: {t}", u = format!("{:.2}", u), t = t)));
    } else {
        r.bewertung.push((Stufe::Hinweis, tr!("Kein Startvorgang erkannt — während der Messung den Motor starten")));
    }
    if let Some(u) = r.laden {
        let (s, t) = if (13.6..=14.9).contains(&u) {
            (Stufe::Gut, tr!("Lichtmaschine lädt"))
        } else if u > 14.9 {
            (Stufe::Warnung, tr!("Ladespannung zu hoch — Regler prüfen"))
        } else if u >= 13.0 {
            (Stufe::Hinweis, tr!("Ladespannung niedrig (bei Start-Stopp/Rekuperation normal)"))
        } else {
            (Stufe::Warnung, tr!("Keine Ladung — Lichtmaschine/Keilriemen prüfen"))
        };
        r.bewertung.push((s, tr!("Nach dem Start {u} V: {t}", u = format!("{:.2}", u), t = t)));
    }
    r
}

/// Messung über `seconds` (ca. 20 Werte/s). `tick` bekommt jeden Wert.
pub fn measure(be: &Backend, seconds: f64, mut tick: impl FnMut(f64, f64)) -> Result<Vec<[f64; 2]>> {
    if matches!(be, Backend::Demo) {
        crate::sim::start_battery_demo();
    }
    let tp = be.open(crate::obd::DEFAULT_TX, crate::obd::DEFAULT_RX)?;
    let o = Obd::new(&*tp);
    let start = std::time::Instant::now();
    let mut pts = Vec::new();
    while start.elapsed().as_secs_f64() < seconds {
        let t = start.elapsed().as_secs_f64();
        if let Ok(v) = sample(&o) {
            pts.push([t, v]);
            tick(t, v);
        }
        std::thread::sleep(std::time::Duration::from_millis(40));
    }
    if pts.is_empty() {
        anyhow::bail!(tr!("Keine Spannungswerte (PID 0x42) — Zündung an? Adapter verbunden?"));
    }
    Ok(pts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analyze_good_start() {
        let mut p = Vec::new();
        for i in 0..300 {
            let t = i as f64 * 0.05;
            let v = if t < 3.0 { 12.6 } else if t < 4.5 { 10.3 } else { 14.2 };
            p.push([t, v]);
        }
        let r = analyze(&p);
        assert_eq!(r.gesamt(), Stufe::Gut, "{:?}", r.bewertung);
        assert!((r.ruhe.unwrap() - 12.6).abs() < 0.01);
        assert!((r.laden.unwrap() - 14.2).abs() < 0.01);
    }

    #[test]
    fn analyze_weak_battery() {
        let p: Vec<[f64; 2]> = (0..200).map(|i| {
            let t = i as f64 * 0.05;
            [t, if t < 2.0 { 12.0 } else if t < 3.0 { 9.1 } else { 13.9 }]
        }).collect();
        assert_eq!(analyze(&p).gesamt(), Stufe::Warnung);
    }

    #[test]
    fn demo_measurement() {
        // die Demo-Spannung ist für alle Tests gleich — nicht parallel zur Verbindungsprüfung
        let _g = crate::sim::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // 9 s: die letzten 2 s liegen sicher hinter dem Hochregeln (ab 6 s), auch auf langsamen Rechnern
        let pts = measure(&Backend::Demo, 9.0, |_, _| {}).unwrap();
        let r = analyze(&pts);
        assert_eq!(r.gesamt(), Stufe::Gut, "{:?}", r.bewertung);
    }
}
