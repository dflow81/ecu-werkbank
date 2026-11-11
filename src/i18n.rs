//! Mehrsprachigkeit.
//!
//! Grundsprache ist Deutsch: die Texte stehen deutsch im Code, eingepackt in
//! [`tr!`](crate::tr) (oder [`trn!`](crate::trn) für feste Tabellen). Jede
//! weitere Sprache ist eine TOML-Datei, die deutsche Texte auf die Übersetzung
//! abbildet:
//!
//! ```toml
//! _name = "English"
//! "Speichern" = "Save"
//! "{n} Fehler gespeichert" = "{n} faults stored"
//! ```
//!
//! * eingebaut: `lang/<code>.toml` im Quellcode (siehe [`EINGEBAUT`])
//! * eigene, ohne neu zu bauen: `<Datenordner>/sprachen/<code>.toml` — hat
//!   Vorrang und ergänzt eine eingebaute Sprache
//!
//! Fehlt eine Übersetzung oder ist sie leer, erscheint der deutsche Text.
//! `ecu-werkbank sprache vorlage <code>` schreibt eine Datei mit allen Texten.
//! Platzhalter in geschweiften Klammern (`{n}`) müssen in der Übersetzung
//! erhalten bleiben.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, OnceLock, RwLock};

/// Eingebaute Sprachen: (Code, Dateiinhalt).
pub const EINGEBAUT: &[(&str, &str)] = &[("en", include_str!("../lang/en.toml"))];

/// Alle Texte im Programm (beim Bauen aus `tr!`/`trn!` gesammelt).
pub const ALLE_TEXTE: &[&str] = include!(concat!(env!("OUT_DIR"), "/texte.rs"));

struct Stand {
    code: String,
    map: Arc<HashMap<String, String>>,
}

fn stand() -> &'static RwLock<Stand> {
    static S: OnceLock<RwLock<Stand>> = OnceLock::new();
    S.get_or_init(|| {
        let code = start_sprache();
        RwLock::new(Stand { map: Arc::new(laden(&code)), code })
    })
}

fn eigene_datei(code: &str) -> std::path::PathBuf {
    crate::history::data_dir().join("sprachen").join(format!("{code}.toml"))
}

fn einstellung_datei() -> std::path::PathBuf {
    crate::history::data_dir().join("sprache")
}

/// Sprache beim Start: VAG_SPRACHE, sonst gespeicherte Wahl, sonst Systemsprache (LANG).
fn start_sprache() -> String {
    if cfg!(test) {
        return "de".into(); // Tests prüfen die deutschen Texte
    }
    if let Ok(s) = std::env::var("VAG_SPRACHE") {
        return norm(&s);
    }
    if let Ok(s) = std::fs::read_to_string(einstellung_datei()) {
        let s = norm(&s);
        if !s.is_empty() {
            return s;
        }
    }
    for var in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(v) = std::env::var(var) {
            let c = norm(&v);
            if !c.is_empty() && c != "c" && c != "posix" {
                return if verfuegbar().iter().any(|(k, _)| *k == c) { c } else { "de".into() };
            }
        }
    }
    "de".into()
}

/// "en_US.UTF-8" → "en".
fn norm(s: &str) -> String {
    s.trim().split(['_', '.', '-', '@']).next().unwrap_or("").to_lowercase()
}

/// Übersetzungen einer Sprache: eingebaute Datei, darüber die eigene.
fn laden(code: &str) -> HashMap<String, String> {
    let mut m = HashMap::new();
    if code == "de" {
        return m;
    }
    let mut add = |text: &str| {
        if let Ok(t) = toml::from_str::<BTreeMap<String, toml::Value>>(text) {
            for (k, v) in t {
                if let toml::Value::String(v) = v {
                    if !v.is_empty() && !k.starts_with('_') {
                        m.insert(k, v);
                    }
                }
            }
        }
    };
    if let Some((_, t)) = EINGEBAUT.iter().find(|(c, _)| *c == code) {
        add(t);
    }
    if let Ok(t) = std::fs::read_to_string(eigene_datei(code)) {
        add(&t);
    }
    m
}

