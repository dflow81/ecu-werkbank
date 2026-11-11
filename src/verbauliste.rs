//! Verbauliste: aus einem Scan die Liste aller verbauten Steuergeräte, nach
//! Systemgruppe geordnet, mit Teilenummer und Status — wie die Verbau-/
//! Installationsliste in gängiger Werkstattdiagnose. Rein lesend, nur aus den
//! Scan-Daten (keine eigene Buskommunikation).
//!
//! Hinweis: ecu-werkbank leitet Namen und Zuordnung aus der Antwort des
//! Steuergeräts ab. Die TP-2.0-Logikadressen sind keine Diagnoseadressen
//! (beim Golf 6 z. B. 0x1F = Gateway, 0x20 = Bordnetz), darum zeigen wir die
//! Adresse so, wie ecu-werkbank sie kennt, und verlassen uns beim Namen auf die
//! Bauteil-Kennung — nicht auf eine geratene Nummern-Übersetzung.

use crate::report::{ModuleReport, ScanReport};

/// Eine Zeile der Verbauliste (ein Steuergerät).
#[derive(Debug, Clone, PartialEq)]
pub struct Zeile {
    pub name: String,
    /// Adresse bzw. CAN-ID, wie ecu-werkbank sie kennt.
    pub adresse: String,
    /// "UDS" oder "TP 2.0".
    pub protokoll: String,
    /// Teilenummer, z. B. "03C 906 016" (leer, wenn nicht lesbar).
    pub teil: String,
    /// Anzahl Fehler; None = Fehlerspeicher nicht lesbar.
    pub fehler: Option<usize>,
}

/// Steuergeräte einer Systemgruppe.
#[derive(Debug, Clone, PartialEq)]
pub struct Gruppe {
    pub name: String,
    pub zeilen: Vec<Zeile>,
}

/// Kurzüberblick über die ganze Verbauliste.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Zusammenfassung {
    /// Steuergeräte insgesamt.
    pub anzahl: usize,
    /// Davon mit mindestens einem Fehler.
    pub mit_fehler: usize,
    /// Davon ohne lesbaren Fehlerspeicher.
    pub ohne_speicher: usize,
}

/// Teilenummer aus der Kennung ziehen und wie gewohnt mit Leerzeichen gruppieren
/// (z. B. "03C906016" → "03C 906 016"). Leerstring, wenn nichts Brauchbares da ist.
pub fn teilenummer(ident: &[String]) -> String {
    let erstes = ident.first().map(String::as_str).unwrap_or("");
    // Teilenummer ist der erste Block aus Ziffern/Buchstaben vor dem Softwarestand.
    let roh: String = erstes.chars().take_while(|c| !c.is_whitespace()).filter(|c| c.is_ascii_alphanumeric()).collect();
    // Eine VAG-Teilenummer hat mindestens 9 Zeichen (z. B. 1K0907379 + Zusatz).
    if roh.len() < 9 || !roh[..3].chars().all(|c| c.is_ascii_alphanumeric()) {
        return String::new();
    }
    let (eins, rest) = roh.split_at(3);
    let (zwei, drei) = rest.split_at(3);
    let (drei, zusatz) = drei.split_at(3);
    let mut s = format!("{eins} {zwei} {drei}");
    if !zusatz.is_empty() {
        s.push(' ');
        s.push_str(zusatz);
    }
    s
}

fn zeile_aus(m: &ModuleReport) -> Zeile {
    Zeile {
        name: if m.name.is_empty() { crate::i18n::t("Steuergerät").to_string() } else { m.name.clone() },
        adresse: m.addr_text(),
        protokoll: m.protocol.clone(),
        teil: teilenummer(&m.ident),
        fehler: m.dtcs.as_ref().map(Vec::len),
    }
}

