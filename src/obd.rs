//! OBD-II / EOBD (SAE J1979 / ISO 15031-5) über ISO-TP.
//!
//! Herstellerunabhängig: jedes Fahrzeug mit OBD-Pflicht (Benziner EU ab 2001,
//! Diesel ab 2004) antwortet über das Motorsteuergerät (0x7E0/0x7E8).
//! Dienste: 01 Live-Daten + Readiness, 03/07/0A Fehlercodes, 04 Löschen,
//! 09 Fahrzeuginfo (VIN).

use crate::tr;
use crate::transport::Transport;
use crate::uds::Uds;
use anyhow::{bail, Result};

pub const DEFAULT_TX: u16 = 0x7E0;
pub const DEFAULT_RX: u16 = 0x7E8;

/// Fehlercode-Dienste.
pub const DTC_STORED: u8 = 0x03;
pub const DTC_PENDING: u8 = 0x07;
pub const DTC_PERMANENT: u8 = 0x0A;

pub struct Obd<'a> {
    uds: Uds<'a>,
}

impl<'a> Obd<'a> {
    pub fn new(tp: &'a dyn Transport) -> Self {
        Self { uds: Uds::new(tp) }
    }

    /// Mode 01: Rohdaten eines PIDs (ohne PID-Echo).
    pub fn read_pid(&self, pid: u8) -> Result<Vec<u8>> {
        let r = self.uds.request(0x01, &[pid])?;
        if r.first() != Some(&pid) {
            bail!(tr!("Antwort gehört nicht zu PID {pid}", pid = format!("{:#04x}", pid)));
        }
        Ok(r[1..].to_vec())
    }

    /// Mode 01: PID lesen und umrechnen.
    pub fn read_value(&self, pid: u8) -> Result<f64> {
        let d = self.read_pid(pid)?;
        decode_pid(pid, &d).ok_or_else(|| anyhow::anyhow!(tr!("PID {pid} nicht dekodierbar", pid = format!("{:#04x}", pid))))
    }

    /// Alle unterstützten PIDs (Bitmasken 0x00, 0x20, 0x40, … abfragen).
    pub fn supported_pids(&self) -> Result<Vec<u8>> {
        let mut all = Vec::new();
        let mut base = 0x00u8;
        loop {
            let mask = self.read_pid(base)?;
            let pids = parse_supported(base, &mask);
            let more = pids.contains(&(base.wrapping_add(0x20)));
            all.extend(pids.into_iter().filter(|p| p % 0x20 != 0));
            if !more || base >= 0xC0 {
                break;
            }
            base += 0x20;
        }
        Ok(all)
    }

    /// Mode 01 PID 01: MIL, Anzahl Fehler, Readiness.
    pub fn readiness(&self) -> Result<Readiness> {
        let d = self.read_pid(0x01)?;
        if d.len() < 4 {
            bail!(tr!("PID 01: 4 Byte erwartet, {a} erhalten", a = d.len()));
        }
        Ok(parse_readiness([d[0], d[1], d[2], d[3]]))
    }

    /// Mode 03/07/0A: Fehlercodes.
    pub fn read_dtcs(&self, service: u8) -> Result<Vec<String>> {
        let r = self.uds.request(service, &[])?;
        Ok(parse_dtcs(&r))
    }

    /// Mode 04: Fehlercodes und Readiness löschen.
    pub fn clear(&self) -> Result<()> {
        self.uds.request(0x04, &[]).map(|_| ())
    }

    /// Mode 02: Umgebungsbedingungen (Freeze-Frame) des Fehlers, der ihn
    /// ausgelöst hat. None = kein Freeze-Frame gespeichert.
    pub fn freeze_frame(&self) -> Result<Option<FreezeFrame>> {
        let r = self.uds.request(0x02, &[0x02, 0x00])?;
        // [PID 02][Frame 00][A][B]
        let (a, b) = (r.get(2).copied().unwrap_or(0), r.get(3).copied().unwrap_or(0));
        if a == 0 && b == 0 {
            return Ok(None);
        }
        let mut values = Vec::new();
        for pid in [0x04, 0x05, 0x06, 0x07, 0x0B, 0x0C, 0x0D, 0x0F, 0x10, 0x11, 0x42] {
            let Ok(d) = self.uds.request(0x02, &[pid, 0x00]) else { continue };
            if d.first() != Some(&pid) {
                continue;
            }
            if let (Some((name, unit)), Some(v)) = (pid_info(pid), decode_pid(pid, d.get(2..).unwrap_or(&[]))) {
                values.push((name.to_string(), format!("{v:.1} {unit}").trim().to_string()));
            }
        }
        Ok(Some(FreezeFrame { code: dtc_code(a, b), values }))
    }

