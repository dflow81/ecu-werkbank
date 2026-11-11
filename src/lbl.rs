//! Texte zu Messwertblöcken aus Ross-Tech-Labeldateien im Klartext (`.lbl`).
//!
//! Die Dateien gehören dem Nutzer (z. B. aus seiner VCDS-Installation, Ordner
//! `Labels`). ecu-werkbank liest sie nur zur Anzeige aus dem eingestellten Ordner
//! und kopiert nichts davon ins Projekt oder in eigene Dateien.
//! Verschlüsselte Labels (`.clb`) werden nicht gelesen.
//!
//! Aufbau einer Zeile (Messwertblock, Block dezimal, Feld 0 = Blockname):
//! `001,0,Leerlauf` / `001,1,Drehzahl,(Soll: 640…800 /min)`
//! Umleitungen: `REDIRECT,ziel.lbl,muster…` (Muster = Teilenummer oder
//! nur der Index, `?` = beliebiges Zeichen).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Gemerkter Ordner (`<Datenordner>/lbl-ordner`), Umgebung `VAG_LBL_DIR` hat Vorrang.
pub fn ordner() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("VAG_LBL_DIR") {
        return Some(PathBuf::from(p));
    }
    let t = std::fs::read_to_string(crate::history::data_dir().join("lbl-ordner")).ok()?;
    let t = t.trim();
    (!t.is_empty()).then(|| PathBuf::from(t))
}

/// Ordner merken (leer = vergessen).
pub fn ordner_setzen(p: Option<&Path>) -> std::io::Result<()> {
    let d = crate::history::data_dir();
    std::fs::create_dir_all(&d)?;
    std::fs::write(d.join("lbl-ordner"), p.map(|p| p.display().to_string()).unwrap_or_default())
}

/// Teilenummer nur aus Buchstaben und Ziffern, groß: „03C 906 016“ → „03C906016“.
pub fn teil_norm(s: &str) -> String {
    s.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_uppercase()).collect()
}

/// Teilenummer aus der Kennung eines Steuergeräts („03C906016  5049“ → „03C906016“).
pub fn teil_aus_ident(ident: &str) -> Option<String> {
    let t = teil_norm(ident.split_whitespace().next()?);
    (t.len() >= 9 && t.chars().take(3).all(|c| c.is_ascii_alphanumeric())).then_some(t)
}

/// Texte eines Steuergeräts: Block → (Blockname, Feld → Name).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Labels {
    pub datei: PathBuf,
    pub bloecke: BTreeMap<u8, (Option<String>, BTreeMap<u8, String>)>,
    /// Anpassungskanäle aus A-Zeilen: Kanalnummer → (Name, Feldnamen)
    pub anpassungen: Gruppen,
    /// Codier-Helfer (C-Zeilen): Titel (C00) und Erklärungen (C01…)
    pub codier_titel: Option<String>,
    pub codier: Vec<String>,
    /// Login-Helfer (L-Zeilen): Titel (L00) und Login-Codes (L01…)
    pub login_titel: Option<String>,
    pub login: Vec<String>,
}

/// Ergebnis der Suche nach einer Labeldatei.
#[derive(Debug, Clone, PartialEq)]
pub enum Fund {
    Gefunden(Labels),
    /// Nur eine verschlüsselte Datei (.clb) passt — nicht lesbar
    Verschluesselt(PathBuf),
    Keine,
}

/// Datei als Text: Labeldateien sind Windows-1252, Umlaute wie Latin-1.
fn text(p: &Path) -> Option<String> {
    let b = std::fs::read(p).ok()?;
    Some(match String::from_utf8(b) {
        Ok(s) => s,
        Err(e) => e.into_bytes().iter().map(|&c| c as char).collect(),
    })
}

/// Passt ein Muster (mit `?` als Platzhalter) auf `s`?
fn muster_passt(muster: &str, s: &str) -> bool {
    muster.len() == s.len() && muster.chars().zip(s.chars()).all(|(m, c)| m == '?' || m == c)
}

