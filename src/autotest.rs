//! Testlauf am echten Fahrzeug: fragt in einem Durchgang alles Wichtige ab —
//! NUR LESEND — und schreibt ein Protokoll (mit Rohdaten) zum Auswerten.
//!
//! 1. Verbindung (Adapter, Bitrate, CAN-Zustand)
//! 2. Fahrgestellnummer
//! 3. Voller Scan aller Steuergeräte mit Fehlerspeicher
//! 4. Messwertblöcke des Motors (TP 2.0, Block 1–30)
//! 5. Kilometerstand im Kombiinstrument suchen (UDS-Kennungen lesen und mit
//!    dem Tachostand vergleichen)

use crate::tr;
use crate::limits::Stufe;
use crate::labels::LabelDb;
use crate::transport::Backend;

#[derive(Debug, Clone)]
pub struct Schritt {
    pub titel: String,
    pub stufe: Stufe,
    pub zeilen: Vec<String>,
}

/// Bereiche, in denen das Kombi den Kilometerstand ablegen könnte (UDS 0x22).
/// Nur lesen; nicht unterstützte Kennungen antworten sofort mit 0x7F.
const KM_BEREICHE: [(u16, u16); 3] = [(0x2200, 0x22FF), (0x0200, 0x02FF), (0x1000, 0x10FF)];

fn hex(d: &[u8]) -> String {
    d.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ")
}

/// Mögliche Kilometerstände in Rohdaten: Ganzzahlen (2–4 Byte, big/little
/// endian, auch ×10 / ÷10).
pub fn km_kandidaten(d: &[u8]) -> Vec<(u64, String)> {
    let mut out = Vec::new();
    for len in [4usize, 3, 2] {
        for start in 0..d.len().saturating_sub(len - 1) {
            let w = &d[start..start + len];
            let be = w.iter().fold(0u64, |a, &b| (a << 8) | u64::from(b));
            let le = w.iter().rev().fold(0u64, |a, &b| (a << 8) | u64::from(b));
            for (v, order) in [(be, "BE"), (le, "LE")] {
                for (f, fname) in [(1.0, ""), (0.1, " ÷10"), (10.0, " ×10")] {
                    let km = (v as f64 * f) as u64;
                    out.push((km, tr!("Byte {start}, {len} Byte {order}{fname}", start = start, len = len, order = order, fname = fname)));
                }
            }
        }
    }
    out
}