    /// Kilometerstand (PID A6, neuere Fahrzeuge ab ~2019).
    pub fn odometer(&self) -> Result<u32> {
        let d = self.read_pid(0xA6)?;
        if d.len() < 4 {
            bail!(tr!("PID A6: 4 Byte erwartet"));
        }
        Ok(u32::from_be_bytes([d[0], d[1], d[2], d[3]]) / 10)
    }

    /// Mode 06: alle unterstützten Prüfergebnisse (On-Board-Monitoring).
    pub fn test_results(&self) -> Result<Vec<TestResult>> {
        let mut mids = Vec::new();
        let mut base = 0x00u8;
        loop {
            let r = self.uds.request(0x06, &[base])?;
            // [MID][4 Byte Bitmaske]
            let mask = r.get(1..5).unwrap_or(&[]);
            let list = parse_supported(base, mask);
            let more = list.contains(&base.wrapping_add(0x20));
            mids.extend(list.into_iter().filter(|m| m % 0x20 != 0));
            if !more || base >= 0xE0 {
                break;
            }
            base += 0x20;
        }
        let mut out = Vec::new();
        for mid in mids {
            let Ok(r) = self.uds.request(0x06, &[mid]) else { continue };
            out.extend(parse_test_records(&r));
        }
        Ok(out)
    }

    /// Mode 09 PID 02: Fahrgestellnummer.
    pub fn vin(&self) -> Result<String> {
        let r = self.uds.request(0x09, &[0x02])?;
        // [02][Anzahl Datensätze][17 Zeichen]
        let text: String = r.iter().skip(2).filter(|b| b.is_ascii_alphanumeric()).map(|&b| b as char).collect();
        if text.is_empty() {
            bail!(tr!("leere VIN-Antwort"));
        }
        Ok(text)
    }
}

/// Prüfergebnis als Rohwerte zum Speichern im Scan (zum späteren Nachprüfen).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Pruefwert {
    pub mid: u8,
    pub tid: u8,
    pub uasid: u8,
    pub wert: f64,
    pub min: f64,
    pub max: f64,
    #[serde(default)]
    pub einheit: String,
}

impl From<&TestResult> for Pruefwert {
    fn from(t: &TestResult) -> Self {
        Self { mid: t.mid, tid: t.tid, uasid: t.uasid, wert: t.value, min: t.min, max: t.max, einheit: t.unit.to_string() }
    }
}

/// Ein Prüfergebnis aus Mode 06.
#[derive(Debug, Clone, PartialEq)]
pub struct TestResult {
    pub mid: u8,
    pub tid: u8,
    pub uasid: u8,
    pub value: f64,
    pub min: f64,
    pub max: f64,
    pub unit: &'static str,
}

impl TestResult {
    pub fn name(&self) -> String {
        let base = mid_name(self.mid);
        match (self.mid, self.tid) {
            (0xA1..=0xAD, 0x0B) => tr!("{base} (Mittel letzte 10 Fahrten)", base = base),
            (0xA1..=0xAD, 0x0C) => tr!("{base} (letzte Fahrt)", base = base),
            _ if self.tid >= 0x80 => tr!("{base} · Test {a} (Herstellertest)", base = base, a = format!("{:#04x}", self.tid)),
            _ => tr!("{base} · Test {a}", base = base, a = format!("{:#04x}", self.tid)),
        }
    }

    /// Hat der Test echte Grenzen? (Zähler haben oft min = max = 0)
    pub fn has_limits(&self) -> bool {
        self.max - self.min > 1e-9
    }

    /// Bestanden? (min ≤ Wert ≤ max; ohne Grenzen immer)
    pub fn passed(&self) -> bool {
        !self.has_limits() || (self.value >= self.min - 1e-9 && self.value <= self.max + 1e-9)
    }

