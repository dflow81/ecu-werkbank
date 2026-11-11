//! Genauer Kilometerstand.
//!
//! Das Golf-6-Kombi meldet über Kennung 0x2203 nur 10-km-Schritte. Wo der
//! km-genaue Stand liegt, ist nicht veröffentlicht und je Kombi anders. Darum
//! wird die Stelle einmal gesucht: Tachostand eingeben, ecu-werkbank liest die
//! Kombi-Kennungen (nur lesen) und sucht genau diese Zahl. Die gefundene Stelle
//! wird im Fahrzeugprofil gemerkt; danach liest jeder Scan den genauen Stand.
//!
//! Jeder Wert wird mit dem 10-km-Stand aus 0x2203 gegengeprüft. Passt er nicht,
//! wird er nicht verwendet.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::tr;
use crate::transport::Backend;
use crate::uds::Uds;

/// Wo der genaue Kilometerstand steht.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct KmQuelle {
    pub tx: u16,
    pub rx: u16,
    pub did: u16,
    /// Erstes Byte in der Antwort
    pub offset: usize,
    /// Länge in Byte (2–4)
    pub laenge: usize,
    /// Byte-Reihenfolge: niederwertiges Byte zuerst
    #[serde(default)]
    pub le: bool,
    /// 1 = km, 10 = 0,1-km-Schritte
    #[serde(default = "eins")]
    pub teiler: u32,
}

fn eins() -> u32 {
    1
}

/// VAG-übliche Kennung „Kilometerstand“ (3 Byte, km), die in den
/// Umgebungsdaten der UDS-Fehler steht. Ob das Kombi sie auch direkt
/// liefert, ist am Golf 6 noch nicht geprüft — darum nur mit Gegenprobe.
pub const VAG_KM_DID: u16 = 0x2002;

/// Gehört diese Kennung zum Kilometerstand? Dann wird sie **nie** geschrieben —
/// von keinem Befehl (Tachomanipulation ist strafbar, § 22b StVG). Geprüft
/// werden die bekannten VAG-Kennungen und jede km-Stelle, die in einem
/// Fahrzeugprofil gelernt wurde (gleiche Kennung am gleichen Steuergerät).
pub fn gesperrt(did: u16, tx: u16) -> bool {
    did == VAG_KM_DID
        || did == crate::report::KOMBI_KM_DID
        || crate::vehicle::list().iter().any(|v| {
            crate::profil::load(&v.vin).and_then(|p| p.km_quelle).is_some_and(|q| q.did == did && q.tx == tx)
        })
}

/// Fehler, wenn die Kennung zum Kilometerstand gehört (vor jedem Schreiben aufrufen).
pub fn schreiben_erlaubt(did: u16, tx: u16) -> Result<()> {
    if gesperrt(did, tx) {
        anyhow::bail!(tr!("Kennung {did} gehört zum Kilometerstand — ECUWerkbank schreibt den Kilometerstand nie.", did = format!("{did:#06X}")));
    }
    Ok(())
}

/// Bereiche im Kombi, die bei der Suche gelesen werden (nur lesen).
pub const SUCH_BEREICHE: [(u16, u16); 3] = [(0x2000, 0x23FF), (0x0200, 0x02FF), (0x1000, 0x10FF)];

impl KmQuelle {
    pub fn wert(&self, d: &[u8]) -> Option<u32> {
        let w = d.get(self.offset..self.offset + self.laenge)?;
        if !(2..=4).contains(&self.laenge) {
            return None;
        }
        let v = if self.le { w.iter().rev().fold(0u64, |a, &b| (a << 8) | u64::from(b)) } else { w.iter().fold(0u64, |a, &b| (a << 8) | u64::from(b)) };
        u32::try_from(v / u64::from(self.teiler.max(1))).ok()
    }

    pub fn beschreibung(&self) -> String {
        tr!(
            "Kennung {did} am Steuergerät {tx}, Byte {o}, {n} Byte{le}{t}",
            did = format!("{:#06X}", self.did),
            tx = format!("{:#05x}", self.tx),
            o = self.offset,
            n = self.laenge,
            le = if self.le { " (LE)" } else { "" },
            t = if self.teiler == 10 { crate::trs!(", 0,1-km-Schritte") } else { "" }
        )
    }
}

