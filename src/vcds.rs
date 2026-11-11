//! VCDS-Protokolle (Auto-Scan / Fehlerspeicher als Text) einlesen: Fehler mit
//! km-Stand und Datum in den Verlauf des Fahrzeugs übernehmen und die
//! Fehlertexte als eigene Texte speichern (nur bei Codes ohne eigenen Text).
//!
//! Die Texte stammen aus dem eigenen VCDS des Nutzers und bleiben in seinem
//! Datenordner — ecu-werkbank liefert davon nichts mit.
//!
//! Erkannt werden die deutsche und die englische Ausgabe. Alte Protokolle sind
//! oft Latin-1 kodiert; das wird automatisch umgewandelt.

use crate::report::{DtcEntry, ModuleReport, ScanReport};
use crate::tr;
use anyhow::Result;

/// Ein eingelesener Fehler.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Fehler {
    /// Code wie ecu-werkbank ihn zeigt: „P1602“ (UDS) oder „03803“ (VAG, TP 2.0)
    pub code: String,
    /// genormter Code zu einem VAG-Code, z. B. „P0300“ zu „00768“
    pub alt: Option<String>,
    /// Beschreibung aus dem Protokoll (Kopfzeile des Fehlers)
    pub text: String,
    /// Fehlerart, z. B. „Spannung zu klein“
    pub art: String,
    /// z. B. „Sporadisch - bestätigt“
    pub status: String,
    /// km-Stand, als der Fehler gespeichert wurde
    pub km: Option<u32>,
    /// „TT.MM.JJJJ“ (fehlt, wenn das Steuergerät kein Datum kennt)
    pub datum: Option<String>,
    /// alle Umgebungsbedingungen (Name, Wert)
    pub umgebung: Vec<(String, String)>,
}

/// Ein Steuergerät im Protokoll.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Geraet {
    pub adresse: u8,
    pub name: String,
    pub teilenummer: String,
    pub fehler: Vec<Fehler>,
}

/// Ein ganzes Protokoll.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Protokoll {
    /// „TT.MM.JJJJ HH:MM“
    pub datum: String,
    pub timestamp: u64,
    pub vin: Option<String>,
    pub km: Option<u32>,
    pub geraete: Vec<Geraet>,
}

/// Bytes → Text (UTF-8, sonst Latin-1; UTF-16 mit BOM).
pub fn dekodieren(b: &[u8]) -> String {
    if let Some(rest) = b.strip_prefix(&[0xFF, 0xFE]) {
        let w: Vec<u16> = rest.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
        return String::from_utf16_lossy(&w);
    }
    let b = b.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(b);
    match std::str::from_utf8(b) {
        Ok(s) => s.to_string(),
        Err(_) => b.iter().map(|&c| c as char).collect(),
    }
}

const MONATE: [[&str; 2]; 12] = [
    ["januar", "january"],
    ["februar", "february"],
    ["märz", "march"],
    ["april", "april"],
    ["mai", "may"],
    ["juni", "june"],
    ["juli", "july"],
    ["august", "august"],
    ["september", "september"],
    ["oktober", "october"],
    ["november", "november"],
    ["dezember", "december"],
];

/// Kopfzeile „Sonntag,11,Oktober,2026,13:45:51:54205“ → (Datum, Unix-Zeit).
fn kopfdatum(z: &str) -> Option<(String, u64)> {
    let t: Vec<&str> = z.split(',').map(str::trim).collect();
    if t.len() < 5 {
        return None;
    }
    let tag: u32 = t[1].parse().ok()?;
    let m = t[2].to_lowercase();
    let monat = MONATE.iter().position(|n| n.contains(&m.as_str()))? as u32 + 1;
    let jahr: i64 = t[3].parse().ok()?;
    let mut hms = t[4].split(':').map(|x| x.parse::<u64>().unwrap_or(0));
    let (h, mi, s) = (hms.next().unwrap_or(0), hms.next().unwrap_or(0), hms.next().unwrap_or(0));
    let datum = format!("{tag:02}.{monat:02}.{jahr}");
    let tage = crate::vehicle::days_of(&datum)?;
    Some((format!("{datum} {h:02}:{mi:02}"), (tage.max(0) as u64) * 86_400 + h * 3600 + mi * 60 + s))
}