/// Verbauliste aus einem Scan, nach Systemgruppe geordnet. Innerhalb einer
/// Gruppe nach Name sortiert; Gruppen in fester, sinnvoller Reihenfolge.
pub fn aus_scan(r: &ScanReport) -> Vec<Gruppe> {
    use std::collections::BTreeMap;
    let mut nach_gruppe: BTreeMap<&'static str, Vec<Zeile>> = BTreeMap::new();
    for m in &r.modules {
        let g = crate::systems::gruppe_modul(&m.protocol, m.address, &m.name);
        nach_gruppe.entry(g).or_default().push(zeile_aus(m));
    }
    // Feste Reihenfolge der bekannten Gruppen, Rest hinten.
    const ORDNUNG: &[&str] = &[
        "Motor & Antrieb",
        "Fahrwerk & Assistenz",
        "Airbag & Rückhaltesysteme",
        "Klima & Heizung",
        "Karosserie & Komfort",
        "Instrumente & Gateway",
        "Infotainment",
        "Sonstige",
    ];
    let rang = |g: &str| ORDNUNG.iter().position(|x| *x == g).unwrap_or(ORDNUNG.len());
    let mut gruppen: Vec<Gruppe> = nach_gruppe
        .into_iter()
        .map(|(name, mut zeilen)| {
            zeilen.sort_by_key(|z| z.name.to_lowercase());
            Gruppe { name: name.to_string(), zeilen }
        })
        .collect();
    gruppen.sort_by_key(|g| rang(&g.name));
    gruppen
}

/// Zählt Steuergeräte, Fehler und nicht lesbare Speicher.
pub fn zusammenfassung(gruppen: &[Gruppe]) -> Zusammenfassung {
    let mut z = Zusammenfassung::default();
    for zeile in gruppen.iter().flat_map(|g| &g.zeilen) {
        z.anzahl += 1;
        match zeile.fehler {
            Some(n) if n > 0 => z.mit_fehler += 1,
            None => z.ohne_speicher += 1,
            _ => {}
        }
    }
    z
}

/// Verbauliste als Text (für Ausgabe und Export).
pub fn text(gruppen: &[Gruppe]) -> String {
    let mut s = String::new();
    for g in gruppen {
        s.push_str(&format!("── {} ──\n", g.name));
        for z in &g.zeilen {
            let status = match z.fehler {
                Some(0) => crate::i18n::t("i.O.").to_string(),
                Some(n) => crate::tr!("{n} Fehler", n = n),
                None => crate::i18n::t("Speicher nicht lesbar").to_string(),
            };
            let teil = if z.teil.is_empty() { String::new() } else { format!("  {}", z.teil) };
            s.push_str(&format!("  {:<5} {:<30}{}  [{}]\n", z.adresse, z.name, teil, status));
        }
        s.push('\n');
    }
    let z = zusammenfassung(gruppen);
    s.push_str(&crate::tr!(
        "{a} Steuergerät(e), davon {b} mit Fehler.",
        a = z.anzahl,
        b = z.mit_fehler
    ));
    s.push('\n');
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::{DtcEntry, ModuleReport};

    fn modul(proto: &str, addr: Option<u16>, name: &str, teil: &str, fehler: Option<usize>) -> ModuleReport {
        ModuleReport {
            protocol: proto.into(),
            address: addr,
            name: name.into(),
            can: "0x714/0x77e".into(),
            ident: vec![teil.into()],
            vin: None,
            dtcs: fehler.map(|n| {
                (0..n)
                    .map(|_| DtcEntry {
                        code: "P0001".into(),
                        alt: None,
                        text: String::new(),
                        status: String::new(),
                        umgebung: vec![],
                        frueher: vec![],
                    })
                    .collect()
            }),
        }
    }

    #[test]
    fn teilenummer_gruppiert() {
        assert_eq!(teilenummer(&["03C906016  5049".into()]), "03C 906 016");
        assert_eq!(teilenummer(&["1K0907379CD  0120".into()]), "1K0 907 379 CD");
        assert_eq!(teilenummer(&["5K0920860F".into()]), "5K0 920 860 F");
        assert_eq!(teilenummer(&["kurz".into()]), "");
        assert_eq!(teilenummer(&[]), "");
    }

    #[test]
    fn gruppen_und_zaehlung() {
        let r = ScanReport {
            modules: vec![
                modul("TP 2.0", Some(0x01), "Motor", "03C906016  5049", Some(1)),
                modul("UDS", Some(0x715), "Airbag", "5K0959655", Some(0)),
                modul("TP 2.0", Some(0x1f), "Gateway (J533)", "1K0907530AA  0081", None),
            ],
            ..Default::default()
        };
        let g = aus_scan(&r);
        // Motor steht vor Instrumente & Gateway
        assert_eq!(g.first().unwrap().name, "Motor & Antrieb");
        let z = zusammenfassung(&g);
        assert_eq!(z.anzahl, 3);
        assert_eq!(z.mit_fehler, 1);
        assert_eq!(z.ohne_speicher, 1);
        let t = text(&g);
        assert!(t.contains("03C 906 016"));
        assert!(t.contains("Gateway"));
    }
}