    /// Welche Grenzen gelten wirklich? Steht min auf dem kleinsten bzw. max auf
    /// dem größten darstellbaren Wert (0 / 0xFFFF), ist der Test einseitig
    /// (SAE J1979) — z. B. Aussetzer-Zähler 0…65535, Kat-Test nur „mindestens“.
    pub fn grenzen_echt(&self) -> (bool, bool) {
        let (f, off, _, signed) = uas_scaling(self.uasid);
        let (lo, hi) = if signed { (f64::from(i16::MIN), f64::from(i16::MAX)) } else { (0.0, 65535.0) };
        let eps = f.abs() / 2.0;
        ((self.min - (lo * f + off)).abs() > eps, (self.max - (hi * f + off)).abs() > eps)
    }

    /// Abstand zur Grenze in % (100 = weit weg, 0 = an der Grenze).
    /// Zweiseitig: bezogen auf den halben Bereich; einseitig: auf die Grenze selbst.
    /// None, wenn keine Grenze gilt (z. B. min = max = 0 bei Zählern).
    pub fn reserve(&self) -> Option<f64> {
        let span = self.max - self.min;
        if span <= 1e-9 {
            return None;
        }
        let f = uas_scaling(self.uasid).0.abs();
        let r = match self.grenzen_echt() {
            (true, true) => (self.value - self.min).min(self.max - self.value) / (span / 2.0),
            (true, false) => (self.value - self.min) / self.min.abs().max(f),
            (false, true) => (self.max - self.value) / self.max.abs().max(f),
            (false, false) => return None,
        };
        Some((r * 100.0).clamp(-100.0, 100.0))
    }

    /// Herstellereigener Test (TID ab 0x80): was er misst, ist nicht veröffentlicht.
    /// Bei Zählern oder Verhältnissen liegt der Wert oft ganz normal nah an einer
    /// Grenze — darum zählt hier nur „bestanden / nicht bestanden“.
    pub fn herstellertest(&self) -> bool {
        self.tid >= 0x80
    }

    pub fn stufe(&self) -> crate::limits::Stufe {
        use crate::limits::Stufe;
        if !self.passed() {
            Stufe::Warnung
        } else if !self.herstellertest() && self.reserve().is_some_and(|r| r < 25.0) {
            Stufe::Hinweis
        } else {
            Stufe::Gut
        }
    }
}

/// Auffällige Prüfergebnisse zusammenfassen: nicht bestanden / knapp / ein
/// Zylinder mit deutlich mehr Aussetzern als die anderen.
pub fn test_summary(results: &[TestResult]) -> Vec<(crate::limits::Stufe, String)> {
    use crate::limits::Stufe;
    let mut out = Vec::new();
    for t in results.iter().filter(|t| t.has_limits()) {
        match t.stufe() {
            Stufe::Warnung => out.push((Stufe::Warnung, tr!("{a}: nicht bestanden ({b} {c}, Grenze {d}–{f})", a = t.name(), b = format!("{:.1}", t.value), c = t.unit, d = format!("{:.1}", t.min), f = format!("{:.1}", t.max)))),
            Stufe::Hinweis => out.push((Stufe::Hinweis, tr!("{a}: knapp an der Grenze (Reserve {b} %)", a = t.name(), b = format!("{:.0}", t.reserve().unwrap_or(0.0))))),
            Stufe::Gut => {}
        }
    }
    // Aussetzer je Zylinder vergleichen (gleicher Test-Typ)
    let cyl: Vec<&TestResult> = results.iter().filter(|t| (0xA2..=0xAD).contains(&t.mid) && t.tid == 0x0B).collect();
    if cyl.len() >= 2 {
        let mut vals: Vec<f64> = cyl.iter().map(|t| t.value).collect();
        vals.sort_by(f64::total_cmp);
        let median = vals[vals.len() / 2].max(1.0);
        for t in &cyl {
            if t.value >= 5.0 && t.value >= 3.0 * median {
                out.push((Stufe::Warnung, tr!("Zylinder {a} hat deutlich mehr Aussetzer ({b}) als die anderen (typisch {median}) — Zündkerze/Zündspule/Einspritzventil prüfen", a = t.mid - 0xA1, b = format!("{:.0}", t.value), median = format!("{:.0}", median))));
            }
        }
    }
    out
}

