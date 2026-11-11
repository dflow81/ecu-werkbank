//! Grund-Klartexte für genormte Fehlercodes (SAE J2012 / ISO 15031-6).
//!
//! Die Codes selbst sind genormt; die deutschen Beschreibungen hier sind
//! eigene Formulierungen. Eigene Label-Texte (labels/*.toml) haben Vorrang —
//! siehe [`crate::labels::LabelDb::dtc_description`].
//!
//! Wiederkehrende Gruppen (Zylinder, Lambdasonden je Bank/Sonde, Zündspulen …)
//! werden per Regel erzeugt, der Rest steht in [`TABLE`].

/// Klartext zu einem genormten Code wie "P0300" oder "U0100".
pub fn lookup(code: &str) -> Option<&'static str> {
    let code = code.trim();
    if code.len() != 5 {
        return None;
    }
    let up = code.to_ascii_uppercase();
    if let Some(t) = TABLE.iter().find(|(c, _)| *c == up).map(|(_, t)| *t) {
        return Some(t);
    }
    pattern(&up).or_else(|| u_bereich(&up))
}

/// Übrige genormte Netzwerk-Codes nach ihrem Bereich (SAE J2012):
/// U0100–U02FF keine Kommunikation, U0300–U03FF Software passt nicht,
/// U0400–U04FF unplausible Daten — jeweils mit einem anderen Steuergerät.
fn u_bereich(code: &str) -> Option<&'static str> {
    let n = u16::from_str_radix(code.strip_prefix("U0")?, 16).ok()?;
    Some(match n {
        0x100..=0x2FF => "Keine Kommunikation mit einem anderen Steuergerät (genormter Code)",
        0x300..=0x3FF => "Software passt nicht zu einem anderen Steuergerät (genormter Code)",
        0x400..=0x4FF => "Unplausible Daten von einem anderen Steuergerät (genormter Code)",
        _ => return None,
    })
}

/// Fehlerart (3. Byte eines UDS-Fehlers, „Failure Type Byte“) im Klartext.
/// Die Gruppen sind in SAE J2012 genormt; die Texte sind eigene Formulierungen.
pub fn fehlerart_text(ftb: u8) -> Option<&'static str> {
    Some(match ftb {
        0x00 => return None,
        0x01 => "elektrischer Fehler",
        0x02 => "Signalfehler",
        0x03 => "Frequenz-/Taktsignal fehlerhaft",
        0x04 => "interner Fehler im Steuergerät",
        0x05 => "Programmier-/Speicherfehler",
        0x06 => "Fehler bei der Auswertung (Rechenwert unplausibel)",
        0x07 => "mechanischer Fehler",
        0x08 => "Bus-Signal/Botschaft fehlerhaft",
        0x09 => "Bauteil defekt",
        0x11 => "Kurzschluss nach Masse",
        0x12 => "Kurzschluss nach Plus",
        0x13 => "Unterbrechung",
        0x14 => "Kurzschluss nach Masse oder Unterbrechung",
        0x15 => "Kurzschluss nach Plus oder Unterbrechung",
        0x16 => "Spannung zu niedrig",
        0x17 => "Spannung zu hoch",
        0x18 => "Strom zu niedrig",
        0x19 => "Strom zu hoch",
        0x1A => "Widerstand zu niedrig",
        0x1B => "Widerstand zu hoch",
        0x1C => "Spannung außerhalb des Bereichs",
        0x1D => "Strom außerhalb des Bereichs",
        0x1E => "Widerstand außerhalb des Bereichs",
        0x1F => "Stromkreis zeitweise unterbrochen",
        0x21 => "Signal zu klein",
        0x22 => "Signal zu groß",
        0x23 => "Signal hängt fest (niedrig)",
        0x24 => "Signal hängt fest (hoch)",
        0x25 => "Signalform falsch",
        0x26 => "Signal ändert sich zu langsam",
        0x27 => "Signal ändert sich zu schnell",
        0x28 => "Signal-Nullpunkt/Abgleich falsch",
        0x29 => "Signal ungültig",
        0x2A => "Signal hängt fest",
        0x2F => "Signal schwankt",
        0x31 => "kein Signal",
        0x41 => "Prüfsummenfehler (allgemein)",
        0x42 => "Speicherfehler (allgemein)",
        0x44 => "Datenspeicherfehler",
        0x45 => "Programmspeicherfehler",
        0x46 => "Kalibrier-/Parameterspeicherfehler",
        0x47 => "Überwachung (Watchdog) ausgelöst",
        0x48 => "Überwachungssoftware meldet Fehler",
        0x49 => "interner elektronischer Fehler",
        0x4A => "falsches Bauteil verbaut",
        0x4B => "Übertemperatur",
        0x51 => "nicht programmiert",
        0x52 => "nicht aktiviert",
        0x53 => "deaktiviert",
        0x54 => "Grundeinstellung/Anlernen fehlt",
        0x55 => "nicht konfiguriert (Codierung fehlt)",
        0x56 => "Codierung/Konfiguration ungültig",
        0x61 => "Rechenwert falsch",
        0x62 => "Vergleich zweier Signale unplausibel",
        0x63 => "Zeitüberschreitung bei Schutzfunktion",
        0x64 => "Signal unplausibel",
        0x65 => "Signal hat zu wenige Wechsel",
        0x66 => "Signal hat zu viele Wechsel",
        0x67 => "Signal falsch nach Ereignis",
        0x68 => "Ereignisinformation",
        0x71 => "Stellglied klemmt",
        0x72 => "Stellglied klemmt offen",
        0x73 => "Stellglied klemmt geschlossen",
        0x74 => "Stellglied rutscht",
        0x75 => "Notlauf aktiv",
        0x76 => "falsche Einbaulage",
        0x77 => "Sollposition nicht erreichbar",
        0x78 => "Ausrichtung/Justage falsch",
        0x79 => "mechanische Verbindung fehlerhaft",
        0x7A => "Flüssigkeitsverlust / undicht",
        0x7B => "Flüssigkeitsstand zu niedrig",
        0x81 => "ungültige Botschaft empfangen",
        0x82 => "Botschaftszähler falsch",
        0x83 => "Botschaft: Prüfsumme falsch",
        0x84 => "Botschaft: Wert zu klein",
        0x85 => "Botschaft: Wert zu groß",
        0x86 => "ungültige Botschaft",
        0x87 => "Botschaft fehlt",
        0x88 => "Bus aus (Bus-Off)",
        0x8F => "Botschaft fehlerhaft",
        0x92 => "Leistung/Funktion falsch",
        0x93 => "keine Funktion",
        0x94 => "unerwarteter Betrieb",
        0x95 => "Montage/Einbau fehlerhaft",
        0x96 => "Bauteil intern defekt",
        0x97 => "Funktion durch Fehler blockiert",
        0x98 => "Übertemperatur des Bauteils",
        _ => return None,
    })
}

