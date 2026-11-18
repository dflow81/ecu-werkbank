//! Offene, selbst geschriebene Label-Datenbank (TOML).
//!
//! Zweck wie bei VCDS-Labels (Klartext für Steuergeräte, DTCs, Messwerte),
//! aber das ist DEIN eigener Inhalt in einem offenen Format. Es wird kein
//! proprietärer Label-Text übernommen — die Beschreibungen schreibst du selbst
//! oder nimmst sie aus offenen, genormten Quellen (ISO 14229 / SAE J1979).
//!
//! Format (eine Datei pro Steuergerät), siehe `labels/beispiel.toml`:
//!
//! ```toml
//! [ecu]
//! address = 0x01
//! name = "Motorsteuergerät"
//!
//! [[dtc]]
//! code = "P0101"
//! description = "Luftmassenmesser: Signal unplausibel"
//!
//! [[measuring]]
//! id = 0xF40C          # UDS-DID oder KWP-Gruppe
//! name = "Motordrehzahl"
//! unit = "1/min"
//! factor = 0.25        # Rohwert * factor + offset
//! offset = 0.0
//! ```

use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

/// Kopf einer Label-Datei.
#[derive(Debug, Deserialize, Clone, Default)]
pub struct EcuHeader {
    /// Fehlt, wenn die Adresse (noch) nicht eingetragen ist.
    #[serde(default)]
    pub address: Option<u16>,
    #[serde(default)]
    pub name: String,
    /// Optional: Teilenummer, z. B. "3C0-937-049-23".
    #[serde(default)]
    pub part: Option<String>,
    /// Optional: Request-CAN-ID (tester → ECU) für Auto-Scan/Codierung.
    #[serde(default)]
    pub tx: Option<u16>,
    /// Optional: Response-CAN-ID (ECU → tester).
    #[serde(default)]
    pub rx: Option<u16>,
    /// Optional: DID, unter dem die (Lange) Codierung liegt.
    #[serde(default)]
    pub coding_did: Option<u16>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DtcLabel {
    pub code: String,
    pub description: String,
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)] // Felder werden mit den Messwertblöcken ausgewertet
pub struct Measuring {
    pub id: u16,
    pub name: String,
    #[serde(default)]
    pub unit: String,
    #[serde(default = "one")]
    pub factor: f64,
    #[serde(default)]
    pub offset: f64,
}

fn one() -> f64 {
    1.0
}

/// Heuristik: Sieht der Text nach einer VCDS-Label-Rohdatei aus
/// (Kommentarzeilen mit ';' oder `Zahl,Zahl,Text`)? Dann ist es kein
/// eigenes Label-TOML, sondern Quellmaterial für den Skeleton-Import.
fn looks_like_vcds(text: &str) -> bool {
    text.lines().take(40).any(|l| {
        let l = l.trim();
        l.starts_with(';')
            || {
                let mut it = l.split(',');
                matches!(
                    (it.next().map(|s| s.trim().parse::<u32>()), it.next().map(|s| s.trim().parse::<u32>())),
                    (Some(Ok(_)), Some(Ok(_)))
                )
            }
    })
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)] // name wird bei der Anpassung ausgegeben
pub struct Adaptation {
    pub channel: u16,
    pub name: String,
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)] // byte/bit/name werden beim Codieren ausgewertet
pub struct Coding {
    pub byte: u16,
    #[serde(default)]
    pub bit: Option<u8>,
    pub name: String,
}

/// Sollwert für einen Messwert (Ampel / Warnung).
///
/// ```toml
/// [[sollwert]]
/// quelle = "kwp"   # kwp (Messwertblock) | uds (DID) | obd (PID)
/// id = 1           # Block, DID oder PID
/// feld = 2         # nur KWP: Feld 1–4
/// min = 80.0
/// max = 105.0
/// name = "Kühlmittel warm"
/// ```
#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct Sollwert {
    pub quelle: String,
    pub id: u16,
    #[serde(default)]
    pub feld: Option<u8>,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    #[serde(default)]
    pub name: String,
}