/// Antwort auf `06 MID` → Prüfergebnisse (je 9 Byte: MID TID UASID Wert Min Max).
pub fn parse_test_records(r: &[u8]) -> Vec<TestResult> {
    r.as_chunks::<9>().0.iter()
        .map(|c| {
            let w = |i: usize| u16::from_be_bytes([c[i], c[i + 1]]);
            let (f, off, unit, signed) = uas_scaling(c[2]);
            let conv = |raw: u16| {
                let v = if signed { f64::from(raw as i16) } else { f64::from(raw) };
                v * f + off
            };
            TestResult { mid: c[0], tid: c[1], uasid: c[2], value: conv(w(3)), min: conv(w(5)), max: conv(w(7)), unit }
        })
        .collect()
}

/// Einheit und Skalierung (SAE J1979, Tabelle „Unit and Scaling ID“) — häufige
/// Einträge; unbekannte werden roh angezeigt. TODO(verify) seltene IDs.
pub fn uas_scaling(id: u8) -> (f64, f64, &'static str, bool) {
    match id {
        0x01 => (1.0, 0.0, "", false),
        0x02 => (0.1, 0.0, "", false),
        0x03 => (0.01, 0.0, "", false),
        0x04 => (0.001, 0.0, "", false),
        0x05 => (0.000_030_5, 0.0, "", false),
        0x06 => (0.000_305, 0.0, "", false),
        0x07 => (0.25, 0.0, "1/min", false),
        0x08 => (0.01, 0.0, "km/h", false),
        0x09 => (1.0, 0.0, "km/h", false),
        0x0A => (0.122, 0.0, "mV", false),
        0x0B => (0.001, 0.0, "V", false),
        0x0C => (0.01, 0.0, "V", false),
        0x0D => (0.003_906_25, 0.0, "mA", false),
        0x0E => (0.001, 0.0, "A", false),
        0x0F => (0.01, 0.0, "A", false),
        0x10 => (1.0, 0.0, "ms", false),
        0x11 => (100.0, 0.0, "ms", false),
        0x12 => (1.0, 0.0, "s", false),
        0x13 => (1.0, 0.0, "mΩ", false),
        0x14 => (1.0, 0.0, "Ω", false),
        0x15 => (1.0, 0.0, "kΩ", false),
        0x16 => (0.1, -40.0, "°C", false),
        0x1E => (0.000_030_5, 0.0, "λ", false),
        0x24 => (1.0, 0.0, "Zähler", false),
        0x25 => (1.0, 0.0, "km", false),
        0x27 => (0.01, 0.0, "g/s", false),
        0x2F => (0.01, 0.0, "%", false),
        0x81 => (1.0, 0.0, "", true),
        0x82 => (0.1, 0.0, "", true),
        0x83 => (0.01, 0.0, "", true),
        0x84 => (0.001, 0.0, "", true),
        0x85 => (0.000_030_5, 0.0, "", true),
        0x86 => (0.000_305, 0.0, "", true),
        0x8A => (0.122, 0.0, "mV", true),
        0x8B => (0.001, 0.0, "V", true),
        0x8C => (0.01, 0.0, "V", true),
        0x8E => (0.001, 0.0, "A", true),
        0x90 => (1.0, 0.0, "ms", true),
        0x96 => (0.1, 0.0, "°C", true),
        _ => (1.0, 0.0, "(roh)", false),
    }
}

