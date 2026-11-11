//! Gemisch-Assistent: Gemischkorrektur (Kurz- + Langzeit) im Leerlauf und bei
//! erhöhter Drehzahl vergleichen und daraus wahrscheinliche Ursachen ableiten.
//!
//! Grundregel: Der Motor korrigiert positiv (+) wenn er zu mager läuft und
//! negativ (−) wenn er zu fett läuft. Ändert sich die Korrektur mit der
//! Drehzahl, verrät das die Ursache (Falschluft wirkt vor allem im Leerlauf).
//! Hinweise sind Richtwerte — sie ersetzen keine Prüfung am Bauteil.

use crate::tr;
use crate::limits::Stufe;
use crate::obd::Obd;
use anyhow::{bail, Result};

#[derive(Debug, Clone, Copy, Default)]
pub struct Sample {
    pub rpm: f64,
    pub stft: f64,
    pub ltft: f64,
    pub maf: Option<f64>,
    pub coolant: Option<f64>,
}

/// Eine Messung (PIDs 0C, 06, 07, 10, 05).
pub fn sample(o: &Obd) -> Result<Sample> {
    let (Ok(stft), Ok(ltft)) = (o.read_value(0x06), o.read_value(0x07)) else {
        bail!(tr!("Gemischkorrektur (PID 06/07) nicht verfügbar — bei Dieselmotoren üblich"));
    };
    Ok(Sample { rpm: o.read_value(0x0C)?, stft, ltft, maf: o.read_value(0x10).ok(), coolant: o.read_value(0x05).ok() })
}

/// Mittelwerte einer Phase.
#[derive(Debug, Clone, Default)]
pub struct Phase {
    pub rpm: f64,
    pub stft: f64,
    pub ltft: f64,
    pub maf: Option<f64>,
    pub coolant: Option<f64>,
    pub n: usize,
}

impl Phase {
    pub fn total(&self) -> f64 {
        self.stft + self.ltft
    }
}

pub fn summarize(samples: &[Sample]) -> Phase {
    let n = samples.len().max(1) as f64;
    let avg = |f: &dyn Fn(&Sample) -> f64| samples.iter().map(f).sum::<f64>() / n;
    let opt = |f: &dyn Fn(&Sample) -> Option<f64>| {
        let v: Vec<f64> = samples.iter().filter_map(f).collect();
        (!v.is_empty()).then(|| v.iter().sum::<f64>() / v.len() as f64)
    };
    Phase {
        rpm: avg(&|s| s.rpm),
        stft: avg(&|s| s.stft),
        ltft: avg(&|s| s.ltft),
        maf: opt(&|s| s.maf),
        coolant: opt(&|s| s.coolant),
        n: samples.len(),
    }
}

fn pct(v: f64) -> String {
    format!("{}{} %", if v > 0.0 { "+" } else { "" }, crate::csvlog::num((v * 10.0).round() / 10.0))
}