/// Eine komplette Label-Datei (zum Bearbeiten im Label-Editor).
#[derive(Debug, Deserialize, Clone, Default)]
pub struct LabelFile {
    pub ecu: EcuHeader,
    #[serde(default)]
    pub dtc: Vec<DtcLabel>,
    #[serde(default)]
    pub measuring: Vec<Measuring>,
    #[serde(default)]
    pub adaptation: Vec<Adaptation>,
    #[serde(default)]
    pub coding: Vec<Coding>,
    #[serde(default)]
    pub sollwert: Vec<Sollwert>,
    #[serde(default)]
    pub stellfunktion: Vec<crate::control::Stellfunktion>,
}

impl LabelFile {
    pub fn parse(text: &str) -> Result<Self> {
        Ok(toml::from_str(text)?)
    }

    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| crate::tr!("Label-Datei lesen: {a}", a = path.display()))?;
        Self::parse(&text).with_context(|| crate::tr!("Label-Datei ungültig: {a}", a = path.display()))
    }

    /// Als TOML schreiben (Adressen/IDs hexadezimal, wie von Hand üblich).
    pub fn to_toml(&self) -> String {
        let q = |s: &str| toml::Value::String(s.to_string()).to_string();
        let hex = |v: u16| if v > 0xFF { format!("{v:#06x}") } else { format!("{v:#04x}") };
        let mut o = String::from("# Eigene Label-Datei (mit dem ECUWerkbank-Label-Editor gespeichert)\n\n[ecu]\n");
        if let Some(a) = self.ecu.address {
            o += &format!("address = {}\n", hex(a));
        }
        o += &format!("name = {}\n", q(&self.ecu.name));
        if let Some(p) = &self.ecu.part {
            o += &format!("part = {}\n", q(p));
        }
        if let Some(v) = self.ecu.tx {
            o += &format!("tx = {}\n", hex(v));
        }
        if let Some(v) = self.ecu.rx {
            o += &format!("rx = {}\n", hex(v));
        }
        if let Some(v) = self.ecu.coding_did {
            o += &format!("coding_did = {}\n", hex(v));
        }
        for d in &self.dtc {
            o += &format!("\n[[dtc]]\ncode = {}\ndescription = {}\n", q(&d.code), q(&d.description));
        }
        for m in &self.measuring {
            // KWP-Blöcke (≤ 255) dezimal, UDS-DIDs hexadezimal
            let id = if m.id > 0xFF { format!("{:#06x}", m.id) } else { m.id.to_string() };
            o += &format!(
                "\n[[measuring]]\nid = {id}\nname = {}\nunit = {}\nfactor = {:?}\noffset = {:?}\n",
                q(&m.name),
                q(&m.unit),
                m.factor,
                m.offset
            );
        }
        for a in &self.adaptation {
            o += &format!("\n[[adaptation]]\nchannel = {}\nname = {}\n", a.channel, q(&a.name));
        }
        for w in &self.sollwert {
            o += &format!("\n[[sollwert]]\nquelle = {}\nid = {}\n", q(&w.quelle), w.id);
            if let Some(f) = w.feld {
                o += &format!("feld = {f}\n");
            }
            if let Some(v) = w.min {
                o += &format!("min = {v:?}\n");
            }
            if let Some(v) = w.max {
                o += &format!("max = {v:?}\n");
            }
            if !w.name.is_empty() {
                o += &format!("name = {}\n", q(&w.name));
            }
        }
        for f in &self.stellfunktion {
            let hex = |v: u16| format!("{v:#06x}");
            o += &format!(
                "\n[[stellfunktion]]\nname = {}\nart = {}\ntx = {}\nrx = {}\nid = {}\nbytes = {}\nfaktor = {:?}\noffset = {:?}\nmin = {:?}\nmax = {:?}\nrueckmeldung_pid = {:#04x}\nmax_dauer_s = {}\neinheit = {}\n",
                q(&f.name), q(&f.art), hex(f.tx), hex(f.rx), hex(f.id), f.bytes, f.faktor, f.offset, f.min, f.max, f.rueckmeldung_pid, f.max_dauer_s, q(&f.einheit)
            );
        }
        for c in &self.coding {
            o += &format!("\n[[coding]]\nbyte = {}\n", c.byte);
            if let Some(b) = c.bit {
                o += &format!("bit = {b}\n");
            }
            o += &format!("name = {}\n", q(&c.name));
        }
        o
    }
}

