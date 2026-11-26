//! Marke aus der Fahrgestellnummer (WMI = erste 3 Zeichen, ISO 3780, öffentlich
//! vergeben) — mit einem Hinweis, was ECUWerkbank bei dieser Marke kann.

use crate::tr;

/// Bekannte Herstellerkennungen (Auswahl häufiger Marken in Europa).
const WMI: &[(&str, &str)] = &[
    ("WVW", "Volkswagen"), ("WV1", "Volkswagen Nutzfahrzeuge"), ("WV2", "Volkswagen Nutzfahrzeuge"), ("WV3", "Volkswagen Nutzfahrzeuge"),
    ("WAU", "Audi"), ("WA1", "Audi"), ("TRU", "Audi"), ("WUA", "Audi Sport"),
    ("VSS", "Seat/Cupra"), ("TMB", "Škoda"), ("WP0", "Porsche"), ("WP1", "Porsche"),
    ("WBA", "BMW"), ("WBS", "BMW M"), ("WBY", "BMW i"), ("WMW", "Mini"),
    ("WDB", "Mercedes-Benz"), ("WDD", "Mercedes-Benz"), ("WDC", "Mercedes-Benz"), ("WDF", "Mercedes-Benz"), ("W1K", "Mercedes-Benz"), ("W1N", "Mercedes-Benz"), ("W1V", "Mercedes-Benz"),
    ("W0L", "Opel"), ("W0V", "Opel"), ("WF0", "Ford"), ("WF1", "Ford"),
    ("VF1", "Renault"), ("UU1", "Dacia"), ("VF3", "Peugeot"), ("VF7", "Citroën"), ("VR3", "Peugeot"),
    ("ZFA", "Fiat"), ("ZAR", "Alfa Romeo"), ("YV1", "Volvo"), ("YS3", "Saab"),
    ("JTD", "Toyota"), ("JT1", "Toyota"), ("SB1", "Toyota"), ("NMT", "Toyota"), ("JHM", "Honda"), ("JMZ", "Mazda"), ("JN1", "Nissan"), ("SJN", "Nissan"),
    ("KMH", "Hyundai"), ("TMA", "Hyundai"), ("KNA", "Kia"), ("U5Y", "Kia"),
    ("5YJ", "Tesla"), ("LRW", "Tesla"), ("XW8", "Volkswagen (Russland)"),
];

/// Marke zur Fahrgestellnummer, falls bekannt.
pub fn aus_vin(vin: &str) -> Option<&'static str> {
    let w = vin.get(..3)?.to_ascii_uppercase();
    WMI.iter().find(|(k, _)| *k == w).map(|(_, m)| *m)
}

/// Gehört die Marke zum VW-Konzern (volle Diagnose mit TP 2.0/UDS)?
pub fn ist_vw_konzern(marke: &str) -> bool {
    ["Volkswagen", "Audi", "Seat", "Škoda", "Porsche"].iter().any(|m| marke.starts_with(m))
}

/// Was ECUWerkbank bei dieser Marke kann (für „Andere Marken“).
pub fn hinweis(marke: &str) -> String {
    match marke {
        m if ist_vw_konzern(m) => tr!("VW-Konzern: Für alle Steuergeräte mit Namen, Codierung und Anpassung besser Diagnose → Steuergeräte nutzen."),
        "BMW" | "BMW M" | "BMW i" | "Mini" => tr!("BMW/Mini nutzen eine eigene Adressierung (Tester 0x6F1 mit Zieladresse im ersten Byte). Mit einem SocketCAN-Adapter fragt die Suche sie mit ab; über ELM327 wird meist nur das Motorsteuergerät über OBD gefunden."),
        "Mercedes-Benz" => tr!("Mercedes: UDS mit 11-Bit-Adressen wird gefunden; Texte zu Herstellerfehlern nur aus eigenen Datenpaketen."),
        "Opel" => tr!("Opel: neuere Modelle (UDS) werden gefunden; ältere mit GMLAN-Diagnose nur über OBD."),
        "Tesla" => tr!("Tesla: kaum OBD-Diagnose über die Buchse — wenig zu erwarten."),
        _ => tr!("Genormte Diagnose (OBD-II, UDS-Kennungen F1xx, Fehlerspeicher); Herstellertexte nur aus eigenen Datenpaketen."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marken() {
        assert_eq!(aus_vin("WVWZZZ1KZAM000017"), Some("Volkswagen"));
        assert_eq!(aus_vin("wdd2050421a000000"), Some("Mercedes-Benz"));
        assert_eq!(aus_vin("WBA8E1C50JA000000"), Some("BMW"));
        assert_eq!(aus_vin("XXX"), None);
        assert_eq!(aus_vin("W"), None);
        assert!(ist_vw_konzern("Škoda") && ist_vw_konzern("Seat/Cupra") && !ist_vw_konzern("BMW"));
        assert!(hinweis("BMW").contains("0x6F1"));
    }
}