/// Umleitung: passt eines der Muster zu dieser Teilenummer?
fn umleitung_passt(muster: &[&str], teil: &str) -> bool {
    let index = teil.get(9..).unwrap_or("");
    muster.iter().map(|m| m.trim().replace('-', "").to_ascii_uppercase()).filter(|m| !m.is_empty()).any(|m| {
        // kurzes Muster = nur der Index („AD“), sonst die ganze Nummer
        if m.len() <= 3 { muster_passt(&m, index) } else { muster_passt(&m, teil) }
    })
}

/// Eine Gruppe benannter Einträge: Nummer → (Name, Felder).
pub type Gruppen = BTreeMap<u16, (Option<String>, BTreeMap<u8, String>)>;

/// Zeilen auswerten: Umleitungen, Messwertblöcke und Anpassungskanäle (A-Zeilen).
/// Rückgabe: (Umleitungen, Messwertblöcke, Anpassungskanäle).
pub fn parse(t: &str) -> (Vec<(String, Vec<String>)>, BTreeMap<u8, (Option<String>, BTreeMap<u8, String>)>, Gruppen) {
    let mut um = Vec::new();
    let mut bl: BTreeMap<u8, (Option<String>, BTreeMap<u8, String>)> = BTreeMap::new();
    let mut anp: Gruppen = BTreeMap::new();
    for z in t.lines() {
        let z = z.split(';').next().unwrap_or("").trim();
        if z.is_empty() {
            continue;
        }
        let tok: Vec<&str> = z.split(',').map(str::trim).collect();
        if tok[0].eq_ignore_ascii_case("REDIRECT") {
            if let Some(ziel) = tok.get(1) {
                um.push((ziel.to_string(), tok[2..].iter().map(|s| s.to_string()).collect()));
            }
            continue;
        }
        eintragen(&tok, &mut bl, &mut anp);
    }
    (um, bl, anp)
}

/// Eine Label-Zeile einsortieren: A-Zeile → Anpassungskanal, sonst Messwertblock.
fn eintragen(tok: &[&str], bl: &mut BTreeMap<u8, (Option<String>, BTreeMap<u8, String>)>, anp: &mut Gruppen) {
    let name = match tok.get(2) {
        Some(n) => n.trim_matches('"').trim(),
        None => return,
    };
    if name.is_empty() {
        return;
    }
    let Some(Ok(f)) = tok.get(1).map(|f| f.parse::<u8>()) else { return };
    // A-Zeile: Anpassungskanal (nur die Zahl nach dem A, dezimal)
    if let Some(rest) = tok[0].strip_prefix(['A', 'a']) {
        if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
            if let Ok(k) = rest.parse::<u16>() {
                let e = anp.entry(k).or_default();
                if f == 0 {
                    e.0 = Some(name.to_string());
                } else {
                    e.1.insert(f, name.to_string());
                }
            }
            return;
        }
    }
    // sonst: Messwertblock (nur reine Zahl; B/C/L/O werden übersprungen)
    let Ok(b) = tok[0].parse::<u16>() else { return };
    if b > 255 {
        return;
    }
    let e = bl.entry(b as u8).or_default();
    if f == 0 {
        e.0 = Some(name.to_string());
    } else {
        e.1.insert(f, name.to_string());
    }
}

/// Wie gut passt ein Dateiname zur Teilenummer? None = gar nicht.
/// „03C-906-016-AD“ (genau) > „03C-906-016“ (ohne Index) > „03C-906-xxx“.
fn name_wert(stamm: &str, teil: &str) -> Option<u8> {
    let g: Vec<&str> = stamm.split('-').collect();
    if g.len() < 3 || teil.len() < 9 {
        return None;
    }
    let mut platz: u8 = 0;
    let mut pos = 0;
    for gr in &g[..3] {
        let n = gr.len();
        let stueck = teil.get(pos..pos + n)?;
        if gr.chars().all(|c| c == 'x' || c == 'X') {
            platz += 1;
        } else if !gr.eq_ignore_ascii_case(stueck) {
            return None;
        }
        pos += n;
    }
    if pos != 9 {
        return None;
    }
    let index = &teil[9..];
    let w: u8 = match g.get(3) {
        None => 2,
        Some(i) if i.eq_ignore_ascii_case(index) => 3,
        Some(_) => return None,
    };
    Some((w * 4).saturating_sub(platz))
}

