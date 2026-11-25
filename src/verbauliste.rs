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

/// Ein Steuergerät im Steuergerätebaum.
#[derive(Debug, Clone, PartialEq)]
pub struct BaumGeraet {
    /// Diagnoseadresse laut Gateway (z. B. 0x17)
    pub adresse: u8,
    pub name: String,
    /// Gateway meldet einen gespeicherten Fehler
    pub fehler_laut_gateway: bool,
    /// Ergebnis im letzten Scan: None = dort nicht gefunden (antwortet nicht?),
    /// Some(None) = gefunden, Speicher nicht lesbar, Some(Some(n)) = n Fehler
    pub im_scan: Option<Option<usize>>,
}

/// Ein Bus am Gateway mit seinen Steuergeräten.
#[derive(Debug, Clone, PartialEq)]
pub struct BusAst {
    pub bus: u8,
    pub name: String,
    pub geraete: Vec<BaumGeraet>,
}

/// Name der Bus-Kennung aus der Gateway-Verbauliste. Am Golf 6 beobachtet
/// (Verbauliste mit der Lage der Steuergeräte verglichen, 15.11. und 22.11.2026);
/// unbekannte Kennungen erscheinen als „Bus xx“.
pub fn bus_name(bus: u8) -> String {
    match bus {
        0x00 => crate::tr!("Antriebs-CAN"),
        0x02 => crate::tr!("Komfort-CAN"),
        0x04 => crate::tr!("Infotainment-CAN"),
        // Golf 6, 22.11.2026: dort hängen nur Kombi (17) und Wegfahrsperre (25)
        0x06 => crate::tr!("Kombi-CAN"),
        // Golf 6, 22.11.2026: nur das Gateway selbst
        0x14 => crate::tr!("Gateway selbst"),
        b => crate::tr!("Bus {a}", a = format!("{b:02X}")),
    }
}

/// Steuergerätebaum: Gateway → Bus → Steuergeräte, abgeglichen mit dem letzten
/// Scan. So fällt auf, was laut Gateway verbaut ist, im Scan aber fehlt.
pub fn baum(gateway: &[crate::kwp::VerbauEintrag], scan: Option<&ScanReport>) -> Vec<BusAst> {
    use std::collections::BTreeMap;
    let mut aeste: BTreeMap<u8, Vec<BaumGeraet>> = BTreeMap::new();
    for e in gateway.iter().filter(|e| e.verbaut()) {
        let treffer = scan.and_then(|r| {
            r.modules
                .iter()
                .find(|m| {
                    let a = m.address;
                    (m.protocol == "TP 2.0" && e.tp20 != 0 && a == Some(u16::from(e.tp20))) || (m.protocol != "TP 2.0" && a == Some(u16::from(e.adresse)))
                })
                // UDS-Geräte stehen im Scan nur mit CAN-Kennung (ohne Diagnoseadresse):
                // dann über den Namen („Airbag“, „Radio/Navigation“ ↔ Radio 56 und Navigation 37)
                .or_else(|| {
                    let soll = crate::systems::name(u16::from(e.adresse)).to_lowercase();
                    (!soll.is_empty()).then_some(())?;
                    r.modules.iter().find(|m| m.protocol != "TP 2.0" && m.address.is_none() && !m.name.is_empty() && {
                        let n = m.name.to_lowercase();
                        n.contains(&soll) || soll.contains(&n) || n.split('/').any(|t| !t.is_empty() && soll.contains(t.trim()))
                    })
                })
        });
        let name = treffer
            .map(|m| m.name.clone())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| {
                let n = crate::systems::name(u16::from(e.adresse));
                if n.is_empty() { crate::tr!("Steuergerät {a}", a = format!("{:02X}", e.adresse)) } else { n.to_string() }
            });
        aeste.entry(e.bus).or_default().push(BaumGeraet {
            adresse: e.adresse,
            name,
            fehler_laut_gateway: e.fehler(),
            im_scan: scan.map(|_| treffer.map(|m| m.dtcs.as_ref().map(Vec::len))).unwrap_or(Some(None)),
        });
    }
    aeste
        .into_iter()
        .map(|(bus, mut geraete)| {
            geraete.sort_by_key(|g| g.adresse);
            BusAst { bus, name: bus_name(bus), geraete }
        })
        .collect()
}

