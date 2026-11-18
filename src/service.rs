//! Service-Intervallanzeige (Inspektion / Ölwechsel) im Kombiinstrument:
//! lesen, Kennungen finden und — wenn eingerichtet — zurückstellen.
//!
//! VW veröffentlicht die Kennungen der Service-Werte nicht, und sie hängen vom
//! Kombi ab. Deshalb stehen sie je Fahrzeug in `fahrzeug.toml` unter
//! `[service]`. Sie lassen sich auf zwei Wegen ermitteln:
//!
//! 1. **Vorher/Nachher (empfohlen):** alle Kennungen des Kombis merken, den
//!    Service mit den Tasten am Kombi zurückstellen (Bordbuch), noch einmal
//!    merken und vergleichen. Was sich geändert hat, sind die Service-Werte, und
//!    der neue Inhalt ist genau das, was eine Rückstellung schreibt.
//! 2. **Wertesuche:** die Werte eingeben, die das Kombi gerade anzeigt
//!    (z. B. „Inspektion in 4800 km / 212 Tagen“); passende Kennungen werden
//!    als Kandidaten gezeigt.
//!
//! Die Rückstellung schreibt nur Kennungen, für die ein Rückstellwert
//! eingetragen ist, sichert vorher die alten Inhalte und prüft danach nach.
//! Verlangt das Kombi eine Anmeldung (Security Access), bricht sie ab — es wird
//! nichts umgangen.

use crate::tr;
use crate::limits::Stufe;
use crate::transport::Backend;
use crate::uds::Uds;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Was ein Service-Wert bedeutet.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Art {
    KmBisInspektion,
    TageBisInspektion,
    KmBisOelwechsel,
    TageBisOelwechsel,
    KmSeitService,
    TageSeitService,
    /// Sonstiger Wert, der bei der Rückstellung mitgeschrieben wird
    Sonstiges,
}

impl Art {
    pub const ALLE: [Art; 7] =
        [Art::KmBisInspektion, Art::TageBisInspektion, Art::KmBisOelwechsel, Art::TageBisOelwechsel, Art::KmSeitService, Art::TageSeitService, Art::Sonstiges];

    pub fn text(self) -> &'static str {
        match self {
            Art::KmBisInspektion => crate::trs!("Inspektion fällig in"),
            Art::TageBisInspektion => crate::trs!("Inspektion fällig in"),
            Art::KmBisOelwechsel => crate::trs!("Ölwechsel fällig in"),
            Art::TageBisOelwechsel => crate::trs!("Ölwechsel fällig in"),
            Art::KmSeitService => crate::trs!("Seit dem letzten Service"),
            Art::TageSeitService => crate::trs!("Seit dem letzten Service"),
            Art::Sonstiges => crate::trs!("Weiterer Service-Wert"),
        }
    }

    pub fn einheit(self) -> &'static str {
        match self {
            Art::KmBisInspektion | Art::KmBisOelwechsel | Art::KmSeitService => "km",
            Art::TageBisInspektion | Art::TageBisOelwechsel | Art::TageSeitService => crate::trs!("Tage"),
            Art::Sonstiges => "",
        }
    }

    fn ist_km(self) -> bool {
        matches!(self, Art::KmBisInspektion | Art::KmBisOelwechsel | Art::KmSeitService)
    }

    fn ist_restwert(self) -> bool {
        matches!(self, Art::KmBisInspektion | Art::TageBisInspektion | Art::KmBisOelwechsel | Art::TageBisOelwechsel)
    }
}

/// Ein Service-Wert: Kennung (DID) im Kombi, Ausschnitt und Umrechnung.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Wert {
    pub art: Art,
    pub did: u16,
    /// Erstes Byte des Werts in der Antwort
    #[serde(default)]
    pub offset: usize,
    /// Länge in Byte (0 = bis zum Ende, höchstens 4)
    #[serde(default)]
    pub laenge: usize,
    #[serde(default = "eins")]
    pub faktor: f64,
    /// Zweierkomplement (negativ = überfällig)
    #[serde(default)]
    pub vorzeichen: bool,
    /// Gesamter Inhalt der Kennung nach einer Rückstellung (Hex, z. B. "3A 98").
    /// Leer = wird bei der Rückstellung nicht geschrieben.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reset: String,
    /// Neuer Wert für dieses Feld bei der Rückstellung (z. B. 15000 km).
    /// Die übrigen Bytes der Kennung bleiben wie gerade gelesen.
    /// Wird nach `reset` angewendet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub neu: Option<f64>,
}

fn eins() -> f64 {
    1.0
}

/// Service-Einstellungen eines Fahrzeugs (`[service]` in fahrzeug.toml).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Config {
    #[serde(default = "kombi_tx")]
    pub tx: u16,
    #[serde(default = "kombi_rx")]
    pub rx: u16,
    /// Sitzung zum Schreiben (0x03 = erweitert)
    #[serde(default = "erweitert")]
    pub sitzung: u8,
    #[serde(default)]
    pub werte: Vec<Wert>,
}

