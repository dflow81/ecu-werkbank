//! Eigene Texte zu Messwertblöcken (TP 2.0 / KWP): Name des Blocks und jedes
//! Felds, einmal eingetragen und danach in GUI, Terminal, Testlauf und
//! Kurven genutzt (Fahrzeug-Datenbank, Bereich „messwert“; früher `messwerttexte.toml`).
//!
//! Ohne eigenen Text zeigt ECUWerkbank eine Art aus der Umrechnungsformel
//! (Drehzahl, Temperatur, Spannung …). Die sagt nur, *was für ein* Wert es
//! ist, nicht welcher — darum steht sie mit „~“ davor.

use crate::kwp::BlockValue;
use crate::tr;
use anyhow::Result;
use std::collections::BTreeMap;

/// Schlüssel: „01-001“ für den Block, „01-001-3“ für Feld 3 (Adresse hex, Block dezimal).
pub fn schluessel(adresse: u8, block: u8, feld: Option<u8>) -> String {
    match feld {
        Some(f) => format!("{adresse:02X}-{block:03}-{f}"),
        None => format!("{adresse:02X}-{block:03}"),
    }
}

/// Alle eigenen Texte.
pub fn alle() -> BTreeMap<String, String> {
    crate::db::eigen::alle("messwert")
}

/// Text setzen (`feld` None = Name des Blocks); leerer Text löscht.
pub fn setzen(adresse: u8, block: u8, feld: Option<u8>, text: &str) -> Result<()> {
    crate::db::eigen::setzen("messwert", &[(schluessel(adresse, block, feld), text.to_string())])
}

/// Mehrere Texte auf einmal (Schlüssel wie [`schluessel`]); leerer Text löscht.
pub fn mehrere_setzen(liste: &[(String, String)]) -> Result<()> {
    crate::db::eigen::setzen("messwert", liste)
}

/// Art des Werts aus der Umrechnungsformel (nur grob).
pub fn art(formel: u8) -> Option<&'static str> {
    Some(match formel {
        0x01 => crate::trs!("Drehzahl"),
        0x02 | 0x17 | 0x21 => crate::trs!("Anteil"),
        0x03 => crate::trs!("Winkel"),
        0x05 => crate::trs!("Temperatur"),
        0x06 | 0x15 => crate::trs!("Spannung"),
        0x07 => crate::trs!("Geschwindigkeit"),
        0x0F | 0x16 => crate::trs!("Zeit"),
        0x10 => crate::trs!("Bits"),
        0x12 => crate::trs!("Druck"),
        0x14 => crate::trs!("Abweichung"),
        0x1B => crate::trs!("Zündwinkel"),
        0x22 => crate::trs!("Leistung"),
        0x23 => crate::trs!("Verbrauch"),
        0x24 => crate::trs!("Kilometerstand"),
        0x04 => crate::trs!("Zündwinkel"),
        0x11 => crate::trs!("Text"),
        0x18 => crate::trs!("Strom"),
        0x19 => crate::trs!("Luftmasse"),
        0x1A => crate::trs!("Temperatur"),
        0x27 | 0x31 | 0x33 => crate::trs!("Menge je Hub"),
        0x37 => crate::trs!("Zeit"),
        0x5E | 0x34 => crate::trs!("Drehmoment"),
        0x1F => crate::trs!("Lambda"),
        0x51 => crate::trs!("Winkel"),
        0x3C => crate::trs!("Zeit"),
        0x41 => crate::trs!("Weg"),
        0x42 => crate::trs!("Spannung"),
        0x60 => crate::trs!("Druck"),
        0x5F => crate::trs!("Text"),
        _ => return None,
    })
}

/// Sollbereich eines Felds aus der Fahrzeug-Datenbank (Diagnose-Referenz).
#[derive(Debug, Clone, PartialEq)]
pub struct DbSoll {
    pub bereich: crate::limits::Range,
    pub einheit: String,
    /// z. B. „Leerlauf“, „Warm“ — leer, wenn immer gültig
    pub bedingung: String,
}

