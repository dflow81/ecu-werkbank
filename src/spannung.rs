//! Bordspannung vor dem Schreiben prüfen. Zu niedrige Spannung beim Codieren
//! oder Anpassen kann ein Steuergerät mitten im Schreiben abstürzen lassen.
//!
//! Gelesen wird die Steuergerätespannung über OBD (Modus 01, PID 0x42) am
//! Motorsteuergerät. Ist sie nicht lesbar (ELM327-Eigenheiten, Zündung aus,
//! älteres Auto), wird nicht blockiert — nur bei einem gelesenen Wert unter
//! der Grenze.
//!
//! Golf 6: Nach einer OBD-Abfrage nimmt das Motorsteuergerät ~3 s keinen
//! TP-2.0-Kanal an. Darum immer **vor** dem Öffnen eines TP-2.0-Kanals prüfen.

use crate::transport::Backend;
use anyhow::{bail, Result};

/// Untergrenze für Schreibvorgänge (V, am Steuergerät gemessen — liegt meist
/// 0,1–0,3 V unter der Batteriespannung).
pub const MINDEST_V: f64 = 12.0;

/// Spannung lesen; `None`, wenn nicht lesbar.
pub fn lesen(be: &Backend) -> Option<f64> {
    let tp = be.open(crate::obd::DEFAULT_TX, crate::obd::DEFAULT_RX).ok()?;
    crate::obd::Obd::new(&*tp).read_value(0x42).ok().filter(|v| (5.0..20.0).contains(v))
}

/// Vor einem Schreibvorgang: Fehler, wenn die Spannung gelesen wurde und zu niedrig ist.
pub fn vor_dem_schreiben(be: &Backend) -> Result<Option<f64>> {
    let v = lesen(be);
    pruefen(v)?;
    Ok(v)
}

/// So oft wird bei langen Abläufen zwischendurch gemessen (höchstens).
pub const ZWISCHEN_S: u64 = 10;

/// Spannungswächter für Abläufe mit vielen Schreibvorgängen: prüft am Anfang
/// und dann vor jedem weiteren Schreiben, wenn die letzte Messung länger als
/// [`ZWISCHEN_S`] Sekunden her ist. Fällt die Spannung unter [`MINDEST_V`],
/// wird vor dem nächsten Schreiben abgebrochen.
///
/// Keine Zwischenmessung, wenn ins Motorsteuergerät geschrieben wird: Die
/// Messung läuft über OBD am Motor (gleiche CAN-IDs bei UDS 0x7E0; beim
/// Golf-6-Motor sperrt OBD kurz den TP-2.0-Kanal).
pub struct Waechter<'a> {
    be: &'a Backend,
    zuletzt: std::time::Instant,
    zwischendurch: bool,
    /// erste und niedrigste gelesene Spannung
    pub start: Option<f64>,
    pub tiefst: Option<f64>,
}

impl<'a> Waechter<'a> {
    /// Erste Messung (Fehler bei zu niedriger Spannung, wie [`vor_dem_schreiben`]).
    /// `ziel_ist_motor`: siehe Typ-Beschreibung.
    pub fn starten(be: &'a Backend, ziel_ist_motor: bool) -> Result<Self> {
        let v = vor_dem_schreiben(be)?;
        Ok(Self { be, zuletzt: std::time::Instant::now(), zwischendurch: !ziel_ist_motor, start: v, tiefst: v })
    }

    /// Vor dem nächsten Schreiben aufrufen. Misst höchstens alle [`ZWISCHEN_S`] s.
    pub fn vor_naechstem(&mut self) -> Result<()> {
        if !self.zwischendurch || self.zuletzt.elapsed() < std::time::Duration::from_secs(ZWISCHEN_S) {
            return Ok(());
        }
        self.zuletzt = std::time::Instant::now();
        let v = lesen(self.be);
        self.merken(v)
    }

    fn merken(&mut self, v: Option<f64>) -> Result<()> {
        if let Some(x) = v {
            self.tiefst = Some(self.tiefst.map_or(x, |t| t.min(x)));
            if x < MINDEST_V {
                bail!(crate::tr!(
                    "Bordspannung ist während des Schreibens auf {a} V gefallen (mindestens {b} V) — abgebrochen, bevor weiter geschrieben wurde. Ladegerät anschließen.",
                    a = format!("{x:.1}"),
                    b = format!("{MINDEST_V:.1}")
                ));
            }
        }
        Ok(())
    }
}

/// Ist das Ziel das Motorsteuergerät (TP 2.0 Adresse 0x01 oder UDS/OBD 0x7E0)?
pub fn ist_motor(tp20: Option<u8>, tx: u16) -> bool {
    tp20 == Some(0x01) || (tp20.is_none() && tx == crate::obd::DEFAULT_TX)
}

fn pruefen(v: Option<f64>) -> Result<()> {
    if let Some(v) = v {
        if v < MINDEST_V {
            bail!(crate::tr!(
                "Bordspannung {a} V ist zu niedrig zum Schreiben (mindestens {b} V). Ladegerät anschließen oder Motor laufen lassen — nichts geschrieben.",
                a = format!("{v:.1}"),
                b = format!("{MINDEST_V:.1}")
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grenze() {
        assert!(pruefen(Some(11.6)).is_err());
        assert!(pruefen(Some(12.4)).is_ok());
        assert!(pruefen(None).is_ok(), "nicht lesbar blockiert nicht");
        let e = pruefen(Some(11.0)).unwrap_err().to_string();
        assert!(e.contains("11.0") && e.contains("nichts geschrieben"), "{e}");
    }

    #[test]
    fn waechter_bricht_bei_abfall_ab() {
        let be = Backend::Demo;
        let mut w = Waechter::starten(&be, false).unwrap();
        assert!(w.vor_naechstem().is_ok(), "gleich danach: keine neue Messung");
        assert!(w.merken(Some(12.6)).is_ok());
        let e = w.merken(Some(11.7)).unwrap_err().to_string();
        assert!(e.contains("11.7") && e.contains("abgebrochen"), "{e}");
        assert_eq!(w.tiefst, Some(11.7));
        assert!(w.merken(None).is_ok(), "nicht lesbar blockiert nicht");
        assert!(ist_motor(Some(0x01), 0) && ist_motor(None, 0x7E0) && !ist_motor(Some(0x20), 0x7E0) && !ist_motor(None, 0x714));
    }

    #[test]
    fn simulation_hat_genug_spannung() {
        let v = vor_dem_schreiben(&Backend::Demo).unwrap();
        assert!(v.is_some_and(|v| v >= MINDEST_V), "{v:?}");
    }
}
