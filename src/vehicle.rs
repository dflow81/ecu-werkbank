//! Fahrzeugakte: ein Ordner pro Fahrzeug, in dem alles gespeichert wird.
//!
//! ```text
//! ~/.local/share/ecu-werkbank/fahrzeuge/
//!   WVWZZZ1KZAW000001/         ← Fahrgestellnummer (VIN)
//!     fahrzeug.toml            ← zugeordnetes Kennzeichen, Bezeichnung, Notiz
//!     berichte/                ← Fehlerspeicher-PDFs
//!     historie/                ← gespeicherte Scans (Vorher/Nachher)
//!     codierung/               ← Codierungs-Sicherungen
//!     aufzeichnungen/          ← CSV-Messwerte
//!     scope/                   ← Scope-/Messwert-PDFs
//!     mitschnitte/             ← CAN-Mitschnitte
//! ```
//!
//! Das Kennzeichen ist nur eine Zuordnung zum leichteren Finden. Fahrzeuge,
//! deren VIN (noch) unbekannt ist, bekommen vorübergehend einen Ordner nach
//! Kennzeichen; sobald die VIN bekannt wird, wird er in die VIN umbenannt.

use crate::tr;
use crate::history::data_dir;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const SUBDIRS: [&str; 8] = ["berichte", "historie", "codierung", "software", "nachruestung", "aufzeichnungen", "scope", "mitschnitte"];
/// Ordner für Scans ohne Kennzeichen und ohne VIN.
pub const UNKNOWN: &str = "ohne-Fahrzeug";

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Vehicle {
    /// Ordnername (nicht in der Datei gespeichert)
    #[serde(skip)]
    pub key: String,
    #[serde(default)]
    pub kennzeichen: String,
    #[serde(default)]
    pub vin: String,
    /// z. B. "Golf 6 1.4 TSI"
    #[serde(default)]
    pub bezeichnung: String,
    #[serde(default)]
    pub notiz: String,
    #[serde(default)]
    pub angelegt: String,
    /// Letzter bekannter Kilometerstand (aus Scan oder eingegeben)
    #[serde(default)]
    pub km: Option<u32>,
    #[serde(default)]
    pub km_datum: String,
    /// Hubraum in Litern (für Plausibilitätsprüfungen, optional)
    #[serde(default)]
    pub hubraum_l: Option<f64>,
    /// Partikelfilter-Kennungen (Diesel, optional)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dpf: Option<crate::dpf::Config>,
    /// Service-Anzeige im Kombi: Kennungen und Rückstellwerte (optional)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service: Option<crate::service::Config>,
    /// Wartungsposten (Tabellen-Felder müssen am Ende stehen: TOML)
    #[serde(default)]
    pub wartung: Vec<Wartung>,
    /// Gewechselte Teile (Tabellen-Feld, steht am Ende)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub teile: Vec<Teil>,
    /// Tankbuch (Tabellen-Feld, steht am Ende)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tankbuch: Vec<Tankung>,
}

/// Eine Tankfüllung.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Tankung {
    /// "TT.MM.JJJJ"
    #[serde(default)]
    pub datum: String,
    /// Kilometerstand beim Tanken
    pub km: u32,
    pub liter: f64,
    /// Bezahlt in Euro (gesamt)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preis: Option<f64>,
    /// Vollgetankt? (nur dann lässt sich der Verbrauch berechnen)
    #[serde(default = "ja")]
    pub voll: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notiz: String,
}

fn ja() -> bool {
    true
}

/// Ein gewechseltes Teil mit Datum und Kilometerstand.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Teil {
    /// "TT.MM.JJJJ"
    #[serde(default)]
    pub datum: String,
    #[serde(default)]
    pub km: Option<u32>,
    /// z. B. "Bremsscheiben + Beläge vorn"
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub teilenummer: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notiz: String,
    /// Kosten in Euro (Teil + ggf. Arbeit)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preis: Option<f64>,
}

/// Ein Wartungsposten mit Intervall.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Wartung {
    pub name: String,
    #[serde(default)]
    pub km_intervall: Option<u32>,
    #[serde(default)]
    pub monate: Option<u32>,
    #[serde(default)]
    pub letzte_km: Option<u32>,
    /// "TT.MM.JJJJ"
    #[serde(default)]
    pub letztes_datum: String,
}