fn kombi_tx() -> u16 {
    0x714
}
fn kombi_rx() -> u16 {
    0x77E
}
fn erweitert() -> u8 {
    0x03
}

impl Default for Config {
    fn default() -> Self {
        Self { tx: kombi_tx(), rx: kombi_rx(), sitzung: erweitert(), werte: vec![] }
    }
}

impl Config {
    /// Lässt sich der Service zurückstellen (mindestens ein Rückstellwert)?
    pub fn kann_zuruecksetzen(&self) -> bool {
        self.werte.iter().any(Wert::schreibt)
    }

    /// Was bei der Rückstellung geschrieben wird, als lesbare Zeilen.
    pub fn vorschau(&self) -> Vec<String> {
        self.werte
            .iter()
            .filter(|w| w.schreibt())
            .map(|w| match w.neu {
                Some(n) => tr!("{a}  {b} → {c} {d}", a = format!("{:#06X}", w.did), b = w.art.text(), c = format!("{n:.0}"), d = w.art.einheit()),
                None => tr!("{a}  {b} → {c}", a = format!("{:#06X}", w.did), b = w.art.text(), c = w.reset.trim()),
            })
            .collect()
    }
}

/// Konfiguration des Demo-Fahrzeugs (Kennungen frei gewählt, nur Simulation).
pub fn demo_config() -> Config {
    let w = |art, did, reset: &str| Wert { art, did, offset: 0, laenge: 0, faktor: 1.0, vorzeichen: true, reset: reset.into(), neu: None };
    Config {
        tx: 0x7E0,
        rx: 0x7E8,
        sitzung: 0x03,
        werte: vec![
            w(Art::KmBisInspektion, crate::sim::DEMO_SERVICE_DID, "75 30"), // 30000 km
            w(Art::TageBisInspektion, crate::sim::DEMO_SERVICE_DID + 1, "02 DA"), // 730 Tage
            w(Art::KmBisOelwechsel, crate::sim::DEMO_SERVICE_DID + 2, "3A 98"), // 15000 km
            w(Art::TageBisOelwechsel, crate::sim::DEMO_SERVICE_DID + 3, "01 6D"), // 365 Tage
        ],
    }
}

/// Zahl aus einem Ausschnitt (Big Endian, optional mit Vorzeichen).
pub fn zahl(d: &[u8], offset: usize, laenge: usize, vorzeichen: bool) -> Option<i64> {
    let rest = d.get(offset..)?;
    let n = if laenge == 0 { rest.len() } else { laenge };
    if n == 0 || n > 4 || rest.len() < n {
        return None;
    }
    let mut v: u64 = 0;
    for &b in &rest[..n] {
        v = (v << 8) | u64::from(b);
    }
    let bits = 8 * n as u32;
    Some(if vorzeichen && v >> (bits - 1) == 1 { v as i64 - (1i64 << bits) } else { v as i64 })
}

impl Wert {
    pub fn aus(&self, d: &[u8]) -> Option<f64> {
        zahl(d, self.offset, self.laenge, self.vorzeichen).map(|v| v as f64 * self.faktor)
    }

    /// Wird dieser Wert bei der Rückstellung geschrieben?
    pub fn schreibt(&self) -> bool {
        !self.reset.trim().is_empty() || self.neu.is_some()
    }

    /// `wert` in die Bytes `d` eintragen (gleiches Format wie beim Lesen).
    /// None, wenn der Wert nicht in das Feld passt.
    pub fn eintragen(&self, d: &[u8], wert: f64) -> Option<Vec<u8>> {
        let n = if self.laenge == 0 { d.len().checked_sub(self.offset)? } else { self.laenge };
        if n == 0 || n > 4 || d.len() < self.offset + n || self.faktor == 0.0 || !wert.is_finite() {
            return None;
        }
        let roh = (wert / self.faktor).round() as i64;
        let bits = 8 * n as u32;
        let (min, max) = if self.vorzeichen { (-(1i64 << (bits - 1)), (1i64 << (bits - 1)) - 1) } else { (0, (1i64 << bits) - 1) };
        if roh < min || roh > max {
            return None;
        }
        let u = (roh as u64) & ((1u64 << bits) - 1);
        let mut out = d.to_vec();
        for i in 0..n {
            out[self.offset + i] = (u >> (8 * (n - 1 - i))) as u8;
        }
        Some(out)
    }
}

/// Typische Intervalle als Vorschlag (bitte mit dem Serviceheft vergleichen):
/// Inspektion 30 000 km / 2 Jahre, Ölwechsel 15 000 km / 1 Jahr.
pub const INTERVALL_VORSCHLAG: [(Art, f64); 4] =
    [(Art::KmBisInspektion, 30_000.0), (Art::TageBisInspektion, 730.0), (Art::KmBisOelwechsel, 15_000.0), (Art::TageBisOelwechsel, 365.0)];

