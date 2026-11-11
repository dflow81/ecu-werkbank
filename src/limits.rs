//! Sollwerte: Messwerte gegen Grenzen prüfen (Ampel).
//!
//! Grenzen kommen aus deinen Labels (`[[sollwert]]`, siehe labels.rs) oder —
//! für einige OBD-II-Werte — aus eingebauten, bewusst großzügigen Grundwerten.

use serde::{Deserialize, Serialize};

/// Ampel-Stufe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Stufe {
    Gut,
    Hinweis,
    Warnung,
}

impl Stufe {
    pub fn text(self) -> &'static str {
        match self {
            Stufe::Gut => "OK",
            Stufe::Hinweis => crate::trs!("Hinweis"),
            Stufe::Warnung => crate::trs!("Warnung"),
        }
    }
    pub fn rgb(self) -> (f32, f32, f32) {
        match self {
            Stufe::Gut => (0.15, 0.55, 0.25),
            Stufe::Hinweis => (0.85, 0.55, 0.05),
            Stufe::Warnung => (0.78, 0.15, 0.12),
        }
    }
}

/// Bereich [min, max]; fehlende Grenze = offen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Range {
    pub min: Option<f64>,
    pub max: Option<f64>,
}

impl Range {
    pub fn check(&self, v: f64) -> Stufe {
        let low = self.min.is_some_and(|m| v < m);
        let high = self.max.is_some_and(|m| v > m);
        if low || high { Stufe::Warnung } else { Stufe::Gut }
    }

    /// "80 – 105", "max 105", "min 12".
    pub fn text(&self) -> String {
        let f = |v: f64| crate::csvlog::num(v);
        match (self.min, self.max) {
            (Some(a), Some(b)) => format!("{} – {}", f(a), f(b)),
            (Some(a), None) => format!("min {}", f(a)),
            (None, Some(b)) => format!("max {}", f(b)),
            (None, None) => String::new(),
        }
    }

    pub fn as_pair(&self) -> (f64, f64) {
        (self.min.unwrap_or(f64::NEG_INFINITY), self.max.unwrap_or(f64::INFINITY))
    }
}

/// Eingebaute Grundwerte für OBD-II-PIDs (großzügig, gelten bei laufendem Motor).
pub fn obd_default(pid: u8) -> Option<Range> {
    let r = |min: Option<f64>, max: Option<f64>| Some(Range { min, max });
    match pid {
        0x05 => r(None, Some(112.0)),            // Kühlmittel: Überhitzung
        0x0F => r(None, Some(75.0)),             // Ansaugluft
        0x42 => r(Some(11.8), Some(15.0)),       // Steuergerätespannung
        0x06 | 0x07 => r(Some(-25.0), Some(25.0)), // Gemischkorrektur
        0x5C => r(None, Some(135.0)),            // Öltemperatur
        _ => None,
    }
}
