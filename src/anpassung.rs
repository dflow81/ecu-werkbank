//! Anpassungen sichern: von jedem UDS-Steuergerät alle lesbaren Kennungen
//! der typischen VAG-Bereiche (0x0100–0x04FF, 0x2000–0x24FF). Dort liegen
//! Anpassungswerte wie Komfortfunktionen oder Service-Intervalle. Vorher und
//! nachher gesichert, zeigt der Vergleich genau, welche Kennung sich
//! geändert hat.
//!
//! Zurückschreiben (z. B. nach dem Tausch eines Steuergeräts): erst
//! [`pruefen`] (nur lesen) — zeigt, welche Kennungen heute anders sind. Dann
//! schreibt [`zurueckschreiben`] nur die ausgewählten, und nur, wenn
//! - die Länge gleich ist (sonst passt der Aufbau nicht, z. B. anderes Teil),
//! - der Wert beim zweiten Lesen gleich bleibt (sonst ist es ein Messwert),
//! - es nicht der Kilometerstand ist (der wird nie geschrieben).
//!
//! Jeder Wert wird nach dem Schreiben zur Kontrolle neu gelesen.
//!
//! Ältere TP-2.0-Steuergeräte (KWP2000): Geräte, die 0x22 mit Kennungen
//! verstehen (am Golf 6 u. a. Bordnetz, ABS, Einparkhilfe), werden genauso
//! gesichert. Ganz alte Geräte ohne 0x22 (z. B. Motor, Türen) geben so nichts her.
//!
//! Ablage: `…/fahrzeuge/<VIN>/anpassungen/<Zeitstempel>.toml`.
//! Dauer am Auto: je Steuergerät etwa 30–60 s.

use crate::labels::LabelDb;
use crate::service::{self, Aenderung, Schnappschuss};
use crate::transport::Backend;
use crate::tr;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const BEREICHE: &[(u16, u16)] = &[(0x0100, 0x04FF), (0x2000, 0x24FF)];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Geraet {
    pub name: String,
    pub tx: u16,
    pub rx: u16,
    /// Kennung (hex) → Inhalt (hex)
    pub kennungen: BTreeMap<String, String>,
    /// TP-2.0-Logikadresse (ältere Steuergeräte); None = UDS
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tp20: Option<u8>,
}

impl Geraet {
    pub fn titel(&self) -> String {
        if let Some(a) = self.tp20 {
            return format!("TP 2.0 {a:#04x} {}", self.name).trim().to_string();
        }
        if self.name.is_empty() {
            format!("UDS {:#05x}", self.tx)
        } else {
            format!("UDS {:#05x} {}", self.tx, self.name)
        }
    }