/// Aus gewählten Suchtreffern eine Service-Einstellung bauen: bei der
/// Rückstellung wird je Feld das neue Intervall eingetragen.
pub fn aus_suche(basis: &Config, gewaehlt: &[Kandidat], intervall: &[(Art, f64)]) -> Result<Config> {
    let mut werte = Vec::new();
    for k in gewaehlt {
        let Some(&(_, n)) = intervall.iter().find(|(a, _)| *a == k.art) else {
            bail!(tr!("Kein neuer Wert für „{a}“ angegeben", a = k.art.text()));
        };
        if werte.iter().any(|w: &Wert| w.art == k.art) {
            bail!(tr!("„{a}“ ist zweimal gewählt — bitte nur eine Kennung je Wert", a = k.art.text()));
        }
        werte.push(Wert { neu: Some(n), reset: String::new(), ..k.wert.clone() });
    }
    if werte.is_empty() {
        bail!(tr!("Keine Kennung gewählt"));
    }
    Ok(Config { werte, ..basis.clone() })
}

/// Ein gelesener Service-Wert.
#[derive(Debug, Clone)]
pub struct Gelesen {
    pub art: Art,
    pub did: u16,
    pub wert: Option<f64>,
    pub roh: Vec<u8>,
}

impl Gelesen {
    /// Wert als Text, z. B. „4650 km“ oder „überfällig seit 350 km“.
    pub fn wert_text(&self) -> String {
        match self.wert {
            None => tr!("nicht lesbar"),
            Some(v) if self.art.ist_restwert() && v < 0.0 => tr!("überfällig seit {a} {b}", a = format!("{:.0}", -v), b = self.art.einheit()),
            Some(v) => format!("{v:.0} {}", self.art.einheit()),
        }
    }

    pub fn zeile(&self) -> String {
        tr!("{a} {b}   (Kennung {c})", a = format!("{:<26}", self.art.text()), b = self.wert_text(), c = format!("{:#06X}", self.did))
    }

    /// Ampel: überfällig = Warnung, bald fällig (< 1000 km / 30 Tage) = Hinweis.
    pub fn stufe(&self) -> Stufe {
        match self.wert {
            Some(v) if self.art.ist_restwert() && v < 0.0 => Stufe::Warnung,
            Some(v) if self.art.ist_restwert() && v < if self.art.ist_km() { 1000.0 } else { 30.0 } => Stufe::Hinweis,
            _ => Stufe::Gut,
        }
    }
}

fn hex(d: &[u8]) -> String {
    d.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ")
}

/// Hex-Text („3A 98“ oder „3A98“) in Bytes.
pub fn parse_hex(s: &str) -> Result<Vec<u8>> {
    let t: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    if t.is_empty() || !t.len().is_multiple_of(2) {
        bail!(tr!("Hex-Wert ungültig: {s}", s = format!("{:?}", s)));
    }
    (0..t.len()).step_by(2).map(|i| u8::from_str_radix(&t[i..i + 2], 16).with_context(|| tr!("Hex-Wert ungültig: {s}", s = format!("{:?}", s)))).collect()
}

/// Service-Werte lesen (nur lesen, Standardsitzung).
pub fn lesen(be: &Backend, c: &Config) -> Result<Vec<Gelesen>> {
    if c.werte.is_empty() {
        bail!(tr!("Für dieses Fahrzeug sind noch keine Service-Kennungen eingetragen — erst „service merken“ / „service vergleich“ oder „service suche“"));
    }
    let tp = be.open(c.tx, c.rx)?;
    let uds = Uds::new(&*tp);
    uds.diagnostic_session(0x01).context(tr!("Kombi antwortet nicht (Zündung an?)"))?;
    Ok(c.werte
        .iter()
        .map(|w| {
            let roh = uds.read_data_by_id(w.did).unwrap_or_default();
            Gelesen { art: w.art, did: w.did, wert: w.aus(&roh), roh }
        })
        .collect())
}

/// Alle lesbaren Kennungen eines Bereichs (nur lesen).
pub type Schnappschuss = BTreeMap<u16, Vec<u8>>;

pub fn schnappschuss(be: &Backend, tx: u16, rx: u16, von: u16, bis: u16, mut fortschritt: impl FnMut(u16, usize)) -> Result<Schnappschuss> {
    if bis < von {
        bail!(tr!("Ende des Bereichs liegt vor dem Anfang"));
    }
    let tp = be.open(tx, rx)?;
    let uds = Uds::new(&*tp);
    uds.diagnostic_session(0x01).context(tr!("Steuergerät antwortet nicht (Zündung an?)"))?;
    let mut out = Schnappschuss::new();
    for did in von..=bis {
        if (did - von).is_multiple_of(256) {
            fortschritt(did, out.len());
            let _ = uds.tester_present();
        }
        if let Ok(d) = uds.read_data_by_id(did) {
            out.insert(did, d);
        }
    }
    Ok(out)
}

/// Schnappschuss als Datei (TOML: Kennung → Hex).
pub fn speichern(s: &Schnappschuss, pfad: &std::path::Path) -> Result<()> {
    let m: BTreeMap<String, String> = s.iter().map(|(k, v)| (format!("{k:04X}"), hex(v))).collect();
    if let Some(p) = pfad.parent() {
        std::fs::create_dir_all(p)?;
    }
    std::fs::write(pfad, toml::to_string(&m)?).with_context(|| tr!("Schreiben: {a}", a = pfad.display()))
}