/// Vorlage mit üblichen Intervallen (Richtwerte — Herstellerangaben beachten).
pub fn wartung_vorlage() -> Vec<Wartung> {
    let w = |name: &str, km: Option<u32>, monate: Option<u32>| Wartung { name: name.into(), km_intervall: km, monate, ..Default::default() };
    vec![
        w("Ölwechsel + Ölfilter", Some(15_000), Some(12)),
        w("Luftfilter", Some(30_000), Some(24)),
        w("Pollenfilter", Some(30_000), Some(24)),
        w("Zündkerzen", Some(60_000), Some(48)),
        w("Bremsflüssigkeit", None, Some(24)),
        w("Zahnriemen / Steuertrieb prüfen", Some(120_000), Some(60)),
        w("HU / AU", None, Some(24)),
    ]
}

/// Tage seit 1970 für ein Datum "TT.MM.JJJJ" (gregorianisch).
pub fn days_of(date: &str) -> Option<i64> {
    let p: Vec<&str> = date.trim().split(' ').next()?.split('.').collect();
    if p.len() != 3 {
        return None;
    }
    let (d, m, y): (i64, i64, i64) = (p[0].parse().ok()?, p[1].parse().ok()?, p[2].parse().ok()?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    // Algorithmus „days_from_civil“ (H. Hinnant)
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

/// Datum "TT.MM.JJJJ" zu Tagen seit 1970 (Gegenstück zu [`days_of`]).
pub fn datum_aus_tagen(z: i64) -> String {
    // Algorithmus „civil_from_days“ (H. Hinnant)
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{d:02}.{m:02}.{y}")
}

/// Datum plus ganze Monate (Tag wird ans Monatsende gekürzt, z. B. 31.01. + 1 → 28./29.02.).
pub fn plus_monate(datum: &str, monate: u32) -> Option<String> {
    days_of(datum)?;
    let p: Vec<i64> = datum.trim().split(' ').next()?.split('.').filter_map(|x| x.parse().ok()).collect();
    let (d, m, y) = (p[0], p[1], p[2]);
    let gesamt = y * 12 + (m - 1) + i64::from(monate);
    let (y, m) = (gesamt.div_euclid(12), gesamt.rem_euclid(12) + 1);
    let schalt = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let letzter = match m {
        4 | 6 | 9 | 11 => 30,
        2 if schalt => 29,
        2 => 28,
        _ => 31,
    };
    Some(format!("{:02}.{m:02}.{y}", d.min(letzter)))
}

pub fn today() -> String {
    crate::report::now_local().split(' ').next().unwrap_or("").to_string()
}

/// Fälligkeit eines Postens.
#[derive(Debug, Clone, PartialEq)]
pub struct Faelligkeit {
    pub stufe: crate::limits::Stufe,
    pub text: String,
}

impl Wartung {
    /// Stand bei `km_now` und Datum `heute` ("TT.MM.JJJJ").
    pub fn faellig(&self, km_now: Option<u32>, heute: &str) -> Faelligkeit {
        use crate::limits::Stufe;
        let mut parts = Vec::new();
        let mut stufe = Stufe::Gut;
        let mut known = false;
        if let (Some(iv), Some(last), Some(now)) = (self.km_intervall, self.letzte_km, km_now) {
            known = true;
            let rest = i64::from(last) + i64::from(iv) - i64::from(now);
            if rest <= 0 {
                stufe = stufe.max(Stufe::Warnung);
                parts.push(tr!("seit {a} km fällig", a = crate::report::group_thousands((-rest) as u32)));
            } else {
                if rest < 1000 || rest * 10 < i64::from(iv) {
                    stufe = stufe.max(Stufe::Hinweis);
                }
                parts.push(tr!("noch {a} km", a = crate::report::group_thousands(rest as u32)));
            }
        }
        if let (Some(mon), Some(last), Some(now)) = (self.monate, days_of(&self.letztes_datum), days_of(heute)) {
            known = true;
            let rest = last + i64::from(mon) * 365 / 12 - now;
            if rest <= 0 {
                stufe = stufe.max(Stufe::Warnung);
                parts.push(tr!("seit {a} Tagen fällig", a = -rest));
            } else {
                if rest <= 30 {
                    stufe = stufe.max(Stufe::Hinweis);
                }
                parts.push(if rest > 60 { tr!("noch {a} Monate", a = rest * 12 / 365) } else { tr!("noch {rest} Tage", rest = rest) });
            }
        }
        if !known {
            return Faelligkeit { stufe: Stufe::Hinweis, text: tr!("letzte Durchführung eintragen") };
        }
        Faelligkeit { stufe, text: parts.join(", ") }
    }
}

impl Vehicle {
    /// Kilometerstand übernehmen (nur wenn größer als der bekannte).
    pub fn note_km(&mut self, km: u32) -> bool {
        if self.km.is_none_or(|k| km > k) {
            self.km = Some(km);
            self.km_datum = today();
            true
        } else {
            false
        }
    }

    /// Posten als heute erledigt markieren.
    pub fn wartung_erledigt(&mut self, name: &str, km: Option<u32>) -> bool {
        let km = km.or(self.km);
        match self.wartung.iter_mut().find(|w| w.name.eq_ignore_ascii_case(name)) {
            Some(w) => {
                w.letzte_km = km;
                w.letztes_datum = today();
                true
            }
            None => false,
        }
    }

    /// Gewechseltes Teil eintragen. Passt der Name zu einem Wartungsposten
    /// (Groß-/Kleinschreibung egal), wird der Posten mit demselben Datum und
    /// km-Stand als erledigt eingetragen; dann kommt dessen Name zurück.
    /// Ein höherer km-Stand wird als aktueller Stand übernommen.
    pub fn teil_eintragen(&mut self, mut t: Teil) -> Option<String> {
        t.name = t.name.trim().to_string();
        t.teilenummer = t.teilenummer.trim().to_string();
        t.notiz = t.notiz.trim().to_string();
        if t.datum.trim().is_empty() {
            t.datum = today();
        }
        if let Some(k) = t.km {
            if self.km.is_none_or(|x| k > x) {
                self.km = Some(k);
                self.km_datum = t.datum.clone();
            }
        }
        let posten = self.wartung.iter_mut().find(|w| w.name.eq_ignore_ascii_case(&t.name)).map(|w| {
            // nur nachtragen, wenn es nicht älter ist als der bekannte Stand
            if days_of(&w.letztes_datum).is_none_or(|d| days_of(&t.datum).is_some_and(|n| n >= d)) {
                w.letztes_datum = t.datum.clone();
                w.letzte_km = t.km.or(w.letzte_km);
            }
            w.name.clone()
        });
        self.teile.push(t);
        self.teile_sortieren();
        posten
    }

    /// Tankfüllung eintragen (leeres Datum = heute). Höherer km-Stand wird übernommen.
    pub fn tanken_eintragen(&mut self, mut t: Tankung) {
        t.notiz = t.notiz.trim().to_string();
        if t.datum.trim().is_empty() {
            t.datum = today();
        }
        if self.km.is_none_or(|x| t.km > x) {
            self.km = Some(t.km);
            self.km_datum = t.datum.clone();
        }
        self.tankbuch.push(t);
        // neueste zuerst (km steigt mit der Zeit)
        self.tankbuch.sort_by_key(|t| std::cmp::Reverse((t.km, days_of(&t.datum).unwrap_or(i64::MIN))));
    }

    /// Neueste zuerst (nach Datum, dann km).
    pub fn teile_sortieren(&mut self) {
        self.teile.sort_by_key(|t| std::cmp::Reverse((days_of(&t.datum).unwrap_or(i64::MIN), t.km.unwrap_or(0))));
    }

    /// Fällige oder bald fällige Posten.
    pub fn wartung_offen(&self) -> Vec<(&Wartung, Faelligkeit)> {
        let heute = today();
        self.wartung
            .iter()
            .map(|w| (w, w.faellig(self.km, &heute)))
            .filter(|(_, f)| f.stufe != crate::limits::Stufe::Gut)
            .collect()
    }
}

impl Vehicle {
    /// Anzeige, z. B. "B-XY-123 · Golf 6" oder die VIN.
    pub fn label(&self) -> String {
        let main = if !self.kennzeichen.is_empty() {
            self.kennzeichen.clone()
        } else if !self.vin.is_empty() {
            self.vin.clone()
        } else {
            self.key.clone()
        };
        if self.bezeichnung.is_empty() { main } else { format!("{main} · {}", self.bezeichnung) }
    }

    pub fn dir(&self) -> PathBuf {
        dir(&self.key)
    }

    /// Unterordner (wird angelegt), z. B. `sub("berichte")`.
    pub fn sub(&self, name: &str) -> Result<PathBuf> {
        let d = self.dir().join(name);
        std::fs::create_dir_all(&d).with_context(|| tr!("Ordner anlegen: {a}", a = d.display()))?;
        Ok(d)
    }
}

pub fn root() -> PathBuf {
    migrate_old_layout();
    data_dir().join("fahrzeuge")
}

pub fn dir(key: &str) -> PathBuf {
    root().join(key)
}

/// Kennzeichen → Ordnername: "b xy 123" → "B-XY-123".
pub fn plate_key(plate: &str) -> String {
    let up = plate.trim().to_uppercase();
    let mut out = String::new();
    for c in up.chars() {
        if c.is_alphanumeric() {
            out.push(c);
        } else if (c == ' ' || c == '-') && !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

fn vin_clean(vin: &str) -> String {
    vin.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_uppercase()
}

/// Ordnername: VIN, ohne VIN das Kennzeichen.
fn key_for(kennzeichen: &str, vin: &str) -> String {
    let v = vin_clean(vin);
    if !v.is_empty() {
        return v;
    }
    let p = plate_key(kennzeichen);
    if !p.is_empty() { p } else { UNKNOWN.into() }
}

pub fn load(key: &str) -> Option<Vehicle> {
    let d = dir(key);
    if !d.is_dir() {
        return None;
    }
    let mut v: Vehicle = std::fs::read_to_string(d.join("fahrzeug.toml"))
        .ok()
        .and_then(|t| toml::from_str(&t).ok())
        .unwrap_or_else(|| Vehicle {
            // Ordner ohne fahrzeug.toml (z. B. nach Migration): Name = VIN?
            vin: if key.len() == 17 { key.to_string() } else { String::new() },
            ..Default::default()
        });
    v.key = key.to_string();
    Some(v)
}

/// Alle Fahrzeuge, sortiert nach Anzeige-Name.
pub fn list() -> Vec<Vehicle> {
    let mut v: Vec<Vehicle> = std::fs::read_dir(root())
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| load(&e.file_name().to_string_lossy()))
        .collect();
    v.sort_by_key(|x| x.label().to_lowercase());
    v
}

pub fn save(v: &Vehicle) -> Result<()> {
    let d = v.dir();
    std::fs::create_dir_all(&d).with_context(|| tr!("Ordner anlegen: {a}", a = d.display()))?;
    std::fs::write(d.join("fahrzeug.toml"), toml::to_string(v)?)
        .with_context(|| tr!("Schreiben: {a}", a = d.join("fahrzeug.toml").display()))
}

/// Neues Fahrzeug anlegen (Ordner nach VIN, ohne VIN nach Kennzeichen).
pub fn create(kennzeichen: &str, vin: &str, bezeichnung: &str) -> Result<Vehicle> {
    let key = key_for(kennzeichen, vin);
    if key == UNKNOWN {
        bail!(tr!("Bitte Kennzeichen oder Fahrgestellnummer angeben"));
    }
    if dir(&key).exists() {
        bail!(tr!("Fahrzeug {key} gibt es schon", key = key));
    }
    let v = Vehicle {
        key,
        kennzeichen: kennzeichen.trim().to_uppercase(),
        vin: vin_clean(vin),
        bezeichnung: bezeichnung.trim().to_string(),
        notiz: String::new(),
        angelegt: crate::report::now_local(),
        ..Default::default()
    };
    save(&v)?;
    Ok(v)
}

/// Fahrzeugdaten ändern. Kommt die VIN neu hinzu, wird der Ordner in die VIN umbenannt.
pub fn update(mut v: Vehicle) -> Result<Vehicle> {
    v.kennzeichen = v.kennzeichen.trim().to_uppercase();
    v.vin = vin_clean(&v.vin);
    let new_key = key_for(&v.kennzeichen, &v.vin);
    if new_key != v.key && new_key != UNKNOWN {
        let (from, to) = (dir(&v.key), dir(&new_key));
        if to.exists() {
            bail!(tr!("Es gibt schon einen Ordner {new_key}", new_key = new_key));
        }
        std::fs::rename(&from, &to).with_context(|| tr!("Umbenennen {a} → {b}", a = from.display(), b = to.display()))?;
        v.key = new_key;
    }
    save(&v)?;
    Ok(v)
}

pub fn find_by_vin(vin: &str) -> Option<Vehicle> {
    let vin = vin_clean(vin);
    if vin.is_empty() {
        return None;
    }
    list().into_iter().find(|v| v.vin == vin || v.key == vin)
}

/// Fahrzeug zu einer Abfrage bestimmen:
/// 1. gelesene VIN gehört zu einem bekannten Fahrzeug → das,
/// 2. sonst das aktive Fahrzeug (bekommt die VIN, falls es noch keine hat),
/// 3. sonst neues Fahrzeug unter der VIN, ohne VIN „ohne-Fahrzeug“.
pub fn resolve(vin: Option<&str>, active: Option<&str>) -> Result<Vehicle> {
    let active = active.filter(|a| !a.is_empty()).and_then(load);
    if let Some(vin) = vin.map(vin_clean).filter(|v| !v.is_empty()) {
        if let Some(v) = find_by_vin(&vin) {
            return Ok(v);
        }
        if let Some(mut a) = active {
            if a.vin.is_empty() {
                a.vin = vin;
                return update(a); // Ordner → VIN
            }
        }
        let v = Vehicle { key: vin.clone(), vin, angelegt: crate::report::now_local(), ..Default::default() };
        save(&v)?;
        return Ok(v);
    }
    if let Some(a) = active {
        return Ok(a);
    }
    let v = load(UNKNOWN).unwrap_or(Vehicle { key: UNKNOWN.into(), ..Default::default() });
    std::fs::create_dir_all(v.dir())?;
    Ok(v)
}

/// Aktives Fahrzeug aus Name, Kennzeichen oder VIN (für `--fahrzeug`).
pub fn find(query: &str) -> Option<Vehicle> {
    let q = query.trim();
    load(q)
        .or_else(|| load(&plate_key(q)))
        .or_else(|| find_by_vin(q))
        .or_else(|| list().into_iter().find(|v| plate_key(&v.kennzeichen) == plate_key(q)))
}

/// Anzahl Dateien je Unterordner.
pub fn stats(v: &Vehicle) -> Vec<(&'static str, usize)> {
    SUBDIRS
        .iter()
        .map(|s| {
            let n = std::fs::read_dir(v.dir().join(s)).map(|r| r.flatten().count()).unwrap_or(0);
            (*s, n)
        })
        .collect()
}

/// Dateiname mit Zeitstempel, z. B. "fehlerspeicher-2026-10-27_1745.pdf".
pub fn stamped(prefix: &str, ext: &str) -> String {
    let t = crate::report::now_local(); // "27.10.2026 17:45"
    let (d, h) = t.split_once(' ').unwrap_or((&t, ""));
    let p: Vec<&str> = d.split('.').collect();
    let date = if p.len() == 3 { format!("{}-{}-{}", p[2], p[1], p[0]) } else { d.to_string() };
    let secs = crate::report::unix_now() % 60;
    format!("{prefix}-{date}_{}{secs:02}.{ext}", h.replace(':', ""))
}

/// Alte Ablage (historie/<VIN>, codierung/<VIN>) in die Fahrzeugordner verschieben.
fn migrate_old_layout() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let base = data_dir();
        for sub in ["historie", "codierung"] {
            let old = base.join(sub);
            let Ok(rd) = std::fs::read_dir(&old) else { continue };
            for e in rd.flatten() {
                if !e.path().is_dir() {
                    continue;
                }
                let target = base.join("fahrzeuge").join(e.file_name()).join(sub);
                if target.exists() {
                    continue;
                }
                if std::fs::create_dir_all(target.parent().expect("hat Eltern")).is_ok() {
                    let _ = std::fs::rename(e.path(), &target);
                }
            }
            let _ = std::fs::remove_dir(&old); // nur wenn leer
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maintenance_due() {
        use crate::limits::Stufe;
        assert_eq!(days_of("01.03.2024").unwrap() - days_of("28.02.2024").unwrap(), 2); // Schaltjahr
        let w = Wartung { name: "Öl".into(), km_intervall: Some(15000), monate: Some(12), letzte_km: Some(140000), letztes_datum: "01.01.2026".into() };
        assert_eq!(w.faellig(Some(156000), "01.06.2026").stufe, Stufe::Warnung);
        assert_eq!(w.faellig(Some(154500), "01.06.2026").stufe, Stufe::Hinweis); // noch 500 km
        assert_eq!(w.faellig(Some(145000), "01.06.2026").stufe, Stufe::Gut);
        assert_eq!(w.faellig(Some(145000), "15.12.2026").stufe, Stufe::Hinweis); // noch ~17 Tage
        let leer = Wartung { name: "x".into(), monate: Some(24), ..Default::default() };
        assert_eq!(leer.faellig(None, "01.06.2026").stufe, Stufe::Hinweis);
    }

    #[test]
    fn teile() {
        let mut v = Vehicle { km: Some(240_000), wartung: wartung_vorlage(), ..Default::default() };
        let t = |d: &str, km: u32, n: &str| Teil { datum: d.into(), km: Some(km), name: n.into(), ..Default::default() };
        assert_eq!(v.teil_eintragen(t("01.05.2026", 241_000, " luftfilter ")), Some("Luftfilter".into()));
        assert_eq!(v.km, Some(241_000));
        let w = v.wartung.iter().find(|w| w.name == "Luftfilter").unwrap();
        assert_eq!((w.letzte_km, w.letztes_datum.as_str()), (Some(241_000), "01.05.2026"));
        assert_eq!(v.teil_eintragen(t("01.02.2026", 238_000, "Bremsscheiben vorn")), None);
        assert_eq!(v.km, Some(241_000)); // kleinerer Stand ändert nichts
        // älterer Eintrag überschreibt den Wartungsposten nicht
        v.teil_eintragen(t("01.01.2025", 220_000, "Luftfilter"));
        assert_eq!(v.wartung.iter().find(|w| w.name == "Luftfilter").unwrap().letzte_km, Some(241_000));
        let namen: Vec<_> = v.teile.iter().map(|t| t.datum.as_str()).collect();
        assert_eq!(namen, ["01.05.2026", "01.02.2026", "01.01.2025"]);
        // Speichern und Laden (TOML) behält alles
        let s = toml::to_string(&v).unwrap();
        let back: Vehicle = toml::from_str(&s).unwrap();
        assert_eq!(back.teile, v.teile);
        assert_eq!(back.wartung, v.wartung);
    }

    #[test]
    fn plate_keys() {
        assert_eq!(plate_key("b xy 123"), "B-XY-123");
        assert_eq!(plate_key(" M-AB 1234E "), "M-AB-1234E");
        assert_eq!(plate_key("--"), "");
    }

    #[test]
    fn create_resolve_rename() {
        let _g = crate::history::TEST_LOCK.lock().unwrap();
        let d = std::env::temp_dir().join(format!("ecu-werkbank-veh-{}", std::process::id()));
        std::env::set_var("VAG_DATA_DIR", &d);
        // Ohne VIN: vorläufig Ordner nach Kennzeichen
        let a = create("B XY 123", "", "Golf 6").unwrap();
        assert_eq!(a.key, "B-XY-123");
        // Scan liefert VIN, aktives Fahrzeug ohne VIN → bekommt sie, Ordner → VIN
        let r = resolve(Some("WVWZZZ1KZAW000001"), Some("B-XY-123")).unwrap();
        assert_eq!(r.key, "WVWZZZ1KZAW000001");
        assert_eq!(r.kennzeichen, "B XY 123");
        assert!(!dir("B-XY-123").exists());
        // Später ohne aktives Fahrzeug: über die VIN gefunden, übers Kennzeichen auch
        assert_eq!(resolve(Some("WVWZZZ1KZAW000001"), None).unwrap().key, "WVWZZZ1KZAW000001");
        assert_eq!(find("b-xy 123").unwrap().key, "WVWZZZ1KZAW000001");
        // Mit VIN angelegt → Ordner = VIN; Kennzeichen ändern benennt nichts um
        let b = create("M AB 1", "WAUZZZ8P0XA000002", "A3").unwrap();
        assert_eq!(b.key, "WAUZZZ8P0XA000002");
        let mut b2 = b.clone();
        b2.kennzeichen = "M CD 2".into();
        assert_eq!(update(b2).unwrap().key, "WAUZZZ8P0XA000002");
        assert!(find("M CD 2").is_some());
        let _ = std::fs::remove_dir_all(d);
    }
}