fn name_aus(text: &str) -> Option<String> {
    toml::from_str::<BTreeMap<String, toml::Value>>(text).ok()?.get("_name")?.as_str().map(str::to_string)
}

/// Verfügbare Sprachen: (Code, Name), Deutsch zuerst.
pub fn verfuegbar() -> Vec<(String, String)> {
    let mut v = vec![("de".to_string(), "Deutsch".to_string())];
    for (c, t) in EINGEBAUT {
        v.push((c.to_string(), name_aus(t).unwrap_or_else(|| c.to_string())));
    }
    if let Ok(rd) = std::fs::read_dir(crate::history::data_dir().join("sprachen")) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("toml") {
                continue;
            }
            let Some(c) = p.file_stem().and_then(|x| x.to_str()).map(norm) else { continue };
            if c.is_empty() || v.iter().any(|(k, _)| *k == c) {
                continue;
            }
            let name = std::fs::read_to_string(&p).ok().and_then(|t| name_aus(&t)).unwrap_or_else(|| c.clone());
            v.push((c, name));
        }
    }
    v
}

/// Aktuelle Sprache (Code).
pub fn sprache() -> String {
    stand().read().unwrap().code.clone()
}

/// Sprache wechseln (nur für dieses Programm; `merken` speichert die Wahl).
pub fn set_sprache(code: &str, merken: bool) {
    let code = norm(code);
    let map = Arc::new(laden(&code));
    {
        let mut s = stand().write().unwrap();
        s.code = code.clone();
        s.map = map;
    }
    if merken {
        let p = einstellung_datei();
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(p, &code);
    }
}

/// Übersetzung eines deutschen Textes (auch für Texte, die erst zur Laufzeit
/// feststehen, z. B. aus Tabellen mit [`trn!`](crate::trn)).
pub fn t(de: &str) -> String {
    let map = stand().read().unwrap().map.clone();
    map.get(de).cloned().unwrap_or_else(|| de.to_string())
}