pub fn laden(pfad: &std::path::Path) -> Result<Schnappschuss> {
    let t = std::fs::read_to_string(pfad).with_context(|| tr!("Lesen: {a}", a = pfad.display()))?;
    let m: BTreeMap<String, String> = toml::from_str(&t)?;
    m.iter()
        .map(|(k, v)| Ok((u16::from_str_radix(k, 16).with_context(|| tr!("Kennung ungültig: {k}", k = k))?, if v.is_empty() { vec![] } else { parse_hex(v)? })))
        .collect()
}

/// Eine geänderte Kennung zwischen zwei Schnappschüssen.
#[derive(Debug, Clone, PartialEq)]
pub struct Aenderung {
    pub did: u16,
    pub vorher: Vec<u8>,
    pub nachher: Vec<u8>,
}

impl Aenderung {
    pub fn zeile(&self) -> String {
        let mut z = tr!("{a}  vorher {b}  →  nachher {c}", a = format!("{:#06X}", self.did), b = hex(&self.vorher), c = hex(&self.nachher));
        if let (Some(a), Some(b)) = (zahl(&self.vorher, 0, 0, false), zahl(&self.nachher, 0, 0, false)) {
            z += &tr!("   (Zahl {a} → {b})", a = a, b = b);
        }
        z
    }
}

/// Kennungen, deren Inhalt sich geändert hat (nur in beiden lesbare).
/// `rauschen`: Kennungen, die sich auch ohne Rückstellung ändern (z. B. Uhrzeit,
/// Spannung) — aus zwei Schnappschüssen ohne Rückstellung dazwischen.
pub fn vergleich(vorher: &Schnappschuss, nachher: &Schnappschuss, rauschen: Option<&Schnappschuss>) -> Vec<Aenderung> {
    vorher
        .iter()
        .filter_map(|(did, a)| {
            let b = nachher.get(did)?;
            if a == b {
                return None;
            }
            if rauschen.is_some_and(|r| r.get(did).is_some_and(|x| x != a)) {
                return None;
            }
            Some(Aenderung { did: *did, vorher: a.clone(), nachher: b.clone() })
        })
        .collect()
}

/// Vorschlag aus einer Änderung: die neuen Inhalte werden die Rückstellwerte.
/// Die Art lässt sich aus den Zahlen raten (große Zahl = km, bis ~1000 = Tage);
/// sicher ist sie erst mit dem Vergleich zur Anzeige im Kombi.
pub fn vorschlag(aenderungen: &[Aenderung]) -> Vec<Wert> {
    let mut km_frei = vec![Art::KmBisInspektion, Art::KmBisOelwechsel];
    let mut tage_frei = vec![Art::TageBisInspektion, Art::TageBisOelwechsel];
    aenderungen
        .iter()
        .map(|a| {
            let n = zahl(&a.nachher, 0, 0, false);
            let art = match n {
                Some(v) if (1000..=60_000).contains(&v) && !km_frei.is_empty() => km_frei.remove(0),
                Some(v) if (30..=1000).contains(&v) && !tage_frei.is_empty() => tage_frei.remove(0),
                _ => Art::Sonstiges,
            };
            Wert { art, did: a.did, offset: 0, laenge: 0, faktor: 1.0, vorzeichen: a.vorher.len() <= 4, reset: hex(&a.nachher), neu: None }
        })
        .collect()
}

/// Kandidat der Wertesuche.
#[derive(Debug, Clone, PartialEq)]
pub struct Kandidat {
    pub art: Art,
    pub wert: Wert,
    pub gelesen: f64,
}

/// Kennungen suchen, die die angezeigten Werte enthalten können.
/// km mit Faktor 1/10/100 (Toleranz 100 km, das Kombi rundet), Tage mit Faktor 1 (±1).
pub fn suche(s: &Schnappschuss, ziele: &[(Art, f64)]) -> Vec<Kandidat> {
    let mut out = Vec::new();
    for (did, d) in s {
        if d.len() > 16 {
            continue;
        }
        for &(art, ziel) in ziele {
            let (faktoren, tol): (&[f64], f64) = if art.ist_km() { (&[1.0, 10.0, 100.0], 100.0) } else { (&[1.0], 1.0) };
            for laenge in [2usize, 1, 3, 4] {
                if laenge == 1 && art.ist_km() {
                    continue; // ein Byte reicht nicht für km
                }
                for offset in 0..d.len().saturating_sub(laenge - 1) {
                    for &f in faktoren {
                        for vz in [false, true] {
                            let w = Wert { art, did: *did, offset, laenge, faktor: f, vorzeichen: vz, reset: String::new(), neu: None };
                            let Some(v) = w.aus(d) else { continue };
                            if (v - ziel).abs() <= tol && !out.iter().any(|k: &Kandidat| k.art == art && k.wert.did == *did && k.wert.offset == offset) {
                                out.push(Kandidat { art, wert: w, gelesen: v });
                            }
                        }
                    }
                }
            }
        }
    }
    // passt die ganze Kennung, sind Teilstücke davon nur Doppelungen
    let ganz: Vec<(u16, Art)> = out.iter().filter(|k| k.wert.offset == 0 && k.wert.laenge == s[&k.wert.did].len()).map(|k| (k.wert.did, k.art)).collect();
    out.retain(|k| (k.wert.offset == 0 && k.wert.laenge == s[&k.wert.did].len()) || !ganz.contains(&(k.wert.did, k.art)));
    // ganze Kennung mit genau 2 Byte zuerst (typisch), dann nach Abstand
    out.sort_by(|a, b| {
        let ka = (s[&a.wert.did].len() != 2, a.wert.did);
        let kb = (s[&b.wert.did].len() != 2, b.wert.did);
        ka.cmp(&kb)
    });
    out
}