    pub fn schnappschuss(&self) -> Schnappschuss {
        self.kennungen
            .iter()
            .filter_map(|(k, v)| Some((u16::from_str_radix(k, 16).ok()?, crate::backup::parse_hex(v).unwrap_or_default())))
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Sicherung {
    pub created: String,
    pub timestamp: u64,
    #[serde(default)]
    pub vehicle: String,
    pub geraete: Vec<Geraet>,
    /// z. B. „vor dem Zurückschreiben“ (leer = normale Sicherung)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notiz: String,
}

impl Sicherung {
    /// Komplette Sicherung (nicht nur die Werte vor einem Zurückschreiben)?
    pub fn vollstaendig(&self) -> bool {
        self.notiz.is_empty()
    }
}

/// Kennungs-Bereiche bei TP-2.0-Steuergeräten (0x22 wie bei UDS; 0x06xx = Codierung).
/// TODO(verify): ob VCDS die Anpassungskanäle der TP-2.0-Geräte über dieselben
/// Kennungen liest, ist nicht belegt — gesichert wird alles, was lesbar ist.
pub const BEREICHE_TP20: &[(u16, u16)] = &[(0x0100, 0x04FF), (0x0600, 0x06FF), (0x2000, 0x24FF)];

/// Alle lesbaren Kennungen eines TP-2.0-Steuergeräts (nur lesen, 0x22).
/// Kennt das Gerät 0x22 gar nicht (ältere Steuergeräte), kommt eine leere Liste.
pub fn tp20_lesen(be: &Backend, adresse: u8, bereiche: &[(u16, u16)], mut fortschritt: impl FnMut(u16)) -> Result<BTreeMap<String, String>> {
    let mut ch = crate::kwp::open_session(be, adresse)?;
    let mut k = crate::kwp::Kwp::new(&mut ch);
    let mut out = BTreeMap::new();
    let mut erste = true;
    for &(von, bis) in bereiche {
        for did in von..=bis {
            if did % 0x40 == 0 {
                fortschritt(did);
            }
            match k.request(0x22, &did.to_be_bytes()) {
                Ok(r) => {
                    if let Some(d) = r.get(2..).filter(|d| !d.is_empty()) {
                        out.insert(format!("{did:04X}"), crate::backup::hex(d));
                    }
                }
                Err(e) => {
                    let t = format!("{e:#}");
                    // Dienst unbekannt: dieses Gerät hat keine Kennungen
                    if erste && (t.contains("NRC 0x11") || t.contains("NRC 0x12")) {
                        return Ok(out);
                    }
                    if !t.contains("NRC") {
                        return Err(e); // Verbindung weg
                    }
                }
            }
            erste = false;
        }
    }
    Ok(out)
}

/// TP-2.0-Steuergeräte sichern (Adressen z. B. aus der Suche bzw. dem Profil).
pub fn sichern_tp20(be: &Backend, gefunden: &[crate::kwp::Tp20Found], bereiche: &[(u16, u16)], mut fortschritt: impl FnMut(&str, u16)) -> Vec<Geraet> {
    let mut out = Vec::new();
    for f in gefunden {
        let name = if f.name.is_empty() { f.ident.get(1).cloned().unwrap_or_default() } else { f.name.clone() };
        let titel = format!("TP 2.0 {:#04x} {name}", f.address);
        // ein zweiter Versuch, falls die Verbindung zwischendurch abreißt
        let r = tp20_lesen(be, f.address, bereiche, |d| fortschritt(&titel, d)).or_else(|_| tp20_lesen(be, f.address, bereiche, |d| fortschritt(&titel, d)));
        if let Ok(kennungen) = r {
            if !kennungen.is_empty() {
                out.push(Geraet { name, tx: f.tx, rx: f.rx, kennungen, tp20: Some(f.address) });
            }
        }
    }
    out
}

/// Alle Steuergeräte sichern (UDS, mit `mit_tp20` auch TP 2.0). `fortschritt(Gerät, Kennung)`.
pub fn sichern(be: &Backend, db: &LabelDb, mit_tp20: bool, mut fortschritt: impl FnMut(&str, u16)) -> Result<Sicherung> {
    let mut s = sichern_uds(be, db, &mut fortschritt)?;
    if mit_tp20 && be.supports_tp20() {
        let profil = crate::profil::fuer_scan(be);
        let gefunden = match &profil {
            Some((_, p)) => crate::kwp::sweep_addrs(be, &p.tp20, |_| fortschritt(&tr!("TP-2.0-Steuergeräte suchen …"), 0))?,
            None => crate::kwp::sweep(be, |_| fortschritt(&tr!("TP-2.0-Steuergeräte suchen …"), 0))?,
        };
        // in Tests nur ein kleiner Bereich (alle Bereiche dauern auch simuliert Minuten)
        let bereiche: &[(u16, u16)] = if cfg!(test) { &[(0x0600, 0x0601)] } else { BEREICHE_TP20 };
        s.geraete.extend(sichern_tp20(be, &gefunden, bereiche, &mut fortschritt));
    }
    Ok(s)
}

/// Alle UDS-Steuergeräte sichern. `fortschritt(Gerät, Kennung)`.
pub fn sichern_uds(be: &Backend, db: &LabelDb, mut fortschritt: impl FnMut(&str, u16)) -> Result<Sicherung> {
    let profil = crate::profil::fuer_scan(be);
    let ziele = crate::report::uds_targets_mit(be, db, profil.as_ref().map(|p| &p.1));
    let mut geraete = Vec::new();
    for (addr, tx, rx) in ziele {
        let name = addr.and_then(|a| db.ecu_name(a)).unwrap_or("").to_string();
        let titel = format!("{tx:#05x} {name}");
        let mut s = Schnappschuss::new();
        let mut erreicht = false;
        for &(von, bis) in BEREICHE {
            match service::schnappschuss(be, tx, rx, von, bis, |did, _| fortschritt(&titel, did)) {
                Ok(t) => {
                    erreicht = true;
                    s.extend(t);
                }
                Err(_) => break, // Steuergerät antwortet nicht (z. B. nur OBD)
            }
        }
        if erreicht && !s.is_empty() {
            geraete.push(Geraet { name, tx, rx, kennungen: s.iter().map(|(k, v)| (format!("{k:04X}"), crate::backup::hex(v))).collect(), tp20: None });
        }
    }
    Ok(Sicherung { created: crate::report::now_local(), timestamp: crate::report::unix_now(), vehicle: String::new(), geraete, notiz: String::new() })
}

pub fn ordner(vehicle: &str) -> PathBuf {
    crate::vehicle::dir(vehicle).join("anpassungen")
}

pub fn speichern(s: &Sicherung) -> Result<PathBuf> {
    let d = ordner(&s.vehicle);
    std::fs::create_dir_all(&d).with_context(|| tr!("Ordner anlegen: {a}", a = d.display()))?;
    let p = d.join(format!("{}.toml", s.timestamp));
    crate::ablage::schreiben(&p, toml::to_string(s)?).with_context(|| tr!("Schreiben: {a}", a = p.display()))?;
    Ok(p)
}

pub fn laden(p: &Path) -> Result<Sicherung> {
    let t = crate::ablage::lesen(p).with_context(|| tr!("Lesen: {a}", a = p.display()))?;
    toml::from_str(&t).with_context(|| tr!("Keine gültige Anpassungs-Sicherung: {a}", a = p.display()))
}

/// Sicherungen eines Fahrzeugs, neueste zuerst.
pub fn liste(vehicle: &str) -> Vec<(PathBuf, Sicherung)> {
    let mut v: Vec<_> = crate::ablage::liste(&ordner(vehicle))
        .into_iter()
        .filter(|p| p.extension().is_some_and(|x| x == "toml"))
        .filter_map(|p| laden(&p).ok().map(|s| (p, s)))
        .collect();
    v.sort_by_key(|(_, s)| std::cmp::Reverse(s.timestamp));
    v
}

/// Geänderte Kennungen je Steuergerät (nur Geräte in beiden Sicherungen).
pub fn vergleichen(alt: &Sicherung, neu: &Sicherung) -> Vec<(String, Vec<Aenderung>)> {
    neu.geraete
        .iter()
        .filter_map(|n| {
            let a = alt.geraete.iter().find(|a| a.tx == n.tx && a.tp20 == n.tp20)?;
            let d = service::vergleich(&a.schnappschuss(), &n.schnappschuss(), None);
            (!d.is_empty()).then(|| (n.titel(), d))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Zurückschreiben

/// Verbindung zu einem Steuergerät, egal ob UDS oder TP 2.0.
enum Verbindung {
    Uds(Box<dyn crate::transport::Transport>),
    Tp20(crate::tp20::Channel),
}

impl Verbindung {
    fn oeffnen(be: &Backend, g: &Geraet) -> Result<Self> {
        match g.tp20 {
            Some(a) => Ok(Verbindung::Tp20(crate::kwp::open_session(be, a)?)),
            None => {
                let tp = be.open(g.tx, g.rx)?;
                crate::uds::Uds::new(&*tp).diagnostic_session(crate::uds::SESSION_DEFAULT).context(tr!("Steuergerät antwortet nicht (Zündung an?)"))?;
                Ok(Verbindung::Uds(tp))
            }
        }
    }

    fn anfrage(&mut self, sid: u8, data: &[u8]) -> Result<Vec<u8>> {
        match self {
            Verbindung::Uds(tp) => crate::uds::Uds::new(&**tp).request(sid, data),
            Verbindung::Tp20(ch) => crate::kwp::Kwp::new(ch).request(sid, data),
        }
    }

    fn lesen(&mut self, did: u16) -> Result<Vec<u8>> {
        let r = self.anfrage(0x22, &did.to_be_bytes())?;
        Ok(r.get(2..).unwrap_or_default().to_vec())
    }

    /// UDS: erweiterte Sitzung zum Schreiben. TP 2.0: die VAG-Sitzung ist schon offen.
    fn schreibsitzung(&mut self) -> Result<()> {
        if let Verbindung::Uds(tp) = self {
            crate::uds::Uds::new(&**tp).diagnostic_session(crate::uds::SESSION_EXTENDED).context(tr!("erweiterte Diagnosesitzung (0x03) abgelehnt"))?;
        }
        Ok(())
    }

    /// Am Steuergerät anmelden (UDS: SA2/fester Key; TP 2.0: KWP-XorKey).
    fn anmelden(&mut self, m: &crate::login::Methode) -> Result<()> {
        match self {
            Verbindung::Uds(tp) => crate::login::anmelden_uds(&crate::uds::Uds::new(&**tp), m),
            Verbindung::Tp20(ch) => crate::login::anmelden_kwp(&mut crate::kwp::Kwp::new(ch), m),
        }
    }

    /// Schreiben mit 0x2E (UDS: WriteDataByIdentifier).
    /// TODO(verify): TP 2.0 — ob die Steuergeräte 0x2E (KWP2000
    /// WriteDataByCommonIdentifier) annehmen, ist am Auto noch nicht geprüft.
    fn schreiben(&mut self, did: u16, data: &[u8]) -> Result<()> {
        let mut d = did.to_be_bytes().to_vec();
        d.extend_from_slice(data);
        self.anfrage(0x2E, &d).map(|_| ())
    }
}

/// Stand einer gesicherten Kennung im Vergleich zum Auto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zustand {
    /// wie in der Sicherung — nichts zu tun
    Gleich,
    /// anders — kann zurückgeschrieben werden
    Anders,
    /// ändert sich von selbst (Messwert, Zähler) — wird nicht geschrieben
    Wechselt,
    /// andere Länge — Aufbau passt nicht (z. B. anderes Teil), wird nicht geschrieben
    LaengeAnders,
    /// Kilometerstand — wird nie geschrieben
    Gesperrt,
    /// heute nicht lesbar
    NichtLesbar,
}

impl Zustand {
    pub fn text(self) -> String {
        match self {
            Zustand::Gleich => tr!("gleich"),
            Zustand::Anders => tr!("anders — kann zurückgeschrieben werden"),
            Zustand::Wechselt => tr!("ändert sich von selbst (Messwert) — wird nicht geschrieben"),
            Zustand::LaengeAnders => tr!("andere Länge (anderes Teil?) — wird nicht geschrieben"),
            Zustand::Gesperrt => tr!("Kilometerstand — wird nie geschrieben"),
            Zustand::NichtLesbar => tr!("heute nicht lesbar"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Punkt {
    pub did: u16,
    pub gesichert: Vec<u8>,
    pub jetzt: Option<Vec<u8>>,
    pub zustand: Zustand,
}

impl Punkt {
    pub fn zeile(&self) -> String {
        let jetzt = self.jetzt.as_deref().map(crate::backup::hex).unwrap_or_else(|| "—".into());
        tr!("{a}  Auto {b}  Sicherung {c}  {d}", a = format!("{:#06X}", self.did), b = jetzt, c = crate::backup::hex(&self.gesichert), d = self.zustand.text())
    }
}

/// Ergebnis für eine geschriebene Kennung. `vorher` = Wert vor dem Schreiben.
#[derive(Debug, Clone, PartialEq)]
pub struct Geschrieben {
    pub did: u16,
    pub vorher: Vec<u8>,
    pub ergebnis: Result<(), String>,
}

/// Kilometerstand-Kennungen: nie schreiben.
fn gesperrt(g: &Geraet, did: u16, vehicle: &str) -> bool {
    crate::km::gesperrt(did, g.tx)
        || crate::profil::load(vehicle).and_then(|p| p.km_quelle).is_some_and(|q| q.did == did && q.tx == g.tx)
}

fn einstufen(g: &Geraet, did: u16, gesichert: &[u8], jetzt: Option<&[u8]>, vehicle: &str) -> Zustand {
    match jetzt {
        _ if gesperrt(g, did, vehicle) => Zustand::Gesperrt,
        None => Zustand::NichtLesbar,
        Some(j) if j == gesichert => Zustand::Gleich,
        Some(j) if j.len() != gesichert.len() => Zustand::LaengeAnders,
        Some(_) => Zustand::Anders,
    }
}

/// Passt die Sicherung zum angeschlossenen Auto? (Fahrgestellnummer, falls lesbar)
pub fn fahrzeug_pruefen(be: &Backend, s: &Sicherung) -> Result<()> {
    if s.vehicle.len() != 17 {
        return Ok(());
    }
    if let Some(vin) = crate::profil::quick_vin(be) {
        if vin != s.vehicle {
            anyhow::bail!(tr!("Die Sicherung gehört zu {a}, angeschlossen ist {b} — nichts geschrieben.", a = s.vehicle, b = vin));
        }
    }
    Ok(())
}

/// Alle gesicherten Kennungen eines Steuergeräts mit dem Auto vergleichen (nur lesen).
/// `fortschritt(erledigt, gesamt)`.
pub fn pruefen(be: &Backend, g: &Geraet, vehicle: &str, mut fortschritt: impl FnMut(usize, usize)) -> Result<Vec<Punkt>> {
    let mut v = Verbindung::oeffnen(be, g)?;
    let soll = g.schnappschuss();
    let mut out = Vec::new();
    for (i, (did, gesichert)) in soll.iter().enumerate() {
        if i % 16 == 0 {
            fortschritt(i, soll.len());
        }
        let jetzt = match v.lesen(*did) {
            Ok(d) => Some(d),
            Err(e) if format!("{e:#}").contains("NRC") => None,
            Err(e) => return Err(e), // Verbindung weg
        };
        let zustand = einstufen(g, *did, gesichert, jetzt.as_deref(), vehicle);
        out.push(Punkt { did: *did, gesichert: gesichert.clone(), jetzt, zustand });
    }
    // Gegenprobe: was sich in der Zwischenzeit von selbst ändert, ist ein Messwert
    std::thread::sleep(std::time::Duration::from_millis(if cfg!(test) { 10 } else { 500 }));
    for p in out.iter_mut().filter(|p| p.zustand == Zustand::Anders) {
        if let Ok(d) = v.lesen(p.did) {
            if Some(&d) != p.jetzt.as_ref() {
                p.zustand = Zustand::Wechselt;
                p.jetzt = Some(d);
            }
        }
    }
    fortschritt(soll.len(), soll.len());
    Ok(out)
}

/// Einen Wert schreiben und zur Kontrolle neu lesen. Fehlertext als String.
fn schreiben_und_pruefen(v: &mut Verbindung, did: u16, gesichert: &[u8]) -> Result<(), String> {
    v.schreiben(did, gesichert).map_err(|e| format!("{e:#}"))?;
    match v.lesen(did) {
        Ok(d) if d == gesichert => Ok(()),
        Ok(d) => Err(tr!("Kontrolle fehlgeschlagen: gelesen {a} statt {b}", a = crate::backup::hex(&d), b = crate::backup::hex(gesichert))),
        Err(e) => Err(tr!("Kontroll-Lesen: {e}", e = format!("{e:#}"))),
    }
}

/// Die Kennungen `dids` aus der Sicherung ins Steuergerät schreiben.
/// Jede wird vorher neu gelesen und geprüft (siehe Moduldoku), danach zur
/// Kontrolle gelesen. Kennungen, die die Prüfung nicht bestehen, werden
/// übersprungen (mit Grund im Ergebnis). Gibt `Err` nur zurück, wenn das
/// Steuergerät gar nicht erreichbar ist.
pub fn zurueckschreiben(be: &Backend, g: &Geraet, vehicle: &str, dids: &[u16]) -> Result<Vec<Geschrieben>> {
    // Spannung am Anfang und — bei vielen Kennungen — zwischendurch prüfen
    let mut waechter = crate::spannung::Waechter::starten(be, crate::spannung::ist_motor(g.tp20, g.tx))?;
    let soll = g.schnappschuss();
    let login = crate::login::fuer(vehicle, login_id(g));
    let mut v = Verbindung::oeffnen(be, g)?;
    let mut out = Vec::new();
    let mut sitzung = false;
    // Ergebnis des Logins: None = nicht versucht, Some(Ok) = angemeldet, Some(Err) = fehlgeschlagen
    let mut anmeldung: Option<Result<(), String>> = None;
    for &did in dids {
        let Some(gesichert) = soll.get(&did) else {
            out.push(Geschrieben { did, vorher: Vec::new(), ergebnis: Err(tr!("nicht in der Sicherung")) });
            continue;
        };
        let vorher = match v.lesen(did) {
            Ok(d) => d,
            Err(e) => {
                out.push(Geschrieben { did, vorher: Vec::new(), ergebnis: Err(tr!("Lesen: {e}", e = format!("{e:#}"))) });
                continue;
            }
        };
        let zustand = einstufen(g, did, gesichert, Some(&vorher), vehicle);
        if zustand == Zustand::Gleich {
            out.push(Geschrieben { did, vorher, ergebnis: Ok(()) });
            continue;
        }
        if zustand != Zustand::Anders {
            out.push(Geschrieben { did, vorher, ergebnis: Err(zustand.text()) });
            continue;
        }
        if !sitzung {
            if let Err(e) = v.schreibsitzung() {
                out.push(Geschrieben { did, vorher, ergebnis: Err(format!("{e:#}")) });
                break;
            }
            sitzung = true;
            // falls ein Login hinterlegt ist: einmal anmelden (die erweiterte Sitzung steht)
            if let Some(m) = &login {
                anmeldung = Some(v.anmelden(m).map_err(|e| format!("{e:#}")));
            }
        }
        if let Err(e) = waechter.vor_naechstem() {
            out.push(Geschrieben { did, vorher, ergebnis: Err(format!("{e:#}")) });
            break;
        }
        // erst schreiben; bei „Zugriff verweigert" ohne Login einen klaren Hinweis geben
        let mut erg = schreiben_und_pruefen(&mut v, did, gesichert);
        if erg.is_err() {
            let t = erg.as_ref().err().cloned().unwrap_or_default();
            if crate::login::ist_zugriff_verweigert(&t) {
                erg = Err(match &anmeldung {
                    None => tr!("abgelehnt — Login nötig; keiner hinterlegt (ecu-werkbank login …)"),
                    Some(Err(le)) => tr!("abgelehnt — Login fehlgeschlagen: {le}", le = le),
                    Some(Ok(())) => tr!("abgelehnt trotz Login — Level/Bytecode passen evtl. nicht ({t})", t = t),
                });
            }
        }
        out.push(Geschrieben { did, vorher, ergebnis: erg });
    }
    Ok(out)
}

/// Unter welcher Kennung der Login gespeichert ist: TP-2.0-Adresse bzw. UDS-Anfrage-ID.
fn login_id(g: &Geraet) -> u16 {
    g.tp20.map(u16::from).unwrap_or(g.tx)
}

/// Sicherer Schreibtest: eine Kennung lesen und **mit genau diesem Inhalt**
/// wieder schreiben (ändert faktisch nichts) — prüft, ob der Schreibweg
/// funktioniert bzw. ob ein Login nötig ist. Schreibt nur, wenn `wirklich`.
pub fn schreibtest(be: &Backend, g: &Geraet, vehicle: &str, did: u16, wirklich: bool) -> Result<Geschrieben> {
    let login = crate::login::fuer(vehicle, login_id(g));
    let mut v = Verbindung::oeffnen(be, g)?;
    let jetzt = v.lesen(did).map_err(|e| anyhow::anyhow!(tr!("Lesen: {e}", e = format!("{e:#}"))))?;
    if !wirklich {
        return Ok(Geschrieben { did, vorher: jetzt, ergebnis: Ok(()) });
    }
    if let Err(e) = v.schreibsitzung() {
        return Ok(Geschrieben { did, vorher: jetzt, ergebnis: Err(format!("{e:#}")) });
    }
    let mut anmeldung: Option<Result<(), String>> = None;
    if let Some(m) = &login {
        anmeldung = Some(v.anmelden(m).map_err(|e| format!("{e:#}")));
    }
    let mut erg = schreiben_und_pruefen(&mut v, did, &jetzt);
    if erg.is_err() {
        let t = erg.as_ref().err().cloned().unwrap_or_default();
        if crate::login::ist_zugriff_verweigert(&t) {
            erg = Err(match &anmeldung {
                None => tr!("abgelehnt — Login nötig; keiner hinterlegt (ecu-werkbank login …)"),
                Some(Err(le)) => tr!("abgelehnt — Login fehlgeschlagen: {le}", le = le),
                Some(Ok(())) => tr!("abgelehnt trotz Login — Level/Bytecode passen evtl. nicht ({t})", t = t),
            });
        }
    }
    Ok(Geschrieben { did, vorher: jetzt, ergebnis: erg })
}

/// Die Werte vor dem Zurückschreiben als eigene Sicherung ablegen
/// (damit lässt sich das Zurückschreiben selbst rückgängig machen).
pub fn vorher_sichern(vehicle: &str, g: &Geraet, ergebnis: &[Geschrieben]) -> Result<Option<PathBuf>> {
    let kennungen: BTreeMap<String, String> = ergebnis
        .iter()
        .filter(|e| e.ergebnis.is_ok() && !e.vorher.is_empty() && g.kennungen.get(&format!("{:04X}", e.did)) != Some(&crate::backup::hex(&e.vorher)))
        .map(|e| (format!("{:04X}", e.did), crate::backup::hex(&e.vorher)))
        .collect();
    if kennungen.is_empty() {
        return Ok(None);
    }
    let s = Sicherung {
        created: crate::report::now_local(),
        timestamp: crate::report::unix_now(),
        vehicle: vehicle.to_string(),
        geraete: vec![Geraet { kennungen, ..g.clone() }],
        notiz: tr!("Werte vor dem Zurückschreiben ({a})", a = g.titel()),
    };
    // nicht über eine andere Sicherung derselben Sekunde schreiben
    let mut s = s;
    while crate::ablage::existiert(ordner(vehicle).join(format!("{}.toml", s.timestamp))) {
        s.timestamp += 1;
    }
    speichern(&s).map(Some)
}

/// Steuergerät in einer Sicherung finden: TP-2.0-Adresse (< 0x100) oder UDS-Request-ID.
pub fn geraet_finden(s: &Sicherung, id: u16) -> Option<&Geraet> {
    if id < 0x100 {
        s.geraete.iter().find(|g| g.tp20 == Some(id as u8))
    } else {
        s.geraete.iter().find(|g| g.tp20.is_none() && g.tx == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_sichern_und_vergleichen() {
        let _g = crate::history::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("vag-anp-{}", std::process::id()));
        std::env::set_var("VAG_DATA_DIR", &dir);
        let be = Backend::Demo;
        let _s = crate::sim::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // TP 2.0: im Golf-6-Modell geben die Geräte mit langer Codierung 0x0600 heraus
        // (nur ein kleiner Bereich — alle Bereiche dauern auch in der Simulation Minuten)
        crate::sim::set_modell(crate::sim::Modell::Golf6);
        let gefunden = crate::kwp::sweep_addrs(&be, &[0x01, 0x20], |_| {});
        let g6 = gefunden.map(|f| sichern_tp20(&be, &f, &[(0x05FE, 0x0601)], |_, _| {}));
        crate::sim::set_modell(crate::sim::Modell::Demo);
        let g6 = g6.unwrap();
        assert!(g6.iter().any(|g| g.tp20 == Some(0x20) && g.kennungen.contains_key("0600")), "{:?}", g6.iter().map(|g| g.titel()).collect::<Vec<_>>());
        crate::sim::reset();
        let mut a = sichern(&be, &LabelDb::default(), false, |_, _| {}).unwrap();
        assert!(!a.geraete.is_empty());
        a.vehicle = "TEST".into();
        speichern(&a).unwrap();
        // eine Kennung „ändern“
        let mut b = a.clone();
        b.timestamp += 1;
        let (k, v) = b.geraete[0].kennungen.iter().next().map(|(k, v)| (k.clone(), v.clone())).unwrap();
        b.geraete[0].kennungen.insert(k.clone(), if v == "FF" { "00".into() } else { "FF".into() });
        speichern(&b).unwrap();
        let l = liste("TEST");
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].1.timestamp, b.timestamp);
        let d = vergleichen(&a, &b);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].1.len(), 1);
        std::env::remove_var("VAG_DATA_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn demo_login_schreiben() {
        let _g = crate::history::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _s = crate::sim::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("vag-anp-login-{}", std::process::id()));
        std::env::set_var("VAG_DATA_DIR", &dir);
        crate::sim::reset();
        let be = Backend::Demo;
        let did = crate::sim::DEMO_LOGIN_DID;
        let k = format!("{did:04X}");
        let mut kennungen = BTreeMap::new();
        kennungen.insert(k.clone(), "ABCD".to_string()); // Auto steht auf 0000
        let g = Geraet { name: "Motor".into(), tx: 0x7E0, rx: 0x7E8, kennungen, tp20: None };
        // ohne Login: wird abgelehnt (Zugriff verweigert)
        let r = zurueckschreiben(&be, &g, "TEST", &[did]).unwrap();
        assert!(r[0].ergebnis.as_ref().err().unwrap().contains("Login"), "{r:?}");
        // mit hinterlegtem SA2-Login: klappt
        let bytecode = crate::backup::hex(&crate::sim::DEMO_SA2);
        crate::login::setzen("TEST", 0x7E0, crate::login::Methode::Sa2 { level: 3, bytecode }).unwrap();
        let r = zurueckschreiben(&be, &g, "TEST", &[did]).unwrap();
        assert_eq!(r[0].ergebnis, Ok(()), "{r:?}");
        let p = pruefen(&be, &g, "TEST", |_, _| {}).unwrap();
        assert_eq!(p[0].zustand, Zustand::Gleich);
        // falscher Bytecode: Login scheitert, Schreiben wird abgelehnt
        crate::sim::reset();
        crate::login::setzen("TEST", 0x7E0, crate::login::Methode::Sa2 { level: 3, bytecode: "68 02 4C".into() }).unwrap();
        let r = zurueckschreiben(&be, &g, "TEST", &[did]).unwrap();
        assert!(r[0].ergebnis.is_err(), "{r:?}");
        std::env::remove_var("VAG_DATA_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn demo_zurueckschreiben() {
        let _g = crate::history::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _s = crate::sim::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("vag-anp-zur-{}", std::process::id()));
        std::env::set_var("VAG_DATA_DIR", &dir);
        crate::sim::reset();
        let be = Backend::Demo;
        let did = crate::sim::DEMO_SERVICE_DID;
        let k = format!("{did:04X}");
        let mut kennungen = BTreeMap::new();
        kennungen.insert(k.clone(), "1234".to_string()); // gesichert: 0x1234
        kennungen.insert("2203".into(), "5ED0".into()); // km — gesperrt
        let g = Geraet { name: "Motor".into(), tx: 0x7E0, rx: 0x7E8, kennungen, tp20: None };
        let p = pruefen(&be, &g, "TEST", |_, _| {}).unwrap();
        assert_eq!(p.iter().find(|x| x.did == did).unwrap().zustand, Zustand::Anders);
        assert_eq!(p.iter().find(|x| x.did == 0x2203).unwrap().zustand, Zustand::Gesperrt);
        let r = zurueckschreiben(&be, &g, "TEST", &[did, 0x2203]).unwrap();
        assert_eq!(r[0].ergebnis, Ok(()), "{r:?}");
        assert!(r[1].ergebnis.is_err());
        // Kontrolle und Sicherung des alten Werts
        let p = pruefen(&be, &g, "TEST", |_, _| {}).unwrap();
        assert_eq!(p.iter().find(|x| x.did == did).unwrap().zustand, Zustand::Gleich);
        let vorher = vorher_sichern("TEST", &g, &r).unwrap().expect("alter Wert gesichert");
        let alt = laden(&vorher).unwrap();
        assert!(!alt.vollstaendig());
        assert_eq!(alt.geraete[0].kennungen.len(), 1);
        assert_ne!(alt.geraete[0].kennungen[&k], "1234");
        // falsche Länge wird nicht geschrieben
        let mut g2 = g.clone();
        g2.kennungen.insert(k.clone(), "123456".into());
        let r = zurueckschreiben(&be, &g2, "TEST", &[did]).unwrap();
        assert!(r[0].ergebnis.is_err());
        // TP 2.0: Codierung (0x0600) zurückschreiben
        crate::sim::set_modell(crate::sim::Modell::Golf6);
        let gefunden = crate::kwp::sweep_addrs(&be, &[0x20], |_| {}).unwrap();
        let mut tp = sichern_tp20(&be, &gefunden, &[(0x0600, 0x0600)], |_, _| {});
        let t = tp.pop().expect("0x20 gesichert");
        let orig = t.kennungen["0600"].clone();
        let mut t2 = t.clone();
        let mut b = crate::backup::parse_hex(&orig).unwrap();
        b[0] ^= 0x01;
        t2.kennungen.insert("0600".into(), crate::backup::hex(&b));
        let r = zurueckschreiben(&be, &t2, "TEST", &[0x0600]);
        let r2 = zurueckschreiben(&be, &t, "TEST", &[0x0600]); // wieder zurück
        crate::sim::set_modell(crate::sim::Modell::Demo);
        assert_eq!(r.unwrap()[0].ergebnis, Ok(()));
        assert_eq!(r2.unwrap()[0].ergebnis, Ok(()));
        crate::sim::reset();
        std::env::remove_var("VAG_DATA_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
