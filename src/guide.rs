//! Geführte Fehlersuche: Prüfplan je Fehlercode (eigene Formulierungen,
//! allgemeine Werkstatt-Vorgehensweise). Schritte können ein ecu-werkbank-Werkzeug
//! direkt starten. Herstellerspezifische Prüfwerte sind nicht enthalten.

/// Werkzeug, das ein Schritt öffnet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Werkzeug {
    Gemisch,
    Pruefergebnisse,
    Batterie,
    Readiness,
    FreezeFrame,
    LiveWerte,
    Websuche,
    FehlerLoeschen,
}

impl Werkzeug {
    pub fn name(self) -> &'static str {
        match self {
            Werkzeug::Gemisch => "Gemisch-Assistent",
            Werkzeug::Pruefergebnisse => "Prüfergebnisse (Mode 06)",
            Werkzeug::Batterie => "Batterietest",
            Werkzeug::Readiness => "Readiness-Assistent",
            Werkzeug::FreezeFrame => "Freeze-Frame",
            Werkzeug::LiveWerte => "Live-Werte (Scope)",
            Werkzeug::Websuche => "Websuche",
            Werkzeug::FehlerLoeschen => "Reparatur-Modus",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Schritt {
    pub text: &'static str,
    pub werkzeug: Option<Werkzeug>,
}

#[derive(Debug, Clone)]
pub struct Plan {
    pub titel: &'static str,
    pub schritte: Vec<Schritt>,
}

fn s(text: &'static str, w: Option<Werkzeug>) -> Schritt {
    Schritt { text, werkzeug: w }
}

use Werkzeug::*;

fn num(code: &str) -> Option<u16> {
    code.get(2..)?.parse().ok()
}

/// VAG-Code 16384…17383 → genormter P0-Code, sonst unverändert (groß).
pub fn normalize(code: &str) -> String {
    let c = code.trim().to_uppercase();
    match c.parse::<u32>() {
        Ok(v) if (16384..=17383).contains(&v) => format!("P{:04}", v - 16384),
        _ => c,
    }
}

/// Prüfplan zu einem Code (P0xxx, VAG 5-stellig über P-Code, U01xx …).
pub fn plan_for(code: &str) -> Plan {
    let c = normalize(code);
    let n = num(&c).unwrap_or(9999);
    let p = c.starts_with("P0");
    let mut plan = if p && matches!(n, 171 | 174) {
        Plan { titel: "Gemisch zu mager", schritte: vec![
            s("Freeze-Frame ansehen: bei welcher Drehzahl/Last trat der Fehler auf?", Some(FreezeFrame)),
            s("Gemisch-Assistent ausführen (Leerlauf und 2500 1/min vergleichen).", Some(Gemisch)),
            s("Nur im Leerlauf mager: Ansaugtrakt auf Falschluft prüfen — Unterdruckschläuche, Ansaugkrümmer-Dichtung, Ölabscheider/Kurbelgehäuseentlüftung, Öleinfülldeckel.", None),
            s("Mager auch bei Drehzahl: Luftmassenmesser (Steckverbindung, Verschmutzung) und Kraftstoffdruck/Filter prüfen.", None),
            s("Ansaugschlauch zwischen Luftmassenmesser und Drosselklappe auf Risse prüfen.", None),
            s("Nach der Reparatur Fehler löschen und Probefahrt, dann erneut prüfen.", Some(FehlerLoeschen)),
        ]}
    } else if p && matches!(n, 172 | 175) {
        Plan { titel: "Gemisch zu fett", schritte: vec![
            s("Gemisch-Assistent ausführen.", Some(Gemisch)),
            s("Fett vor allem im Leerlauf: Tankentlüftungsventil (hängt offen?) prüfen — Schlauch abklemmen und Gemisch beobachten.", Some(LiveWerte)),
            s("Fett überall: Luftmassenmesser (misst zu viel?), Kraftstoffdruck, undichte Einspritzventile.", None),
            s("Motoröl auf Kraftstoffgeruch prüfen (Hinweis auf undichte Einspritzventile).", None),
            s("Nach der Reparatur Fehler löschen und Probefahrt.", Some(FehlerLoeschen)),
        ]}
    } else if p && (300..=312).contains(&n) {
        Plan { titel: "Verbrennungsaussetzer", schritte: vec![
            s("Prüfergebnisse lesen: welcher Zylinder hat die meisten Aussetzer?", Some(Pruefergebnisse)),
            s("Freeze-Frame ansehen: kalt/warm, Leerlauf/Last?", Some(FreezeFrame)),
            s("Zündspule des auffälligen Zylinders mit einem Nachbarzylinder tauschen, Fehler löschen, fahren: wandert der Aussetzer mit, ist die Spule defekt.", None),
            s("Zündkerzen prüfen (Abbrand, Ölkohle, Elektrodenabstand) — Wechselintervall?", None),
            s("Bleibt der Aussetzer am Zylinder: Einspritzventil, Kompression/Druckverlust prüfen.", None),
            s("Aussetzer bei allen Zylindern: Gemisch prüfen (Falschluft, Kraftstoffdruck).", Some(Gemisch)),
            s("Nach der Reparatur Fehler löschen und Probefahrt, Aussetzerzähler erneut lesen.", Some(FehlerLoeschen)),
        ]}
    } else if p && matches!(n, 420 | 421 | 430 | 431) {
        Plan { titel: "Katalysator-Wirkung zu gering", schritte: vec![
            s("Prüfergebnisse lesen: wie weit ist der Kat-Wert über der Grenze?", Some(Pruefergebnisse)),
            s("Zuerst Aussetzer und Gemischfehler ausschließen — sie schädigen den Kat und verfälschen den Test.", Some(Gemisch)),
            s("Abgasanlage vor/zwischen den Lambdasonden auf Undichtigkeiten prüfen (Falschluft verfälscht die Messung).", None),
            s("Hintere Lambdasonde im Scope beobachten: sie sollte bei warmem Motor ruhig sein; springt sie wie die vordere, speichert der Kat nicht mehr.", Some(LiveWerte)),
            s("Nach der Reparatur löschen, Readiness-Fahrt bis Katalysator „fertig“.", Some(Readiness)),
        ]}
    } else if p && (130..=167).contains(&n) || p && (30..=64).contains(&n) {
        Plan { titel: "Lambdasonde / Lambdasonden-Heizung", schritte: vec![
            s("Prüfergebnisse der Lambdasonden lesen.", Some(Pruefergebnisse)),
            s("Stecker und Kabel der Sonde auf Scheuerstellen und Korrosion prüfen; Heizung: Sicherung prüfen.", None),
            s("Abgasanlage vor der Sonde auf Undichtigkeit prüfen.", None),
            s("Sondensignal bei warmem Motor im Scope beobachten.", Some(LiveWerte)),
            s("Nach der Reparatur löschen, Readiness-Fahrt.", Some(Readiness)),
        ]}
    } else if p && (100..=104).contains(&n) {
        Plan { titel: "Luftmassenmesser", schritte: vec![
            s("Luftmasse im Leerlauf und bei 2500 1/min im Scope ansehen (Gemisch-Assistent prüft die Plausibilität mit Hubraum).", Some(Gemisch)),
            s("Luftfilter und Ansaugschlauch prüfen; Stecker des Luftmassenmessers auf Korrosion.", None),
            s("Sensor nicht mit Druckluft reinigen; ggf. Spezialreiniger oder Tausch.", None),
            s("Nach der Reparatur löschen und Probefahrt.", Some(FehlerLoeschen)),
        ]}
    } else if p && (115..=119).contains(&n) || c == "P0128" {
        Plan { titel: "Kühlmitteltemperatur / Thermostat", schritte: vec![
            s("Kühlmitteltemperatur im Scope während des Warmlaufs beobachten: sollte zügig auf 80–105 °C steigen.", Some(LiveWerte)),
            s("Steigt sie nur langsam oder bleibt unter 80 °C: Thermostat hängt offen.", None),
            s("Springende Werte: Temperaturgeber und Stecker prüfen.", None),
            s("Kühlmittelstand prüfen.", None),
        ]}
    } else if p && matches!(n, 234 | 299 | 45..=48 | 235..=238 | 243) {
        Plan { titel: "Ladedruck", schritte: vec![
            s("Freeze-Frame ansehen (Drehzahl, Last beim Auftreten).", Some(FreezeFrame)),
            s("Ladeluftschläuche und Ladeluftkühler auf Risse/lose Schellen prüfen (Ölspuren!).", None),
            s("Unterdruckleitungen zum Ladedruckregler und das Magnetventil prüfen.", None),
            s("Ladedruck Soll/Ist bei Volllast im Scope vergleichen (Messwertblock).", Some(LiveWerte)),
        ]}
    } else if p && (400..=406).contains(&n) {
        Plan { titel: "Abgasrückführung", schritte: vec![
            s("Freeze-Frame ansehen.", Some(FreezeFrame)),
            s("AGR-Ventil und Leitungen auf Verkokung prüfen; Stecker prüfen.", None),
            s("Nach der Reparatur löschen, Readiness-Fahrt.", Some(Readiness)),
        ]}
    } else if p && (440..=456).contains(&n) {
        Plan { titel: "Tankentlüftung (EVAP)", schritte: vec![
            s("Tankdeckel prüfen (Dichtung, richtig eingerastet) — häufigste Ursache.", None),
            s("Schläuche am Aktivkohlebehälter und Tankentlüftungsventil prüfen.", None),
            s("Nach der Reparatur löschen; der EVAP-Test läuft oft erst nach Standzeit über Nacht.", Some(Readiness)),
        ]}
    } else if p && (335..=349).contains(&n) || p && (16..=19).contains(&n) {
        Plan { titel: "Kurbel-/Nockenwellengeber, Steuerzeiten", schritte: vec![
            s("Freeze-Frame ansehen.", Some(FreezeFrame)),
            s("Geber und Stecker prüfen (Öl im Stecker, Kabelbruch).", None),
            s("Bei Zuordnungsfehler (P0016–P0019): Steuerkette/-riemen gelängt oder übersprungen? Fachwerkstatt!", None),
        ]}
    } else if p && matches!(n, 560 | 562 | 563) {
        Plan { titel: "Bordspannung", schritte: vec![
            s("Batterietest durchführen (Ruhe-, Start-, Ladespannung).", Some(Batterie)),
            s("Batteriepole und Masseband auf Korrosion/Festsitz prüfen.", None),
            s("Ladespannung zu niedrig/hoch: Lichtmaschine, Regler, Keilriemen prüfen.", None),
        ]}
    } else if c.starts_with("U0") || c.starts_with("U1") {
        Plan { titel: "Kommunikationsfehler (Datenbus)", schritte: vec![
            s("Batterietest — Unterspannung verursacht oft Kommunikationsfehler.", Some(Batterie)),
            s("Welche Steuergeräte melden den Fehler? Liegt die Ursache beim genannten Partner (Stecker, Sicherung, Masse)?", None),
            s("Kürzlich Batterie getrennt oder Starthilfe? Dann Fehler löschen und beobachten.", Some(FehlerLoeschen)),
        ]}
    } else {
        Plan { titel: "Allgemeine Fehlersuche", schritte: vec![
            s("Umgebungsbedingungen/Freeze-Frame ansehen: wann trat der Fehler auf?", Some(FreezeFrame)),
            s("Code im Internet nachschlagen (mit Fahrzeugmodell).", Some(Websuche)),
            s("Stecker, Kabel, Sicherungen und Masse des betroffenen Bauteils prüfen.", None),
            s("Fehler löschen, Probefahrt, erneut lesen: kommt er sofort wieder (aktuell) oder nur sporadisch?", Some(FehlerLoeschen)),
        ]}
    };
    if !plan.schritte.iter().any(|x| x.werkzeug == Some(Websuche)) {
        plan.schritte.push(s("Erfahrungen anderer im Internet nachlesen.", Some(Websuche)));
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plans() {
        assert_eq!(plan_for("P0171").titel, "Gemisch zu mager");
        assert_eq!(plan_for("p0303").titel, "Verbrennungsaussetzer");
        assert_eq!(plan_for("16684").titel, "Verbrennungsaussetzer"); // = P0300
        assert_eq!(plan_for("P0420").titel, "Katalysator-Wirkung zu gering");
        assert_eq!(plan_for("U0155").titel, "Kommunikationsfehler (Datenbus)");
        assert_eq!(plan_for("18101").titel, "Allgemeine Fehlersuche");
        assert!(plan_for("P0300").schritte.iter().any(|s| s.werkzeug == Some(Werkzeug::Pruefergebnisse)));
    }
}