/// „170140km“ / „169151 km“ / „107287km-66667mi“ → 170140
fn km_wert(s: &str) -> Option<u32> {
    let s = s.trim();
    let ende = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    let n: u32 = s[..ende].parse().ok()?;
    s[ende..].trim_start().to_lowercase().starts_with("km").then_some(n)
}

/// „2024.11.12“ → „12.11.2024“ (2000.00.00 = unbekannt)
fn datum_wert(s: &str) -> Option<String> {
    let p: Vec<&str> = s.trim().split('.').collect();
    if p.len() != 3 || p[1] == "00" || p[2] == "00" {
        return None;
    }
    let d = format!("{}.{}.{}", p[2], p[1], p[0]);
    crate::vehicle::days_of(&d).map(|_| d)
}

fn ist_code(s: &str) -> bool {
    s.len() == 5 && s.starts_with(['P', 'B', 'C', 'U']) && s[1..].chars().all(|c| c.is_ascii_hexdigit())
}

/// Zeile „Name: Wert“ → (Name, Wert)
fn paar(z: &str) -> Option<(String, String)> {
    let (k, v) = z.split_once(':')?;
    let (k, v) = (k.trim(), v.trim());
    (!k.is_empty() && !v.is_empty() && k.len() < 60).then(|| (k.to_string(), v.to_string()))
}

/// Erste Zeile eines Fehlers: „6443 - Spannungsversorgung Klemme 30“
fn fehlerkopf(z: &str) -> Option<(String, String)> {
    let (n, rest) = z.split_once(" - ")?;
    let n = n.trim();
    (z.starts_with(|c: char| c.is_ascii_digit()) && (4..=7).contains(&n.len()) && n.chars().all(|c| c.is_ascii_digit()))
        .then(|| (n.to_string(), rest.trim().to_string()))
}

