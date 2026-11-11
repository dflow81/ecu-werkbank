//! Grundeinstellung an TP-2.0/KWP-Steuergeräten (wie VCDS „04 – Grundeinstellung“).
//!
//! Welche Anfragen VAG dafür über TP 2.0 schickt, ist nicht verlässlich
//! veröffentlicht. Darum rät ecu-werkbank hier nichts: Der Ablauf wird aus einem
//! **eigenen Mitschnitt** gelernt — VCDS macht am eigenen Auto eine
//! Grundeinstellung, ecu-werkbank hört über das CAN-Interface mit. Gelernt werden
//! die Anfragen vor dem Ablesen (Start), die wiederholte Abfrage der Werte und
//! die Anfragen danach (Aus). Später wird nur die Gruppennummer ausgetauscht.
//!
//! Gesperrt, solange es für das Steuergerät des **gewählten Fahrzeugs** keinen
//! gelernten Ablauf gibt (`<Fahrzeugordner>/grundeinstellung.toml`). Ein am
//! Golf gelernter Ablauf gilt also nie für ein anderes Auto. Übernommen wird
//! nur, was das Steuergerät im Mitschnitt positiv beantwortet hat.

use crate::canlog::{Message, Proto};
use crate::tr;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Eine Anfrage; `gruppe_an` = Stelle des Gruppen-Bytes (wird ersetzt).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Anfrage {
    pub bytes: Vec<u8>,
    #[serde(default)]
    pub gruppe_an: Option<usize>,
}

impl Anfrage {
    /// Die Anfrage für Gruppe `g`.
    pub fn mit_gruppe(&self, g: u8) -> Vec<u8> {
        let mut b = self.bytes.clone();
        if let Some(i) = self.gruppe_an {
            if let Some(x) = b.get_mut(i) {
                *x = g;
            }
        }
        b
    }