/// Ergebnis einer Rückstellung.
#[derive(Debug, Clone)]
pub struct Rueckstellung {
    pub zeilen: Vec<String>,
    pub sicherung: Option<std::path::PathBuf>,
    pub ok: bool,
}

/// Service zurückstellen: alte Inhalte sichern, Rückstellwerte schreiben, nachlesen.
/// Schreibt etwas ins Kombi — nur nach Bestätigung durch den Nutzer aufrufen.
pub fn zuruecksetzen(be: &Backend, c: &Config, sicherung_ordner: Option<&std::path::Path>) -> Result<Rueckstellung> {
    let werte: Vec<&Wert> = c.werte.iter().filter(|w| w.schreibt()).collect();
    if werte.is_empty() {
        bail!(tr!("Keine Rückstellwerte eingetragen — erst mit „service merken“ und „service vergleich“ lernen"));
    }
    crate::spannung::vor_dem_schreiben(be)?;
    let tp = be.open(c.tx, c.rx)?;
    let uds = Uds::new(&*tp);
    uds.diagnostic_session(0x01).context(tr!("Kombi antwortet nicht (Zündung an?)"))?;

    // 1. alte Inhalte lesen und sichern
    let mut alt = Schnappschuss::new();
    for w in &werte {
        if alt.contains_key(&w.did) {
            continue;
        }
        let d = uds.read_data_by_id(w.did).with_context(|| tr!("Kennung {a} nicht lesbar — nichts geschrieben", a = format!("{:#06X}", w.did)))?;
        alt.insert(w.did, d);
    }
    // neue Inhalte je Kennung: erst `reset`, dann einzelne Felder (`neu`)
    let mut ziel: BTreeMap<u16, Vec<u8>> = BTreeMap::new();
    for w in &werte {
        let basis = match ziel.get(&w.did) {
            Some(d) => d.clone(),
            None if !w.reset.trim().is_empty() => parse_hex(&w.reset)?,
            None => alt[&w.did].clone(),
        };
        let d = match w.neu {
            Some(n) => w.eintragen(&basis, n).with_context(|| {
                tr!("{a}: {b} {c} passt nicht in das Feld — nichts geschrieben", a = format!("{:#06X}", w.did), b = format!("{n:.0}"), c = w.art.einheit())
            })?,
            None => basis,
        };
        ziel.insert(w.did, d);
    }
    let ziele: Vec<(u16, Vec<u8>)> = ziel.into_iter().collect();
    let sicherung = match sicherung_ordner {
        Some(o) => {
            let p = o.join(crate::platzhalter!("service-sicherung-{a}.toml", a = crate::report::unix_now()));
            speichern(&alt, &p)?;
            Some(p)
        }
        None => None,
    };

    // 2. schreiben — vorher alle Kennungen prüfen, damit nichts halb geschrieben wird
    for (did, _) in &ziele {
        crate::km::schreiben_erlaubt(*did, c.tx)?;
    }
    uds.diagnostic_session(c.sitzung).context(tr!("Kombi lässt die erweiterte Sitzung nicht zu — nichts geschrieben"))?;
    let mut zeilen = Vec::new();
    let mut ok = true;
    for (did, neu) in &ziele {
        let did = *did;
        if alt.get(&did) == Some(neu) {
            zeilen.push(tr!("{a}  schon auf Rückstellwert ({b})", a = format!("{:#06X}", did), b = hex(neu)));
            continue;
        }
        match uds.write_data_by_id(did, neu) {
            Ok(()) => {}
            Err(e) => {
                let s = e.to_string();
                let grund = if s.contains("0x33") {
                    crate::trs!("Kombi verlangt eine Anmeldung (Security Access) — ECUWerkbank umgeht das nicht. Bitte über die Tasten am Kombi zurückstellen.")
                } else if s.contains("0x22") {
                    crate::trs!("Bedingungen nicht erfüllt (Zündung an, Motor aus, Fahrzeug steht?)")
                } else if s.contains("0x31") || s.contains("0x13") {
                    crate::trs!("Kennung oder Wert passt nicht zu diesem Kombi")
                } else {
                    "Schreiben fehlgeschlagen"
                };
                zeilen.push(format!("{:#06X}  {grund} ({s})", did));
                ok = false;
                break; // nach dem ersten Fehler nichts weiter schreiben
            }
        }
        // 3. nachlesen
        match uds.read_data_by_id(did) {
            Ok(d) if &d == neu => zeilen.push(tr!("{a}  {b} → {c}  geprüft", a = format!("{:#06X}", did), b = hex(&alt[&did]), c = hex(neu))),
            Ok(d) => {
                zeilen.push(tr!("{a}  geschrieben, aber gelesen {b} statt {c}", a = format!("{:#06X}", did), b = hex(&d), c = hex(neu)));
                ok = false;
            }
            Err(e) => {
                zeilen.push(tr!("{a}  geschrieben, Nachlesen fehlgeschlagen: {e}", a = format!("{:#06X}", did), e = e));
                ok = false;
            }
        }
    }
    let _ = uds.diagnostic_session(0x01);
    Ok(Rueckstellung { zeilen, sicherung, ok })
}