pub fn lesen(text: &str) -> Result<Protokoll> {
    let mut p = Protokoll::default();
    let mut geraet: Option<Geraet> = None;
    let mut fehler: Option<(Fehler, String)> = None; // (Fehler, VAG-Nummer)
    let mut in_umgebung = false;
    // Kurzübersicht „01-Motorelektronik -- Status: Fehler 0010“
    let mut uebersicht: Vec<(u8, String, bool)> = Vec::new();
    let fertig_fehler = |g: &mut Option<Geraet>, f: &mut Option<(Fehler, String)>| {
        if let (Some(g), Some((mut f, nr))) = (g.as_mut(), f.take()) {
            if f.code.is_empty() {
                // TP 2.0: VAG-Nummer fünfstellig wie bei ecu-werkbank
                f.code = nr.parse::<u64>().map(|n| format!("{n:05}")).unwrap_or(nr);
            }
            g.fehler.push(f);
        }
    };
    for (i, roh) in text.lines().enumerate() {
        let z = roh.trim_end_matches('\r');
        let t = z.trim();
        if i < 3 && p.timestamp == 0 {
            if let Some((d, ts)) = kopfdatum(t) {
                p.datum = d;
                p.timestamp = ts;
                continue;
            }
        }
        // Fahrgestellnummer und km aus dem Kopf
        if t.starts_with("Fahrzeug-Ident.-Nr.:") || t.starts_with("VIN:") {
            if let Some(v) = t.split_whitespace().nth(1).filter(|v| v.len() == 17) {
                p.vin = Some(v.to_string());
            }
            for key in ["Kilometerstand:", "Mileage:"] {
                if let Some(pos) = t.find(key) {
                    p.km = p.km.or(km_wert(&t[pos + key.len()..]));
                }
            }
            continue;
        }
        if geraet.is_none() {
            if let Some((links, status)) = t.split_once(" -- Status:") {
                if let Some((adr, name)) = links.split_once('-') {
                    if let Ok(a) = u8::from_str_radix(adr.trim(), 16) {
                        let st = status.trim().to_lowercase();
                        if !st.starts_with("nicht angemeldet") && !st.starts_with("not registered") && !st.starts_with("keine") && !st.starts_with("cannot") {
                            uebersicht.push((a, name.trim().to_string(), st.starts_with("i.o.") || st.starts_with("ok")));
                        }
                        continue;
                    }
                }
            }
        }
        // neues Steuergerät
        if let Some(rest) = t.strip_prefix("Adresse ").or_else(|| t.strip_prefix("Address ")) {
            if let Some((adr, name)) = rest.split_once(':') {
                if let Ok(a) = u8::from_str_radix(adr.trim(), 16) {
                    fertig_fehler(&mut geraet, &mut fehler);
                    if let Some(g) = geraet.take() {
                        p.geraete.push(g);
                    }
                    let name = name.split("Labeldatei").next().unwrap_or(name).split("Labels:").next().unwrap_or(name).trim().to_string();
                    geraet = Some(Geraet { adresse: a, name, ..Default::default() });
                    in_umgebung = false;
                    continue;
                }
            }
        }
        let Some(g) = geraet.as_mut() else { continue };
        if g.teilenummer.is_empty() {
            if let Some(rest) = t.strip_prefix("Teilenummer SW:").or_else(|| t.strip_prefix("Part No SW:")) {
                g.teilenummer = rest.split("HW:").next().unwrap_or("").trim().to_string();
                continue;
            }
        }
        if t.starts_with("-----") {
            fertig_fehler(&mut geraet, &mut fehler);
            in_umgebung = false;
            continue;
        }
        // neuer Fehler (Zeile beginnt ganz links mit der Nummer)
        if let Some((nr, text)) = fehlerkopf(z) {
            fertig_fehler(&mut geraet, &mut fehler);
            fehler = Some((Fehler { text, ..Default::default() }, nr));
            in_umgebung = false;
            continue;
        }
        let Some((f, _)) = fehler.as_mut() else { continue };
        if t.starts_with("Umgebungsbedingungen") || t.starts_with("Freeze Frame") {
            in_umgebung = true;
            continue;
        }
        // „Readiness: …“ gehört zum Steuergerät, nicht zum Fehler
        if t.starts_with("Readiness") {
            in_umgebung = false;
            continue;
        }
        if in_umgebung {
            if let Some((k, v)) = paar(t) {
                // unbekannte Uhrzeit
                if matches!(k.as_str(), "Zeit" | "Time") && v.trim_start_matches(['0', ':']).is_empty() {
                    continue;
                }
                match k.as_str() {
                    // 0 km = Steuergerät kennt den Stand nicht
                    "Kilometerstand" | "Mileage" => f.km = f.km.or(km_wert(&v).filter(|&k| k > 0)),
                    "Datum" | "Date" => f.datum = f.datum.clone().or(datum_wert(&v)),
                    _ => {}
                }
                f.umgebung.push((k, v));
            }
            continue;
        }
        // Zeilen direkt unter dem Kopf: Code, Fehlerart, Status
        let erstes = t.split_whitespace().next().unwrap_or("");
        if f.code.is_empty() && f.art.is_empty() && ist_code(erstes) && t.contains('[') {
            // UDS: „P1602 00 [032] - Spannung zu klein“
            f.code = erstes.to_string();
            f.art = t.split_once("] -").map(|x| x.1.trim().trim_matches('-').trim().to_string()).unwrap_or_default();
        } else if f.art.is_empty() && f.alt.is_none() && ist_code(erstes) && t.contains(" - ") {
            // TP 2.0 mit genormtem Code: „P0304 - 000 - Verbrennungsaussetzer erkannt - Sporadisch“
            f.alt = Some(erstes.to_string());
            let teile: Vec<&str> = t.split(" - ").map(str::trim).collect();
            f.art = teile.get(2).copied().unwrap_or("").to_string();
            f.status = teile.get(3..).map(|x| x.iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(" - ")).unwrap_or_default();
        } else if f.art.is_empty() && erstes.len() == 3 && erstes.chars().all(|c| c.is_ascii_digit()) && t.contains(" - ") {
            // TP 2.0: „010 - Unterbrechung/Kurzschluss nach Plus - Sporadisch“
            let teile: Vec<&str> = t.split(" - ").map(str::trim).collect();
            f.art = teile.get(1).copied().unwrap_or("").to_string();
            f.status = teile.get(2..).map(|x| x.iter().filter(|s| !s.is_empty() && *s != &"-").copied().collect::<Vec<_>>().join(" - ")).unwrap_or_default();
        } else if t.contains("bestätigt") || t.contains("confirmed") || t.starts_with("Sporadisch") || t.starts_with("Intermittent") || t.starts_with("Warnleuchte") {
            f.status = t.to_string();
        }
    }
    fertig_fehler(&mut geraet, &mut fehler);
    if let Some(g) = geraet.take() {
        p.geraete.push(g);
    }
    for f in p.geraete.iter_mut().flat_map(|g| g.fehler.iter_mut()) {
        if f.art.trim_matches(['-', ' ']).is_empty() {
            f.art.clear();
        }
    }
    // nur die Kurzübersicht im Protokoll: Steuergeräte mit „Fehler“ ohne Details
    if p.geraete.is_empty() && !uebersicht.is_empty() {
        p.geraete = uebersicht
            .into_iter()
            .map(|(adresse, name, ok)| Geraet {
                adresse,
                name,
                fehler: if ok { vec![] } else { vec![Fehler { code: "?".into(), text: tr!("Fehler vorhanden (Details nicht im Protokoll)"), ..Default::default() }] },
                ..Default::default()
            })
            .collect();
    }
    if p.geraete.is_empty() {
        anyhow::bail!(tr!("Kein Auto-Scan-Protokoll erkannt (keine Zeile „Adresse xx:“)."));
    }
    Ok(p)
}

