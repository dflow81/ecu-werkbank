//! Katalog üblicher VAG-Grundeinstellungen und Stellglied-/Werkstattfunktionen
//! — reine Nachschlage-Referenz (kein Eingriff ins Auto). Zeigt, was es gibt,
//! wozu es dient und worauf zu achten ist. Das Ausführen selbst passiert über
//! die passenden Befehle (`stellen`, `service`, VCDS) und nur am stehenden Auto.
//!
//! Die Liste ist bewusst knapp und allgemein gehalten (keine geschützten
//! Hersteller-Daten); die genaue Kanal-/Routine-Nummer ist je Modell anders.

/// Wie heikel ein Vorgang ist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stufe {
    /// unkritisch, gut umkehrbar
    Harmlos,
    /// mit Bedacht; falsch gemacht Komfort-/Fahrverhalten betroffen
    Vorsicht,
    /// sicherheitsrelevant — nur mit Sachverstand / Fachbetrieb
    Kritisch,
}

impl Stufe {
    pub fn text(self) -> &'static str {
        match self {
            Stufe::Harmlos => "harmlos",
            Stufe::Vorsicht => "mit Bedacht",
            Stufe::Kritisch => "sicherheitsrelevant",
        }
    }
    pub fn ampel(self) -> &'static str {
        match self {
            Stufe::Harmlos => "🟢",
            Stufe::Vorsicht => "🟡",
            Stufe::Kritisch => "🔴",
        }
    }
}

/// Ein Eintrag im Katalog.
pub struct Eintrag {
    /// Steuergerät/System (für die Filterung), z. B. "Motor", "Bremse".
    pub system: &'static str,
    /// Kurzname der Funktion.
    pub name: &'static str,
    /// Wozu man das macht.
    pub zweck: &'static str,
    /// Worauf zu achten ist.
    pub hinweis: &'static str,
    pub stufe: Stufe,
}

use Stufe::*;