/// Wie [`t`], aber mit festem Rückgabetyp `&'static str` — für Funktionen, die
/// feste Texte liefern ([`trs!`](crate::trs)). Jede Übersetzung wird je Sprache
/// einmal angelegt und wiederverwendet.
pub fn ts(de: &'static str) -> &'static str {
    use std::sync::Mutex;
    static CACHE: OnceLock<Mutex<HashMap<(String, &'static str), &'static str>>> = OnceLock::new();
    let (code, map) = {
        let s = stand().read().unwrap();
        (s.code.clone(), s.map.clone())
    };
    let Some(v) = map.get(de) else { return de };
    let mut c = CACHE.get_or_init(Default::default).lock().unwrap();
    c.entry((code, de)).or_insert_with(|| Box::leak(v.clone().into_boxed_str()))
}

/// Vorlage für eine Sprache: alle Texte, vorhandene Übersetzungen eingetragen,
/// fehlende leer (dann erscheint Deutsch).
pub fn vorlage(code: &str) -> String {
    let code = norm(code);
    let vorhanden = laden(&code);
    let name = verfuegbar().into_iter().find(|(c, _)| *c == code).map(|(_, n)| n).unwrap_or_else(|| code.clone());
    let mut out = String::new();
    out += "# ECUWerkbank — Übersetzung. Links der deutsche Text (nicht ändern), rechts die Übersetzung.\n";
    out += "# Leer = deutscher Text wird angezeigt. Platzhalter wie {n} müssen erhalten bleiben.\n";
    out += &format!("# Speichern als: {}\n\n", eigene_datei(&code).display());
    out += &format!("_name = {}\n\n", toml_str(&name));
    let mut texte: Vec<&str> = ALLE_TEXTE.to_vec();
    texte.sort_unstable();
    texte.dedup();
    let (mut fehlt, mut da) = (Vec::new(), Vec::new());
    for k in texte {
        match vorhanden.get(k) {
            Some(v) => da.push(format!("{} = {}\n", toml_str(k), toml_str(v))),
            None => fehlt.push(format!("{} = \"\"\n", toml_str(k))),
        }
    }
    if !fehlt.is_empty() {
        out += &format!("# --- {} noch nicht übersetzt ---\n", fehlt.len());
        out += &fehlt.concat();
        out += "\n# --- übersetzt ---\n";
    }
    out += &da.concat();
    out
}

/// Anzahl übersetzter / aller Texte einer Sprache.
pub fn abdeckung(code: &str) -> (usize, usize) {
    let m = laden(&norm(code));
    let mut texte: Vec<&str> = ALLE_TEXTE.to_vec();
    texte.sort_unstable();
    texte.dedup();
    (texte.iter().filter(|k| m.contains_key(**k)).count(), texte.len())
}

/// Schlüssel einer einzelnen Zeile `"Text" = "Übersetzung"` (sonst `None`:
/// Kommentar, Leerzeile, `_name` …).
fn zeilen_schluessel(zeile: &str) -> Option<String> {
    let t = toml::from_str::<BTreeMap<String, toml::Value>>(zeile).ok()?;
    let mut it = t.into_keys();
    let k = it.next()?;
    (it.next().is_none() && !k.starts_with('_')).then_some(k)
}

/// Verwaiste Einträge einer Sprachdatei: Übersetzungen zu deutschen Texten,
/// die es im Programm nicht mehr gibt (Text geändert oder Funktion entfernt).
pub fn verwaist(datei_text: &str) -> Vec<String> {
    let alle: std::collections::HashSet<&str> = ALLE_TEXTE.iter().copied().collect();
    datei_text.lines().filter_map(zeilen_schluessel).filter(|k| !alle.contains(k.as_str())).collect()
}

/// Verwaiste Einträge entfernen — zeilenweise, Reihenfolge und Kommentare
/// bleiben. Liefert den neuen Text und die Anzahl entfernter Zeilen.
pub fn aufraeumen(datei_text: &str) -> (String, usize) {
    let alle: std::collections::HashSet<&str> = ALLE_TEXTE.iter().copied().collect();
    let mut weg = 0;
    let mut out = String::with_capacity(datei_text.len());
    for zeile in datei_text.split_inclusive('\n') {
        match zeilen_schluessel(zeile.trim_end()) {
            Some(k) if !alle.contains(k.as_str()) => weg += 1,
            _ => out.push_str(zeile),
        }
    }
    (out, weg)
}

/// Als einzeiliger TOML-String (auch als Schlüssel gültig).
fn toml_str(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o += "\\\"",
            '\\' => o += "\\\\",
            '\n' => o += "\\n",
            '\t' => o += "\\t",
            '\r' => o += "\\r",
            c if (c as u32) < 0x20 || c as u32 == 0x7F => o += &format!("\\u{:04X}", c as u32),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// Übersetzt einen deutschen Text; mit Platzhaltern `{name}`:
/// `tr!("{n} Fehler gespeichert", n = anzahl)`.
#[macro_export]
macro_rules! tr {
    ($s:literal) => {
        $crate::i18n::t($s)
    };
    ($s:literal, $($k:ident = $v:expr),+ $(,)?) => {{
        let mut s = $crate::i18n::t($s);
        $( s = s.replace(concat!("{", stringify!($k), "}"), &::std::string::ToString::to_string(&$v)); )+
        s
    }};
}

/// Wie [`tr!`](crate::tr), aber ohne Übersetzung — für Pfade, Dateiformate u. Ä.
#[macro_export]
macro_rules! platzhalter {
    ($s:literal, $($k:ident = $v:expr),+ $(,)?) => {{
        let mut s = ::std::string::String::from($s);
        $( s = s.replace(concat!("{", stringify!($k), "}"), &::std::string::ToString::to_string(&$v)); )+
        s
    }};
}

/// Übersetzt einen festen Text und liefert `&'static str` (siehe [`ts`]).
#[macro_export]
macro_rules! trs {
    ($s:literal) => {
        $crate::i18n::ts($s)
    };
}

/// Markiert einen Text für die Übersetzung, ohne ihn schon zu übersetzen
/// (für feste Tabellen); angezeigt wird er mit [`t`].
#[macro_export]
macro_rules! trn {
    ($s:literal) => {
        $s
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platzhalter_und_rueckfall() {
        let _g = crate::history::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set_sprache("de", false);
        assert_eq!(crate::tr!("{n} Fehler", n = 3), "3 Fehler");
        set_sprache("en", false);
        assert_eq!(sprache(), "en");
        // unbekannter Text bleibt deutsch
        assert_eq!(t("gibt es nicht äöü"), "gibt es nicht äöü");
        set_sprache("de", false);
    }

    #[test]
    fn englisch_ist_gueltig_und_platzhalter_passen() {
        let m = laden("en");
        assert!(!m.is_empty());
        let re = |s: &str| {
            let mut v: Vec<String> = Vec::new();
            let mut rest = s;
            while let Some(i) = rest.find('{') {
                let Some(j) = rest[i..].find('}') else { break };
                v.push(rest[i..i + j + 1].to_string());
                rest = &rest[i + j + 1..];
            }
            v.sort();
            v
        };
        for (k, v) in &m {
            assert_eq!(re(k), re(v), "Platzhalter passen nicht: {k:?} → {v:?}");
        }
    }

    #[test]
    fn vorlage_ist_gueltiges_toml() {
        let v = vorlage("xx");
        let m: BTreeMap<String, toml::Value> = toml::from_str(&v).expect("Vorlage muss gültiges TOML sein");
        assert!(m.len() > 100);
        assert_eq!(toml_str("a\"b\nc"), "\"a\\\"b\\nc\"");
    }

    #[test]
    fn norm_codes() {
        assert_eq!(norm("en_US.UTF-8"), "en");
        assert_eq!(norm(" DE "), "de");
    }

    #[test]
    fn nrc_bleibt_in_uebersetzung() {
        // Mehrere Stellen erkennen Fehler an „NRC 0x..“ — das muss in jeder
        // eingebauten Sprache so stehen bleiben.
        for (code, text) in EINGEBAUT {
            let m: BTreeMap<String, toml::Value> = toml::from_str(text).unwrap();
            for k in ["Negative Antwort auf SID {sid}: NRC {a} ({b})", "Negative Response auf SID {a}: NRC {b} ({c})"] {
                let v = m.get(k).and_then(|v| v.as_str()).unwrap_or_else(|| panic!("{code}: {k} fehlt"));
                assert!(v.contains("NRC {"), "{code}: {v}");
            }
        }
    }

    #[test]
    fn verwaiste_eintraege() {
        // „Gruppe:“ gibt es im Programm, den zweiten Text nicht
        let t = "# Kommentar\n_name = \"English\"\n\n\"Gruppe:\" = \"Group:\"\n\"Gibt es nicht mehr xyz\" = \"Gone\"\n\"Gruppe {g} kommt im Mitschnitt nicht vor — richtige Gruppe angegeben?\" = \"x\"\n";
        assert_eq!(verwaist(t), vec!["Gibt es nicht mehr xyz".to_string()]);
        let (neu, n) = aufraeumen(t);
        assert_eq!(n, 1);
        assert!(!neu.contains("xyz"));
        // alles andere unverändert, auch Kommentar, Leerzeile und _name
        assert_eq!(neu, t.replace("\"Gibt es nicht mehr xyz\" = \"Gone\"\n", ""));
        assert!(verwaist(&neu).is_empty());
    }
}