/// Den Testlauf ausführen. `km_tacho` = Stand laut Tacho (für die Suche).
pub fn run(be: &Backend, db: &LabelDb, km_tacho: Option<u32>, mut progress: impl FnMut(&str)) -> Vec<Schritt> {
    let mut out = Vec::new();

    // 1. Verbindung
    progress(crate::trs!("Verbindung…"));
    let c = crate::connect::check(be);
    let stufe = c.iter().map(|x| x.0).max().unwrap_or(Stufe::Gut);
    let mut z: Vec<String> = c.iter().map(|(_, t)| t.clone()).collect();
    if let Backend::SocketCan { iface } = be {
        if let Some(st) = crate::connect::can_state(iface) {
            z.push(tr!("CAN-Zustand: {st}", st = st));
        }
    }
    out.push(Schritt { titel: crate::tr!("Verbindung"), stufe, zeilen: z });

    // 2. VIN
    progress(crate::trs!("Fahrgestellnummer…"));
    let vin = crate::profil::quick_vin(be);
    out.push(Schritt {
        titel: crate::tr!("Fahrgestellnummer (OBD-II)"),
        stufe: if vin.is_some() { Stufe::Gut } else { Stufe::Hinweis },
        zeilen: vec![vin.clone().unwrap_or_else(|| crate::tr!("nicht lesbar"))],
    });

    // 3. Voller Scan
    let t0 = std::time::Instant::now();
    let scan = crate::report::scan_voll(be, db, true, |p| progress(p));
    let modules = match scan {
        Ok(r) => {
            let mut z = vec![tr!("{a} Steuergeräte, {b} Fehler, Dauer {c} s", a = r.modules.len(), b = r.total_dtcs(), c = format!("{:.0}", t0.elapsed().as_secs_f64()))];
            for m in &r.modules {
                z.push(tr!("{a} {b} {c} Fehler: {d}", a = format!("{:<6}", m.protocol), b = format!("{:<34}", m.title()), c = format!("{:<22}", m.ident.first().cloned().unwrap_or_default()), d = m.dtcs.as_ref().map(|d| d.len().to_string()).unwrap_or_else(|| crate::tr!("nicht lesbar"))));
                for d in m.dtcs.iter().flatten() {
                    let alt = d.alt.as_deref().map(|a| format!(" ({a})")).unwrap_or_default();
                    z.push(format!("      {}{alt}  {}  [{}]", d.code, d.text, d.status));
                    for (k, v) in &d.umgebung {
                        z.push(format!("          {k}: {v}"));
                    }
                }
            }
            out.push(Schritt { titel: crate::tr!("Steuergeräte und Fehlerspeicher"), stufe: Stufe::Gut, zeilen: z });
            r.modules
        }
        Err(e) => {
            out.push(Schritt { titel: crate::tr!("Steuergeräte und Fehlerspeicher"), stufe: Stufe::Warnung, zeilen: vec![format!("{e:#}")] });
            Vec::new()
        }
    };

    // 4. Messwertblöcke Motor (TP 2.0)
    if let Some(a) = modules.iter().find(|m| m.protocol == "TP 2.0" && m.name.starts_with("Motor")).and_then(|m| m.address) {
        progress(crate::trs!("Messwertblöcke Motor…"));
        let mut z = Vec::new();
        match crate::kwp::open_session(be, a as u8) {
            Ok(mut ch) => {
                let mut k = crate::kwp::Kwp::new(&mut ch);
                let mut fehler_folge = 0;
                let texte = crate::messwerttexte::Texte::laden();
                for block in 1..=30u8 {
                    match k.read_block(block) {
                        Ok(vals) => {
                            fehler_folge = 0;
                            let txt: Vec<String> = vals
                                .iter()
                                .enumerate()
                                .map(|(i, v)| match v.decode() {
                                    _ if v.is_leer() => "—".into(),
                                    Some((x, u)) => format!("{}: {}", texte.feld(a as u8, block, i as u8 + 1, v), format!("{x:.1} {u}").trim_end()),
                                    None => format!("? (F{:02X} {:02X} {:02X})", v.formula, v.a, v.b),
                                })
                                .collect();
                            if vals.is_empty() {
                                z.push(tr!("Block {block}: (leer)", block = format!("{:03}", block)));
                                continue;
                            }
                            let raw: Vec<String> = vals.iter().map(|v| format!("{:02X}:{:02X}{:02X}", v.formula, v.a, v.b)).collect();
                            z.push(tr!("Block {block}: {a}    [roh {b}]", block = format!("{:03}", block), a = txt.join(" | "), b = raw.join(" ")));
                        }
                        Err(e) => {
                            z.push(tr!("Block {block}: {e}", block = format!("{:03}", block), e = format!("{:#}", e)));
                            fehler_folge += 1;
                            if fehler_folge >= 8 {
                                z.push(crate::tr!("(8 Blöcke in Folge nicht lesbar — abgebrochen)"));
                                break;
                            }
                        }
                    }
                }
            }
            Err(e) => z.push(tr!("Kanal: {e}", e = format!("{:#}", e))),
        }
        let ok = z.iter().any(|l| l.contains('|') || l.contains("/min"));
        out.push(Schritt { titel: tr!("Messwertblöcke Motor (TP 2.0 {a})", a = format!("{:#04x}", a)), stufe: if ok { Stufe::Gut } else { Stufe::Hinweis }, zeilen: z });
    }

    // 5. Kilometerstand im Kombi suchen (UDS)
    let kombi = modules.iter().find(|m| m.protocol == "UDS" && m.name.starts_with("Kombi"));
    if let Some((tx, rx)) = kombi.and_then(|m| crate::profil::parse_can(&m.can)) {
        progress(crate::trs!("Kilometerstand im Kombi suchen…"));
        let mut z = vec![match km_tacho {
            Some(k) => tr!("Gesucht: {k} km (Tacho)", k = k),
            None => crate::tr!("Kein Tachostand angegeben — es werden alle Werte zwischen 1 000 und 999 999 gelistet."),
        }];
        let mut treffer = 0;
        match be.open(tx, rx) {
            Ok(tp) => {
                let uds = crate::uds::Uds::new(&*tp);
                let _ = uds.diagnostic_session(0x01);
                // Bekannte Stelle zuerst (Golf 6: 0x2203, 10-km-Schritte)
                let direkt = uds.read_data_by_id(crate::report::KOMBI_KM_DID).ok();
                let direkt_km = direkt.as_deref().and_then(crate::report::kombi_km_aus);
                if let (Some(d), Some(k)) = (&direkt, direkt_km) {
                    z.push(tr!("Kennung {a}: {k} km (10-km-Schritte)    [roh {b}]", a = format!("{:#06X}", crate::report::KOMBI_KM_DID), k = k, b = hex(d)));
                }
                let passt = match (direkt_km, km_tacho) {
                    (Some(k), Some(t)) => k <= t && t - k < 10,
                    (Some(_), None) => true,
                    _ => false,
                };
                if passt {
                    treffer = 1;
                    z.push(if km_tacho.is_some() { crate::tr!("→ passt zum Tacho, keine weitere Suche nötig") } else { crate::tr!("→ ohne Tachostand nicht geprüft") });
                }
                let mut gelesen = 0;
                for (von, bis) in if passt { &[][..] } else { &KM_BEREICHE[..] } {
                    for did in *von..=*bis {
                        let Ok(d) = uds.read_data_by_id(did) else { continue };
                        gelesen += 1;
                        let hits: Vec<String> = km_kandidaten(&d)
                            .into_iter()
                            .filter(|(km, wo)| match km_tacho {
                                // ×10-Werte (10-km-Schritte) können bis 9 km unter dem Tacho liegen
                                Some(t) => km.abs_diff(u64::from(t)) <= if wo.contains("×10") { 9 } else { 2 },
                                None => (1_000..=999_999).contains(km),
                            })
                            .map(|(km, wo)| tr!("{km} km ({wo})", km = km, wo = wo))
                            .collect();
                        if !hits.is_empty() {
                            treffer += 1;
                            if km_tacho.is_some() || treffer <= 40 {
                                z.push(tr!("DID {did}: {a}    [roh {b}]", did = format!("{:#06X}", did), a = hits.join(", "), b = hex(&d)));
                            }
                        }
                    }
                }
                if !passt {
                    z.insert(1, tr!("{gelesen} Kennungen lesbar, {treffer} mit passendem Wert", gelesen = gelesen, treffer = treffer));
                }
            }
            Err(e) => z.push(tr!("Kombi nicht erreichbar: {e}", e = format!("{:#}", e))),
        }
        out.push(Schritt {
            titel: tr!("Kilometerstand im Kombi ({tx}/{rx})", tx = format!("{:#05x}", tx), rx = format!("{:#05x}", rx)),
            stufe: if treffer > 0 { Stufe::Gut } else { Stufe::Hinweis },
            zeilen: z,
        });
    }
    out
}