/// Bezeichnung einer Überwachungsfunktion (OBD Monitor ID).
pub fn mid_name(mid: u8) -> String {
    let s = match mid {
        0x01 => "Lambdasonde B1 S1",
        0x02 => "Lambdasonde B1 S2",
        0x03 => "Lambdasonde B1 S3",
        0x05 => "Lambdasonde B2 S1",
        0x06 => "Lambdasonde B2 S2",
        0x21 => "Katalysator Bank 1",
        0x22 => "Katalysator Bank 2",
        0x31 => "Abgasrückführung Bank 1",
        0x35 => "Nockenwellenverstellung Bank 1",
        0x39 => "Tankentlüftung Leck 0,150\"",
        0x3A => "Tankentlüftung Leck 0,090\"",
        0x3B => "Tankentlüftung Leck 0,040\"",
        0x3C => "Tankentlüftung Leck 0,020\"",
        0x3D => "Tankentlüftung Spülstrom",
        0x41 => "Lambdasonden-Heizung B1 S1",
        0x42 => "Lambdasonden-Heizung B1 S2",
        0x45 => "Lambdasonden-Heizung B2 S1",
        0x61 => "Kat-Heizung Bank 1",
        0x71 => "Sekundärluft",
        0x81 => "Kraftstoffsystem Bank 1",
        0x82 => "Kraftstoffsystem Bank 2",
        0x85 => "Ladedruck Bank 1",
        0x98 => "NOx/SCR-Katalysator Bank 1",
        0xA1 => "Verbrennungsaussetzer gesamt",
        0xB0 => "Partikelfilter Bank 1",
        0xA2..=0xAD => return tr!("Verbrennungsaussetzer Zylinder {a}", a = mid - 0xA1),
        _ => return tr!("Überwachung {mid}", mid = format!("{:#04x}", mid)),
    };
    s.to_string()
}

/// Freeze-Frame: auslösender Fehler + Werte zu dem Zeitpunkt.
#[derive(Debug, Clone, PartialEq)]
pub struct FreezeFrame {
    pub code: String,
    pub values: Vec<(String, String)>,
}

/// Bitmaske (4 Byte) → Liste der PIDs `base+1 ..= base+32`.
pub fn parse_supported(base: u8, mask: &[u8]) -> Vec<u8> {
    let mut v = Vec::new();
    for (i, byte) in mask.iter().take(4).enumerate() {
        for bit in 0..8 {
            if byte & (0x80 >> bit) != 0 {
                v.push(base.wrapping_add((i * 8 + bit + 1) as u8));
            }
        }
    }
    v
}

/// Mode-03-Antwort (CAN): [Anzahl][hi lo]* → "P0101", …
pub fn parse_dtcs(r: &[u8]) -> Vec<String> {
    let Some((_, rest)) = r.split_first() else { return vec![] };
    rest.as_chunks::<2>().0.iter()
        .filter(|c| c[0] != 0 || c[1] != 0)
        .map(|c| dtc_code(c[0], c[1]))
        .collect()
}

/// Zwei Byte → genormter Fehlercode (SAE J2012).
pub fn dtc_code(hi: u8, lo: u8) -> String {
    let letter = ['P', 'C', 'B', 'U'][(hi >> 6) as usize];
    format!("{letter}{}{:X}{lo:02X}", (hi >> 4) & 0x03, hi & 0x0F)
}

/// Name und Einheit eines PIDs (eigene Kurzbezeichnungen). Der Name ist deutsch (so wird er
/// gespeichert); zum Anzeigen `crate::i18n::t(name)`.
pub fn pid_info(pid: u8) -> Option<(&'static str, &'static str)> {
    Some(match pid {
        0x04 => (crate::trn!("Motorlast"), "%"),
        0x05 => (crate::trn!("Kühlmitteltemperatur"), "°C"),
        0x06 => (crate::trn!("Kurzzeit-Gemischkorrektur B1"), "%"),
        0x07 => (crate::trn!("Langzeit-Gemischkorrektur B1"), "%"),
        0x0A => (crate::trn!("Kraftstoffdruck"), "kPa"),
        0x0B => (crate::trn!("Saugrohrdruck"), "kPa"),
        0x0C => (crate::trn!("Drehzahl"), "1/min"),
        0x0D => (crate::trn!("Geschwindigkeit"), "km/h"),
        0x0E => (crate::trn!("Zündzeitpunkt"), "°"),
        0x0F => (crate::trn!("Ansauglufttemperatur"), "°C"),
        0x10 => (crate::trn!("Luftmasse"), "g/s"),
        0x11 => (crate::trn!("Drosselklappe"), "%"),
        0x1F => (crate::trn!("Laufzeit seit Start"), "s"),
        0x21 => (crate::trn!("Strecke mit MIL an"), "km"),
        0x23 => (crate::trn!("Kraftstoff-Raildruck"), "kPa"),
        0x2F => (crate::trn!("Tankfüllstand"), "%"),
        0x30 => (crate::trn!("Warmlaufzyklen seit Löschen"), ""),
        0x31 => (crate::trn!("Strecke seit Löschen"), "km"),
        0x33 => (crate::trn!("Luftdruck"), "kPa"),
        0x42 => (crate::trn!("Steuergerätespannung"), "V"),
        0x45 => (crate::trn!("Rel. Drosselklappe"), "%"),
        0x46 => (crate::trn!("Umgebungstemperatur"), "°C"),
        0x49 => (crate::trn!("Fahrpedal D"), "%"),
        0x4D => (crate::trn!("Zeit mit MIL an"), "min"),
        0x4E => (crate::trn!("Zeit seit Löschen"), "min"),
        0x5C => (crate::trn!("Öltemperatur"), "°C"),
        0x5E => (crate::trn!("Kraftstoffverbrauch"), "l/h"),
        0xA6 => (crate::trn!("Kilometerstand"), "km"),
        _ => return None,
    })
}