impl DbSoll {
    /// Wert prüfen: im Bereich → Gut; außerhalb → Warnung, aber nur wenn der
    /// Sollwert immer gilt. Mit Bedingung („Leerlauf“, „Warm“) kann ECUWerkbank
    /// nicht wissen, ob sie gerade erfüllt ist — dann keine Bewertung (None).
    pub fn pruefen(&self, wert: f64) -> Option<crate::limits::Stufe> {
        match self.bereich.check(wert) {
            crate::limits::Stufe::Gut => Some(crate::limits::Stufe::Gut),
            _ if self.bedingung.is_empty() => Some(crate::limits::Stufe::Warnung),
            _ => None,
        }
    }

    /// Bereich für Dauer-Überwachung (Live-Grafik): nur ohne Bedingung.
    pub fn immer(&self) -> Option<crate::limits::Range> {
        self.bedingung.is_empty().then_some(self.bereich)
    }

    /// „640 – 800 U/min (Leerlauf)“
    pub fn text(&self) -> String {
        let mut t = self.bereich.text();
        if !self.einheit.is_empty() {
            t = format!("{t} {}", self.einheit);
        }
        if !self.bedingung.is_empty() {
            t = format!("{t} ({})", self.bedingung);
        }
        t
    }
}

/// Was die Fahrzeug-Datenbank zu den Messwertblöcken EINES Steuergeräts weiß
/// (über seine Teilenummer): Feldnamen und Sollbereiche.
#[derive(Debug, Clone, Default)]
struct DbBloecke {
    namen: BTreeMap<(u8, u8), String>,
    soll: BTreeMap<(u8, u8), DbSoll>,
}

impl DbBloecke {
    fn aus(w: &crate::db::teile::SgWissen) -> Self {
        let mut d = Self::default();
        let block = |b: u16| u8::try_from(b).ok();
        for r in &w.diagnose {
            for m in &r.messwertfelder {
                if let Some(b) = block(m.block) {
                    d.namen.entry((b, m.feld)).or_insert_with(|| m.name.trim().to_string());
                }
            }
            for s in &r.sollbereiche {
                if let Some(b) = block(s.block) {
                    d.namen.entry((b, s.feld)).or_insert_with(|| s.name.trim().to_string());
                    // mehrere Bedingungen: die erste gilt (meist Leerlauf/betriebswarm)
                    d.soll.entry((b, s.feld)).or_insert_with(|| DbSoll {
                        bereich: crate::limits::Range { min: Some(s.min), max: Some(s.max) },
                        einheit: s.einheit.trim().to_string(),
                        bedingung: s.bedingung.trim().to_string(),
                    });
                }
            }
        }
        d.namen.retain(|_, n| !n.is_empty());
        d
    }
}

/// Texte für einen Block (eigene Texte + Fahrzeug-Datenbank + Art aus der Formel).
#[derive(Debug, Clone, Default)]
pub struct Texte {
    m: BTreeMap<String, String>,
    /// Adresse → Wissen aus der Fahrzeug-Datenbank (über die Teilenummer)
    db: BTreeMap<u8, DbBloecke>,
}

impl Texte {
    /// Eigene Texte laden.
    pub fn laden() -> Self {
        Self { m: alle(), db: BTreeMap::new() }
    }

    /// Eigene Texte laden und für `adresse` zusätzlich das Wissen der
    /// Fahrzeug-Datenbank zur Teilenummer (liest die Kennung am Auto, nur lesen).
    pub fn laden_fuer(be: &crate::transport::Backend, adresse: u8) -> Self {
        let mut t = Self::laden();
        if let Some(teil) = crate::kwp::teilenummer_lesen(be, adresse) {
            t.teilenummer_setzen(adresse, &teil);
        }
        t
    }

    /// Wissen der Fahrzeug-Datenbank zur Teilenummer des Steuergeräts an
    /// `adresse` übernehmen (Datenbank nur lesend; ohne Treffer bleibt es leer).
    pub fn teilenummer_setzen(&mut self, adresse: u8, teilenummer: &str) {
        match crate::db::teile::wissen_falls_vorhanden(teilenummer) {
            Some(w) => self.wissen_setzen(adresse, Some(&w)),
            None => self.wissen_setzen(adresse, None),
        }
    }