/// Datei im Ordner ohne Rücksicht auf Groß-/Kleinschreibung finden.
fn datei_im(ordner: &Path, name: &str) -> Option<PathBuf> {
    let name = Path::new(name).file_name()?.to_str()?.to_string();
    std::fs::read_dir(ordner).ok()?.flatten().map(|e| e.path()).find(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.eq_ignore_ascii_case(&name)))
}

fn laden(ordner: &Path, p: &Path, teil: &str, tiefe: u8) -> Fund {
    if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("clb")) {
        return Fund::Verschluesselt(p.to_path_buf());
    }
    let Some(t) = text(p) else { return Fund::Keine };
    let (um, bl, anp) = parse(&t);
    if tiefe < 3 {
        for (ziel, muster) in &um {
            let m: Vec<&str> = muster.iter().map(String::as_str).collect();
            if umleitung_passt(&m, teil) {
                if let Some(z) = datei_im(ordner, ziel) {
                    return laden(ordner, &z, teil, tiefe + 1);
                }
            }
        }
    }
    let (ct, cz, lt, lz) = parse_codierhilfe(&t);
    if bl.is_empty() && anp.is_empty() && cz.is_empty() && lz.is_empty() {
        return Fund::Keine;
    }
    Fund::Gefunden(Labels { datei: p.to_path_buf(), bloecke: bl, anpassungen: anp, codier_titel: ct, codier: cz, login_titel: lt, login: lz })
}

/// Passende Labeldatei zur Teilenummer suchen und lesen.
pub fn suchen(ordner: &Path, teil: &str) -> Fund {
    let teil = teil_norm(teil);
    let mut beste: Option<(u8, bool, PathBuf)> = None;
    for p in std::fs::read_dir(ordner).into_iter().flatten().flatten().map(|e| e.path()) {
        let Some(ext) = p.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase) else { continue };
        if ext != "lbl" && ext != "clb" {
            continue;
        }
        let Some(stamm) = p.file_stem().and_then(|s| s.to_str()) else { continue };
        let Some(w) = name_wert(stamm, &teil) else { continue };
        let lbl = ext == "lbl";
        if beste.as_ref().is_none_or(|(bw, bl, _)| (w, lbl) > (*bw, *bl)) {
            beste = Some((w, lbl, p));
        }
    }
    match beste {
        Some((_, _, p)) => laden(ordner, &p, &teil, 0),
        None => Fund::Keine,
    }
}

/// Teilenummern je (kurzer) Adresse aus dem letzten Scan des Fahrzeugs.
/// Alle Protokolle (TP 2.0 und UDS) — Hauptsache es gibt eine 2-stellige
/// Adresse und eine erkennbare Teilenummer; sonst fällt der Eintrag weg.
pub fn teile_aus_scan(fahrzeug: &str) -> Vec<(u8, String)> {
    let Some((_, r)) = crate::history::list(fahrzeug).pop() else { return vec![] };
    r.modules
        .iter()
        .filter_map(|m| Some((u8::try_from(m.address?).ok()?, teil_aus_ident(m.ident.first()?)?)))
        .collect()
}

/// Teilenummern je (kurzer) Adresse aus der neuesten Software-Sicherung.
/// So gibt es .lbl-Namen auch ohne frischen Komplett-Scan.
pub fn teile_aus_software(fahrzeug: &str) -> Vec<(u8, String)> {
    let Some(st) = crate::swinfo::list(fahrzeug).pop() else { return vec![] };
    st.module
        .iter()
        .filter(|m| !m.teilenummer.is_empty())
        .filter_map(|m| {
            // Adresse wie "0x01"; "ID 0x7e0" hat keine 2-stellige Adresse → weg
            let a = m.adresse.strip_prefix("0x").and_then(|h| u8::from_str_radix(h, 16).ok())?;
            Some((a, teil_norm(&m.teilenummer)))
        })
        .collect()
}

