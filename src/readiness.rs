//! Readiness-Assistent: Stand der Abgas-Tests (OBD PID 01), was davon in der
//! laufenden Fahrt schon geprüft wurde (PID 41, falls das Auto es kann), der
//! Fortschritt seit dem Löschen (PID 30/31/4E) und je Test eine Fahranleitung.
//!
//! Die Anleitungen sind allgemeine Richtwerte in eigenen Worten — VW
//! veröffentlicht die genauen Fahrzyklen nicht, sie unterscheiden sich je Motor.

use crate::obd::{parse_readiness, Obd, Readiness};
use crate::trs;
use anyhow::Result;

/// Alles, was der Assistent anzeigt.
#[derive(Debug, Clone, PartialEq)]
pub struct Stand {
    pub readiness: Readiness,
    /// Tests in dieser Fahrt (PID 41): `available` = läuft in dieser Fahrt, `complete` = in dieser Fahrt fertig
    pub fahrt: Option<Readiness>,
    pub warmlaeufe: Option<f64>,
    pub km_seit_loeschen: Option<f64>,
    pub min_seit_loeschen: Option<f64>,
}

/// Nur lesen (Mode 01).
pub fn lesen(o: &Obd) -> Result<Stand> {
    let readiness = o.readiness()?;
    let fahrt = o.read_pid(0x41).ok().filter(|d| d.len() >= 4).map(|d| parse_readiness([d[0], d[1], d[2], d[3]]));
    Ok(Stand {
        readiness,
        fahrt,
        warmlaeufe: o.read_value(0x30).ok(),
        km_seit_loeschen: o.read_value(0x31).ok(),
        min_seit_loeschen: o.read_value(0x4E).ok(),
    })
}

impl Stand {
    /// Stand eines Tests in dieser Fahrt: Some(true) fertig, Some(false) läuft noch, None unbekannt/nicht aktiv.
    pub fn in_fahrt(&self, name: &str) -> Option<bool> {
        let m = self.fahrt.as_ref()?.monitors.iter().find(|m| m.name == name)?;
        m.available.then_some(m.complete)
    }

    /// Verfügbare, noch nicht fertige Tests.
    pub fn offen(&self) -> Vec<&'static str> {
        self.readiness.monitors.iter().filter(|m| m.available && !m.complete).map(|m| m.name).collect()
    }
}

/// So läuft ein Test durch (Richtwerte). Namen wie in [`crate::obd::parse_readiness`].
pub fn anleitung(name: &str) -> &'static str {
    match name {
        "Verbrennungsaussetzer" | "Kraftstoffsystem" | "Bauteile" => trs!("Läuft bei jeder Fahrt von selbst mit — einfach normal fahren."),
        "Katalysator" => trs!("Motor ganz warm fahren, dann 5–10 Minuten möglichst gleichmäßig 60–90 km/h (Landstraße), ohne starkes Gasgeben oder Bremsen."),
        "Lambdasonde" => trs!("Bei warmem Motor mehrmals aus 70–100 km/h das Gas ganz wegnehmen und einige Sekunden rollen lassen (Schubbetrieb, nicht bremsen), danach wieder gleichmäßig fahren."),
        "Lambdasonden-Heizung" => trs!("Braucht einen Kaltstart: Auto mehrere Stunden (am besten über Nacht) abkühlen lassen, starten, 1–2 Minuten im Leerlauf laufen lassen, dann normal losfahren."),
        "Tankentlüftung (EVAP)" => trs!("Tank zwischen 1/4 und 3/4 voll. Kaltstart, danach eine längere Fahrt mit gleichmäßigem Tempo und ein paar Minuten Leerlauf am Ende. Kann mehrere Fahrten dauern."),
        "Sekundärluft" => trs!("Kaltstart bei kaltem Motor (Kühlmittel unter etwa 30 °C) und 2 Minuten Leerlauf — die Pumpe läuft nur kurz nach dem Kaltstart."),
        "Kat-Heizung" | "Klimaanlage" => trs!("Bei den meisten Autos nicht vorhanden. Sonst: normal fahren, mehrere Fahrten."),
        "AGR/VVT" => trs!("Bei warmem Motor gleichmäßig fahren, mit sanftem Gasgeben und einigen Schubphasen (Gas weg, rollen lassen)."),
        "NMHC-Katalysator" | "NOx/SCR" => trs!("Warm fahren, dann längere gleichmäßige Fahrt auf Landstraße oder Autobahn."),
        "Ladedruck" => trs!("Bei warmem Motor ein paar Mal zügig beschleunigen (z. B. auf der Auffahrt), dazwischen gleichmäßig fahren."),
        "Abgassensor" => trs!("Warm fahren, dann gleichmäßige Fahrt mit einigen Schubphasen."),
        "Partikelfilter" => trs!("15–20 Minuten Autobahn oder Landstraße mit über 2.500 1/min, ohne Stop-and-go."),
        _ => trs!("Normal fahren, mehrere Fahrten mit Kaltstart und warmem Motor."),
    }
}

/// Allgemeine Tipps.
pub fn tipps() -> [&'static str; 3] {
    [
        trs!("Fehler nicht wieder löschen — sonst fangen alle Tests von vorn an."),
        trs!("Zündung zwischendurch nicht nur kurz aus- und wieder einschalten; ein Fahrzyklus = Kaltstart, Fahrt, Motor aus."),
        trs!("Meist sind nach 2–5 normalen Fahrten (Kaltstart, Stadt, Landstraße) alle Tests fertig. Für die AU sollten alle Tests fertig sein."),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::DemoUds;

    #[test]
    fn stand_aus_der_simulation() {
        let tp = DemoUds::new(crate::obd::DEFAULT_TX);
        let s = lesen(&Obd::new(&tp)).unwrap();
        assert!(s.offen().contains(&"Tankentlüftung (EVAP)"));
        assert!(s.km_seit_loeschen.is_some());
        // diese Fahrt: Lambdasonden-Heizung läuft noch, Katalysator fertig
        assert_eq!(s.in_fahrt("Lambdasonden-Heizung"), Some(false));
        assert_eq!(s.in_fahrt("Katalysator"), Some(true));
        for m in &s.readiness.monitors {
            assert!(!anleitung(m.name).is_empty());
        }
    }
}