    /// Wissen zu einem Steuergerät setzen (None = entfernen).
    pub fn wissen_setzen(&mut self, adresse: u8, w: Option<&crate::db::teile::SgWissen>) {
        match w.map(DbBloecke::aus).filter(|d| !d.namen.is_empty() || !d.soll.is_empty()) {
            Some(d) => {
                self.db.insert(adresse, d);
            }
            None => {
                self.db.remove(&adresse);
            }
        }
    }

    /// Ist für `adresse` Wissen aus der Datenbank geladen?
    pub fn hat_wissen(&self, adresse: u8) -> bool {
        self.db.contains_key(&adresse)
    }

    /// Feldname aus der Fahrzeug-Datenbank (ohne eigenen Text).
    pub fn db_name(&self, adresse: u8, block: u8, feld: u8) -> Option<&str> {
        self.db.get(&adresse)?.namen.get(&(block, feld)).map(String::as_str)
    }

    /// Sollbereich aus der Fahrzeug-Datenbank.
    pub fn db_soll(&self, adresse: u8, block: u8, feld: u8) -> Option<&DbSoll> {
        self.db.get(&adresse)?.soll.get(&(block, feld))
    }

    /// Eigener Name des Blocks.
    pub fn block(&self, adresse: u8, block: u8) -> Option<&str> {
        self.m.get(&schluessel(adresse, block, None)).map(String::as_str)
    }

    /// Name eines Felds — eigener Text, sonst aus der Fahrzeug-Datenbank; ohne „~Art“.
    pub fn name(&self, adresse: u8, block: u8, feld: u8) -> Option<&str> {
        self.eigen(adresse, block, feld).or_else(|| self.db_name(adresse, block, feld))
    }

    /// Eigener Text eines Felds (Feld ab 1).
    pub fn eigen(&self, adresse: u8, block: u8, feld: u8) -> Option<&str> {
        self.m.get(&schluessel(adresse, block, Some(feld))).map(String::as_str)
    }

    /// Name eines Felds: eigener Text, sonst Datenbank, sonst „~Art“, sonst „Feld n“.
    pub fn feld(&self, adresse: u8, block: u8, feld: u8, v: &BlockValue) -> String {
        match (self.name(adresse, block, feld), art(v.formula)) {
            (Some(t), _) => t.to_string(),
            (None, Some(a)) => format!("~{a}"),
            (None, None) => tr!("Feld {a}", a = feld),
        }
    }