/// Stellen in `d`, an denen genau `tacho` km stehen.
pub fn kandidaten(tx: u16, rx: u16, did: u16, d: &[u8], tacho: u32) -> Vec<KmQuelle> {
    let mut out = Vec::new();
    for laenge in [3usize, 4, 2] {
        for offset in 0..d.len().saturating_sub(laenge - 1) {
            for le in [false, true] {
                for teiler in [1u32, 10] {
                    let q = KmQuelle { tx, rx, did, offset, laenge, le, teiler };
                    if q.wert(d) == Some(tacho) && !out.iter().any(|x: &KmQuelle| x.offset == offset && x.teiler == teiler) {
                        out.push(q);
                    }
                }
            }
        }
    }
    out
}

/// Lässt sich mit diesem Tachostand eine km-genaue Stelle lernen?
/// Nicht bei einer 0 am Ende: dann passt auch jeder 10-km-Wert.
pub fn kann_lernen(tacho: u32) -> bool {
    !tacho.is_multiple_of(10)
}

/// Passt ein genauer Wert zum 10-km-Stand aus 0x2203?
pub fn passt(genau: u32, km10: Option<u32>) -> bool {
    match km10 {
        Some(k) => genau >= k && genau - k < 10,
        None => genau > 0,
    }
}

/// Genauen Stand lesen: erst die gelernte Stelle, sonst die VAG-übliche Kennung
/// 0x2002 im Kombi — beides nur, wenn es zum 10-km-Stand passt.
pub fn genau(be: &Backend, gelernt: Option<&KmQuelle>, kombi: (u16, u16), km10: Option<u32>) -> Option<(u32, KmQuelle)> {
    let standard = KmQuelle { tx: kombi.0, rx: kombi.1, did: VAG_KM_DID, offset: 0, laenge: 3, le: false, teiler: 1 };
    let mut versuche: Vec<KmQuelle> = gelernt.copied().into_iter().collect();
    // ohne 10-km-Gegenprobe nur die gelernte Stelle
    if km10.is_some() && gelernt != Some(&standard) {
        versuche.push(standard);
    }
    for q in versuche {
        let Ok(tp) = be.open(q.tx, q.rx) else { continue };
        let uds = Uds::new(&*tp);
        let Ok(d) = uds.read_data_by_id(q.did) else { continue };
        if let Some(v) = q.wert(&d).filter(|&v| passt(v, km10)) {
            return Some((v, q));
        }
    }
    None
}

/// Ergebnis der Suche.
#[derive(Debug, Clone)]
pub struct Suche {
    pub gelesen: usize,
    pub km10: Option<u32>,
    pub treffer: Vec<(KmQuelle, Vec<u8>)>,
}

impl Suche {
    /// Die eine sichere Stelle (genau ein Treffer bzw. alle Treffer in derselben Kennung,
    /// dann der erste) — sonst None.
    pub fn eindeutig(&self) -> Option<KmQuelle> {
        let did = self.treffer.first()?.0.did;
        self.treffer.iter().all(|(q, _)| q.did == did).then(|| self.treffer[0].0)
    }
}

/// Kombi nach dem Tachostand durchsuchen (nur lesen, je nach Kombi 10–60 s).
pub fn suchen(be: &Backend, kombi: (u16, u16), tacho: u32, mut fortschritt: impl FnMut(u16, usize)) -> Result<Suche> {
    let tp = be.open(kombi.0, kombi.1)?;
    let uds = Uds::new(&*tp);
    uds.diagnostic_session(0x01).context(tr!("Kombi antwortet nicht (Zündung an?)"))?;
    let km10 = uds.read_data_by_id(crate::report::KOMBI_KM_DID).ok().as_deref().and_then(crate::report::kombi_km_aus);
    let mut s = Suche { gelesen: 0, km10, treffer: Vec::new() };
    for (von, bis) in SUCH_BEREICHE {
        for did in von..=bis {
            if (did - von).is_multiple_of(64) {
                fortschritt(did, s.gelesen);
                let _ = uds.tester_present();
            }
            let Ok(d) = uds.read_data_by_id(did) else { continue };
            s.gelesen += 1;
            for q in kandidaten(kombi.0, kombi.1, did, &d, tacho) {
                s.treffer.push((q, d.clone()));
            }
        }
    }
    Ok(s)
}