/// Leere Label-Datei aus den eigenen Scan-Daten eines Steuergeräts vorbefüllen
/// (nur Adresse, Name, Teilenummer und CAN-IDs — alles aus dem eigenen Scan).
/// Fehlercodes, Messwerte usw. bleiben leer und werden im Editor ergänzt.
pub fn skelett(address: u16, name: &str, part: Option<&str>, tx: Option<u16>, rx: Option<u16>) -> LabelFile {
    LabelFile {
        ecu: EcuHeader {
            address: Some(address),
            name: name.trim().to_string(),
            part: part.map(|p| p.trim().to_string()).filter(|p| !p.is_empty()),
            tx,
            rx,
            coding_did: None,
        },
        ..Default::default()
    }
}

/// Dateiname-Vorschlag für ein Steuergerät (ohne Endung), z. B. „01-motor“.
pub fn dateiname(address: u16, name: &str) -> String {
    let teile: Vec<String> = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .map(str::to_string)
        .collect();
    let n = teile.join("-");
    let n: String = n.chars().take(24).collect();
    let n = n.trim_matches('-');
    if n.is_empty() { format!("{address:02x}") } else { format!("{address:02x}-{n}") }
}

/// Eigener Label-Ordner des Nutzers (ohne Projektordner/VAG_LABELS):
/// Windows `%APPDATA%\ecu-werkbank\labels`, sonst `~/.config/ecu-werkbank/labels`.
pub fn nutzer_dir() -> std::path::PathBuf {
    use std::path::PathBuf;
    #[cfg(windows)]
    let base = std::env::var("APPDATA").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("."));
    #[cfg(not(windows))]
    let base = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into())).join(".config"));
    crate::history::programmordner(&base).join("labels")
}

/// Standard-Label-Ordner:
/// 1. `$VAG_LABELS`, 2. `./labels` (Projektordner), 3. `~/.config/ecu-werkbank/labels`
///    (wird beim ersten Mal mit der Beispieldatei angelegt).
pub fn default_dir() -> std::path::PathBuf {
    use std::path::PathBuf;
    if let Ok(d) = std::env::var("VAG_LABELS") {
        return PathBuf::from(d);
    }
    let local = PathBuf::from("labels");
    if local.is_dir() {
        return local;
    }
    let dir = nutzer_dir();
    if !dir.exists() && std::fs::create_dir_all(&dir).is_ok() {
        let _ = std::fs::write(dir.join("beispiel.toml"), include_str!("../labels/beispiel.toml"));
    }
    dir
}

/// Alle geladenen Labels, nach ECU-Adresse indexiert.
#[derive(Default)]
pub struct LabelDb {
    ecu_names: HashMap<u16, String>,
    dtcs: HashMap<String, String>,          // code -> description
    /// Fehlertexte aus der Fahrzeug-Datenbank (nach den Labels, vor der eingebauten Liste)
    db_dtcs: HashMap<String, String>,
    measuring: HashMap<(u16, u16), Measuring>, // (ecu, id) -> label
    adaptation: HashMap<(u16, u16), Adaptation>, // (ecu, channel) -> label
    coding: HashMap<u16, Vec<Coding>>,        // ecu -> coding bits
    sollwerte: Vec<(Option<u16>, Sollwert)>,  // (ecu, Sollwert)
    stellfunktionen: Vec<crate::control::Stellfunktion>,
    ecu_can: HashMap<u16, (u16, u16)>,        // ecu -> (tx, rx)
    coding_did: HashMap<u16, u16>,            // ecu -> coding DID
    files: usize,                             // gelesene Label-Dateien
    without_address: usize,                   // davon ohne Adresse (nicht zugeordnet)
}

impl LabelDb {
    /// Prüft, ob ein Text eine gültige Label-Datei ist (für den Import).
    pub fn check_file(text: &str) -> Result<()> {
        toml::from_str::<LabelFile>(text)?;
        Ok(())
    }