/// Rohdaten eines PIDs nach SAE J1979 umrechnen.
pub fn decode_pid(pid: u8, d: &[u8]) -> Option<f64> {
    let a = f64::from(*d.first()?);
    let b = d.get(1).map(|&x| f64::from(x));
    let ab = || b.map(|b| a * 256.0 + b);
    Some(match pid {
        0x04 | 0x11 | 0x2F | 0x45 | 0x49 => a * 100.0 / 255.0,
        0x05 | 0x0F | 0x46 | 0x5C => a - 40.0,
        0x06 | 0x07 => (a - 128.0) * 100.0 / 128.0,
        0x0A => a * 3.0,
        0x0B | 0x0D | 0x33 | 0x30 => a,
        0x0C => ab()? / 4.0,
        0x0E => a / 2.0 - 64.0,
        0x10 => ab()? / 100.0,
        0x1F | 0x21 | 0x31 | 0x4D | 0x4E => ab()?,
        0x23 => ab()? * 10.0,
        0x42 => ab()? / 1000.0,
        0x5E => ab()? / 20.0,
        0xA6 if d.len() >= 4 => f64::from(u32::from_be_bytes([d[0], d[1], d[2], d[3]])) / 10.0,
        _ => return None,
    })
}

/// Ein Readiness-Test.
#[derive(Debug, Clone, PartialEq)]
pub struct Monitor {
    pub name: &'static str,
    pub available: bool,
    pub complete: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Readiness {
    pub mil: bool,
    pub dtc_count: u8,
    /// true = Diesel (Selbstzünder), false = Benziner
    pub compression: bool,
    pub monitors: Vec<Monitor>,
}

impl Readiness {
    /// Alle verfügbaren Tests abgeschlossen?
    pub fn all_complete(&self) -> bool {
        self.monitors.iter().filter(|m| m.available).all(|m| m.complete)
    }
}

/// PID 01 (Bytes A–D) nach SAE J1979 auswerten.
pub fn parse_readiness(d: [u8; 4]) -> Readiness {
    let [a, b, c, dd] = d;
    let compression = b & 0x08 != 0;
    let mut monitors = vec![
        // Kontinuierliche Tests: verfügbar = Bit 0..2, unvollständig = Bit 4..6
        Monitor { name: "Verbrennungsaussetzer", available: b & 0x01 != 0, complete: b & 0x10 == 0 },
        Monitor { name: "Kraftstoffsystem", available: b & 0x02 != 0, complete: b & 0x20 == 0 },
        Monitor { name: "Bauteile", available: b & 0x04 != 0, complete: b & 0x40 == 0 },
    ];
    let names: [&str; 8] = if compression {
        ["NMHC-Katalysator", "NOx/SCR", "", "Ladedruck", "", "Abgassensor", "Partikelfilter", "AGR/VVT"]
    } else {
        [
            "Katalysator", "Kat-Heizung", "Tankentlüftung (EVAP)", "Sekundärluft", "Klimaanlage",
            "Lambdasonde", "Lambdasonden-Heizung", "AGR/VVT",
        ]
    };
    for (bit, name) in names.iter().enumerate() {
        if name.is_empty() {
            continue;
        }
        monitors.push(Monitor {
            name,
            available: c & (1 << bit) != 0,
            complete: dd & (1 << bit) == 0,
        });
    }
    Readiness { mil: a & 0x80 != 0, dtc_count: a & 0x7F, compression, monitors }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::DemoUds;

    #[test]
    fn decodes_standard_examples() {
        assert_eq!(decode_pid(0x0C, &[0x1A, 0xF8]), Some(1726.0)); // (26*256+248)/4
        assert_eq!(decode_pid(0x05, &[0x7B]), Some(83.0));
        assert_eq!(dtc_code(0x01, 0x71), "P0171");
        assert_eq!(dtc_code(0xC1, 0x23), "U0123");
        assert_eq!(parse_supported(0x00, &[0x80, 0x00, 0x00, 0x01]), vec![0x01, 0x20]);
    }

    #[test]
    fn mode06_records() {
        // Kat Bank 1: Wert 0x0190 (400) bei Grenzen 0..450, Skalierung 0x01
        let r = parse_test_records(&[0x21, 0x01, 0x01, 0x01, 0x90, 0x00, 0x00, 0x01, 0xC2]);
        assert_eq!(r.len(), 1);
        assert!(r[0].passed());
        assert_eq!(r[0].stufe(), crate::limits::Stufe::Hinweis); // nur 22 % Reserve
        // derselbe Wert als Herstellertest (TID ab 0x80): nur bestanden/nicht bestanden zählt
        let h = TestResult { tid: 0x81, ..r[0].clone() };
        assert!(h.herstellertest());
        assert_eq!(h.stufe(), crate::limits::Stufe::Gut);
        assert!(h.name().contains("Herstellertest"));
        assert_eq!(TestResult { value: 500.0, ..h }.stufe(), crate::limits::Stufe::Warnung);
        // Rohwerte zum Speichern
        let p = Pruefwert::from(&r[0]);
        assert_eq!((p.mid, p.tid, p.wert, p.max), (0x21, 0x01, 400.0, 450.0));
        let misfire = TestResult { mid: 0xA4, tid: 0x0B, uasid: 0x24, value: 12.0, min: 0.0, max: 0.0, unit: "Zähler" };
        assert_eq!(misfire.name(), "Verbrennungsaussetzer Zylinder 3 (Mittel letzte 10 Fahrten)");
        assert!(misfire.passed()); // Zähler ohne Grenzen
        // Golf 6 (29.10.2026): Aussetzer 2 bei 0…65535 = keine echte Grenze → gut
        let a = TestResult { mid: 0xA5, tid: 0x0B, uasid: 0x24, value: 2.0, min: 0.0, max: 65535.0, unit: "Zähler" };
        assert_eq!(a.reserve(), None);
        assert_eq!(a.stufe(), crate::limits::Stufe::Gut);
        // Kat nur „mindestens 4096“: 10591 ist weit darüber → gut
        let k = TestResult { mid: 0x21, tid: 0x84, uasid: 0x01, value: 10591.0, min: 4096.0, max: 65535.0, unit: "" };
        assert_eq!(k.stufe(), crate::limits::Stufe::Gut);
        let tp = DemoUds::new(DEFAULT_TX);
        let all = Obd::new(&tp).test_results().unwrap();
        assert!(all.iter().any(|t| t.mid == 0x21));
        assert!(all.iter().any(|t| t.mid == 0xA4 && t.value > 0.0));
        let sum = test_summary(&all);
        assert!(sum.iter().any(|(_, t)| t.contains("Zylinder 3")), "{sum:?}");
        assert!(sum.iter().any(|(_, t)| t.contains("Katalysator")), "{sum:?}");
    }

    #[test]
    fn readiness_bits() {
        let r = parse_readiness([0x82, 0x07, 0x65, 0x04]);
        assert!(r.mil);
        assert_eq!(r.dtc_count, 2);
        assert!(!r.compression);
        let evap = r.monitors.iter().find(|m| m.name.starts_with("Tankentl")).unwrap();
        assert!(evap.available && !evap.complete);
        assert!(!r.all_complete());
    }

    #[test]
    fn talks_to_demo_vehicle() {
        let tp = DemoUds::new(DEFAULT_TX);
        let obd = Obd::new(&tp);
        let pids = obd.supported_pids().unwrap();
        assert!(pids.contains(&0x0C) && pids.contains(&0x42));
        assert!(obd.read_value(0x0C).unwrap() > 500.0);
        assert_eq!(obd.vin().unwrap(), crate::sim::VIN);
        assert!(obd.readiness().is_ok());
    }
}