/// Baum als Text (für die CLI).
pub fn baum_text(aeste: &[BusAst], mit_scan: bool) -> String {
    let mut s = crate::tr!("Gateway");
    s.push('\n');
    for (i, a) in aeste.iter().enumerate() {
        let letzter = i + 1 == aeste.len();
        s.push_str(&format!("{} {}\n", if letzter { "└─" } else { "├─" }, a.name));
        for (j, g) in a.geraete.iter().enumerate() {
            let zweig = if letzter { "   " } else { "│  " };
            let ende = if j + 1 == a.geraete.len() { "└─" } else { "├─" };
            let mut st = if g.fehler_laut_gateway { crate::tr!("Fehler gespeichert") } else { crate::tr!("i.O.") };
            if mit_scan && g.im_scan.is_none() {
                st.push_str(&crate::tr!(" · im letzten Scan nicht gefunden"));
            }
            s.push_str(&format!("{zweig}{ende} {:02X} {}  —  {st}\n", g.adresse, g.name));
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baum_mit_abgleich() {
        use crate::kwp::VerbauEintrag;
        let gw = vec![
            VerbauEintrag { adresse: 0x01, tp20: 0x01, bus: 0x00, status: 0x01 },
            VerbauEintrag { adresse: 0x09, tp20: 0x20, bus: 0x02, status: 0x05 },
            VerbauEintrag { adresse: 0x56, tp20: 0x00, bus: 0x04, status: 0x01 },
            VerbauEintrag { adresse: 0x15, tp20: 0x15, bus: 0x00, status: 0x00 }, // nicht verbaut
        ];
        let mut r = ScanReport::default();
        r.modules.push(ModuleReport { protocol: "TP 2.0".into(), address: Some(0x01), name: "Motor".into(), dtcs: Some(vec![]), ..Default::default() });
        r.modules.push(ModuleReport { protocol: "TP 2.0".into(), address: Some(0x20), name: "Bordnetz".into(), dtcs: None, ..Default::default() });
        let b = baum(&gw, Some(&r));
        assert_eq!(b.len(), 3, "drei Busse");
        assert_eq!(b[0].name, "Antriebs-CAN");
        assert_eq!(b[0].geraete.len(), 1, "nicht verbautes Airbag-Gerät fehlt");
        assert_eq!(b[0].geraete[0].im_scan, Some(Some(0)));
        assert!(b[1].geraete[0].fehler_laut_gateway);
        assert_eq!(b[1].geraete[0].im_scan, Some(None));
        assert_eq!(b[2].geraete[0].im_scan, None, "Radio im Scan nicht gefunden");
        let t = baum_text(&b, true);
        assert!(t.contains("56 Radio  —  i.O. · im letzten Scan nicht gefunden"), "{t}");
        assert!(t.contains("09 Bordnetz  —  Fehler gespeichert"), "{t}");
        assert_eq!(bus_name(0x07), "Bus 07");
        assert_eq!(bus_name(0x06), "Kombi-CAN");
        // UDS ohne Diagnoseadresse: über den Namen gefunden (Golf 6, 22.11.2026)
        r.modules.push(ModuleReport { protocol: "UDS".into(), address: None, name: "Radio/Navigation".into(), dtcs: Some(vec![]), ..Default::default() });
        let b = baum(&gw, Some(&r));
        assert_eq!(b[2].geraete[0].im_scan, Some(Some(0)), "Radio 56 über den Namen");
    }
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
            fremd: false,
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