    /// Ein Block als Textzeile: „Drehzahl 800 1/min | ~Temperatur 90 °C | …“.
    pub fn zeile(&self, adresse: u8, block: u8, werte: &[BlockValue]) -> String {
        werte
            .iter()
            .enumerate()
            .filter(|(_, v)| !v.is_leer())
            .map(|(i, v)| {
                let feld = i as u8 + 1;
                let soll = match (self.db_soll(adresse, block, feld), v.decode()) {
                    (Some(s), Some((x, _))) => match s.pruefen(x) {
                        Some(crate::limits::Stufe::Gut) => " ✓".to_string(),
                        Some(_) => format!(" ⚠ {}", tr!("Soll {a}", a = s.text())),
                        None => format!(" ({})", tr!("Soll {a}", a = s.text())),
                    },
                    _ => String::new(),
                };
                format!("{}: {}{soll}", self.feld(adresse, block, feld, v), v.display())
            })
            .collect::<Vec<_>>()
            .join("  |  ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texte_setzen_und_anzeigen() {
        let _g = crate::history::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("vag-mw-{}", std::process::id()));
        std::env::set_var("VAG_DATA_DIR", &dir);
        let werte = [BlockValue { formula: 0x01, a: 200, b: 20, text: None }, BlockValue { formula: 0x05, a: 10, b: 190, text: None }, BlockValue { formula: 0x99, a: 1, b: 2, text: None }];
        let t = Texte::laden();
        assert_eq!(t.feld(1, 1, 1, &werte[0]), "~Drehzahl");
        assert_eq!(t.feld(1, 1, 3, &werte[2]), "Feld 3");

        setzen(0x01, 1, Some(2), "Kühlmitteltemperatur").unwrap();
        setzen(0x01, 1, None, "Leerlauf").unwrap();
        let t = Texte::laden();
        assert_eq!(t.block(1, 1), Some("Leerlauf"));
        assert_eq!(t.feld(1, 1, 2, &werte[1]), "Kühlmitteltemperatur");
        assert_eq!(t.feld(1, 2, 2, &werte[1]), "~Temperatur"); // anderer Block
        let z = t.zeile(1, 1, &werte);
        assert!(z.starts_with("~Drehzahl: 800.00 1/min  |  Kühlmitteltemperatur: 90.00 °C"), "{z}");
        assert_eq!(alle().keys().cloned().collect::<Vec<_>>(), ["01-001", "01-001-2"]);

        setzen(0x01, 1, Some(2), "").unwrap();
        assert_eq!(alle().len(), 1);
        std::env::remove_var("VAG_DATA_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn namen_und_sollwerte_aus_der_datenbank() {
        use crate::db::diagnosedaten::{DiagnoseReferenz, Messwertfeld, Sollbereich};
        use crate::db::teile::SgWissen;
        let soll = |feld, min, max, name: &str, bedingung: &str| Sollbereich {
            block: 1, feld, name: name.into(), min, max, einheit: "U/min".into(), bedingung: bedingung.into(), im_abbild: false, labeldatei: String::new(),
        };
        let w = SgWissen {
            teilenummer: "03C906016".into(),
            diagnose: vec![DiagnoseReferenz {
                sollbereiche: vec![soll(1, 640.0, 800.0, "Motordrehzahl", "Leerlauf"), soll(1, 0.0, 9.0, "anders", "Volllast")],
                messwertfelder: vec![Messwertfeld { block: 1, feld: 2, name: "Kühlmittel".into(), im_abbild: false, labeldatei: String::new() }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut t = Texte::default();
        t.wissen_setzen(0x01, Some(&w));
        assert!(t.hat_wissen(0x01));
        assert_eq!(t.name(0x01, 1, 1), Some("Motordrehzahl"));
        assert_eq!(t.name(0x01, 1, 2), Some("Kühlmittel"));
        assert_eq!(t.name(0x02, 1, 1), None, "nur für dieses Steuergerät");
        assert_eq!(t.eigen(0x01, 1, 1), None, "kein eigener Text");
        let s = t.db_soll(0x01, 1, 1).unwrap();
        assert_eq!(s.text(), "640 – 800 U/min (Leerlauf)", "erste Bedingung gilt");
        // Drehzahl-Formel 0x01: a*b/5 → 200*20/5 = 800 (im Bereich), 200*25/5 = 1000 (zu hoch)
        let gut = [BlockValue { formula: 0x01, a: 200, b: 20, text: None }];
        let hoch = [BlockValue { formula: 0x01, a: 200, b: 25, text: None }];
        assert!(t.zeile(0x01, 1, &gut).ends_with('✓'), "{}", t.zeile(0x01, 1, &gut));
        // mit Bedingung („Leerlauf“): außerhalb nur Hinweis, keine Warnung
        assert!(t.zeile(0x01, 1, &hoch).ends_with("(Soll 640 – 800 U/min (Leerlauf))"), "{}", t.zeile(0x01, 1, &hoch));
        assert_eq!(s.pruefen(1000.0), None);
        assert!(s.immer().is_none());
        let ohne = DbSoll { bedingung: String::new(), ..s.clone() };
        assert_eq!(ohne.pruefen(1000.0), Some(crate::limits::Stufe::Warnung));
        assert_eq!(ohne.pruefen(700.0), Some(crate::limits::Stufe::Gut));
        // eigener Text gewinnt
        t.m.insert(schluessel(0x01, 1, Some(1)), "Drehzahl eigen".into());
        assert_eq!(t.name(0x01, 1, 1), Some("Drehzahl eigen"));
        t.wissen_setzen(0x01, None);
        assert!(!t.hat_wissen(0x01) && t.db_soll(0x01, 1, 1).is_none());
    }
}