/// Gefundene Stelle im Profil des Fahrzeugs merken.
pub fn merken(vin: &str, q: KmQuelle) -> bool {
    let mut p = crate::profil::load(vin).unwrap_or_default();
    p.km_quelle = Some(q);
    crate::profil::save(vin, &p)
}

/// Kombi des Fahrzeugs (UDS): aus dem Profil, sonst Golf 6 / PQ35 (0x714/0x77E).
pub fn kombi(vin: Option<&str>) -> (u16, u16) {
    vin.and_then(crate::profil::load).and_then(|p| p.uds.iter().copied().find(|&(tx, _)| tx == 0x714)).unwrap_or((0x714, 0x77E))
}

/// Ergebnis für Anzeige und Speichern.
#[derive(Debug, Clone, Default)]
pub struct Ergebnis {
    pub zeilen: Vec<String>,
    /// genauer Stand (oder None)
    pub km: Option<u32>,
    /// Stelle neu im Profil gemerkt
    pub gemerkt: bool,
    /// Fahrgestellnummer des gelesenen Autos
    pub vin: Option<String>,
}

impl Ergebnis {
    /// Gehört das Ergebnis zu diesem Fahrzeug (gleiche VIN bzw. keine bekannt)?
    pub fn passt_zu(&self, v: &crate::vehicle::Vehicle) -> bool {
        match &self.vin {
            Some(vin) => v.vin.is_empty() || v.vin.eq_ignore_ascii_case(vin),
            None => true,
        }
    }
}