/// Platzhalter-Texte, die VCDS selbst nicht kennt („ROD - Unbekannter Fehlercode“).
fn unbekannt(t: &str) -> bool {
    let t = t.to_lowercase();
    t.starts_with("rod -") || t.contains("unbekannter fehlercode") || t.contains("unknown fault code") || t.contains("unknown dtc")
}

/// Fehlertexte (Code → Text) aus dem Protokoll.
pub fn texte(p: &Protokoll) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = p
        .geraete
        .iter()
        .flat_map(|g| &g.fehler)
        .filter(|f| !f.code.is_empty() && f.code != "?" && !f.text.is_empty() && !unbekannt(&f.text))
        .map(|f| (f.code.clone(), f.text.clone()))
        .collect();
    v.sort();
    v.dedup_by(|a, b| a.0 == b.0);
    v
}

/// Als Scan für den Verlauf.
pub fn als_scan(p: &Protokoll, vehicle: &str) -> ScanReport {
    let modules = p
        .geraete
        .iter()
        .map(|g| ModuleReport {
            protocol: "Import".into(),
            address: Some(u16::from(g.adresse)),
            name: g.name.clone(),
            can: format!("{:02X}", g.adresse),
            ident: [g.teilenummer.clone()].into_iter().filter(|s| !s.is_empty()).collect(),
            vin: None,
            dtcs: Some(
                g.fehler
                    .iter()
                    .map(|f| {
                        let mut umgebung = Vec::new();
                        if let Some(k) = f.km {
                            umgebung.push((crate::trs!("Kilometerstand").to_string(), format!("{} km", crate::report::group_thousands(k))));
                        }
                        if let Some(d) = &f.datum {
                            umgebung.push((crate::trs!("Datum").to_string(), d.clone()));
                        }
                        umgebung.extend(f.umgebung.iter().filter(|(k, _)| !matches!(k.as_str(), "Kilometerstand" | "Mileage" | "Datum" | "Date")).cloned());
                        DtcEntry {
                            code: f.code.clone(),
                            alt: f.alt.clone().or_else(|| (!f.art.is_empty()).then(|| f.art.clone())),
                            text: f.text.clone(),
                            status: f.status.clone(),
                            umgebung,
                            frueher: vec![],
                        }
                    })
                    .collect(),
            ),
        })
        .collect();
    ScanReport {
        created: p.datum.clone(),
        timestamp: p.timestamp,
        transport: tr!("Auto-Scan-Protokoll (importiert)"),
        vin: p.vin.clone(),
        vehicle: vehicle.to_string(),
        km: p.km,
        note: tr!("aus Auto-Scan-Protokoll importiert"),
        modules,
        ..Default::default()
    }
}

