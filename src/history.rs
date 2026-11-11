//! Fahrzeug-Historie: Scans je Fahrzeug speichern und vergleichen.
//!
//! Ablage im Fahrzeugordner: `…/fahrzeuge/<Kennzeichen|VIN>/historie/<Unix-Zeit>.toml`
//! (siehe [`crate::vehicle`]). Eine Datei je Scan, lesbares TOML.

use crate::report::ScanReport;
use anyhow::{Context, Result};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Datenordner von ECUWerkbank (Historie, Codierungs-Sicherungen).
pub fn data_dir() -> PathBuf {
    if let Ok(d) = std::env::var("VAG_DATA_DIR") {
        return PathBuf::from(d);
    }
    // Windows: %APPDATA%\ecu-werkbank
    #[cfg(windows)]
    if let Ok(d) = std::env::var("APPDATA") {
        return programmordner(&PathBuf::from(d));
    }
    if let Ok(d) = std::env::var("XDG_DATA_HOME") {
        if !d.is_empty() {
            return programmordner(&PathBuf::from(d));
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    programmordner(&PathBuf::from(home).join(".local/share"))
}

/// Programmordner unter `basis`, mit Umzug des alten Ordners — außer in Tests:
/// Die dürfen nie echte Ordner des Nutzers umbenennen.
pub(crate) fn programmordner(basis: &Path) -> PathBuf {
    if cfg!(test) {
        return basis.join("ecu-werkbank");
    }
    mit_umzug(basis)
}

/// Programmordner `basis/ecu-werkbank`. Gibt es ihn noch nicht, aber den
/// Ordner unter dem alten Namen `vag-diag`, wird dieser einmal umbenannt —
/// so bleiben Fahrzeuge, Scans und Sicherungen nach dem Namenswechsel da.
/// Klappt das Umbenennen nicht, wird der alte Ordner weiter benutzt.
pub fn mit_umzug(basis: &Path) -> PathBuf {
    let neu = basis.join("ecu-werkbank");
    let alt = basis.join("vag-diag");
    if !neu.exists() && alt.is_dir() && std::fs::rename(&alt, &neu).is_err() {
        return alt;
    }
    neu
}

#[cfg(test)]
pub(crate) static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Historie-Ordner eines Fahrzeugs.
pub fn history_dir(vehicle_key: &str) -> PathBuf {
    crate::vehicle::dir(vehicle_key).join("historie")
}

/// Fahrzeug-Schlüssel eines Scans (gesetzt über `ScanReport::assign_vehicle`).
fn key_of(r: &ScanReport) -> String {
    if !r.vehicle.is_empty() {
        return r.vehicle.clone();
    }
    crate::vehicle::resolve(r.vin.as_deref(), None).map(|v| v.key).unwrap_or_else(|_| crate::vehicle::UNKNOWN.into())
}

/// Scan in die Historie seines Fahrzeugs schreiben. Gibt den Dateipfad zurück.
pub fn save(r: &ScanReport) -> Result<PathBuf> {
    let dir = history_dir(&key_of(r));
    std::fs::create_dir_all(&dir).with_context(|| format!("Ordner anlegen: {}", dir.display()))?;
    let path = dir.join(format!("{}.toml", r.timestamp));
    let text = toml::to_string(r).context("Scan in TOML umwandeln")?;
    std::fs::write(&path, text).with_context(|| format!("Historie schreiben: {}", path.display()))?;
    let _ = crate::trend::add(&key_of(r), r.km, &[("Fehler gesamt".into(), r.total_dtcs() as f64, String::new())]);
    Ok(path)
}

/// Frühere Reparaturen zu einem Fehlercode bei einem Fahrzeug: Scans mit
/// Reparatur-Notiz, in denen der Code (oder im Scan davor) vorkam.
pub fn earlier_repairs(vehicle_key: &str, code: &str) -> Vec<String> {
    let scans = list(vehicle_key);
    let has = |r: &ScanReport| r.modules.iter().flat_map(|m| m.dtcs.iter().flatten()).any(|d| d.code == code || d.alt.as_deref() == Some(code));
    let mut out = Vec::new();
    for (i, (_, r)) in scans.iter().enumerate() {
        if r.reparatur.is_empty() {
            continue;
        }
        if has(r) || (i > 0 && has(&scans[i - 1].1)) {
            let km = r.km.map(|k| format!(" bei {} km", crate::report::group_thousands(k))).unwrap_or_default();
            out.push(format!("{}{km}: {}", r.created.split(' ').next().unwrap_or(""), r.reparatur));
        }
    }
    out
}

/// Frühere Reparaturen zu einem Code bei *anderen* eigenen Fahrzeugen.
pub fn other_vehicle_repairs(exclude_key: &str, code: &str) -> Vec<String> {
    crate::vehicle::list()
        .into_iter()
        .filter(|v| v.key != exclude_key)
        .flat_map(|v| earlier_repairs(&v.key, code).into_iter().map(move |t| format!("bei {}: {t}", v.label())))
        .collect()
}

/// Reparatur-Notiz eines gespeicherten Scans ändern.
pub fn set_repair_note(path: &Path, note: &str) -> Result<()> {
    let mut r = load(path)?;
    r.reparatur = note.trim().to_string();
    std::fs::write(path, toml::to_string(&r)?).with_context(|| format!("Schreiben: {}", path.display()))
}

pub fn load(path: &Path) -> Result<ScanReport> {
    let text = std::fs::read_to_string(path).with_context(|| format!("Lesen: {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("Kein gültiger Scan: {}", path.display()))
}

/// Alle gespeicherten Scans eines Fahrzeugs, älteste zuerst.
pub fn list(vehicle_key: &str) -> Vec<(PathBuf, ScanReport)> {
    let dir = history_dir(vehicle_key);
    let mut v: Vec<(PathBuf, ScanReport)> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("toml"))
        .filter_map(|p| load(&p).ok().map(|r| (p, r)))
        .collect();
    v.sort_by_key(|(_, r)| r.timestamp);
    v
}

/// Fahrzeuge mit gespeicherten Scans: (Fahrzeug, Anzahl, Datum des letzten).
pub fn vehicles() -> Vec<(crate::vehicle::Vehicle, usize, String)> {
    crate::vehicle::list()
        .into_iter()
        .filter_map(|v| {
            let scans = list(&v.key);
            let last = scans.last()?.1.created.clone();
            Some((v, scans.len(), last))
        })
        .collect()
}

/// Letzter gespeicherter Scan desselben Fahrzeugs vor `r`.
pub fn previous(r: &ScanReport) -> Option<ScanReport> {
    list(&key_of(r))
        .into_iter()
        .map(|(_, s)| s).rfind(|s| s.timestamp < r.timestamp)
}

#[derive(Debug, Clone, PartialEq)]
pub struct DiffItem {
    pub module: String,
    pub code: String,
    pub text: String,
    /// Schon in einem früheren Scan (vor dem Vergleichs-Scan) gesehen.
    pub recurring: bool,
    /// km-Stand, bei dem das Steuergerät den Fehler gespeichert hat (aus den Umgebungsbedingungen)
    pub km: Option<String>,
    /// Alle Umgebungsbedingungen des Fehlers (Name, Wert)
    pub umgebung: Vec<(String, String)>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Diff {
    pub new: Vec<DiffItem>,
    pub gone: Vec<DiffItem>,
    pub still: Vec<DiffItem>,
}

impl Diff {
    pub fn is_empty(&self) -> bool {
        self.new.is_empty() && self.gone.is_empty() && self.still.is_empty()
    }
}

/// Vergleich eines Scans mit einem älteren.
#[derive(Debug, Clone)]
pub struct Comparison {
    /// Datum des älteren Scans
    pub since: String,
    pub diff: Diff,
}

impl Comparison {
    pub fn to_text(&self) -> String {
        let mut s = format!("Vergleich mit Scan vom {}:\n", self.since);
        if self.diff.is_empty() {
            s += "  keine Fehler – weder damals noch heute\n";
        }
        for (label, items) in [("NEU", &self.diff.new), ("BEHOBEN", &self.diff.gone), ("weiterhin", &self.diff.still)] {
            for it in items {
                let rec = if it.recurring { "  (wiederkehrend)" } else { "" };
                s += &format!("  {label:<9} {}  {}  [{}]{rec}\n", it.code, it.text, it.module);
            }
        }
        s
    }
}

type Key = (String, String); // (Steuergerät, Code)

fn faults(r: &ScanReport) -> Vec<(Key, DiffItem)> {
    let mut v = Vec::new();
    for m in &r.modules {
        let module = format!("{} {}", m.protocol, m.title());
        let mkey = format!("{} {}", m.protocol, m.addr_text());
        for d in m.dtcs.iter().flatten() {
            v.push((
                (mkey.clone(), d.code.clone()),
                DiffItem {
                    module: module.clone(),
                    code: d.code.clone(),
                    text: d.text.clone(),
                    recurring: false,
                    km: d.umgebung.iter().find(|(k, _)| k == "Kilometerstand" || k == "Mileage").map(|(_, v)| v.clone()),
                    umgebung: d.umgebung.clone(),
                },
            ));
        }
    }
    v
}

/// Unterschiede `old` → `new`. `earlier` = noch ältere Scans (für „wiederkehrend“).
pub fn diff(old: &ScanReport, new: &ScanReport, earlier: &[ScanReport]) -> Diff {
    let old_f = faults(old);
    let new_f = faults(new);
    let old_keys: BTreeSet<&Key> = old_f.iter().map(|(k, _)| k).collect();
    let new_keys: BTreeSet<&Key> = new_f.iter().map(|(k, _)| k).collect();
    let seen_before: BTreeSet<Key> = earlier.iter().flat_map(|r| faults(r).into_iter().map(|(k, _)| k)).collect();
    // Steuergeräte, die im neuen Scan gar nicht erreicht wurden oder deren
    // Fehlerspeicher nicht lesbar war (z. B. Radio aus), gelten nicht als „behoben“.
    let new_modules: BTreeSet<String> = new.modules.iter().filter(|m| m.dtcs.is_some()).map(|m| format!("{} {}", m.protocol, m.addr_text())).collect();

    // Steuergeräte, die im alten Scan fehlten oder nicht lesbar waren: dort mit
    // dem letzten früheren Scan vergleichen, in dem sie lesbar waren
    let lesbar = |r: &ScanReport, mkey: &str| r.modules.iter().any(|m| m.dtcs.is_some() && format!("{} {}", m.protocol, m.addr_text()) == mkey);
    let vorher_da = |k: &Key| -> bool {
        if lesbar(old, &k.0) {
            return false;
        }
        earlier.iter().rev().find(|r| lesbar(r, &k.0)).is_some_and(|r| faults(r).iter().any(|(k2, _)| k2 == k))
    };

    let mut d = Diff::default();
    for (k, it) in &new_f {
        if old_keys.contains(k) || vorher_da(k) {
            d.still.push(it.clone());
        } else {
            let mut it = it.clone();
            it.recurring = seen_before.contains(k);
            d.new.push(it);
        }
    }
    for (k, it) in &old_f {
        if !new_keys.contains(k) && new_modules.contains(&k.0) {
            d.gone.push(it.clone());
        }
    }
    d
}

/// Vergleich mit dem letzten gespeicherten Scan desselben Fahrzeugs.
pub fn compare_with_previous(r: &ScanReport) -> Option<Comparison> {
    let all: Vec<ScanReport> = list(&key_of(r))
        .into_iter()
        .map(|(_, s)| s)
        .filter(|s| s.timestamp < r.timestamp)
        .collect();
    let (prev, earlier) = all.split_last()?;
    Some(Comparison { since: prev.created.clone(), diff: diff(prev, r, earlier) })
}

/// Fehler-Übersicht eines Fahrzeugs über alle gespeicherten Scans.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Uebersicht {
    /// Anzahl gespeicherter Scans
    pub scans: usize,
    /// Datum des letzten Scans
    pub letzter: String,
    /// Datum des vorletzten Scans (Vergleichsbasis), falls vorhanden
    pub seit: Option<String>,
    /// letzter gegen vorletzten Scan
    pub diff: Diff,
    /// Fehler, die in mindestens zwei Scans vorkamen: (Fehler, Anzahl Scans), häufigste zuerst
    pub haeufig: Vec<(DiffItem, usize)>,
}

pub fn uebersicht(vehicle_key: &str) -> Option<Uebersicht> {
    let all: Vec<ScanReport> = list(vehicle_key).into_iter().map(|(_, s)| s).collect();
    uebersicht_aus(&all)
}

/// Wie [`uebersicht`], aus einer Liste von Scans (älteste zuerst).
pub fn uebersicht_aus(all: &[ScanReport]) -> Option<Uebersicht> {
    let (last, rest) = all.split_last()?;
    let mut u = Uebersicht { scans: all.len(), letzter: last.created.clone(), ..Default::default() };
    if let Some((prev, earlier)) = rest.split_last() {
        u.seit = Some(prev.created.clone());
        u.diff = diff(prev, last, earlier);
    } else {
        u.diff.still = faults(last).into_iter().map(|(_, it)| it).collect();
    }
    let mut count: std::collections::BTreeMap<Key, (DiffItem, usize)> = Default::default();
    for r in all {
        // je Scan jeden Fehler nur einmal zählen
        let mut seen = BTreeSet::new();
        for (k, it) in faults(r) {
            if seen.insert(k.clone()) {
                count.entry(k).or_insert((it, 0)).1 += 1;
            }
        }
    }
    u.haeufig = count.into_values().filter(|(_, n)| *n >= 2).collect();
    u.haeufig.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.code.cmp(&b.0.code)));
    Some(u)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::{DtcEntry, ModuleReport};

    fn scan(ts: u64, codes: &[&str]) -> ScanReport {
        ScanReport {
            readiness: Vec::new(),
            created: format!("Scan {ts}"),
            timestamp: ts,
            transport: "Test".into(),
            vin: Some("WVWZZZ1KZAW000001".into()),
            note: String::new(),
            vehicle: "WVWZZZ1KZAW000001".into(),
            kennzeichen: String::new(),
            fahrzeug: String::new(),
            km: None,
            reparatur: String::new(),
            freeze: None,
            check: Vec::new(),
            pruefwerte: Vec::new(),
            modules: vec![ModuleReport {
                protocol: "UDS".into(),
                address: Some(1),
                name: "Motor".into(),
                can: "0x7e0/0x7e8".into(),
                ident: vec![],
                vin: None,
                dtcs: Some(
                    codes
                        .iter()
                        .map(|c| DtcEntry { code: c.to_string(), alt: None, text: String::new(), status: String::new(), umgebung: vec![], frueher: vec![] })
                        .collect(),
                ),
            }],
            comparison: None,
        }
    }

    #[test]
    fn diff_new_gone_still_recurring() {
        let a = scan(1, &["P0300"]);
        let b = scan(2, &["P0171"]);
        let c = scan(3, &["P0171", "P0300"]);
        let d = diff(&b, &c, &[a]);
        assert_eq!(d.still.len(), 1);
        assert_eq!(d.new.len(), 1);
        assert!(d.new[0].recurring);
        assert!(d.gone.is_empty());
        let d2 = diff(&c, &scan(4, &[]), &[]);
        assert_eq!(d2.gone.len(), 2);
    }

    #[test]
    fn uebersicht_neu_weg_haeufig() {
        let all = [scan(1, &["P0300", "P0300"]), scan(2, &["P0171"]), scan(3, &["P0171", "P0300"])];
        let u = uebersicht_aus(&all).unwrap();
        assert_eq!(u.scans, 3);
        assert_eq!(u.seit.as_deref(), Some("Scan 2"));
        assert_eq!(u.diff.new.len(), 1);
        assert!(u.diff.new[0].recurring);
        let h: Vec<(&str, usize)> = u.haeufig.iter().map(|(i, n)| (i.code.as_str(), *n)).collect();
        assert_eq!(h, vec![("P0171", 2), ("P0300", 2)]);
        assert!(uebersicht_aus(&[]).is_none());
    }

    #[test]
    fn unreachable_module_is_not_fixed() {
        let mut later = scan(2, &[]);
        later.modules.clear();
        assert!(diff(&scan(1, &["P0300"]), &later, &[]).gone.is_empty());
        // erreicht, aber Fehlerspeicher nicht lesbar (Golf 6: Radio aus) → nicht „behoben“
        let mut later = scan(2, &[]);
        later.modules[0].dtcs = None;
        assert!(diff(&scan(1, &["P0300"]), &later, &[]).gone.is_empty());
        // danach wieder lesbar: Fehler ist „weiterhin“ da, nicht „neu“
        let d = diff(&later, &scan(3, &["P0300"]), &[scan(1, &["P0300"])]);
        assert_eq!((d.still.len(), d.new.len()), (1, 0));
    }

    #[test]
    fn save_list_roundtrip() {
        let _g = TEST_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("ecu-werkbank-hist-{}", std::process::id()));
        std::env::set_var("VAG_DATA_DIR", &dir);
        let a = scan(100, &["P0300"]);
        let b = scan(200, &[]);
        save(&a).unwrap();
        save(&b).unwrap();
        let l = list(&a.vehicle);
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].1.modules[0].dtcs.as_ref().unwrap()[0].code, "P0300");
        let c = compare_with_previous(&b).unwrap();
        assert_eq!(c.diff.gone.len(), 1);
        assert_eq!(vehicles().len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[cfg(test)]
mod umzug_tests {
    use super::mit_umzug;

    #[test]
    fn alter_ordner_wird_uebernommen() {
        let basis = std::env::temp_dir().join(format!("ecu-umzug-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&basis);
        std::fs::create_dir_all(basis.join("vag-diag/fahrzeuge")).unwrap();
        std::fs::write(basis.join("vag-diag/fahrzeuge/x.toml"), "a").unwrap();
        let d = mit_umzug(&basis);
        assert_eq!(d, basis.join("ecu-werkbank"));
        assert!(d.join("fahrzeuge/x.toml").exists());
        assert!(!basis.join("vag-diag").exists());
        // gibt es beide, bleibt der neue — der alte wird nicht angefasst
        std::fs::create_dir_all(basis.join("vag-diag")).unwrap();
        assert_eq!(mit_umzug(&basis), basis.join("ecu-werkbank"));
        assert!(basis.join("vag-diag").exists());
        let _ = std::fs::remove_dir_all(&basis);
    }
}