    /// Lädt alle `*.toml` UND TOML-ähnliche `*.txt` aus einem Verzeichnis
    /// inklusive Unterordnern.
    /// Fehlt es, ist die DB leer (das Tool funktioniert dann ohne Klartext weiter).
    /// Eine `.txt`, die kein gültiges Label-TOML ist (z. B. VCDS-Rohtext), wird
    /// übersprungen — es wird nichts daraus übernommen.
    pub fn load_dir(dir: &Path) -> Result<Self> {
        let mut db = Self::load_dir_files(dir)?;
        // eigene Fehlertexte gewinnen
        for (code, text) in crate::fehlertexte::alle() {
            db.dtcs.insert(code, text);
        }
        db.db_dtcs = crate::db::fehlertexte_falls_vorhanden().into_iter().collect();
        Ok(db)
    }

    /// Eigenen Fehlertext sofort übernehmen (gespeichert wird über
    /// [`crate::fehlertexte::setzen`]).
    pub fn set_dtc_text(&mut self, code: &str, text: &str) {
        let code = crate::fehlertexte::norm(code);
        if text.trim().is_empty() {
            self.dtcs.remove(&code);
        } else {
            self.dtcs.insert(code, text.trim().to_string());
        }
    }

    fn load_dir_files(dir: &Path) -> Result<Self> {
        let mut db = LabelDb::default();
        if !dir.is_dir() {
            return Ok(db); // kein Label-Ordner -> leere DB
        }
        let mut paths = Vec::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&d) else { continue };
            for entry in rd.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    stack.push(p);
                } else {
                    paths.push(p);
                }
            }
        }
        // Feste Reihenfolge: eigene Dateien (oberste Ebene) vor Importen,
        // damit bei gleichen Einträgen dein Text gewinnt.
        paths.sort_by_key(|p| (p.components().count(), p.clone()));
        for path in paths {
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_ascii_lowercase());
            if !matches!(ext.as_deref(), Some("toml") | Some("txt")) {
                continue;
            }
            let text = std::fs::read_to_string(&path)
                .with_context(|| crate::tr!("Label-Datei lesen: {a}", a = path.display()))?;
            // Kaputte/leere/kein-TOML Dateien überspringen, nicht das Tool crashen.
            let lf: LabelFile = match toml::from_str(&text) {
                Ok(lf) => lf,
                Err(e) => {
                    let hint = if looks_like_vcds(&text) {
                        crate::trs!("  (sieht nach einer fremden Labeldatei aus — ECUWerkbank liest nur eigene Label-Dateien im eigenen Format, siehe beispiel.toml)")
                    } else {
                        ""
                    };
                    eprintln!("{}", crate::tr!("Warnung: überspringe {a} ({e}){hint}", a = path.display(), e = e, hint = hint));
                    continue;
                }
            };
            db.files += 1;
            let Some(addr) = lf.ecu.address else {
                db.without_address += 1;
                continue;
            };
            // Zusammenführen ohne Verlust: Ein leerer Eintrag (z. B. aus einer
            // frisch angelegten Datei) überschreibt nie einen bereits ausgefüllten.
            let name_slot = db.ecu_names.entry(addr).or_default();
            if name_slot.is_empty() {
                *name_slot = lf.ecu.name.clone();
            }
            if let (Some(tx), Some(rx)) = (lf.ecu.tx, lf.ecu.rx) {
                db.ecu_can.entry(addr).or_insert((tx, rx));
            }
            if let Some(cd) = lf.ecu.coding_did {
                db.coding_did.entry(addr).or_insert(cd);
            }
            for d in lf.dtc {
                let slot = db.dtcs.entry(d.code.to_uppercase()).or_default();
                if slot.is_empty() {
                    *slot = d.description;
                }
            }
            for m in lf.measuring {
                match db.measuring.get(&(addr, m.id)) {
                    Some(old) if !old.name.is_empty() => {}
                    _ => {
                        db.measuring.insert((addr, m.id), m);
                    }
                }
            }
            for a in lf.adaptation {
                match db.adaptation.get(&(addr, a.channel)) {
                    Some(old) if !old.name.is_empty() => {}
                    _ => {
                        db.adaptation.insert((addr, a.channel), a);
                    }
                }
            }
            db.stellfunktionen.extend(lf.stellfunktion.iter().cloned());
            for sw in lf.sollwert.iter().cloned() {
                db.sollwerte.push((Some(addr), sw));
            }
            if !lf.coding.is_empty() {
                db.coding.entry(addr).or_insert(lf.coding);
            }
        }
        Ok(db)
    }

    #[allow(dead_code)] // wird mit Messwertblöcken / scan genutzt
    pub fn ecu_name(&self, address: u16) -> Option<&str> {
        self.ecu_names.get(&address).map(|s| s.as_str()).filter(|s| !s.is_empty())
    }

    /// Klartext zu einem Fehlercode: zuerst deine Labels, sonst der
    /// eingebaute Grundtext für genormte Codes (siehe [`crate::dtc_db`]).
    pub fn dtc_description(&self, code: &str) -> Option<&str> {
        self.dtcs
            .get(&code.to_uppercase())
            .map(|s| s.as_str())
            .filter(|s| !s.is_empty())
            .or_else(|| self.db_dtcs.get(&code.to_uppercase()).map(|s| s.as_str()))
            .or_else(|| crate::dtc_db::lookup(code))
    }

    /// Sollwert-Bereich: Label (passendes Steuergerät, sonst beliebiges), bei
    /// OBD sonst eingebauter Grundwert.
    pub fn sollwert(&self, quelle: &str, ecu: Option<u16>, id: u16, feld: Option<u8>) -> Option<crate::limits::Range> {
        let matches = |s: &Sollwert| s.quelle.eq_ignore_ascii_case(quelle) && s.id == id && (s.feld.is_none() || s.feld == feld);
        let hit = self
            .sollwerte
            .iter()
            .find(|(e, s)| ecu.is_some() && *e == ecu && matches(s))
            .or_else(|| self.sollwerte.iter().find(|(_, s)| matches(s)))
            .map(|(_, s)| crate::limits::Range { min: s.min, max: s.max });
        hit.or_else(|| if quelle.eq_ignore_ascii_case("obd") { crate::limits::obd_default(id as u8) } else { None })
    }

    /// Stellfunktionen aus allen Label-Dateien.
    pub fn stellfunktionen(&self) -> &[crate::control::Stellfunktion] {
        &self.stellfunktionen
    }

    /// Nur deine eigenen Labels (ohne eingebaute Grundtexte).
    pub fn own_dtc_description(&self, code: &str) -> Option<&str> {
        self.dtcs.get(&code.to_uppercase()).map(|s| s.as_str())
    }

    #[allow(dead_code)] // wird mit Messwertblöcken genutzt
    pub fn measuring(&self, ecu: u16, id: u16) -> Option<&Measuring> {
        self.measuring.get(&(ecu, id))
    }

    /// Messwert-Label anhand der ID, egal welches Steuergerät.
    pub fn measuring_any(&self, id: u16) -> Option<&Measuring> {
        self.measuring.iter().find(|((_, i), _)| *i == id).map(|(_, m)| m)
    }

    #[allow(dead_code)] // wird beim Anpassungs-Befehl genutzt
    pub fn adaptation(&self, ecu: u16, channel: u16) -> Option<&Adaptation> {
        self.adaptation.get(&(ecu, channel))
    }

    pub fn coding(&self, ecu: u16) -> Option<&[Coding]> {
        self.coding.get(&ecu).map(|v| v.as_slice())
    }

    /// Hinterlegte CAN-IDs (tx, rx) eines Steuergeräts, falls im Label gesetzt.
    pub fn ecu_can(&self, ecu: u16) -> Option<(u16, u16)> {
        self.ecu_can.get(&ecu).copied()
    }

    /// DID, unter dem die Codierung liegt, falls im Label gesetzt.
    pub fn coding_did(&self, ecu: u16) -> Option<u16> {
        self.coding_did.get(&ecu).copied()
    }

    /// Alle Steuergeräte mit hinterlegten CAN-IDs (für Auto-Scan).
    pub fn ecus_with_can(&self) -> Vec<(u16, u16, u16)> {
        let mut v: Vec<_> = self
            .ecu_can
            .iter()
            .map(|(a, (tx, rx))| (*a, *tx, *rx))
            .collect();
        v.sort_by_key(|(a, _, _)| *a);
        v
    }

    /// Alle bekannten Steuergeräte (Adresse, Name), sortiert.
    pub fn ecus(&self) -> Vec<(u16, String)> {
        let mut v: Vec<_> = self.ecu_names.iter().map(|(a, n)| (*a, n.clone())).collect();
        v.sort_by_key(|(a, _)| *a);
        v
    }

    /// Alle bekannten Messwerte (ECU, ID, Label), sortiert.
    pub fn measurements(&self) -> Vec<(u16, u16, Measuring)> {
        let mut v: Vec<_> = self
            .measuring
            .iter()
            .map(|((e, i), m)| (*e, *i, m.clone()))
            .collect();
        v.sort_by_key(|(e, i, _)| (*e, *i));
        v
    }

    /// (gelesene Dateien, davon ohne Adresse)
    pub fn file_counts(&self) -> (usize, usize) {
        (self.files, self.without_address)
    }

    pub fn len(&self) -> (usize, usize, usize, usize) {
        (
            self.ecu_names.len(),
            self.dtcs.len(),
            self.measuring.len(),
            self.adaptation.len() + self.coding.values().map(|v| v.len()).sum::<usize>(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_file_roundtrip_keeps_content() {
        let text = std::fs::read_to_string("labels/beispiel.toml").unwrap();
        let a = LabelFile::parse(&text).unwrap();
        let written = a.to_toml();
        assert!(written.contains("address = 0x01"));
        assert!(written.contains("id = 0xf40c"));
        let b = LabelFile::parse(&written).unwrap();
        assert_eq!(a.ecu.address, b.ecu.address);
        assert_eq!(a.ecu.name, b.ecu.name);
        assert_eq!(a.dtc.len(), b.dtc.len());
        assert_eq!(a.measuring.len(), b.measuring.len());
        for (x, y) in a.measuring.iter().zip(&b.measuring) {
            assert_eq!((x.id, &x.name, &x.unit, x.factor, x.offset), (y.id, &y.name, &y.unit, y.factor, y.offset));
        }
        // Sonderzeichen in Texten werden korrekt maskiert
        let mut c = b.clone();
        c.dtc[0].description = "Text mit \"Anführungszeichen\" und \\ Backslash".into();
        let d = LabelFile::parse(&c.to_toml()).unwrap();
        assert_eq!(d.dtc[0].description, c.dtc[0].description);
    }

    #[test]
    fn skelett_aus_scan() {
        // Aus eigenen Scan-Daten vorbefüllt; nichts erfunden.
        let lf = skelett(0x01, " Motor ", Some(" 03L906022 "), Some(0x7E0), Some(0x7E8));
        assert_eq!(lf.ecu.address, Some(0x01));
        assert_eq!(lf.ecu.name, "Motor");
        assert_eq!(lf.ecu.part.as_deref(), Some("03L906022"));
        assert_eq!((lf.ecu.tx, lf.ecu.rx), (Some(0x7E0), Some(0x7E8)));
        assert!(lf.dtc.is_empty() && lf.measuring.is_empty());
        // leere Teilenummer fällt weg, als TOML wieder einlesbar
        let leer = skelett(0x09, "Bordnetz", Some("  "), None, None);
        assert_eq!(leer.ecu.part, None);
        let wieder = LabelFile::parse(&leer.to_toml()).unwrap();
        assert_eq!(wieder.ecu.address, Some(0x09));

        // Dateinamen
        assert_eq!(dateiname(0x01, "Motor Elektronik"), "01-motor-elektronik");
        assert_eq!(dateiname(0x09, "Zentralelektrik (BCM)"), "09-zentralelektrik-bcm");
        assert_eq!(dateiname(0x17, ""), "17");
    }
}