/// Auswertung: Leerlauf vs. erhöhte Drehzahl. `hubraum_l` für die
/// Luftmassen-Plausibilität (optional).
pub fn analyze(idle: &Phase, high: &Phase, hubraum_l: Option<f64>) -> Vec<(Stufe, String)> {
    let mut out = Vec::new();
    let (a, b) = (idle.total(), high.total());
    out.push((Stufe::Gut, tr!("Gemischkorrektur gesamt: Leerlauf {a}, bei {b} 1/min {c}", a = pct(a), b = format!("{:.0}", high.rpm), c = pct(b))));
    if idle.coolant.or(high.coolant).is_some_and(|c| c < 70.0) {
        out.push((Stufe::Hinweis, tr!("Motor noch nicht betriebswarm (< 70 °C) — Ergebnis unsicher, warm fahren und wiederholen")));
    }
    if high.rpm < idle.rpm + 800.0 {
        out.push((Stufe::Hinweis, tr!("Drehzahl in Phase 2 kaum erhöht — für eine sichere Aussage ca. 2500 1/min halten")));
    }
    let lean = |v: f64| v > 10.0;
    let rich = |v: f64| v < -10.0;
    let ursache = if lean(a) && b < a - 6.0 && !lean(b) {
        Some((Stufe::Warnung, tr!("Mager vor allem im Leerlauf: Falschluft wahrscheinlich: Unterdruckschläuche, Ansaugkrümmer-Dichtung, Kurbelgehäuseentlüftung/Ölabscheider, Bremskraftverstärker-Leitung prüfen (z. B. mit Bremsenreiniger/Rauchtest).")))
    } else if lean(a) && lean(b) {
        Some((Stufe::Warnung, tr!("Mager bei allen Drehzahlen: Kraftstoffversorgung (Druck, Pumpe, Filter) oder Luftmassenmesser misst zu wenig.")))
    } else if lean(b) && !lean(a) {
        Some((Stufe::Warnung, tr!("Mager vor allem bei höherer Drehzahl: Kraftstoffdruck/-pumpe zu schwach oder Luftmassenmesser verschmutzt.")))
    } else if rich(a) && rich(b) {
        Some((Stufe::Warnung, tr!("Zu fett bei allen Drehzahlen: Luftmassenmesser misst zu viel, Kraftstoffdruck zu hoch oder Einspritzventil undicht.")))
    } else if rich(a) && !rich(b) {
        Some((Stufe::Warnung, tr!("Zu fett vor allem im Leerlauf: Tankentlüftungsventil hängt offen oder Einspritzventil tropft.")))
    } else if a.abs() > 6.0 || b.abs() > 6.0 {
        Some((Stufe::Hinweis, tr!("Leichte Abweichung (6–10 %) — noch im Rahmen, bei Wiederholung beobachten (Trends).")))
    } else {
        Some((Stufe::Gut, tr!("Gemisch in Ordnung (Korrektur unter ±6 %).")))
    };
    out.extend(ursache.map(|(s, t)| (s, t.to_string())));
    if let (Some(l), Some(maf)) = (hubraum_l, idle.maf) {
        // Richtwert: Leerlauf-Luftmasse ≈ 1,5 … 4,5 g/s je Liter Hubraum
        if maf < 1.2 * l {
            out.push((Stufe::Hinweis, tr!("Luftmasse im Leerlauf niedrig ({maf} g/s bei {l} l) — Luftmassenmesser verschmutzt oder Falschluft hinter dem Messer?", maf = format!("{:.1}", maf), l = format!("{:.1}", l))));
        } else if maf > 5.0 * l {
            out.push((Stufe::Hinweis, tr!("Luftmasse im Leerlauf hoch ({maf} g/s bei {l} l) — Verbraucher an? Luftmassenmesser prüfen.", maf = format!("{:.1}", maf), l = format!("{:.1}", l))));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ph(rpm: f64, stft: f64, ltft: f64) -> Phase {
        Phase { rpm, stft, ltft, maf: Some(3.0), coolant: Some(90.0), n: 10 }
    }

    #[test]
    fn rules() {
        let f = |a: Phase, b: Phase| analyze(&a, &b, Some(1.4)).into_iter().map(|(_, t)| t).collect::<Vec<_>>().join(" | ");
        assert!(f(ph(800.0, 3.0, 15.0), ph(2500.0, 1.0, 3.0)).contains("Falschluft"));
        assert!(f(ph(800.0, 2.0, 12.0), ph(2500.0, 2.0, 13.0)).contains("Kraftstoffversorgung"));
        assert!(f(ph(800.0, -4.0, -12.0), ph(2500.0, -1.0, -2.0)).contains("Tankentlüftungsventil"));
        assert!(f(ph(800.0, 1.0, 2.0), ph(2500.0, 0.0, 1.0)).contains("in Ordnung"));
    }

    #[test]
    fn demo_vacuum_leak() {
        let _g = crate::sim::TEST_LOCK.lock().unwrap();
        use crate::sim::{set_fehlerbild, set_muster, Fehlerbild, Param};
        set_fehlerbild(Fehlerbild::Falschluft);
        let tp = crate::sim::DemoUds::new(crate::obd::DEFAULT_TX);
        let o = Obd::new(&tp);
        set_muster(Param::Drehzahl, Some(crate::control::Muster::Halten(800.0)));
        let idle: Vec<Sample> = (0..5).map(|_| sample(&o).unwrap()).collect();
        set_muster(Param::Drehzahl, Some(crate::control::Muster::Halten(2500.0)));
        let high: Vec<Sample> = (0..5).map(|_| sample(&o).unwrap()).collect();
        let r = analyze(&summarize(&idle), &summarize(&high), None);
        crate::sim::clear_muster();
        set_fehlerbild(Fehlerbild::Keins);
        assert!(r.iter().any(|(_, t)| t.contains("Falschluft")), "{r:?}");
    }
}
