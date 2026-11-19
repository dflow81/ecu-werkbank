//! Eigene Texte für Messwerte, die das Steuergerät als **Textnummer** liefert
//! (TP 2.0, Formel 0x25: „roh F25 02 FD“ → „normal“).
//!
//! Die Tabelle gehört dem Nutzer: `<Datenordner>/wertetexte.toml`. Sie wird
//! nie mit dem Programm ausgeliefert und nie hochgeladen. Füllen lässt sie sich
//! von Hand (`wertetext`) oder durch Abgleich: was das eigene Werkstattgerät am
//! eigenen Auto angezeigt hat (Export der Messwertblöcke als CSV) gegen die
//! Rohwerte, die ECUWerkbank zur selben Zeit gelesen hat (`wertetexte-lernen`).
//! Übernommen wird eine Zuordnung nur, wenn sie überall gleich ist.

use anyhow::{Context, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::RwLock;

/// Schlüssel einer Textnummer, z. B. „F25:02FD“.
pub fn schluessel(formel: u8, a: u8, b: u8) -> String {
    format!("F{formel:02X}:{a:02X}{b:02X}")
}

pub fn pfad() -> PathBuf {
    crate::history::data_dir().join("wertetexte.toml")
}

fn datei_laden() -> BTreeMap<String, String> {
    std::fs::read_to_string(pfad()).ok().and_then(|t| toml::from_str(&t).ok()).unwrap_or_default()
}

static TABELLE: RwLock<Option<BTreeMap<String, String>>> = RwLock::new(None);

/// Text zu einer Textnummer (Tabelle wird beim ersten Zugriff geladen).
pub fn text(formel: u8, a: u8, b: u8) -> Option<String> {
    if let Ok(g) = TABELLE.read() {
        if let Some(t) = g.as_ref() {
            return t.get(&schluessel(formel, a, b)).cloned();
        }
    }
    let t = datei_laden();
    let r = t.get(&schluessel(formel, a, b)).cloned();
    if let Ok(mut g) = TABELLE.write() {
        *g = Some(t);
    }
    r
}

/// Alle eigenen Texte.
pub fn alle() -> BTreeMap<String, String> {
    datei_laden()
}

/// Texte speichern (leerer Text löscht den Eintrag) und die Anzeige auffrischen.
pub fn mehrere_setzen(liste: &[(String, String)]) -> Result<()> {
    let mut t = datei_laden();
    for (k, v) in liste {
        if v.trim().is_empty() {
            t.remove(k);
        } else {
            t.insert(k.clone(), v.trim().to_string());
        }
    }
    let p = pfad();
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    let mut text = String::from("# Eigene Texte für Messwerte (Textnummern). Nur auf diesem Rechner — nicht weitergeben.\n");
    text.push_str(&toml::to_string(&t)?);
    std::fs::write(&p, text).with_context(|| crate::tr!("{a} schreiben", a = p.display()))?;
    if let Ok(mut g) = TABELLE.write() {
        *g = Some(t);
    }
    Ok(())
}

/// „F25:02FD“ oder „25 02 FD“ → (Formel, a, b).
pub fn schluessel_lesen(s: &str) -> Option<(u8, u8, u8)> {
    let h: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    let h = h.strip_prefix(['F', 'f']).filter(|r| r.len() == 6).unwrap_or(&h);
    if h.len() != 6 {
        return None;
    }
    let b = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some((b(0)?, b(2)?, b(4)?))
}

/// Ein Rohwert aus ECUWerkbank: (Block, Feld 1–4, Formel, a, b).
pub type Roh = (u8, u8, u8, u8, u8);

/// Rohwerte aus der Ausgabe von `kwp-blockscan` lesen („Feld 2: roh F25 02 FD“).
pub fn roh_aus_blockscan(text: &str) -> Vec<Roh> {
    let mut out = Vec::new();
    for z in text.lines() {
        let Some((block, rest)) = z.split_once(':') else { continue };
        let Ok(block) = block.trim().parse::<u8>() else { continue };
        for teil in rest.split('|') {
            let w: Vec<&str> = teil.split_whitespace().collect();
            // ["Feld", "2:", "roh", "F25", "02", "FD"]
            if let ["Feld", n, "roh", f, a, b] = w.as_slice() {
                let (Ok(n), Some(f)) = (n.trim_end_matches(':').parse::<u8>(), f.strip_prefix('F')) else { continue };
                if let (Ok(f), Ok(a), Ok(b)) = (u8::from_str_radix(f, 16), u8::from_str_radix(a, 16), u8::from_str_radix(b, 16)) {
                    out.push((block, n, f, a, b));
                }
            }
        }
    }
    out
}

/// Export der Messwertblöcke eines Werkstattgeräts (CSV: `Block,Feld 1,Typ,Feld 2,Typ,…`)
/// → (Block, Feld) → angezeigter Text. Kommentarzeilen mit „;“ werden übersprungen.
/// Die Datei ist meist Windows-1252; Bytes über 0x7F werden als Latin-1 gelesen.
pub fn anzeige_aus_csv(roh: &[u8]) -> BTreeMap<(u8, u8), String> {
    let text: String = roh.iter().map(|&b| b as char).collect();
    let mut out = BTreeMap::new();
    for z in text.lines() {
        if z.starts_with(';') {
            continue;
        }
        let f: Vec<&str> = z.split(',').collect();
        let Some(Ok(block)) = f.first().map(|b| b.trim().parse::<u8>()) else { continue };
        for feld in 1..=4u8 {
            if let Some(w) = f.get(1 + 2 * usize::from(feld - 1)) {
                let w = w.trim();
                if !w.is_empty() {
                    out.insert((block, feld), w.to_string());
                }
            }
        }
    }
    out
}

/// Ergebnis des Abgleichs.
#[derive(Debug, Default, PartialEq)]
pub struct Gelernt {
    /// Schlüssel → Text, überall gleich angezeigt
    pub eindeutig: BTreeMap<String, String>,
    /// Schlüssel mit verschiedenen Anzeigen (nicht übernommen)
    pub widerspruch: BTreeMap<String, BTreeSet<String>>,
}

/// Rohwerte (Formel 0x25) und Anzeige abgleichen. Zahlen und leere/Platzhalter-Anzeigen
/// werden nie als Text übernommen.
pub fn lernen(roh: &[Roh], anzeige: &BTreeMap<(u8, u8), String>) -> Gelernt {
    let mut kandidaten: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for &(block, feld, f, a, b) in roh {
        if f != 0x25 || (a == 0 && b == 0) {
            continue;
        }
        let Some(t) = anzeige.get(&(block, feld)) else { continue };
        let zahl = t.replace(',', ".").trim_end_matches(|c: char| !c.is_ascii_digit()).parse::<f64>().is_ok();
        if zahl || matches!(t.as_str(), "n.v." | "-" | "—") || t.contains("FEHLER") {
            continue;
        }
        kandidaten.entry(schluessel(f, a, b)).or_default().insert(t.clone());
    }
    let mut g = Gelernt::default();
    for (k, v) in kandidaten {
        if v.len() == 1 {
            g.eindeutig.insert(k, v.into_iter().next().unwrap_or_default());
        } else {
            g.widerspruch.insert(k, v);
        }
    }
    g
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schluessel_hin_und_zurueck() {
        assert_eq!(schluessel(0x25, 0x02, 0xFD), "F25:02FD");
        assert_eq!(schluessel_lesen("F25:02FD"), Some((0x25, 0x02, 0xFD)));
        assert_eq!(schluessel_lesen("25 02 fd"), Some((0x25, 0x02, 0xFD)));
        assert_eq!(schluessel_lesen("F25:02"), None);
    }

    #[test]
    fn abgleich_nur_eindeutige_texte() {
        // erfundene Werte im Aufbau echter Ausgaben
        let scan = "071: Feld 1: roh F25 00 31  |  Feld 2: roh F25 00 88  |  Feld 3: roh F25 02 1C\n\
                    072: Feld 1: roh F25 00 F8  |  ~Temperatur: 8.00 °C\n\
                    078: Feld 1: 0.00  |  Feld 2: roh F25 00 88  |  Feld 3: roh F25 00 99\n\
                    079: Feld 1: roh F25 00 99\n";
        let roh = roh_aus_blockscan(scan);
        assert_eq!(roh.len(), 7);
        let csv = b";Kopf\n\nBlock,Feld 1,Datentyp,Feld 2,Datentyp,Feld 3,Datentyp,Feld 4,Datentyp,\n\
                    71,Kl.15 EIN,,aus,,inaktiv,, ,,\n72,n. verbaut,,8.0\xb0C,, ,, ,,\n78,0.0,,aus,,ein,, ,,\n79,auf,, ,, ,, ,,\n";
        let anzeige = anzeige_aus_csv(csv);
        assert_eq!(anzeige[&(72, 2)], "8.0°C", "Latin-1");
        let g = lernen(&roh, &anzeige);
        assert_eq!(g.eindeutig["F25:0031"], "Kl.15 EIN");
        assert_eq!(g.eindeutig["F25:0088"], "aus", "zweimal gleich");
        assert_eq!(g.eindeutig["F25:00F8"], "n. verbaut");
        assert!(g.widerspruch.contains_key("F25:0099"), "ein/auf widersprechen sich");
        assert!(!g.eindeutig.values().any(|t| t.contains("8.0")), "Zahlen nie als Text");
    }
}