/// Codier-Helfer (C-Zeilen) und Login-Helfer (L-Zeilen) auswerten.
/// Rückgabe: (Codier-Titel, Codier-Zeilen, Login-Titel, Login-Zeilen).
fn parse_codierhilfe(t: &str) -> (Option<String>, Vec<String>, Option<String>, Vec<String>) {
    let (mut ct, mut lt) = (None, None);
    let (mut cz, mut lz) = (Vec::new(), Vec::new());
    for z in t.lines() {
        let z = z.split(';').next().unwrap_or("").trim();
        let Some((kopf, text)) = z.split_once(',') else { continue };
        let text = text.trim().trim_matches('"').trim();
        if text.is_empty() {
            continue;
        }
        // C<nn> / L<nn>: nn=00 ist der Titel, sonst eine Erklärzeile
        let art = kopf.chars().next();
        let nummer: String = kopf.chars().skip(1).collect();
        if !nummer.chars().all(|c| c.is_ascii_digit()) || nummer.is_empty() {
            continue;
        }
        let titel = nummer == "00";
        match art {
            Some('C') | Some('c') => {
                if titel {
                    ct = Some(text.to_string());
                } else {
                    cz.push(text.to_string());
                }
            }
            Some('L') | Some('l') => {
                if titel {
                    lt = Some(text.to_string());
                } else {
                    lz.push(text.to_string());
                }
            }
            _ => {}
        }
    }
    (ct, cz, lt, lz)
}

/// Codier-/Login-Helfer zu einer Teilenummer (nur Nachschlagen aus .lbl).
/// Rückgabe: (Codier-Titel, Codier-Zeilen, Login-Titel, Login-Zeilen).
pub fn codier_hilfe(teil: &str) -> (Option<String>, Vec<String>, Option<String>, Vec<String>) {
    let Some(o) = ordner() else { return (None, vec![], None, vec![]) };
    match suchen(&o, teil) {
        Fund::Gefunden(l) => (l.codier_titel, l.codier, l.login_titel, l.login),
        _ => (None, vec![], None, vec![]),
    }
}

/// Benannte Anpassungskanäle zu einer Teilenummer: Kanalnummer → Name.
/// Nur Kanäle mit Namen (A<nnn>,0,…). Leer, wenn kein Label passt.
pub fn anpassung_namen(teil: &str) -> BTreeMap<u16, String> {
    let mut out = BTreeMap::new();
    let Some(o) = ordner() else { return out };
    if let Fund::Gefunden(l) = suchen(&o, teil) {
        for (k, (name, _)) in &l.anpassungen {
            if let Some(n) = name {
                out.insert(*k, n.clone());
            }
        }
    }
    out
}

/// Das in der Oberfläche gewählte Fahrzeug.
pub fn aktives_fahrzeug() -> Option<String> {
    let k = std::fs::read_to_string(crate::history::data_dir().join("aktives-fahrzeug")).ok()?;
    crate::vehicle::load(k.trim()).map(|v| v.key)
}