/// Fehlerart für Anzeigen: „Fehlerart 13: Unterbrechung“ (ohne Text nur die Nummer).
pub fn fehlerart_kurz(ftb: u8) -> Option<String> {
    (ftb != 0).then(|| match fehlerart_text(ftb) {
        Some(t) => format!("Fehlerart {ftb:02X}: {t}"),
        None => format!("Fehlerart {ftb:02X}"),
    })
}

/// Anzahl der direkt hinterlegten Codes (ohne Regel-Gruppen).
pub fn table_len() -> usize {
    TABLE.len()
}

fn leak(s: String) -> &'static str {
    // Regel-Texte werden einmalig erzeugt und zwischengespeichert.
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<String, &'static str>>> = OnceLock::new();
    let mut c = CACHE.get_or_init(Default::default).lock().unwrap();
    if let Some(v) = c.get(&s) {
        return v;
    }
    let v: &'static str = Box::leak(s.clone().into_boxed_str());
    c.insert(s, v);
    v
}

fn pattern(code: &str) -> Option<&'static str> {
    if !code.starts_with("P0") {
        return None;
    }
    let num = code[2..].parse::<u16>().ok()?; // z. B. "0301" → 301

    // Lambdasonden P0130–P0167: je Bank/Sonde 6 Codes
    let o2_kinds = [
        "Stromkreis",
        "Spannung zu niedrig",
        "Spannung zu hoch",
        "reagiert zu langsam",
        "keine Aktivität",
        "Heizung – Stromkreis",
    ];
    let o2 = |base: u16, bank: u8, sensor: u8| -> Option<&'static str> {
        let k = num.checked_sub(base)? as usize;
        (k < 6).then(|| leak(format!("Lambdasonde Bank {bank} Sonde {sensor}: {}", o2_kinds[k])))
    };
    for (base, bank, sensor) in [(130, 1, 1), (136, 1, 2), (142, 1, 3), (150, 2, 1), (156, 2, 2), (162, 2, 3)] {
        if let Some(t) = o2(base, bank, sensor) {
            return Some(t);
        }
    }

    match num {
        // Einspritzventile
        201..=212 => Some(leak(format!("Einspritzventil Zylinder {}: Stromkreis", num - 200))),
        261..=296 => {
            let k = num - 261;
            let cyl = k / 3 + 1;
            let what = ["Stromkreis Spannung zu niedrig", "Stromkreis Spannung zu hoch", "Beitrag/Laufruhe unplausibel"][(k % 3) as usize];
            Some(leak(format!("Einspritzventil Zylinder {cyl}: {what}")))
        }
        // Verbrennungsaussetzer
        301..=312 => Some(leak(format!("Zylinder {}: Verbrennungsaussetzer erkannt", num - 300))),
        // Zündspulen A–L
        351..=362 => {
            let letter = (b'A' + (num - 351) as u8) as char;
            Some(leak(format!("Zündspule {letter}: Primär-/Sekundärkreis (Zylinder {})", num - 350)))
        }
        // Glühkerzen (Diesel)
        671..=678 => Some(leak(format!("Glühkerze Zylinder {}: Stromkreis", num - 670))),
        // Falsche Übersetzung je Gang
        731..=735 => Some(leak(format!("Getriebe: Übersetzung im {}. Gang unplausibel", num - 730))),
        _ => None,
    }
}