/// Protokoll als Text.
pub fn to_text(schritte: &[Schritt], transport: &str) -> String {
    let mut s = tr!("ECUWerkbank — Testlauf\nErstellt:   {a}\nVerbindung: {transport}\n", a = crate::report::now_local(), transport = transport);
    for st in schritte {
        let z = match st.stufe {
            Stufe::Gut => "OK",
            Stufe::Hinweis => "Hinweis",
            Stufe::Warnung => "Warnung",
        };
        s += &format!("\n=== {} [{z}]\n", st.titel);
        for l in &st.zeilen {
            s += &format!("  {l}\n");
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn km_in_rohdaten_finden() {
        // 154300 km = 0x025ABC, big endian in 3 Byte
        let k = km_kandidaten(&[0x00, 0x02, 0x5A, 0xBC]);
        assert!(k.iter().any(|(km, _)| *km == 154_300));
        let k = km_kandidaten(&[0xBC, 0x5A, 0x02]);
        assert!(k.iter().any(|(km, w)| *km == 154_300 && w.contains("LE")));
    }

    #[test]
    fn testlauf_demo() {
        let _g = crate::sim::TEST_LOCK.lock().unwrap();
        crate::sim::reset();
        let s = run(&Backend::Demo, &LabelDb::default(), None, |_| {});
        let t = to_text(&s, "Demo");
        assert!(t.contains("Steuergeräte und Fehlerspeicher"), "{t}");
        assert!(t.contains("Messwertblöcke Motor"), "{t}");
    }
}