pub const KATALOG: &[Eintrag] = &[
    Eintrag { system: "Motor", name: "Drosselklappe anlernen", zweck: "Nach Reinigung oder Tausch der Drosselklappe die Endanschläge neu anlernen.", hinweis: "Zündung an, Motor aus, nichts berühren, bis fertig. Danach kurz warten.", stufe: Vorsicht },
    Eintrag { system: "Motor", name: "Leerlauf-/Gemischanpassung zurücksetzen", zweck: "Lernwerte nach Reparatur (Falschluft, Sensoren) zurücksetzen.", hinweis: "Nach dem Reset eine Lernfahrt/Leerlaufphase einplanen.", stufe: Harmlos },
    Eintrag { system: "Motor", name: "Bereitschaft (Readiness) herstellen", zweck: "Abgas-Selbsttests für die AU/HU durchlaufen lassen.", hinweis: "Folgt einem festen Fahrprofil; nicht sicherheitskritisch.", stufe: Harmlos },
    Eintrag { system: "Motor", name: "Partikelfilter-Regeneration (Diesel)", zweck: "Rußbeladung im Stand abbrennen, wenn die Fahr-Regeneration nicht reicht.", hinweis: "Nur im Freien, heiß! Abstand halten, Motor überwacht laufen lassen.", stufe: Kritisch },
    Eintrag { system: "Motor", name: "Injektor-Mengenabgleich (IMA/Einspritzmengen)", zweck: "Nach Injektor-Tausch die aufgedruckten Korrekturwerte eintragen.", hinweis: "Falsche Werte → Laufunruhe. Codes genau vom Injektor übernehmen.", stufe: Vorsicht },
    Eintrag { system: "Getriebe", name: "Getriebe-Grundeinstellung (DSG/Automatik)", zweck: "Nach Service/Reparatur Kupplungs-/Druckwerte neu anlernen.", hinweis: "Genaues Modell-Vorgehen beachten (Temperaturfenster, Wählhebel).", stufe: Vorsicht },
    Eintrag { system: "Bremse", name: "Lenkwinkelsensor G85 Grundeinstellung", zweck: "Nach Spur-/Fahrwerksarbeiten oder Lenkungstausch den Nullpunkt setzen.", hinweis: "Räder exakt geradeaus. Danach Probefahrt, ESP-Leuchte muss aus sein.", stufe: Kritisch },
    Eintrag { system: "Bremse", name: "ABS/ESP entlüften", zweck: "Luft aus dem Hydroaggregat bekommen (nach Bremsarbeiten).", hinweis: "Nur mit richtigem Ablauf und genug Bremsflüssigkeit — sonst kein Bremsdruck.", stufe: Kritisch },
    Eintrag { system: "Bremse", name: "Elektrische Parkbremse: Service-Modus", zweck: "Bremskolben hinten zum Belagwechsel auf-/zufahren.", hinweis: "Fahrzeug sichern. Nach dem Wechsel Service-Modus beenden und Funktion prüfen.", stufe: Kritisch },
    Eintrag { system: "Kombi", name: "Service-Intervall zurückstellen", zweck: "Nach Ölwechsel/Inspektion die Anzeige zurücksetzen.", hinweis: "In ECUWerkbank über `service`. Richtiges Intervall (fest/flexibel) wählen.", stufe: Harmlos },
    Eintrag { system: "Lenkung", name: "Servolenkung anlernen", zweck: "Nach Tausch/Spannungsabfall die Lenkung neu initialisieren.", hinweis: "Lenkung von Anschlag zu Anschlag, dann geradeaus.", stufe: Vorsicht },
    Eintrag { system: "Komfort", name: "Fensterheber / Schiebedach neu anlernen", zweck: "Automatik (ein Tastendruck) nach Batterietrennung wiederherstellen.", hinweis: "Taster in Endstellung halten — modellabhängige Abfolge.", stufe: Harmlos },
    Eintrag { system: "Licht", name: "Leuchtweite/Scheinwerfer Grundeinstellung", zweck: "Nach Scheinwerfer- oder Sensortausch die Nulllage setzen.", hinweis: "Fahrzeug eben, unbeladen. Danach Einstellung prüfen.", stufe: Vorsicht },
    Eintrag { system: "Airbag", name: "Gurtstraffer/Crashdaten (nach Auslösung)", zweck: "Nach einem Unfall ausgelöste Zähler/Daten behandeln.", hinweis: "Sicherheitsrelevant — in der Regel Fachbetrieb. Kein gebrauchtes Teil blind einbauen.", stufe: Kritisch },
    Eintrag { system: "Gateway", name: "Transportmodus aus/ein", zweck: "Energiesparmodus ab Werk aus- oder einschalten.", hinweis: "Falscher Zustand → Verbraucher schlafen/wachen ungewollt.", stufe: Vorsicht },
];

/// Katalog filtern nach System und/oder Suchtext (beides optional, Kleinschreibung egal).
pub fn filtern<'a>(system: Option<&str>, suche: Option<&str>) -> Vec<&'a Eintrag> {
    let s = system.map(str::to_lowercase);
    let q = suche.map(str::to_lowercase);
    KATALOG
        .iter()
        .filter(|e| s.as_deref().is_none_or(|s| e.system.to_lowercase().contains(s)))
        .filter(|e| {
            q.as_deref().is_none_or(|q| {
                e.name.to_lowercase().contains(q) || e.zweck.to_lowercase().contains(q) || e.system.to_lowercase().contains(q)
            })
        })
        .collect()
}

/// Alle vorkommenden Systeme (für die Hilfe).
pub fn systeme() -> Vec<&'static str> {
    let mut v: Vec<&str> = KATALOG.iter().map(|e| e.system).collect();
    v.sort_unstable();
    v.dedup();
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_nach_system_und_suche() {
        assert!(filtern(Some("bremse"), None).len() >= 3);
        assert!(filtern(None, Some("g85")).iter().any(|e| e.name.contains("G85")));
        assert!(filtern(None, Some("öl")).iter().any(|e| e.system == "Kombi") || filtern(None, Some("service")).iter().any(|e| e.system == "Kombi"));
        assert_eq!(filtern(Some("gibtsnicht"), None).len(), 0);
        assert!(systeme().contains(&"Motor"));
    }
}