/// Alte Inhalte aus einer Sicherung zurückschreiben (macht eine Rückstellung rückgängig).
pub fn sicherung_zurueck(be: &Backend, c: &Config, pfad: &std::path::Path) -> Result<Vec<String>> {
    let s = laden(pfad)?;
    for did in s.keys() {
        crate::km::schreiben_erlaubt(*did, c.tx)?;
    }
    crate::spannung::vor_dem_schreiben(be)?;
    let tp = be.open(c.tx, c.rx)?;
    let uds = Uds::new(&*tp);
    uds.diagnostic_session(c.sitzung)?;
    let mut z = Vec::new();
    for (did, d) in &s {
        uds.write_data_by_id(*did, d).with_context(|| tr!("Kennung {did}", did = format!("{:#06X}", did)))?;
        z.push(format!("{did:#06X}  ← {}", hex(d)));
    }
    let _ = uds.diagnostic_session(0x01);
    Ok(z)
}

/// Übliche Kennungs-Bereiche im Kombi für Merken und Suche.
pub const BEREICHE: [(u16, u16); 4] = [(0x0100, 0x04FF), (0x1000, 0x10FF), (0x2200, 0x22FF), (0x2400, 0x24FF)];

/// Läuft die Demo-Simulation mit Demo-Service-Kennungen?
pub fn ist_demo(be: &Backend) -> bool {
    matches!(be, Backend::Demo) && crate::sim::modell() == crate::sim::Modell::Demo
}

/// Bereiche, die beim Merken gelesen werden.
pub fn bereiche(be: &Backend) -> Vec<(u16, u16)> {
    if ist_demo(be) {
        vec![(crate::sim::DEMO_SERVICE_DID - 0x20, crate::sim::DEMO_SERVICE_DID + 0x20)]
    } else {
        BEREICHE.to_vec()
    }
}

/// Einstellungen: aus dem Fahrzeug, sonst Demo bzw. Kombi-Standard ohne Kennungen.
pub fn config_fuer(be: &Backend, v: Option<&crate::vehicle::Vehicle>) -> Config {
    match v.and_then(|v| v.service.clone()) {
        Some(c) => c,
        None if ist_demo(be) => demo_config(),
        None => Config::default(),
    }
}

/// Ordner für gemerkte Stände und Sicherungen (Fahrzeugordner; Demo: eigener Ordner).
pub fn ordner(be: &Backend, v: Option<&crate::vehicle::Vehicle>) -> Option<std::path::PathBuf> {
    match v {
        Some(v) => Some(v.dir()),
        None if matches!(be, Backend::Demo) => Some(crate::history::data_dir().join("demo-service")),
        None => None,
    }
}

pub fn merk_pfad(ordner: &std::path::Path, name: &str) -> std::path::PathBuf {
    let n: String = name.chars().map(|c| if c.is_alphanumeric() || c == '-' { c } else { '_' }).collect();
    ordner.join(crate::platzhalter!("service-merken-{n}.toml", n = n))
}

/// Neueste Service-Sicherung im Ordner.
pub fn neueste_sicherung(ordner: &std::path::Path) -> Option<std::path::PathBuf> {
    let mut l: Vec<_> = std::fs::read_dir(ordner)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("service-sicherung-")))
        .collect();
    l.sort();
    l.pop()
}

/// Nach einer Rückstellung den Ölwechsel im Wartungsplan als heute erledigt
/// eintragen (Posten, die mit „Öl“ beginnen; fehlt einer, wird er aus der
/// Vorlage angelegt). Gibt die eingetragenen Posten zurück.
pub fn wartung_eintragen(v: &mut crate::vehicle::Vehicle) -> Vec<String> {
    if !v.wartung.iter().any(|w| w.name.to_lowercase().starts_with("öl")) {
        if let Some(w) = crate::vehicle::wartung_vorlage().into_iter().find(|w| w.name.to_lowercase().starts_with("öl")) {
            v.wartung.insert(0, w);
        }
    }
    let namen: Vec<String> = v.wartung.iter().filter(|w| w.name.to_lowercase().starts_with("öl")).map(|w| w.name.clone()).collect();
    for n in &namen {
        v.wartung_erledigt(n, None);
    }
    namen
}

