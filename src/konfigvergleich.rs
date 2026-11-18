//! Ganze Fahrzeug-Konfiguration vergleichen: Codierung, Anpassungen und
//! Softwarestände in einem Bericht — entweder vorher/nachher am selben
//! Fahrzeug oder zwei Fahrzeuge nebeneinander (z. B. „was ist beim anderen
//! Golf anders codiert?“). Nur gespeicherte Sicherungen, kein Auto nötig.

use crate::labels::LabelDb;
use std::fmt::Write as _;

/// Gespeicherter Stand eines Fahrzeugs (jeweils eine Sicherung, falls vorhanden).
#[derive(Debug, Clone, Default)]
pub struct Stand {
    pub fahrzeug: String,
    pub codierung: Option<crate::backup::CodingBackup>,
    pub anpassung: Option<crate::anpassung::Sicherung>,
    pub software: Option<crate::swinfo::Stand>,
}

/// Stand aus den Sicherungen eines Fahrzeugs: `alter` 0 = neueste, 1 = die davor.
pub fn laden(fahrzeug: &str, alter: usize) -> Stand {
    let codierung = crate::backup::list_all().into_iter().filter(|(_, b)| b.vehicle == fahrzeug).map(|(_, b)| b).nth(alter);
    let anpassung = crate::anpassung::liste(fahrzeug).into_iter().filter(|x| x.1.vollstaendig()).map(|x| x.1).nth(alter);
    let mut sw = crate::swinfo::list(fahrzeug);
    sw.reverse();
    Stand { fahrzeug: fahrzeug.to_string(), codierung, anpassung, software: sw.into_iter().nth(alter) }
}

/// Bericht `a` → `b` als Text.
pub fn text(a: &Stand, b: &Stand, db: &LabelDb) -> String {
    let mut s = String::new();
    let fehlt = |was: &str, wo: &Stand| crate::tr!("  ({a} fehlt bei {b} — erst sichern)", a = was, b = wo.fahrzeug);
    let ein = |x: bool| if x { crate::tr!("ein") } else { crate::tr!("aus") };

    let _ = writeln!(s, "{}", crate::tr!("Codierung"));
    match (&a.codierung, &b.codierung) {
        (Some(x), Some(y)) => {
            let _ = writeln!(s, "  {}", crate::tr!("Sicherung vom {a}  →  {b}", a = x.created, b = y.created));
            let d = crate::backup::compare(x, y, db);
            if d.is_empty() {
                let _ = writeln!(s, "  {}", crate::tr!("gleich"));
            }
            for c in d {
                let _ = writeln!(s, "  {}: {}  →  {}", c.module, c.old_hex, c.new_hex);
                for (byte, bit, x, y, name) in c.bits {
                    let name = if name.is_empty() { String::new() } else { format!("  {name}") };
                    let _ = writeln!(s, "    {}", crate::tr!("Byte {byte} Bit {bit}: {a} → {b}{name}", byte = byte, bit = bit, a = ein(x), b = ein(y), name = name));
                }
            }
        }
        (None, _) => s.push_str(&format!("{}\n", fehlt(&crate::tr!("Codier-Sicherung"), a))),
        (_, None) => s.push_str(&format!("{}\n", fehlt(&crate::tr!("Codier-Sicherung"), b))),
    }

    let _ = writeln!(s, "\n{}", crate::tr!("Anpassungen"));
    match (&a.anpassung, &b.anpassung) {
        (Some(x), Some(y)) => {
            let _ = writeln!(s, "  {}", crate::tr!("Sicherung vom {a}  →  {b}", a = x.created, b = y.created));
            let d = crate::anpassung::vergleichen(x, y);
            if d.is_empty() {
                let _ = writeln!(s, "  {}", crate::tr!("gleich"));
            }
            for (geraet, aend) in d {
                let _ = writeln!(s, "  {geraet}");
                for x in aend {
                    let _ = writeln!(s, "    {}", x.zeile());
                }
            }
        }
        (None, _) => s.push_str(&format!("{}\n", fehlt(&crate::tr!("Anpassungs-Sicherung"), a))),
        (_, None) => s.push_str(&format!("{}\n", fehlt(&crate::tr!("Anpassungs-Sicherung"), b))),
    }

    let _ = writeln!(s, "\n{}", crate::tr!("Softwarestände"));
    match (&a.software, &b.software) {
        (Some(x), Some(y)) => {
            let _ = writeln!(s, "  {}", crate::tr!("Stand vom {a}  →  {b}", a = x.created, b = y.created));
            let d = crate::swinfo::compare(x, y);
            if d.is_empty() {
                let _ = writeln!(s, "  {}", crate::tr!("gleich"));
            }
            for (m, t) in d {
                let _ = writeln!(s, "  {m}: {t}");
            }
        }
        (None, _) => s.push_str(&format!("{}\n", fehlt(&crate::tr!("Softwarestand"), a))),
        (_, None) => s.push_str(&format!("{}\n", fehlt(&crate::tr!("Softwarestand"), b))),
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fehlende_sicherungen_werden_genannt() {
        let a = Stand { fahrzeug: "A".into(), ..Default::default() };
        let b = Stand { fahrzeug: "B".into(), ..Default::default() };
        let t = text(&a, &b, &LabelDb::default());
        assert!(t.contains("Codier-Sicherung fehlt bei A"), "{t}");
        assert!(t.contains("Anpassungs-Sicherung fehlt bei A"), "{t}");
        assert!(t.contains("Softwarestand fehlt bei A"), "{t}");
    }

    #[test]
    fn softwarestaende_im_vergleich() {
        let sw: crate::swinfo::Stand = toml::from_str(
            "created = \"1.1.2026\"\ntimestamp = 1\n[[module]]\nadresse = \"01\"\nname = \"Motor\"\nprotocol = \"UDS\"\nteilenummer = \"03C906016\"\nsoftware = \"0010\"\n",
        )
        .expect("Stand");
        let mut neu = sw.clone();
        neu.module[0].software = "0020".into();
        let a = Stand { fahrzeug: "A".into(), software: Some(sw), ..Default::default() };
        let b = Stand { fahrzeug: "A".into(), software: Some(neu), ..Default::default() };
        let t = text(&a, &b, &LabelDb::default());
        assert!(t.contains("neue Software: 0010 → 0020"), "{t}");
    }
}