/// Direkt hinterlegte Codes (eigene Formulierungen).
const TABLE: &[(&str, &str)] = &[
    // --- Nockenwellenverstellung / Nockenwelle-Kurbelwelle ---
    ("P0010", "Nockenwellenversteller Einlass Bank 1: Stromkreis"),
    ("P0011", "Nockenwellenverstellung Einlass Bank 1: zu weit auf Früh / Funktion"),
    ("P0012", "Nockenwellenverstellung Einlass Bank 1: zu weit auf Spät"),
    ("P0013", "Nockenwellenversteller Auslass Bank 1: Stromkreis"),
    ("P0014", "Nockenwellenverstellung Auslass Bank 1: zu weit auf Früh / Funktion"),
    ("P0015", "Nockenwellenverstellung Auslass Bank 1: zu weit auf Spät"),
    ("P0016", "Kurbelwelle/Nockenwelle Bank 1 Geber A: Zuordnung unplausibel"),
    ("P0017", "Kurbelwelle/Nockenwelle Bank 1 Geber B: Zuordnung unplausibel"),
    ("P0018", "Kurbelwelle/Nockenwelle Bank 2 Geber A: Zuordnung unplausibel"),
    ("P0019", "Kurbelwelle/Nockenwelle Bank 2 Geber B: Zuordnung unplausibel"),
    ("P0020", "Nockenwellenversteller Einlass Bank 2: Stromkreis"),
    ("P0021", "Nockenwellenverstellung Einlass Bank 2: zu weit auf Früh / Funktion"),
    ("P0022", "Nockenwellenverstellung Einlass Bank 2: zu weit auf Spät"),
    // --- Lambdasonden-Heizung ---
    ("P0030", "Lambdasonden-Heizung Bank 1 Sonde 1: Steuerkreis"),
    ("P0031", "Lambdasonden-Heizung Bank 1 Sonde 1: Spannung zu niedrig"),
    ("P0032", "Lambdasonden-Heizung Bank 1 Sonde 1: Spannung zu hoch"),
    ("P0036", "Lambdasonden-Heizung Bank 1 Sonde 2: Steuerkreis"),
    ("P0037", "Lambdasonden-Heizung Bank 1 Sonde 2: Spannung zu niedrig"),
    ("P0038", "Lambdasonden-Heizung Bank 1 Sonde 2: Spannung zu hoch"),
    ("P0050", "Lambdasonden-Heizung Bank 2 Sonde 1: Steuerkreis"),
    ("P0056", "Lambdasonden-Heizung Bank 2 Sonde 2: Steuerkreis"),
    // --- Ladedruck ---
    ("P0033", "Schubumluftventil Turbolader: Steuerkreis"),
    ("P0034", "Schubumluftventil Turbolader: Spannung zu niedrig"),
    ("P0035", "Schubumluftventil Turbolader: Spannung zu hoch"),
    ("P0045", "Ladedruckregelventil A: Stromkreis offen"),
    ("P0046", "Ladedruckregelventil A: Bereich/Funktion"),
    ("P0047", "Ladedruckregelventil A: Spannung zu niedrig"),
    ("P0048", "Ladedruckregelventil A: Spannung zu hoch"),
    ("P0068", "Saugrohrdruck/Luftmasse zu Drosselklappenstellung unplausibel"),
    ("P0069", "Saugrohrdruck zu Umgebungsdruck unplausibel"),
    ("P0070", "Umgebungstemperaturgeber: Stromkreis"),
    ("P0071", "Umgebungstemperaturgeber: Bereich/Funktion"),
    ("P0072", "Umgebungstemperaturgeber: Spannung zu niedrig"),
    ("P0073", "Umgebungstemperaturgeber: Spannung zu hoch"),
    // --- Kraftstoffdruck ---
    ("P0087", "Kraftstoffdruck im Verteilerrohr zu niedrig"),
    ("P0088", "Kraftstoffdruck im Verteilerrohr zu hoch"),
    ("P0089", "Kraftstoffdruckregler 1: Funktion"),
    ("P0090", "Kraftstoffdruckregler 1: Steuerkreis"),
    ("P0091", "Kraftstoffdruckregler 1: Steuerkreis Spannung zu niedrig"),
    ("P0092", "Kraftstoffdruckregler 1: Steuerkreis Spannung zu hoch"),
    ("P0093", "Kraftstoffsystem: großes Leck erkannt"),
    // --- Luftmasse / Saugrohrdruck / Temperaturen ---
    ("P0100", "Luftmassen-/Luftmengenmesser: Stromkreis"),
    ("P0101", "Luftmassen-/Luftmengenmesser: Signal unplausibel"),
    ("P0102", "Luftmassen-/Luftmengenmesser: Signal zu niedrig"),
    ("P0103", "Luftmassen-/Luftmengenmesser: Signal zu hoch"),
    ("P0104", "Luftmassen-/Luftmengenmesser: Signal sporadisch"),
    ("P0105", "Saugrohrdruckgeber: Stromkreis"),
    ("P0106", "Saugrohrdruckgeber: Signal unplausibel"),
    ("P0107", "Saugrohrdruckgeber: Signal zu niedrig"),
    ("P0108", "Saugrohrdruckgeber: Signal zu hoch"),
    ("P0109", "Saugrohrdruckgeber: Signal sporadisch"),
    ("P0110", "Ansauglufttemperaturgeber: Stromkreis"),
    ("P0111", "Ansauglufttemperaturgeber: Signal unplausibel"),
    ("P0112", "Ansauglufttemperaturgeber: Signal zu niedrig"),
    ("P0113", "Ansauglufttemperaturgeber: Signal zu hoch"),
    ("P0114", "Ansauglufttemperaturgeber: Signal sporadisch"),
    ("P0115", "Kühlmitteltemperaturgeber: Stromkreis"),
    ("P0116", "Kühlmitteltemperaturgeber: Signal unplausibel"),
    ("P0117", "Kühlmitteltemperaturgeber: Signal zu niedrig"),
    ("P0118", "Kühlmitteltemperaturgeber: Signal zu hoch"),
    ("P0119", "Kühlmitteltemperaturgeber: Signal sporadisch"),
    ("P0120", "Drosselklappen-/Pedalwertgeber A: Stromkreis"),
    ("P0121", "Drosselklappen-/Pedalwertgeber A: Signal unplausibel"),
    ("P0122", "Drosselklappen-/Pedalwertgeber A: Signal zu niedrig"),
    ("P0123", "Drosselklappen-/Pedalwertgeber A: Signal zu hoch"),
    ("P0124", "Drosselklappen-/Pedalwertgeber A: Signal sporadisch"),
    ("P0125", "Kühlmitteltemperatur zu niedrig für Lambdaregelung"),
    ("P0128", "Thermostat: Kühlmitteltemperatur unter Regeltemperatur"),
    // --- Gemischregelung ---
    ("P0170", "Gemischregelung Bank 1: Fehlfunktion"),
    ("P0171", "Gemisch zu mager (Bank 1)"),
    ("P0172", "Gemisch zu fett (Bank 1)"),
    ("P0173", "Gemischregelung Bank 2: Fehlfunktion"),
    ("P0174", "Gemisch zu mager (Bank 2)"),
    ("P0175", "Gemisch zu fett (Bank 2)"),
    ("P0180", "Kraftstofftemperaturgeber A: Stromkreis"),
    ("P0181", "Kraftstofftemperaturgeber A: Signal unplausibel"),
    ("P0190", "Kraftstoffdruckgeber Verteilerrohr: Stromkreis"),
    ("P0191", "Kraftstoffdruckgeber Verteilerrohr: Signal unplausibel"),
    ("P0192", "Kraftstoffdruckgeber Verteilerrohr: Signal zu niedrig"),
    ("P0193", "Kraftstoffdruckgeber Verteilerrohr: Signal zu hoch"),
    ("P0200", "Einspritzventile: Stromkreis"),
    // --- Drosselklappe B / Pumpe / Lader ---
    ("P0220", "Drosselklappen-/Pedalwertgeber B: Stromkreis"),
    ("P0221", "Drosselklappen-/Pedalwertgeber B: Signal unplausibel"),
    ("P0222", "Drosselklappen-/Pedalwertgeber B: Signal zu niedrig"),
    ("P0223", "Drosselklappen-/Pedalwertgeber B: Signal zu hoch"),
    ("P0230", "Kraftstoffpumpe: Primärkreis"),
    ("P0234", "Ladedruck: Regelgrenze überschritten (Überladung)"),
    ("P0235", "Ladedruckgeber A: Stromkreis"),
    ("P0236", "Ladedruckgeber A: Signal unplausibel"),
    ("P0237", "Ladedruckgeber A: Signal zu niedrig"),
    ("P0238", "Ladedruckgeber A: Signal zu hoch"),
    ("P0243", "Ladedruckregelventil A (Wastegate): Stromkreis"),
    ("P0244", "Ladedruckregelventil A (Wastegate): Bereich/Funktion"),
    ("P0245", "Ladedruckregelventil A (Wastegate): Spannung zu niedrig"),
    ("P0246", "Ladedruckregelventil A (Wastegate): Spannung zu hoch"),
    ("P0299", "Ladedruck: Regelgrenze unterschritten (zu wenig Ladedruck)"),
    // --- Aussetzer / Klopfen / Geber ---
    ("P0300", "Verbrennungsaussetzer erkannt (mehrere Zylinder / zufällig)"),
    ("P0320", "Drehzahlgeber Zündung/Verteiler: Stromkreis"),
    ("P0321", "Drehzahlgeber Zündung/Verteiler: Signal unplausibel"),
    ("P0322", "Drehzahlgeber Zündung/Verteiler: kein Signal"),
    ("P0325", "Klopfsensor 1 (Bank 1): Stromkreis"),
    ("P0326", "Klopfsensor 1 (Bank 1): Signal unplausibel"),
    ("P0327", "Klopfsensor 1 (Bank 1): Signal zu niedrig"),
    ("P0328", "Klopfsensor 1 (Bank 1): Signal zu hoch"),
    ("P0329", "Klopfsensor 1 (Bank 1): Signal sporadisch"),
    ("P0330", "Klopfsensor 2 (Bank 2): Stromkreis"),
    ("P0331", "Klopfsensor 2 (Bank 2): Signal unplausibel"),
    ("P0332", "Klopfsensor 2 (Bank 2): Signal zu niedrig"),
    ("P0333", "Klopfsensor 2 (Bank 2): Signal zu hoch"),
    ("P0335", "Kurbelwellengeber A: Stromkreis"),
    ("P0336", "Kurbelwellengeber A: Signal unplausibel"),
    ("P0337", "Kurbelwellengeber A: Signal zu niedrig"),
    ("P0338", "Kurbelwellengeber A: Signal zu hoch"),
    ("P0339", "Kurbelwellengeber A: Signal sporadisch"),
    ("P0340", "Nockenwellengeber A (Bank 1): Stromkreis"),
    ("P0341", "Nockenwellengeber A (Bank 1): Signal unplausibel"),
    ("P0342", "Nockenwellengeber A (Bank 1): Signal zu niedrig"),
    ("P0343", "Nockenwellengeber A (Bank 1): Signal zu hoch"),
    ("P0344", "Nockenwellengeber A (Bank 1): Signal sporadisch"),
    ("P0345", "Nockenwellengeber A (Bank 2): Stromkreis"),
    ("P0346", "Nockenwellengeber A (Bank 2): Signal unplausibel"),
    ("P0350", "Zündspulen: Primär-/Sekundärkreis"),
    ("P0365", "Nockenwellengeber B (Bank 1): Stromkreis"),
    ("P0366", "Nockenwellengeber B (Bank 1): Signal unplausibel"),
    ("P0380", "Glühkerzen/Heizung Kreis A: Stromkreis"),
    ("P0381", "Glühzeit-Kontrollleuchte: Stromkreis"),
    // --- Abgasrückführung / Sekundärluft / Katalysator ---
    ("P0400", "Abgasrückführung: Durchfluss fehlerhaft"),
    ("P0401", "Abgasrückführung: Durchfluss zu gering"),
    ("P0402", "Abgasrückführung: Durchfluss zu groß"),
    ("P0403", "Abgasrückführungsventil: Stromkreis"),
    ("P0404", "Abgasrückführung: Bereich/Funktion"),
    ("P0405", "AGR-Positionsgeber A: Signal zu niedrig"),
    ("P0406", "AGR-Positionsgeber A: Signal zu hoch"),
    ("P0410", "Sekundärluftsystem: Fehlfunktion"),
    ("P0411", "Sekundärluftsystem: Durchfluss unplausibel"),
    ("P0412", "Sekundärluft-Umschaltventil A: Stromkreis"),
    ("P0418", "Sekundärluftpumpen-Relais A: Stromkreis"),
    ("P0420", "Katalysator Bank 1: Wirkung unter Schwellwert"),
    ("P0421", "Vorkatalysator Bank 1: Wirkung unter Schwellwert"),
    ("P0430", "Katalysator Bank 2: Wirkung unter Schwellwert"),
    ("P0431", "Vorkatalysator Bank 2: Wirkung unter Schwellwert"),
    // --- Tankentlüftung ---
    ("P0440", "Tankentlüftung (EVAP): Fehlfunktion"),
    ("P0441", "Tankentlüftung: Spüldurchfluss unplausibel"),
    ("P0442", "Tankentlüftung: kleines Leck erkannt"),
    ("P0443", "Tankentlüftungsventil: Stromkreis"),
    ("P0444", "Tankentlüftungsventil: Stromkreis offen"),
    ("P0446", "Tankentlüftung Belüftungsventil: Stromkreis"),
    ("P0450", "Tankdruckgeber: Stromkreis"),
    ("P0452", "Tankdruckgeber: Spannung zu niedrig"),
    ("P0453", "Tankdruckgeber: Spannung zu hoch"),
    ("P0455", "Tankentlüftung: großes Leck erkannt"),
    ("P0456", "Tankentlüftung: sehr kleines Leck erkannt"),
    ("P0458", "Magnetventil Aktivkohlebehälter: Stromkreis Spannung zu niedrig"),
    ("P0459", "Magnetventil Aktivkohlebehälter: Stromkreis Spannung zu hoch"),
    ("P0460", "Tankfüllstandsgeber: Stromkreis"),
    ("P0461", "Tankfüllstandsgeber: Signal unplausibel"),
    // --- Abgasdruck / -temperatur ---
    ("P0470", "Abgasdruckgeber A: Stromkreis"),
    ("P0471", "Abgasdruckgeber A: Signal unplausibel"),
    ("P0472", "Abgasdruckgeber A: Signal zu niedrig"),
    ("P0473", "Abgasdruckgeber A: Signal zu hoch"),
    ("P0480", "Kühlerlüfter 1: Steuerkreis"),
    ("P0481", "Kühlerlüfter 2: Steuerkreis"),
    ("P0483", "Kühlerlüfter: Plausibilitätsprüfung fehlgeschlagen"),
    // --- Geschwindigkeit / Leerlauf ---
    ("P0500", "Geschwindigkeitsgeber A: Fehlfunktion"),
    ("P0501", "Geschwindigkeitsgeber A: Signal unplausibel"),
    ("P0502", "Geschwindigkeitsgeber A: Signal zu niedrig"),
    ("P0503", "Geschwindigkeitsgeber A: Signal sporadisch/zu hoch"),
    ("P0504", "Bremslichtschalter A/B: Signale unplausibel zueinander"),
    ("P0505", "Leerlaufregelung: Fehlfunktion"),
    ("P0506", "Leerlaufdrehzahl niedriger als erwartet"),
    ("P0507", "Leerlaufdrehzahl höher als erwartet"),
    ("P0520", "Öldruckgeber/-schalter: Stromkreis"),
    ("P0521", "Öldruckgeber/-schalter: Signal unplausibel"),
    ("P0524", "Motoröldruck zu niedrig"),
    ("P0530", "Kältemitteldruckgeber Klimaanlage A: Stromkreis"),
    ("P0532", "Kältemitteldruckgeber Klimaanlage A: Spannung zu niedrig"),
    ("P0533", "Kältemitteldruckgeber Klimaanlage A: Spannung zu hoch"),
    ("P0544", "Abgastemperaturgeber Bank 1 Sonde 1: Stromkreis"),
    ("P0545", "Abgastemperaturgeber Bank 1 Sonde 1: Signal zu niedrig"),
    ("P0546", "Abgastemperaturgeber Bank 1 Sonde 1: Signal zu hoch"),
    ("P0555", "Bremskraftverstärker-Drucksensor: Stromkreis"),
    ("P0556", "Bremskraftverstärker-Drucksensor: Signal unplausibel"),
    // --- Bordnetz / Bremslicht / GRA ---
    ("P0560", "Bordnetzspannung: Fehlfunktion"),
    ("P0562", "Bordnetzspannung zu niedrig"),
    ("P0563", "Bordnetzspannung zu hoch"),
    ("P0564", "Geschwindigkeitsregelanlage (GRA): Bedienhebel-Signal A"),
    ("P0571", "Bremslichtschalter A: Stromkreis"),
    // --- Steuergerät intern ---
    ("P0600", "Serielle Kommunikationsverbindung: Fehlfunktion"),
    ("P0601", "Steuergerät: Prüfsumme Speicher fehlerhaft"),
    ("P0602", "Steuergerät: nicht programmiert / Programmierfehler"),
    ("P0603", "Steuergerät: Dauerspeicher (KAM) fehlerhaft"),
    ("P0604", "Steuergerät: Arbeitsspeicher (RAM) fehlerhaft"),
    ("P0605", "Steuergerät: Festwertspeicher (ROM) fehlerhaft"),
    ("P0606", "Steuergerät: Prozessor fehlerhaft"),
    ("P0607", "Steuergerät: Funktion fehlerhaft"),
    ("P0615", "Anlasser-Relais: Stromkreis"),
    ("P0620", "Generator: Steuerkreis"),
    ("P0628", "Kraftstoffpumpe A: Steuerkreis Spannung zu niedrig"),
    ("P0629", "Kraftstoffpumpe A: Steuerkreis Spannung zu hoch"),
    ("P0627", "Kraftstoffpumpe: Steuerkreis offen"),
    ("P0638", "Drosselklappensteller Bank 1: Bereich/Funktion"),
    ("P0641", "Sensor-Referenzspannung A: Stromkreis offen"),
    ("P0651", "Sensor-Referenzspannung B: Stromkreis offen"),
    ("P0670", "Glühzeitsteuergerät: Steuerkreis"),
    ("P0685", "Hauptrelais Motorsteuerung: Steuerkreis/Unterbrechung"),
    // --- Getriebe ---
    ("P0700", "Getriebesteuerung: Fehler gespeichert (dort auslesen)"),
    ("P0705", "Wählhebel-Positionsgeber (PRNDL): Stromkreis"),
    ("P0710", "Getriebeöltemperaturgeber A: Stromkreis"),
    ("P0711", "Getriebeöltemperaturgeber A: Signal unplausibel"),
    ("P0715", "Getriebe-Eingangsdrehzahlgeber A: Stromkreis"),
    ("P0716", "Getriebe-Eingangsdrehzahlgeber A: Signal unplausibel"),
    ("P0720", "Getriebe-Ausgangsdrehzahlgeber: Stromkreis"),
    ("P0721", "Getriebe-Ausgangsdrehzahlgeber: Signal unplausibel"),
    ("P0730", "Getriebe: Übersetzung unplausibel"),
    ("P0736", "Getriebe: Übersetzung im Rückwärtsgang unplausibel"),
    ("P0740", "Wandlerüberbrückungskupplung: Stromkreis"),
    ("P0741", "Wandlerüberbrückungskupplung: Funktion / bleibt offen"),
    ("P0750", "Magnetventil A (Schaltung): Stromkreis"),
    ("P0755", "Magnetventil B (Schaltung): Stromkreis"),
    ("P0760", "Magnetventil C (Schaltung): Stromkreis"),
    ("P0765", "Magnetventil D (Schaltung): Stromkreis"),
    ("P0770", "Magnetventil E (Schaltung): Stromkreis"),
    ("P0841", "Getriebeöldruckgeber/-schalter A: Signal unplausibel"),
    // --- P2xxx: Drosselklappe, Saugrohrklappen, Gemisch, DPF ---
    ("P2002", "Partikelfilter Bank 1: Wirkung unter Schwellwert"),
    ("P2004", "Saugrohrklappen Bank 1: klemmen offen"),
    ("P2008", "Saugrohrklappen Bank 1: Steuerkreis offen"),
    ("P2088", "Nockenwellenverstellung A (Einlass) Bank 1: Steuerkreis Spannung zu niedrig"),
    ("P2089", "Nockenwellenverstellung A (Einlass) Bank 1: Steuerkreis Spannung zu hoch"),
    ("P2090", "Nockenwellenverstellung B (Auslass) Bank 1: Steuerkreis Spannung zu niedrig"),
    ("P2091", "Nockenwellenverstellung B (Auslass) Bank 1: Steuerkreis Spannung zu hoch"),
    ("P2015", "Saugrohrklappen-Positionsgeber Bank 1: Signal unplausibel"),
    ("P2096", "Gemischregelung nach Katalysator zu mager (Bank 1)"),
    ("P2097", "Gemischregelung nach Katalysator zu fett (Bank 1)"),
    ("P2100", "Drosselklappensteller-Motor: Stromkreis offen"),
    ("P2101", "Drosselklappensteller-Motor: Bereich/Funktion"),
    ("P2106", "Drosselklappensteller: Notlauf – Leistung begrenzt"),
    ("P2111", "Drosselklappensteller: klemmt offen"),
    ("P2112", "Drosselklappensteller: klemmt geschlossen"),
    ("P2119", "Drosselklappensteller: Drosselklappe Bereich/Funktion"),
    ("P2122", "Pedalwertgeber D: Signal zu niedrig"),
    ("P2123", "Pedalwertgeber D: Signal zu hoch"),
    ("P2127", "Pedalwertgeber E: Signal zu niedrig"),
    ("P2128", "Pedalwertgeber E: Signal zu hoch"),
    ("P2135", "Drosselklappengeber A/B: Signale unplausibel zueinander"),
    ("P2138", "Pedalwertgeber D/E: Signale unplausibel zueinander"),
    ("P2177", "Gemisch außerhalb Leerlauf zu mager (Bank 1)"),
    ("P2178", "Gemisch außerhalb Leerlauf zu fett (Bank 1)"),
    ("P2181", "Kühlsystem: Funktion"),
    ("P2187", "Gemisch im Leerlauf zu mager (Bank 1)"),
    ("P2188", "Gemisch im Leerlauf zu fett (Bank 1)"),
    ("P2195", "Lambdasonde Bank 1 Sonde 1: Signal bleibt mager"),
    ("P2196", "Lambdasonde Bank 1 Sonde 1: Signal bleibt fett"),
    ("P2237", "Lambdasonde Bank 1 Sonde 1: Pumpstromkreis offen"),
    ("P2270", "Lambdasonde Bank 1 Sonde 2: Signal bleibt mager"),
    ("P2271", "Lambdasonde Bank 1 Sonde 2: Signal bleibt fett"),
    ("P2279", "Ansaugsystem: Falschluft erkannt"),
    ("P2293", "Kraftstoffdruckregler 2 (bei TSI: Hochdruck-Regelventil): Funktion"),
    ("P2400", "Leckdiagnosepumpe Tankentlüftung: Steuerkreis offen"),
    ("P2401", "Leckdiagnosepumpe Tankentlüftung: Spannung zu niedrig"),
    ("P2402", "Leckdiagnosepumpe Tankentlüftung: Spannung zu hoch"),
    ("P2404", "Leckdiagnosepumpe Tankentlüftung: Rückmeldung unplausibel"),
    ("P242F", "Partikelfilter: Durchfluss eingeschränkt (Asche)"),
    ("P2452", "Partikelfilter-Differenzdruckgeber A: Stromkreis"),
    ("P2453", "Partikelfilter-Differenzdruckgeber A: Signal unplausibel"),
    ("P2454", "Partikelfilter-Differenzdruckgeber A: Signal zu niedrig"),
    ("P2455", "Partikelfilter-Differenzdruckgeber A: Signal zu hoch"),
    ("P2463", "Partikelfilter: Rußbeladung zu hoch"),
    ("P2459", "Partikelfilter: Regeneration zu häufig"),
    ("P2562", "Lageerkennung Ladedrucksteller A: Stromkreis"),
    ("P2563", "Lageerkennung Ladedrucksteller A: Bereich/Funktion"),
    ("P2626", "Lambdasonde Bank 1 Sonde 1: Pumpstrom-Abgleich Stromkreis/Unterbrechung"),
    ("P2539", "Kraftstoff-Niederdruckgeber: Stromkreis"),
    ("P2610", "Steuergerät: interner Motor-aus-Zeitgeber unplausibel"),
    // --- Netzwerk (U-Codes) ---
    // Datenbus (SAE J2012, U00xx)
    ("U0001", "CAN-Bus (High-Speed): Kommunikationsfehler"),
    ("U0002", "CAN-Bus (High-Speed): Funktion gestört"),
    ("U0003", "CAN-Bus (High-Speed), Leitung High: Unterbrechung"),
    ("U0004", "CAN-Bus (High-Speed), Leitung High: Kurzschluss nach Masse"),
    ("U0005", "CAN-Bus (High-Speed), Leitung High: Kurzschluss nach Plus"),
    ("U0006", "CAN-Bus (High-Speed), Leitung Low: Unterbrechung"),
    ("U0007", "CAN-Bus (High-Speed), Leitung Low: Kurzschluss nach Masse"),
    ("U0008", "CAN-Bus (High-Speed), Leitung Low: Kurzschluss nach Plus"),
    ("U0009", "CAN-Bus (High-Speed): Leitungen High und Low untereinander kurzgeschlossen"),
    ("U0010", "CAN-Bus (mittlere Geschwindigkeit): Kommunikationsfehler"),
    ("U0011", "CAN-Bus (mittlere Geschwindigkeit): Funktion gestört"),
    ("U0020", "CAN-Bus (Low-Speed): Kommunikationsfehler"),
    ("U0021", "CAN-Bus (Low-Speed): Funktion gestört"),
    ("U0073", "Steuergeräte-Datenbus A: Bus aus"),
    ("U0074", "Steuergeräte-Datenbus B: Bus aus"),
    // Keine Kommunikation mit … (U01xx/U02xx)
    ("U0100", "Keine Kommunikation mit Motorsteuergerät"),
    ("U0101", "Keine Kommunikation mit Getriebesteuergerät"),
    ("U0102", "Keine Kommunikation mit Verteilergetriebe-Steuergerät"),
    ("U0103", "Keine Kommunikation mit Wählhebel-Steuergerät"),
    ("U0104", "Keine Kommunikation mit Tempomat-Steuergerät"),
    ("U0105", "Keine Kommunikation mit Einspritz-Steuergerät"),
    ("U0106", "Keine Kommunikation mit Vorglüh-Steuergerät"),
    ("U0107", "Keine Kommunikation mit Drosselklappen-Steuergerät"),
    ("U0109", "Keine Kommunikation mit Kraftstoffpumpen-Steuergerät"),
    ("U0121", "Keine Kommunikation mit ABS-Steuergerät"),
    ("U0122", "Keine Kommunikation mit Fahrdynamik-Steuergerät (ESP)"),
    ("U0123", "Keine Kommunikation mit Drehratensensor"),
    ("U0124", "Keine Kommunikation mit Querbeschleunigungssensor"),
    ("U0126", "Keine Kommunikation mit Lenkwinkelsensor"),
    ("U0127", "Keine Kommunikation mit Reifendruck-Kontrolle"),
    ("U0128", "Keine Kommunikation mit Feststellbremse-Steuergerät"),
    ("U0129", "Keine Kommunikation mit Bremsen-Steuergerät"),
    ("U0131", "Keine Kommunikation mit Lenkhilfe-Steuergerät"),
    ("U0140", "Keine Kommunikation mit Bordnetzsteuergerät"),
    ("U0141", "Keine Kommunikation mit Bordnetzsteuergerät A"),
    ("U0142", "Keine Kommunikation mit Bordnetzsteuergerät B"),
    ("U0146", "Keine Kommunikation mit Gateway"),
    ("U0151", "Keine Kommunikation mit Airbag-Steuergerät"),
    ("U0155", "Keine Kommunikation mit Kombiinstrument"),
    ("U0156", "Keine Kommunikation mit Informationsanzeige A"),
    ("U0159", "Keine Kommunikation mit Einparkhilfe-Steuergerät"),
    ("U0164", "Keine Kommunikation mit Klima-Steuergerät"),
    ("U0167", "Keine Kommunikation mit Wegfahrsperre"),
    ("U0168", "Keine Kommunikation mit Diebstahlwarnanlage"),
    ("U0184", "Keine Kommunikation mit Radio"),
    ("U0199", "Keine Kommunikation mit Türsteuergerät A"),
    ("U0200", "Keine Kommunikation mit Türsteuergerät B"),
    ("U0201", "Keine Kommunikation mit Türsteuergerät C"),
    ("U0202", "Keine Kommunikation mit Türsteuergerät D"),
    ("U0212", "Keine Kommunikation mit Lenksäulen-Steuergerät"),
    ("U0214", "Keine Kommunikation mit Funkfernbedienung/Zugang"),
    // Software passt nicht (U03xx)
    ("U0300", "Software des Steuergeräts passt nicht (intern)"),
    ("U0301", "Software passt nicht zum Motorsteuergerät"),
    ("U0302", "Software passt nicht zum Getriebesteuergerät"),
    ("U0315", "Software passt nicht zum ABS-Steuergerät"),
    ("U0316", "Software passt nicht zum Fahrdynamik-Steuergerät"),
    ("U0322", "Software passt nicht zum Bordnetzsteuergerät"),
    ("U0323", "Software passt nicht zum Kombiinstrument"),
    // Unplausible Daten (U04xx)
    ("U0401", "Unplausible Daten vom Motorsteuergerät empfangen"),
    ("U0402", "Unplausible Daten vom Getriebesteuergerät empfangen"),
    ("U0415", "Unplausible Daten vom ABS-Steuergerät empfangen"),
    ("U0416", "Unplausible Daten vom Fahrdynamik-Steuergerät empfangen"),
    ("U0418", "Unplausible Daten vom Bremsen-Steuergerät empfangen"),
    ("U0420", "Unplausible Daten vom Lenkhilfe-Steuergerät empfangen"),
    ("U0422", "Unplausible Daten vom Bordnetzsteuergerät empfangen"),
    ("U0423", "Unplausible Daten vom Kombiinstrument empfangen"),
    ("U0428", "Unplausible Daten vom Lenkwinkelsensor empfangen"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_and_patterns() {
        assert_eq!(lookup("P0300"), Some("Verbrennungsaussetzer erkannt (mehrere Zylinder / zufällig)"));
        assert_eq!(lookup("p0303"), Some("Zylinder 3: Verbrennungsaussetzer erkannt"));
        assert_eq!(lookup("P0135"), Some("Lambdasonde Bank 1 Sonde 1: Heizung – Stromkreis"));
        assert_eq!(lookup("P0157"), Some("Lambdasonde Bank 2 Sonde 2: Spannung zu niedrig"));
        assert_eq!(lookup("P0264"), Some("Einspritzventil Zylinder 2: Stromkreis Spannung zu niedrig"));
        assert_eq!(lookup("P0296"), Some("Einspritzventil Zylinder 12: Beitrag/Laufruhe unplausibel"));
        assert_eq!(lookup("P0353"), Some("Zündspule C: Primär-/Sekundärkreis (Zylinder 3)"));
        assert!(lookup("P0242F").is_none());
        assert!(lookup("P1234").is_none()); // herstellerspezifisch
        assert!(table_len() > 200);
        assert_eq!(lookup("U0167"), Some("Keine Kommunikation mit Wegfahrsperre"));
        assert_eq!(lookup("U0250"), Some("Keine Kommunikation mit einem anderen Steuergerät (genormter Code)"));
        assert_eq!(lookup("U04A0"), Some("Unplausible Daten von einem anderen Steuergerät (genormter Code)"));
        assert!(lookup("U1100").is_none()); // herstellerspezifisch
    }

    #[test]
    fn fehlerarten() {
        assert_eq!(fehlerart_kurz(0), None);
        assert_eq!(fehlerart_kurz(0x13).as_deref(), Some("Fehlerart 13: Unterbrechung"));
        assert_eq!(fehlerart_kurz(0xF0).as_deref(), Some("Fehlerart F0"));
    }

    #[test]
    fn no_duplicate_codes() {
        let mut seen = std::collections::HashSet::new();
        for (c, _) in TABLE {
            assert!(seen.insert(*c), "doppelt: {c}");
            assert!(pattern(c).is_none(), "{c} steht doppelt (Tabelle + Regel)");
        }
    }
}