/// Texte für die Messwerttexte: Schlüssel wie dort („01-001“, „01-001-3“).
pub fn texte(teile: &[(u8, String)]) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let Some(o) = ordner() else { return out };
    for (adr, teil) in teile {
        if let Fund::Gefunden(l) = suchen(&o, teil) {
            for (b, (name, felder)) in &l.bloecke {
                if let Some(n) = name {
                    out.insert(crate::messwerttexte::schluessel(*adr, *b, None), n.clone());
                }
                for (f, n) in felder {
                    out.insert(crate::messwerttexte::schluessel(*adr, *b, Some(*f)), n.clone());
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zeilen_und_umleitung() {
        let (um, bl, anp) = parse("; Kommentar\nREDIRECT,03C-906-016-XX.lbl,AD,B?\n001,0,Leerlauf\n001,1,Drehzahl,(640…800)\n001,2,\"Temperatur\"\nB001,1,Grundeinstellung\nA005,0,Abgasrückführung\nA005,2,Soll\n");
        assert_eq!(um[0].0, "03C-906-016-XX.lbl");
        assert_eq!(bl[&1].0.as_deref(), Some("Leerlauf"));
        assert_eq!(bl[&1].1[&2], "Temperatur");
        assert_eq!(bl.len(), 1); // B-Zeile zählt nicht als Messwertblock
        assert_eq!(anp[&5].0.as_deref(), Some("Abgasrückführung"));
        assert_eq!(anp[&5].1[&2], "Soll");
        assert!(umleitung_passt(&["AD", "B?"], "03C906016BK"));
        assert!(!umleitung_passt(&["AD"], "03C906016AE"));
    }

    #[test]
    fn dateiname_passt() {
        assert_eq!(teil_aus_ident("03C906016  5049").as_deref(), Some("03C906016"));
        assert!(name_wert("03C-906-016-AD", "03C906016AD") > name_wert("03C-906-016", "03C906016AD"));
        assert!(name_wert("03C-906-016", "03C906016AD") > name_wert("03C-906-xxx", "03C906016AD"));
        assert_eq!(name_wert("03C-906-016-AE", "03C906016AD"), None);
        assert_eq!(name_wert("1K0-907-379", "03C906016"), None);
    }

    #[test]
    fn ordner_suchen() {
        let d = std::env::temp_dir().join(format!("vag-lbl-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("03C-906-016.LBL"), "REDIRECT,03C-906-016-AD.lbl,AD\n001,1,Allgemein\n").unwrap();
        std::fs::write(d.join("03c-906-016-ad.lbl"), b"002,1,K\xfchlmittel\n").unwrap();
        std::fs::write(d.join("1K0-907-379.clb"), "x").unwrap();
        match suchen(&d, "03C906016AD") {
            Fund::Gefunden(l) => assert_eq!(l.bloecke[&2].1[&1], "Kühlmittel"),
            f => panic!("{f:?}"),
        }
        match suchen(&d, "03C906016") {
            Fund::Gefunden(l) => assert_eq!(l.bloecke[&1].1[&1], "Allgemein"),
            f => panic!("{f:?}"),
        }
        assert!(matches!(suchen(&d, "1K0907379CD"), Fund::Verschluesselt(_)));
        assert_eq!(suchen(&d, "5K0937086"), Fund::Keine);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn codierhilfe_parsen() {
        let (ct, cz, lt, lz) = parse_codierhilfe("C00,Motor (J220) Codierung\nC01,00000 = Schaltgetriebe\nC02,00001 = Automatik\nL00,Motor Login\nL01,01283 = Codieren freischalten\n001,1,Messwert\n");
        assert_eq!(ct.as_deref(), Some("Motor (J220) Codierung"));
        assert_eq!(cz, vec!["00000 = Schaltgetriebe", "00001 = Automatik"]);
        assert_eq!(lt.as_deref(), Some("Motor Login"));
        assert_eq!(lz, vec!["01283 = Codieren freischalten"]);
    }

    #[test]
    fn anpassungskanaele_aus_lbl() {
        let _g = crate::history::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let d = std::env::temp_dir().join(format!("vag-lbl-anp-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("038-906-016-BEW.lbl"), "A005,0,Abgasrückführung\nA005,2,Soll\nA006,0,Leerlauf\n001,1,Nur Messwert\n").unwrap();
        std::env::set_var("VAG_LBL_DIR", &d);
        let namen = anpassung_namen("038906016BEW");
        assert_eq!(namen.get(&5).map(String::as_str), Some("Abgasrückführung"));
        assert_eq!(namen.get(&6).map(String::as_str), Some("Leerlauf"));
        assert_eq!(namen.len(), 2); // nur benannte Kanäle, Messwertblock zählt nicht
        std::env::remove_var("VAG_LBL_DIR");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn teile_aus_software_nur_kurze_adressen() {
        let _g = crate::history::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("vag-lbl-sw-{}", std::process::id()));
        std::env::set_var("VAG_DATA_DIR", &dir);
        let modul = |adresse: &str, teil: &str| crate::swinfo::ModulSw {
            protocol: "UDS".into(),
            adresse: adresse.into(),
            teilenummer: teil.into(),
            ..Default::default()
        };
        let st = crate::swinfo::Stand {
            created: "x".into(),
            timestamp: 1,
            vin: None,
            module: vec![
                modul("0x09", "1K0 907 279"), // kurze Adresse → übernommen (normiert)
                modul("ID 0x7e0", "03C906016"), // keine 2-stellige Adresse → weg
                modul("0x19", ""),              // ohne Teilenummer → weg
            ],
        };
        crate::swinfo::save("TESTKEY", &st).unwrap();
        let teile = teile_aus_software("TESTKEY");
        assert_eq!(teile, vec![(0x09u8, "1K0907279".to_string())]);
        std::env::remove_var("VAG_DATA_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