/// Ergebnis eines Imports.
#[derive(Debug, Clone, Default)]
pub struct Ergebnis {
    pub fahrzeug: String,
    pub datum: String,
    pub fehler: usize,
    /// false = dieser Scan war schon im Verlauf
    pub neu_im_verlauf: bool,
    pub neue_texte: usize,
}

/// Datei einlesen: Scan in den Verlauf (falls noch nicht da), km-Stand
/// übernehmen, Fehlertexte nur für Codes ohne eigenen Text speichern.
pub fn importieren(pfad: &std::path::Path) -> Result<Ergebnis> {
    use anyhow::Context;
    let b = std::fs::read(pfad).with_context(|| tr!("Lesen: {a}", a = pfad.display()))?;
    let p = lesen(&dekodieren(&b))?;
    let mut v = crate::vehicle::resolve(p.vin.as_deref(), None)?;
    if let Some(k) = p.km {
        if v.km.is_none_or(|x| k > x) {
            v.km = Some(k);
            v.km_datum = p.datum.split(' ').next().unwrap_or("").to_string();
            crate::vehicle::save(&v)?;
        }
    }
    let scan = als_scan(&p, &v.key);
    let schon_da = crate::history::history_dir(&v.key).join(format!("{}.toml", scan.timestamp)).exists();
    if !schon_da {
        crate::history::save(&scan)?;
    }
    let eigene = crate::fehlertexte::alle();
    let neu: Vec<(String, String)> = texte(&p).into_iter().filter(|(c, _)| !eigene.contains_key(&crate::fehlertexte::norm(c))).collect();
    crate::fehlertexte::mehrere_setzen(&neu)?;
    Ok(Ergebnis {
        fahrzeug: v.label(),
        datum: p.datum.clone(),
        fehler: p.geraete.iter().map(|g| g.fehler.len()).sum(),
        neu_im_verlauf: !schon_da,
        neue_texte: neu.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Ausgedachtes Beispiel im Aufbau eines VCDS-Protokolls (keine echten Daten)
    const BEISPIEL: &str = "Montag,02,März,2026,09:15:30:12345\r
VCDS -- Beispiel\r
\r
Fahrzeug-Ident.-Nr.: WVWZZZ1KZ9W000042   Kilometerstand: 123450km\r
\r
01-Motorelektronik -- Status: Fehler 0010\r
\r
-------------------------------------------------------------------------------\r
Adresse 01: Motorelektronik (ABC)       Labeldatei: x\r
   Teilenummer SW: 03C 906 016     HW: 03C 906 016 \r
\r
2 Fehlercodes gefunden:\r
000772 - Zylinder 4 \r
               P0304 - 000 - Verbrennungsaussetzer erkannt - Sporadisch\r
             Umgebungsbedingungen:\r
                    Fehlerstatus: 00100000\r
                    Kilometerstand: 120001 km\r
                    Datum: 2026.01.20\r
                    Zeit: 14:47:55\r
\r
03803 - Testgeber vorn \r
            000 - - - Warnleuchte EIN\r
\r
-------------------------------------------------------------------------------\r
Adresse 17: Schalttafeleinsatz (J285)       Labeldatei: y\r
   Teilenummer SW: 5K0 920 000 A    HW: 5K0 920 000 A\r
\r
1 Fehler gefunden:\r
6443 - Testversorgung Klemme 30 \r
          P1602 00 [032] - Spannung zu klein\r
          Sporadisch - unbestätigt - geprüft seit letzter Löschung\r
             Umgebungsbedingungen:\r
                    Kilometerstand: 122000 km\r
                    Datum: 2000.00.00\r
                    Zeit: 00:00:00\r
\r
Readiness: 0000 0000\r
\r
-------------------------------------------------------------------------------\r
Adresse 25: Wegfahrsperre (J334)       Labeldatei: z\r
\r
Kein Fehlercode gefunden.\r
";

    #[test]
    fn protokoll_lesen() {
        let p = lesen(BEISPIEL).unwrap();
        assert_eq!(p.datum, "02.03.2026 09:15");
        assert_eq!(p.vin.as_deref(), Some("WVWZZZ1KZ9W000042"));
        assert_eq!(p.km, Some(123_450));
        assert_eq!(p.geraete.len(), 3);
        let m = &p.geraete[0];
        assert_eq!((m.adresse, m.name.as_str(), m.teilenummer.as_str()), (0x01, "Motorelektronik (ABC)", "03C 906 016"));
        assert_eq!(m.fehler.len(), 2);
        let f = &m.fehler[0];
        assert_eq!((f.code.as_str(), f.alt.as_deref(), f.text.as_str()), ("00772", Some("P0304"), "Zylinder 4"));
        assert_eq!((f.km, f.datum.as_deref(), f.art.as_str(), f.status.as_str()), (Some(120_001), Some("20.01.2026"), "Verbrennungsaussetzer erkannt", "Sporadisch"));
        assert_eq!((m.fehler[1].code.as_str(), m.fehler[1].status.as_str()), ("03803", "Warnleuchte EIN"));
        let k = &p.geraete[1].fehler[0];
        assert_eq!((k.code.as_str(), k.text.as_str(), k.art.as_str()), ("P1602", "Testversorgung Klemme 30", "Spannung zu klein"));
        assert_eq!((k.km, k.datum.as_deref()), (Some(122_000), None));
        assert!(k.status.starts_with("Sporadisch - unbestätigt"));
        assert!(k.umgebung.iter().all(|(n, _)| n != "Readiness" && n != "Zeit"));
        assert!(p.geraete[2].fehler.is_empty());
        let t = texte(&p);
        assert_eq!(t.len(), 3);
        let s = als_scan(&p, "X");
        assert_eq!(s.total_dtcs(), 3);
        assert_eq!(s.modules[0].dtcs.as_ref().unwrap()[0].umgebung[0].1, "120.001 km");
    }

    #[test]
    fn nur_kurzuebersicht() {
        let t = "Mittwoch,06,September,2023,16:55:39:54205\nFahrzeug-Ident.-Nr.: WVWZZZ1KZ9W000042   Kilometerstand: 98980km\n01-Motorelektronik -- Status: i.O. 0000\n37-Navigation -- Status: Fehler 0010\n46-Komfort -- Status: nicht angemeldet 0011\n";
        let p = lesen(t).unwrap();
        assert_eq!(p.geraete.len(), 2);
        assert_eq!((p.geraete[1].adresse, p.geraete[1].fehler.len()), (0x37, 1));
        assert!(texte(&p).is_empty());
    }

    #[test]
    fn unbekannte_texte_nicht_uebernehmen() {
        assert!(unbekannt("ROD - Unbekannter Fehlercode"));
        assert!(!unbekannt("Datenbus empfangener Fehlerwert"));
    }

    #[test]
    fn latin1_und_import() {
        let _g = crate::history::TEST_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("vag-vcds-{}", std::process::id()));
        std::env::set_var("VAG_DATA_DIR", &dir);
        // Latin-1 wie in alten Protokollen
        let roh: Vec<u8> = BEISPIEL.chars().map(|c| c as u32 as u8).collect();
        assert_eq!(dekodieren(&roh), BEISPIEL);
        let datei = dir.join("scan.txt");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&datei, &roh).unwrap();
        crate::fehlertexte::setzen("03803", "mein eigener Text").unwrap();
        let e = importieren(&datei).unwrap();
        assert!(e.neu_im_verlauf);
        assert_eq!((e.fehler, e.neue_texte), (3, 2));
        // eigener Text bleibt
        assert_eq!(crate::fehlertexte::alle().get("03803").map(String::as_str), Some("mein eigener Text"));
        assert_eq!(crate::fehlertexte::alle().get("P1602").map(String::as_str), Some("Testversorgung Klemme 30"));
        let v = crate::vehicle::find_by_vin("WVWZZZ1KZ9W000042").unwrap();
        assert_eq!(v.km, Some(123_450));
        assert_eq!(crate::history::list(&v.key).len(), 1);
        // zweimal importieren: kein doppelter Scan
        assert!(!importieren(&datei).unwrap().neu_im_verlauf);
        assert_eq!(crate::history::list(&v.key).len(), 1);
        std::env::remove_var("VAG_DATA_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