/// Genauen Kilometerstand ermitteln. Mit `tacho` wird die Stelle gesucht und
/// (wenn eindeutig) im Profil gemerkt, ohne nur gelesen. Liest nur.
pub fn ermitteln(be: &Backend, tacho: Option<u32>, fortschritt: impl FnMut(u16, usize)) -> Result<Ergebnis> {
    let vin = crate::profil::quick_vin(be);
    let k = kombi(vin.as_deref());
    let gelernt = vin.as_deref().and_then(crate::profil::load).and_then(|p| p.km_quelle);
    let mut e = Ergebnis { vin: vin.clone(), ..Default::default() };
    let Some(t) = tacho else {
        let km10 = crate::report::uds_kombi_odometer(be, k.0, k.1);
        match (genau(be, gelernt.as_ref(), k, km10), km10) {
            (Some((v, q)), _) => {
                e.km = Some(v);
                e.zeilen.push(tr!("Kilometerstand: {a} km (genau)", a = crate::report::group_thousands(v)));
                e.zeilen.push(tr!("Quelle: {a}", a = q.beschreibung()));
            }
            (None, Some(k10)) => {
                e.zeilen.push(tr!("Kilometerstand: {a} km (nur 10-km-Schritte)", a = crate::report::group_thousands(k10)));
                e.zeilen.push(tr!("Für den genauen Stand einmal mit dem Tachostand suchen (--tacho)."));
            }
            (None, None) => e.zeilen.push(tr!("Kilometerstand nicht lesbar (Kombi antwortet nicht — Zündung an?)")),
        }
        return Ok(e);
    };
    let s = suchen(be, k, t, fortschritt)?;
    e.zeilen.push(tr!("{a} Kennungen im Kombi gelesen, {b} Treffer für {c} km.", a = s.gelesen, b = s.treffer.len(), c = crate::report::group_thousands(t)));
    if let Some(k10) = s.km10 {
        e.zeilen.push(tr!("Zum Vergleich 0x2203: {a} km (10-km-Schritte)", a = crate::report::group_thousands(k10)));
        if !passt(t, Some(k10)) {
            e.zeilen.push(tr!("Achtung: Tachostand passt nicht zu 0x2203 — richtig abgelesen? Auto seither bewegt?"));
        }
    }
    for (q, d) in &s.treffer {
        e.zeilen.push(tr!("  {a}   [roh {b}]", a = q.beschreibung(), b = d.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ")));
    }
    // Endet der Stand auf 0, passt auch ein Wert, der nur in 10-km-Schritten zählt —
    // dann ist nicht zu unterscheiden, ob die Stelle wirklich km-genau ist.
    if !kann_lernen(t) && !s.treffer.is_empty() {
        e.zeilen.push(tr!("Der Tachostand endet auf 0 — damit lässt sich ein genauer Wert nicht von einem 10-km-Wert unterscheiden. Nichts gemerkt. Bitte suchen, wenn die letzte Ziffer nicht 0 ist (ein paar km fahren)."));
        return Ok(e);
    }
    match (s.eindeutig(), s.treffer.is_empty()) {
        (Some(q), _) => {
            e.km = Some(t);
            e.gemerkt = vin.as_deref().is_some_and(|v| merken(v, q));
            e.zeilen.push(if e.gemerkt {
                tr!("Stelle gemerkt — ab jetzt zeigt jeder Scan den genauen Stand.")
            } else {
                tr!("Stelle gefunden, aber nicht gemerkt (Fahrzeug unbekannt — erst einen Scan machen bzw. Fahrzeug anlegen).")
            });
        }
        (None, true) => e.zeilen.push(tr!("Keine Kennung mit genau diesem Stand. Das Kombi gibt den genauen Stand vermutlich nicht über die Diagnose heraus — 0x2203 (10-km-Schritte) bleibt die Quelle.")),
        (None, false) => e.zeilen.push(tr!("Mehrere Kennungen passen — nicht eindeutig, nichts gemerkt. Nach ein paar gefahrenen km mit dem neuen Tachostand noch einmal suchen.")),
    }
    Ok(e)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stellen_finden() {
        // 242 553 km = 0x03B379
        let d = [0x11, 0x03, 0xB3, 0x79, 0x00];
        let k = kandidaten(1, 2, 0x2100, &d, 242_553);
        assert_eq!(k[0].offset, 1);
        assert_eq!(k[0].laenge, 3);
        assert_eq!(k[0].wert(&d), Some(242_553));
        // 0,1-km-Schritte: 2 425 534 = 0x2502BE
        let d = [0x25, 0x02, 0xBE];
        let k = kandidaten(1, 2, 0x2100, &d, 242_553);
        assert!(k.iter().any(|q| q.teiler == 10));
        assert!(passt(242_553, Some(242_550)));
        assert!(!passt(242_563, Some(242_550)));
        assert!(!passt(242_549, Some(242_550)));
    }

    #[test]
    fn golf6_genauer_stand() {
        let _g = crate::sim::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::sim::set_modell(crate::sim::Modell::Golf6);
        crate::sim::reset();
        crate::sim::set_km_genau(true);
        let be = Backend::Demo;
        let kombi = (0x714, 0x77E);
        let tacho = crate::sim::odometer_km();
        let s = suchen(&be, kombi, tacho, |_, _| {}).unwrap();
        crate::sim::set_km_genau(false);
        assert_eq!(s.km10, Some(tacho / 10 * 10));
        let q = s.eindeutig().expect("eine Stelle");
        crate::sim::set_km_genau(true);
        assert_eq!(genau(&be, Some(&q), kombi, s.km10).map(|x| x.0), Some(tacho));
        // auch ohne gelernte Stelle über 0x2002, mit Gegenprobe
        assert_eq!(genau(&be, None, kombi, s.km10).map(|x| x.0), Some(tacho));
        // ohne Gegenprobe wird die ungelernte Kennung nicht benutzt
        assert_eq!(genau(&be, None, kombi, None), None);
        crate::sim::set_km_genau(false);
        assert_eq!(genau(&be, None, kombi, s.km10), None);
    }

    #[test]
    fn kilometerstand_nie_schreiben() {
        assert!(gesperrt(VAG_KM_DID, 0x714));
        assert!(gesperrt(crate::report::KOMBI_KM_DID, 0x714));
        assert!(schreiben_erlaubt(0x2203, 0x714).is_err());
        // normale Codierung bleibt erlaubt
        assert!(schreiben_erlaubt(0x0600, 0x714).is_ok());
    }

    #[test]
    fn null_am_ende_wird_nicht_gelernt() {
        assert!(!kann_lernen(242_550));
        assert!(kann_lernen(242_553));
    }
}