    pub fn hex(&self) -> String {
        self.bytes
            .iter()
            .enumerate()
            .map(|(i, b)| if Some(i) == self.gruppe_an { "<Gr>".to_string() } else { format!("{b:02X}") })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Gelernter Ablauf für ein Steuergerät.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ablauf {
    pub adresse: u8,
    /// Gruppe, die im Mitschnitt benutzt wurde
    pub gruppe_gelernt: u8,
    pub start: Vec<Anfrage>,
    pub lesen: Anfrage,
    #[serde(default)]
    pub stop: Vec<Anfrage>,
    /// Mitschnitt-Datei, aus der gelernt wurde
    pub quelle: String,
    pub datum: String,
}

impl Ablauf {
    /// Kurzbeschreibung für Anzeige und Rückfrage.
    pub fn beschreibung(&self) -> Vec<String> {
        let mut z = Vec::new();
        for a in &self.start {
            z.push(tr!("Start: {a}", a = a.hex()));
        }
        z.push(tr!("Werte lesen: {a}", a = self.lesen.hex()));
        for a in &self.stop {
            z.push(tr!("Aus: {a}", a = a.hex()));
        }
        z
    }
}

/// Anfragen, die mit der Grundeinstellung nichts zu tun haben (Wach halten,
/// Identifikation, Fehlerspeicher).
fn nebensache(sid: u8) -> bool {
    matches!(sid, 0x3E | 0x1A | 0x14 | 0x17 | 0x18)
}

/// Positive Antwort auf eine Anfrage mit `sid`?
fn positiv(sid: u8, antwort: &[u8]) -> bool {
    antwort.first() == Some(&sid.wrapping_add(0x40))
}

/// Ablauf aus den Nachrichten eines Mitschnitts lernen. `gruppe` = die Gruppe,
/// die in VCDS benutzt wurde (muss in der Werte-Abfrage vorkommen).
pub fn lernen(msgs: &[Message], adresse: u8, gruppe: u8, quelle: &str) -> Result<Ablauf> {
    // Anfrage → Antwort paaren (nur dieses Steuergerät, KWP; 0x7F 0x78 = „Antwort folgt“ überspringen)
    let mut paare: Vec<(Vec<u8>, Option<Vec<u8>>)> = Vec::new();
    for m in msgs.iter().filter(|m| m.proto == Proto::Kwp && m.address == Some(adresse) && !m.data.is_empty()) {
        if m.from_tester {
            paare.push((m.data.clone(), None));
        } else if m.data.first() == Some(&0x7F) && m.data.get(2) == Some(&0x78) {
            continue;
        } else if let Some(p) = paare.last_mut().filter(|p| p.1.is_none()) {
            p.1 = Some(m.data.clone());
        }
    }
    let paare: Vec<_> = paare.into_iter().filter(|(a, _)| !nebensache(a[0])).collect();
    if paare.is_empty() {
        bail!(tr!("Im Mitschnitt keine Anfragen an Steuergerät {a} gefunden.", a = format!("{adresse:02X}")));
    }

    // Werte-Abfrage: die häufigste Anfrage, die die Gruppe enthält
    let mut zaehl: BTreeMap<Vec<u8>, usize> = BTreeMap::new();
    for (a, _) in &paare {
        if a.iter().skip(1).any(|&b| b == gruppe) {
            *zaehl.entry(a.clone()).or_default() += 1;
        }
    }
    let Some((lesen, n)) = zaehl.into_iter().max_by_key(|(_, n)| *n) else {
        bail!(tr!("Gruppe {g} kommt im Mitschnitt nicht vor — richtige Gruppe angegeben?", g = gruppe));
    };
    if n < 2 {
        bail!(tr!("Die Werte von Gruppe {g} wurden nur einmal abgefragt — bitte die Grundeinstellung im Diagnoseprogramm ein paar Sekunden laufen lassen.", g = gruppe));
    }
    let erste = paare.iter().position(|(a, _)| *a == lesen).expect("kommt vor");
    let letzte = paare.iter().rposition(|(a, _)| *a == lesen).expect("kommt vor");

    // Start: ab der letzten Sitzungs-Anfrage (0x10) vor der ersten Abfrage
    let von = paare[..erste].iter().rposition(|(a, _)| a[0] == 0x10).unwrap_or(0);
    let mut start = Vec::new();
    for (a, r) in &paare[von..erste] {
        if a[0] == 0x21 {
            continue; // normales Messwert-Lesen davor (VCDS zeigt erst die Messwerte)
        }
        match r {
            Some(r) if positiv(a[0], r) => {}
            Some(r) => bail!(tr!("Das Steuergerät hat {a} im Mitschnitt abgelehnt ({b}) — so nicht übernommen.", a = hex(a), b = hex(r))),
            None => bail!(tr!("Auf {a} kam im Mitschnitt keine Antwort — so nicht übernommen.", a = hex(a))),
        }
        if !start.iter().any(|x: &Anfrage| x.bytes == *a) {
            start.push(anfrage(a, gruppe));
        }
    }
    // Ohne eigene Start-Anfrage wäre es nur normales Messwert-Lesen
    if lesen[0] == 0x21 && start.iter().all(|a| a.bytes[0] == 0x10) {
        bail!(tr!("Im Mitschnitt ist keine Grundeinstellung zu erkennen — nur normales Messwert-Lesen."));
    }
    let stop: Vec<Anfrage> = paare[letzte + 1..]
        .iter()
        .filter(|(a, r)| a[0] != 0x21 && r.as_deref().is_some_and(|r| positiv(a[0], r)))
        .map(|(a, _)| anfrage(a, gruppe))
        .fold(Vec::new(), |mut v, a| {
            if !v.contains(&a) {
                v.push(a);
            }
            v
        });

    Ok(Ablauf {
        adresse,
        gruppe_gelernt: gruppe,
        start,
        lesen: anfrage(&lesen, gruppe),
        stop,
        quelle: quelle.to_string(),
        datum: crate::report::now_local(),
    })
}

/// Anfrage mit markierter Gruppenstelle (erstes Vorkommen hinter dem Dienst).
fn anfrage(bytes: &[u8], gruppe: u8) -> Anfrage {
    let gruppe_an = bytes.iter().skip(1).position(|&b| b == gruppe).map(|i| i + 1);
    Anfrage { bytes: bytes.to_vec(), gruppe_an }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02X}")).collect::<Vec<_>>().join(" ")
}

// --- Ablage im Fahrzeugordner ---

fn datei(fahrzeug: &str) -> std::path::PathBuf {
    crate::vehicle::dir(fahrzeug).join("grundeinstellung.toml")
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Datei {
    #[serde(default)]
    ablauf: Vec<Ablauf>,
}

fn alle(fahrzeug: &str) -> Vec<Ablauf> {
    std::fs::read_to_string(datei(fahrzeug)).ok().and_then(|t| toml::from_str::<Datei>(&t).ok()).map(|d| d.ablauf).unwrap_or_default()
}

/// Gelernter Ablauf für dieses Steuergerät dieses Fahrzeugs — `None` = gesperrt.
pub fn ablauf(fahrzeug: &str, adresse: u8) -> Option<Ablauf> {
    alle(fahrzeug).into_iter().find(|a| a.adresse == adresse)
}

/// Ablauf merken (ersetzt einen alten für dieselbe Adresse).
pub fn merken(fahrzeug: &str, a: &Ablauf) -> Result<()> {
    let mut v = alle(fahrzeug);
    v.retain(|x| x.adresse != a.adresse);
    v.push(a.clone());
    v.sort_by_key(|x| x.adresse);
    let p = datei(fahrzeug);
    std::fs::create_dir_all(p.parent().expect("hat Eltern"))?;
    std::fs::write(&p, toml::to_string(&Datei { ablauf: v })?).with_context(|| tr!("Schreiben: {a}", a = p.display()))
}

/// Ablauf vergessen (Steuergerät wieder sperren).
pub fn vergessen(fahrzeug: &str, adresse: u8) -> Result<()> {
    let mut v = alle(fahrzeug);
    v.retain(|x| x.adresse != adresse);
    let p = datei(fahrzeug);
    std::fs::write(&p, toml::to_string(&Datei { ablauf: v })?).with_context(|| tr!("Schreiben: {a}", a = p.display()))
}

// --- Ausführen ---

/// Meldungen aus dem laufenden Ablauf.
#[derive(Debug, Clone)]
pub enum Meldung {
    /// Start-Anfragen sind durch, Grundeinstellung läuft
    Laeuft,
    /// Aktuelle Werte (Text je Feld) bzw. Rohdaten
    Werte(Vec<String>),
    /// Beendet (mit Hinweis, z. B. ob „Aus“ bestätigt wurde)
    Fertig(String),
    Fehler(String),
}

/// Antwort auf die Werte-Abfrage lesbar machen. Bei `21 <Gr>` sind es normale
/// Messwertblöcke; sonst wird nach dem Gruppen-Echo dasselbe Format versucht.
pub fn werte_text(antwort: &[u8], gruppe: u8) -> Vec<String> {
    let rest = match antwort.first() {
        Some(&g) if g == gruppe => &antwort[1..],
        _ => antwort,
    };
    if !rest.is_empty() && rest.len() % 3 == 0 {
        let v = crate::kwp::parse_block(rest);
        if v.iter().any(|x| x.decode().is_some()) {
            return v.iter().filter(|x| !x.is_leer()).map(|x| x.display()).collect();
        }
    }
    vec![tr!("roh: {a}", a = hex(antwort))]
}

/// Grundeinstellung ausführen, bis `stop` gesetzt wird. Nur nach Rückfrage aufrufen.
pub fn ausfuehren(
    be: &crate::transport::Backend,
    a: &Ablauf,
    gruppe: u8,
    stop: &std::sync::atomic::AtomicBool,
    mut melden: impl FnMut(Meldung),
) -> Result<()> {
    use std::sync::atomic::Ordering;
    let mut ch = crate::kwp::open_session(be, a.adresse)?;
    let mut k = crate::kwp::Kwp::new(&mut ch);
    let senden = |k: &mut crate::kwp::Kwp, b: &[u8]| k.request(b[0], &b[1..]);
    for s in &a.start {
        let b = s.mit_gruppe(gruppe);
        senden(&mut k, &b).with_context(|| tr!("Start {a}", a = hex(&b)))?;
    }
    melden(Meldung::Laeuft);
    let lesen = a.lesen.mit_gruppe(gruppe);
    let mut fehler = None;
    while !stop.load(Ordering::Relaxed) {
        match senden(&mut k, &lesen) {
            Ok(r) => melden(Meldung::Werte(werte_text(&r, gruppe))),
            Err(e) => {
                fehler = Some(format!("{e:#}"));
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    // Immer versuchen auszuschalten — auch nach einem Lesefehler
    let mut aus_ok = true;
    for s in &a.stop {
        if senden(&mut k, &s.mit_gruppe(gruppe)).is_err() {
            aus_ok = false;
        }
    }
    let hinweis = match (a.stop.is_empty(), aus_ok) {
        (true, _) => tr!("Beendet. (Im Mitschnitt gab es keine eigene Aus-Anfrage — das Steuergerät beendet die Grundeinstellung mit dem Schließen des Kanals.)"),
        (false, true) => tr!("Beendet, Aus bestätigt."),
        (false, false) => tr!("Beendet, aber die Aus-Anfrage wurde nicht bestätigt — Zündung einmal aus und wieder an."),
    };
    match fehler {
        Some(e) => melden(Meldung::Fehler(tr!("{e} — {h}", e = e, h = hinweis))),
        None => melden(Meldung::Fertig(hinweis)),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Synthetischer Mitschnitt: Bytes frei erfunden, nur um die Lern-Logik zu
    // prüfen — NICHT der echte VAG-Ablauf (der kommt aus dem eigenen Mitschnitt).
    fn m(tester: bool, data: &[u8]) -> Message {
        Message { t: None, id: 0x300, proto: Proto::Kwp, from_tester: tester, address: Some(0x01), data: data.to_vec() }
    }

    fn mitschnitt() -> Vec<Message> {
        vec![
            m(true, &[0x10, 0x89]),
            m(false, &[0x50, 0x89]),
            m(true, &[0x21, 0x3C]), // erst normales Messwert-Lesen
            m(false, &[0x61, 0x3C, 0x01, 0xC8, 0x14]),
            m(true, &[0x3E]),
            m(false, &[0x7E]),
            m(true, &[0xA1, 0x3C]), // „Start“
            m(false, &[0x7F, 0xA1, 0x78]),
            m(false, &[0xE1, 0x3C]),
            m(true, &[0xA2, 0x3C]), // „Werte“
            m(false, &[0xE2, 0x3C, 0x01, 0xC8, 0x14]),
            m(true, &[0xA2, 0x3C]),
            m(false, &[0xE2, 0x3C, 0x01, 0xC8, 0x15]),
            m(true, &[0xA2, 0x3C]),
            m(false, &[0xE2, 0x3C, 0x01, 0xC8, 0x16]),
            m(true, &[0xA3, 0x3C]), // „Aus“
            m(false, &[0xE3, 0x3C]),
        ]
    }

    #[test]
    fn ablauf_aus_mitschnitt() {
        let a = lernen(&mitschnitt(), 0x01, 0x3C, "test.log").unwrap();
        assert_eq!(a.start.iter().map(|x| x.bytes.clone()).collect::<Vec<_>>(), vec![vec![0x10, 0x89], vec![0xA1, 0x3C]]);
        assert_eq!(a.lesen.bytes, vec![0xA2, 0x3C]);
        assert_eq!(a.stop.len(), 1);
        // Gruppe wird ausgetauscht, Sitzung bleibt
        assert_eq!(a.start[0].mit_gruppe(0x3C), vec![0x10, 0x89]);
        assert_eq!(a.start[1].mit_gruppe(98), vec![0xA1, 98]);
        assert_eq!(a.lesen.hex(), "A2 <Gr>");
        // Werte: Gruppen-Echo weg, Rest als Messwertblock
        assert_eq!(werte_text(&[0x3C, 0x01, 0xC8, 0x14], 0x3C), vec!["800.00 1/min".to_string()]);
    }

    #[test]
    fn lehnt_unsichere_mitschnitte_ab() {
        // falsche Gruppe
        assert!(lernen(&mitschnitt(), 0x01, 0x05, "x").is_err());
        // anderes Steuergerät
        assert!(lernen(&mitschnitt(), 0x03, 0x3C, "x").is_err());
        // Start wurde abgelehnt
        let mut v = mitschnitt();
        v[8] = m(false, &[0x7F, 0xA1, 0x31]);
        assert!(lernen(&v, 0x01, 0x3C, "x").is_err());
        // nur normales Messwert-Lesen, keine Grundeinstellung
        let nur_lesen = vec![
            m(true, &[0x10, 0x89]),
            m(false, &[0x50, 0x89]),
            m(true, &[0x21, 0x3C]),
            m(false, &[0x61, 0x3C, 0x01, 0xC8, 0x14]),
            m(true, &[0x21, 0x3C]),
            m(false, &[0x61, 0x3C, 0x01, 0xC8, 0x14]),
        ];
        assert!(lernen(&nur_lesen, 0x01, 0x3C, "x").is_err());
    }

    #[test]
    fn ausfuehren_mit_demo() {
        // Ablauf von Hand (Demo-Steuergerät kennt nur normales Messwert-Lesen):
        // prüft Start → Werte → Aus-Schalter → Ende, nicht den echten VAG-Ablauf.
        let _g = crate::sim::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::sim::set_modell(crate::sim::Modell::Golf6);
        crate::sim::reset();
        let a = Ablauf {
            adresse: 0x01,
            gruppe_gelernt: 1,
            start: vec![Anfrage { bytes: vec![0x10, 0x89], gruppe_an: None }],
            lesen: Anfrage { bytes: vec![0x21, 0x01], gruppe_an: Some(1) },
            stop: vec![],
            quelle: "hand".into(),
            datum: String::new(),
        };
        let stop = std::sync::atomic::AtomicBool::new(false);
        let mut meldungen = Vec::new();
        ausfuehren(&crate::transport::Backend::Demo, &a, 1, &stop, |m| {
            if matches!(m, Meldung::Werte(_)) {
                stop.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            meldungen.push(m);
        })
        .unwrap();
        assert!(matches!(meldungen[0], Meldung::Laeuft));
        assert!(matches!(&meldungen[1], Meldung::Werte(w) if !w.is_empty()), "{meldungen:?}");
        assert!(matches!(meldungen.last(), Some(Meldung::Fertig(_))), "{meldungen:?}");
    }

    #[test]
    fn ablage_je_fahrzeug() {
        let _g = crate::history::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("vag-ge-{}", std::process::id()));
        std::env::set_var("VAG_DATA_DIR", &dir);
        let a = lernen(&mitschnitt(), 0x01, 0x3C, "test.log").unwrap();
        assert!(ablauf("GOLF", 0x01).is_none()); // gesperrt
        merken("GOLF", &a).unwrap();
        assert_eq!(ablauf("GOLF", 0x01).unwrap().lesen, a.lesen);
        assert!(ablauf("ANDERES", 0x01).is_none()); // gilt nicht für ein anderes Auto
        vergessen("GOLF", 0x01).unwrap();
        assert!(ablauf("GOLF", 0x01).is_none());
        std::env::remove_var("VAG_DATA_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
