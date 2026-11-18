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
    fn simulation_hat_genug_spannung() {
        let v = vor_dem_schreiben(&Backend::Demo).unwrap();
        assert!(v.is_some_and(|v| v >= MINDEST_V), "{v:?}");
    }
}