/// Anleitung: Service über die Tasten am Kombi zurückstellen (ohne Diagnosegerät).
pub fn anleitung_tasten(bezeichnung: &str) -> Vec<&'static str> {
    let b = bezeichnung.to_lowercase();
    let mut v = vec![crate::trs!("Service ohne Diagnosegerät zurückstellen (der sicherste Weg):")];
    if b.contains("golf") || b.contains("passat") || b.contains("touran") || b.contains("jetta") || b.contains("scirocco") || b.contains("tiguan") || b.contains("caddy") {
        v.extend([
            crate::trs!("  Mit Bordcomputer-Menü (Multifunktionsanzeige „Plus“):"),
            crate::trs!("    Zündung an → Menü „Einstellungen“ → „Service“ → „Zurücksetzen“ bestätigen."),
            crate::trs!("  Ohne Menü:"),
            crate::trs!("    1. Zündung aus."),
            crate::trs!("    2. Taste „0.0/SET“ (Tageskilometer-Rückstellung) gedrückt halten."),
            crate::trs!("    3. Zündung an, Taste loslassen — im Display erscheint der Service-Hinweis."),
            crate::trs!("    4. Innerhalb von etwa 20 Sekunden die Uhr-Taste (Minuten, rechts) kurz drücken."),
            crate::trs!("    5. Zündung aus und wieder an: die Anzeige muss jetzt das neue Intervall zeigen."),
        ]);
    } else {
        v.push(crate::trs!("  Ablauf steht in der Betriebsanleitung (Kapitel „Service-Intervallanzeige“)."));
    }
    v.push(crate::trs!("  Bei LongLife-Service (QG1) und festem Intervall (QG2) ist die Rückstellung gleich;"));
    v.push(crate::trs!("  das Intervall selbst richtet sich nach der Codierung bzw. Anpassung des Kombis."));
    v.push(crate::trs!("  Wichtig: Abweichungen je nach Baujahr möglich — im Zweifel die Betriebsanleitung prüfen."));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oelwechsel_im_wartungsplan() {
        let mut v = crate::vehicle::Vehicle { km: Some(242_488), ..Default::default() };
        assert_eq!(wartung_eintragen(&mut v), vec!["Ölwechsel + Ölfilter".to_string()]);
        assert_eq!(v.wartung[0].letzte_km, Some(242_488));
        assert_eq!(v.wartung.len(), 1);
        // zweites Mal: kein doppelter Posten
        wartung_eintragen(&mut v);
        assert_eq!(v.wartung.len(), 1);
    }

    #[test]
    fn zahlen_mit_vorzeichen() {
        assert_eq!(zahl(&[0x12, 0xC0], 0, 0, false), Some(4800));
        assert_eq!(zahl(&[0xFF, 0x38], 0, 0, true), Some(-200));
        assert_eq!(zahl(&[0x00, 0xD4, 0x00], 1, 1, false), Some(212));
        assert_eq!(zahl(&[1, 2, 3, 4, 5], 0, 0, false), None); // zu lang
        assert_eq!(zahl(&[1], 1, 0, false), None);
    }

    #[test]
    fn vergleich_und_vorschlag() {
        let a: Schnappschuss = [(0x2203, vec![0x5E, 0xB8]), (0x22A0, vec![0x00, 0x30]), (0x22A1, vec![0x00, 0x10]), (0x2300, vec![1])].into();
        let b: Schnappschuss = [(0x2203, vec![0x5E, 0xB8]), (0x22A0, vec![0x3A, 0x98]), (0x22A1, vec![0x01, 0x6D]), (0x2300, vec![2])].into();
        // 0x2300 ändert sich auch ohne Rückstellung (z. B. Uhrzeit)
        let r: Schnappschuss = [(0x2300, vec![3])].into();
        let d = vergleich(&a, &b, Some(&r));
        assert_eq!(d.iter().map(|x| x.did).collect::<Vec<_>>(), vec![0x22A0, 0x22A1]);
        let v = vorschlag(&d);
        assert_eq!(v[0].art, Art::KmBisInspektion);
        assert_eq!(v[0].reset, "3A 98");
        assert_eq!(v[1].art, Art::TageBisInspektion);
    }

    #[test]
    fn feld_eintragen() {
        let w = Wert { art: Art::KmBisOelwechsel, did: 1, offset: 1, laenge: 2, faktor: 10.0, vorzeichen: true, reset: String::new(), neu: None };
        let d = [0xAA, 0xFF, 0xDD, 0xBB]; // -35 × 10 = -350 km
        assert_eq!(w.aus(&d), Some(-350.0));
        let n = w.eintragen(&d, 15_000.0).unwrap();
        assert_eq!(n, vec![0xAA, 0x05, 0xDC, 0xBB]); // übrige Bytes unverändert
        assert_eq!(w.aus(&n), Some(15_000.0));
        assert_eq!(w.eintragen(&d, 1e9), None); // passt nicht
    }

    #[test]
    fn demo_einrichten_aus_suche_und_zuruecksetzen() {
        let _g = crate::sim::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::sim::set_modell(crate::sim::Modell::Demo);
        crate::sim::reset();
        let be = Backend::Demo;
        let basis = Config { werte: vec![], ..demo_config() };
        let mut s = Schnappschuss::new();
        for (a, b) in bereiche(&be) {
            s.extend(schnappschuss(&be, basis.tx, basis.rx, a, b, |_, _| {}).unwrap());
        }
        // Werte wie im Kombi abgelesen
        let k = suche(&s, &[(Art::KmBisInspektion, -350.0), (Art::KmBisOelwechsel, 4650.0), (Art::TageBisOelwechsel, 153.0)]);
        let mut gewaehlt: Vec<Kandidat> = Vec::new();
        for k in k {
            if !gewaehlt.iter().any(|g| g.art == k.art) {
                gewaehlt.push(k);
            }
        }
        assert_eq!(gewaehlt.len(), 3);
        let c = aus_suche(&basis, &gewaehlt, &INTERVALL_VORSCHLAG).unwrap();
        assert!(c.kann_zuruecksetzen());
        assert_eq!(c.vorschau().len(), 3);
        // doppelt gewählt → Fehler
        assert!(aus_suche(&basis, &[gewaehlt[0].clone(), gewaehlt[0].clone()], &INTERVALL_VORSCHLAG).is_err());

        let dir = std::env::temp_dir().join(format!("vag-service-test2-{}", std::process::id()));
        let r = zuruecksetzen(&be, &c, Some(&dir)).unwrap();
        assert!(r.ok, "{:?}", r.zeilen);
        let w = lesen(&be, &c).unwrap();
        let wert = |a: Art| w.iter().find(|g| g.art == a).and_then(|g| g.wert);
        assert_eq!(wert(Art::KmBisInspektion), Some(30_000.0));
        assert_eq!(wert(Art::KmBisOelwechsel), Some(15_000.0));
        assert_eq!(wert(Art::TageBisOelwechsel), Some(365.0));
        // Config übersteht Speichern/Laden (fahrzeug.toml)
        let t = toml::to_string(&c).unwrap();
        assert_eq!(toml::from_str::<Config>(&t).unwrap(), c);
        sicherung_zurueck(&be, &c, r.sicherung.as_ref().unwrap()).unwrap();
        assert_eq!(lesen(&be, &c).unwrap()[0].wert, Some(-350.0));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn wertesuche_findet_kennung() {
        let s: Schnappschuss = [(0x22A0, vec![0x12, 0xC0]), (0x22A1, vec![0x00, 0xD4]), (0x0100, vec![0x30; 20])].into();
        let k = suche(&s, &[(Art::KmBisInspektion, 4800.0), (Art::TageBisInspektion, 212.0)]);
        assert!(k.iter().any(|k| k.wert.did == 0x22A0 && k.art == Art::KmBisInspektion && k.wert.faktor == 1.0));
        assert!(k.iter().any(|k| k.wert.did == 0x22A1 && k.art == Art::TageBisInspektion));
        assert!(!k.iter().any(|k| k.wert.did == 0x0100)); // zu lang, ignoriert
    }

    #[test]
    fn demo_lesen_zuruecksetzen_und_rueckgaengig() {
        let _g = crate::sim::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::sim::set_modell(crate::sim::Modell::Demo);
        crate::sim::reset();
        let be = Backend::Demo;
        let c = demo_config();
        let w = lesen(&be, &c).unwrap();
        assert_eq!(w[0].wert, Some(-350.0)); // Inspektion überfällig
        assert_eq!(w[0].stufe(), Stufe::Warnung);
        assert!(w[0].zeile().contains("überfällig seit 350 km"));

        let dir = std::env::temp_dir().join(format!("vag-service-test-{}", std::process::id()));
        let r = zuruecksetzen(&be, &c, Some(&dir)).unwrap();
        assert!(r.ok, "{:?}", r.zeilen);
        let w = lesen(&be, &c).unwrap();
        assert_eq!(w[0].wert, Some(30000.0));
        assert_eq!(w[3].wert, Some(365.0));

        sicherung_zurueck(&be, &c, r.sicherung.as_ref().unwrap()).unwrap();
        assert_eq!(lesen(&be, &c).unwrap()[0].wert, Some(-350.0));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn demo_vorher_nachher_lernt_rueckstellwerte() {
        let _g = crate::sim::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::sim::set_modell(crate::sim::Modell::Demo);
        crate::sim::reset();
        let be = Backend::Demo;
        let von = crate::sim::DEMO_SERVICE_DID - 0x10;
        let bis = crate::sim::DEMO_SERVICE_DID + 0x10;
        let a = schnappschuss(&be, 0x7E0, 0x7E8, von, bis, |_, _| {}).unwrap();
        // „Tasten-Rückstellung“ am Kombi nachstellen
        crate::sim::service_tasten_reset();
        let b = schnappschuss(&be, 0x7E0, 0x7E8, von, bis, |_, _| {}).unwrap();
        let v = vorschlag(&vergleich(&a, &b, None));
        let erwartet = demo_config().werte;
        assert_eq!(v.len(), 4);
        for (x, y) in v.iter().zip(&erwartet) {
            assert_eq!((x.did, &x.reset), (y.did, &y.reset));
        }
    }
}
