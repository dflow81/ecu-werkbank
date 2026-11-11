//! Gebrauchtwagen-Check: Hinweise auf gelöschte Fehler, Tacho-Manipulation,
//! getauschte Steuergeräte und versteckte Mängel — mit PDF „Kaufberatung“.
//!
//! Das ist eine Plausibilitätsprüfung der Diagnosedaten, kein Gutachten.

use crate::tr;
use crate::labels::LabelDb;
use crate::limits::Stufe;
use crate::pdf::{self, Font};
use crate::report::{clip, group_thousands, Layout, ScanReport, MARGIN};
use crate::transport::Backend;
use anyhow::Result;

/// Ein Prüfpunkt.
#[derive(Debug, Clone)]
pub struct Punkt {
    pub bereich: &'static str,
    pub stufe: Stufe,
    pub text: String,
}

pub struct Ergebnis {
    pub report: ScanReport,
    pub punkte: Vec<Punkt>,
    pub gesamt: Stufe,
    /// Gefundene Kilometerstände: (Quelle, km)
    pub km_quellen: Vec<(String, u32)>,
    /// Readiness je Abgas-Test: (Name, fertig) — leer = nicht gelesen
    pub readiness: Vec<(String, bool)>,
    /// Motorkontrollleuchte an? (None = unbekannt)
    pub mil: Option<bool>,
    /// Aus einem gespeicherten Scan erstellt (Datum), nicht live
    pub aus_scan: Option<String>,
    /// Abgas-Prüfwerte (OBD Mode 06), falls lesbar
    pub tests: Vec<crate::obd::TestResult>,
}

fn p(bereich: &'static str, stufe: Stufe, text: impl Into<String>) -> Punkt {
    Punkt { bereich, stufe, text: text.into() }
}

/// Größter Kilometerstand aus den Umgebungsbedingungen der Fehler.
fn snapshot_km(r: &ScanReport) -> Option<u32> {
    r.modules
        .iter()
        .flat_map(|m| m.dtcs.iter().flatten())
        .flat_map(|d| d.umgebung.iter())
        .filter(|(k, _)| k == "Kilometerstand")
        .filter_map(|(_, v)| v.split_whitespace().next()?.replace('.', "").parse::<u32>().ok())
        .max()
}

/// Steuergeräte, Fehlerspeicher und Fahrgestellnummern aus einem Scan.
fn grundpunkte(r: &ScanReport) -> Vec<Punkt> {
    let mut pts = Vec::new();
    // --- Steuergeräte & Fehler ---
    if r.modules.is_empty() {
        pts.push(p(crate::trs!("Verbindung"), Stufe::Warnung, tr!("Kein Steuergerät erreichbar — Zündung an? Adapter/Bitrate prüfen.")));
    } else {
        pts.push(p(crate::trs!("Steuergeräte"), Stufe::Gut, tr!("{a} Steuergerät(e) erreicht", a = r.modules.len())));
    }
    let total = r.total_dtcs();
    if total == 0 {
        pts.push(p(crate::trs!("Fehlerspeicher"), Stufe::Gut, tr!("Keine Fehler gespeichert")));
    } else {
        let list: Vec<String> = r
            .modules
            .iter()
            .filter(|m| m.dtc_count() > 0)
            .map(|m| format!("{} ({})", if m.name.is_empty() { m.addr_text() } else { m.name.clone() }, m.dtc_count()))
            .collect();
        pts.push(p(crate::trs!("Fehlerspeicher"), Stufe::Hinweis, tr!("{total} Fehler gespeichert: {a}", total = total, a = list.join(", "))));
    }

    // --- Fahrgestellnummern ---
    let mut vins: Vec<(String, String)> = r.modules.iter().filter_map(|m| m.vin.clone().map(|v| (m.title(), v))).collect();
    if let Some(v) = &r.vin {
        if !vins.iter().any(|(_, x)| x == v) {
            vins.push((crate::tr!("Fahrzeug"), v.clone()));
        }
    }
    let mut distinct: Vec<&String> = vins.iter().map(|(_, v)| v).collect();
    distinct.sort();
    distinct.dedup();
    match distinct.len() {
        0 => pts.push(p(crate::trs!("Fahrgestellnummer"), Stufe::Hinweis, tr!("Fahrgestellnummer nicht lesbar — mit Fahrzeugschein und Typenschild vergleichen"))),
        1 => pts.push(p(
            crate::trs!("Fahrgestellnummer"),
            Stufe::Gut,
            tr!("{a} — in {b} Steuergerät(en) gleich. Mit Fahrzeugschein vergleichen!", a = distinct[0], b = vins.len()),
        )),
        _ => {
            // häufigste VIN = Fahrzeug; Abweichler nennen
            let mut counts: std::collections::BTreeMap<&String, usize> = Default::default();
            for (_, v) in &vins {
                *counts.entry(v).or_default() += 1;
            }
            let main = counts.iter().max_by_key(|(_, n)| **n).map(|(v, _)| (*v).clone()).unwrap_or_default();
            let det: Vec<String> = vins.iter().filter(|(_, v)| *v != main).map(|(m, v)| format!("{} meldet {v}", m.trim())).collect();
            pts.push(p(
                crate::trs!("Fahrgestellnummer"),
                Stufe::Warnung,
                tr!("Fahrzeug {main}, aber {a} — Steuergerät getauscht (Gebraucht-/Unfallteil?) oder nicht angelernt", main = main, a = det.join(", ")),
            ));
        }
    }
    pts
}

/// Kompletter Check (alle Steuergeräte, OBD-II, Plausibilität).
pub fn run(be: &Backend, db: &LabelDb, progress: impl FnMut(&str)) -> Result<Ergebnis> {
    let mut r = crate::report::scan_voll(be, db, true, progress)?;
    let mut pts = Vec::new();
    let mut km_quellen: Vec<(String, u32)> = Vec::new();

    pts.extend(grundpunkte(&r));

    // --- OBD-II: gelöscht? Readiness, km ---
    let mut cleared_km = None;
    let mut mil = None;
    let mut tests = Vec::new();
    if let Ok(tp) = be.open(crate::obd::DEFAULT_TX, crate::obd::DEFAULT_RX) {
        let o = crate::obd::Obd::new(&*tp);
        let km_since = o.read_value(0x31).ok();
        let warm = o.read_value(0x30).ok();
        cleared_km = km_since;
        match (km_since, warm) {
            (Some(k), w) if k < 200.0 || w.is_some_and(|w| w < 10.0) => pts.push(p(
                crate::trs!("Fehler gelöscht?"),
                Stufe::Warnung,
                tr!("Fehlerspeicher wurde vor kurzem gelöscht: vor {k} km{a} — Mängel könnten verborgen sein. Nach einer längeren Probefahrt erneut prüfen.", k = format!("{:.0}", k), a = w.map(|w| format!(" / {w:.0} Warmläufen")).unwrap_or_default()),
            )),
            (Some(k), w) => pts.push(p(
                crate::trs!("Fehler gelöscht?"),
                Stufe::Gut,
                tr!("Letztes Löschen vor {a} km{b} — unauffällig", a = group_thousands(k as u32), b = w.map(|w| format!(" / {w:.0} Warmläufen")).unwrap_or_default()),
            )),
            (None, _) => pts.push(p(crate::trs!("Fehler gelöscht?"), Stufe::Hinweis, tr!("„Strecke seit Löschen“ nicht lesbar"))),
        }
        if let Ok(rd) = o.readiness() {
            let open: Vec<&str> = rd.monitors.iter().filter(|m| m.available && !m.complete).map(|m| m.name).collect();
            if rd.mil {
                pts.push(p(crate::trs!("Motorkontrollleuchte"), Stufe::Warnung, tr!("Motorkontrollleuchte AN")));
            }
            let fertig: Vec<&str> = rd.monitors.iter().filter(|m| m.available && m.complete).map(|m| m.name).collect();
            if open.is_empty() {
                pts.push(p(crate::trs!("Readiness"), Stufe::Gut, tr!("Alle Abgas-Tests fertig: {a}", a = fertig.join(", "))));
            } else {
                let st = if open.len() >= 3 { Stufe::Warnung } else { Stufe::Hinweis };
                pts.push(p(crate::trs!("Readiness"), st, tr!("Offene Abgas-Tests: {a} — typisch nach Fehlerlöschen oder Batterie ab", a = open.join(", "))));
                if !fertig.is_empty() {
                    pts.push(p(crate::trs!("Readiness"), Stufe::Gut, tr!("Fertig: {a}", a = fertig.join(", "))));
                }
            }
            mil = Some(rd.mil);
            r.readiness = rd.monitors.iter().filter(|m| m.available).map(|m| (m.name.to_string(), m.complete)).collect();
        }
        if let Ok(k) = o.odometer() {
            km_quellen.push((crate::trn!("Motor (OBD)").into(), k));
        }
        if let Ok(t) = o.test_results() {
            let auffaellig = crate::obd::test_summary(&t);
            let mit_grenze = t.iter().filter(|x| x.has_limits()).count();
            if auffaellig.is_empty() && mit_grenze > 0 {
                pts.push(p(crate::trs!("Abgas-Prüfergebnisse"), Stufe::Gut, tr!("{a} Tests ohne Auffälligkeit (Werte in der Tabelle unten)", a = mit_grenze)));
            }
            for (st, txt) in auffaellig {
                pts.push(p(crate::trs!("Abgas-Prüfergebnisse"), st, txt));
            }
            r.pruefwerte = t.iter().map(crate::obd::Pruefwert::from).collect();
            tests = t;
        }
        if let Ok(u) = o.read_value(0x42) {
            let st = if (12.2..=15.0).contains(&u) { Stufe::Gut } else { Stufe::Hinweis };
            pts.push(p(crate::trs!("Batterie"), st, tr!("Bordspannung {u} V (genauer: Batterietest)", u = format!("{:.1}", u))));
        }
    }
    if let Some(k) = r.km {
        if !km_quellen.iter().any(|(_, x)| *x == k) {
            km_quellen.push((crate::trn!("Kombiinstrument").into(), k));
        }
    }
    if let Some(a) = r.modules.iter().find(|m| m.protocol == "TP 2.0" && m.name.starts_with("Kombi")).and_then(|m| m.address) {
        if let Some(k) = crate::report::kwp_odometer(be, a as u8) {
            if !km_quellen.iter().any(|(q, _)| q == "Kombiinstrument") {
                km_quellen.push((crate::trn!("Kombiinstrument").into(), k));
            }
        }
    }
    let snap = snapshot_km(&r);
    if let Some(k) = snap {
        km_quellen.push((crate::trn!("Fehlerspeicher (höchster Stand)").into(), k));
    }

    // --- Kilometerstand plausibel? ---
    let current = km_quellen.iter().filter(|(q, _)| !q.starts_with("Fehlerspeicher")).map(|(_, k)| *k).max();
    match (current, snap) {
        (Some(now), Some(s)) if s > now + 100 => pts.push(p(
            crate::trs!("Kilometerstand"),
            Stufe::Warnung,
            tr!("Ein Fehler wurde bei {a} km gespeichert, der Tacho zeigt aber nur {b} km — Tacho manipuliert oder getauscht?", a = group_thousands(s), b = group_thousands(now)),
        )),
        _ => {}
    }
    let shown: Vec<u32> = km_quellen.iter().filter(|(q, _)| !q.starts_with("Fehlerspeicher")).map(|(_, k)| *k).collect();
    if let (Some(lo), Some(hi)) = (shown.iter().min(), shown.iter().max()) {
        if hi - lo > 1000 {
            pts.push(p(crate::trs!("Kilometerstand"), Stufe::Warnung, tr!("Steuergeräte melden unterschiedliche Kilometerstände ({a} bis {b} km)", a = group_thousands(*lo), b = group_thousands(*hi))));
        } else if !pts.iter().any(|x| x.bereich == "Kilometerstand") {
            pts.push(p(crate::trs!("Kilometerstand"), Stufe::Gut, tr!("{a} km — in allen Quellen plausibel. Mit Serviceheft/TÜV-Berichten vergleichen.", a = group_thousands(*hi))));
        }
        if r.km.is_none() {
            r.km = Some(*hi);
        }
    } else {
        pts.push(p(crate::trs!("Kilometerstand"), Stufe::Hinweis, tr!("Kilometerstand nicht auslesbar — mit Serviceheft/TÜV-Berichten vergleichen")));
    }
    if let (Some(c), Some(now)) = (cleared_km, current) {
        if c as u32 > now {
            pts.push(p(crate::trs!("Kilometerstand"), Stufe::Warnung, tr!("„Strecke seit Löschen“ größer als der Tachostand — Tacho manipuliert?")));
        }
    }

    let gesamt = pts.iter().map(|x| x.stufe).max().unwrap_or(Stufe::Gut);
    r.check = pts.iter().map(|x| (x.stufe, format!("{}: {}", x.bereich, x.text))).collect();
    Ok(Ergebnis { readiness: r.readiness.clone(), report: r, punkte: pts, gesamt, km_quellen, mil, tests, aus_scan: None })
}

/// Letzter gespeicherter Scan eines Fahrzeugs mit Abgasdaten (Readiness /
/// Prüfwerte, z. B. ein Schnell-Check); sonst der letzte überhaupt.
pub fn letzter_scan(vehicle_key: &str) -> Option<ScanReport> {
    let scans = crate::history::list(vehicle_key);
    let mit_abgas = |r: &ScanReport| !r.pruefwerte.is_empty() || !r.readiness.is_empty() || r.check.iter().any(|(_, t)| t.starts_with("Readiness"));
    scans.iter().rev().find(|(_, r)| mit_abgas(r)).or_else(|| scans.last()).map(|(_, r)| r.clone())
}

/// Gebrauchtwagen-Check aus einem gespeicherten Scan (ohne Auto).
/// Nimmt, was der Scan enthält: Steuergeräte, Fehler, Fahrgestellnummern,
/// km, Readiness und Abgas-Prüfwerte (wenn es ein Schnell-Check war).
pub fn aus_scan(mut r: ScanReport) -> Ergebnis {
    let mut pts = grundpunkte(&r);
    let mut km_quellen: Vec<(String, u32)> = Vec::new();
    // Readiness: gespeichert je Test, bei älteren Scans nur als Text
    let mut readiness = r.readiness.clone();
    let text = |prefix: &str| r.check.iter().find(|(_, t)| t.starts_with(prefix)).map(|(st, t)| (*st, t.clone()));
    if readiness.is_empty() {
        if let Some((_, t)) = text("Readiness offen: ") {
            readiness = t.trim_start_matches("Readiness offen: ").split(", ").map(|n| (n.to_string(), false)).collect();
            readiness.push((tr!("alle übrigen Tests"), true));
        } else if text("Readiness: alle").is_some() {
            readiness.push((tr!("alle Abgas-Tests"), true));
        }
    }
    let offen: Vec<&str> = readiness.iter().filter(|(_, f)| !f).map(|(n, _)| n.as_str()).collect();
    if readiness.is_empty() {
        pts.push(p(crate::trs!("Readiness"), Stufe::Hinweis, tr!("Im gespeicherten Scan nicht enthalten")));
    } else if offen.is_empty() {
        pts.push(p(crate::trs!("Readiness"), Stufe::Gut, tr!("Alle Abgas-Tests fertig: {a}", a = readiness.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>().join(", "))));
    } else {
        let st = if offen.len() >= 3 { Stufe::Warnung } else { Stufe::Hinweis };
        pts.push(p(crate::trs!("Readiness"), st, tr!("Offene Abgas-Tests: {a} — typisch nach Fehlerlöschen oder Batterie ab", a = offen.join(", "))));
    }
    if let Some((st, t)) = text("Motorkontrollleuchte AN") {
        pts.push(p(crate::trs!("Motorkontrollleuchte"), st, t));
    }
    // Abgas-Prüfwerte
    let tests: Vec<crate::obd::TestResult> = r
        .pruefwerte
        .iter()
        .map(|w| crate::obd::TestResult { mid: w.mid, tid: w.tid, uasid: w.uasid, value: w.wert, min: w.min, max: w.max, unit: crate::obd::uas_scaling(w.uasid).2 })
        .collect();
    if !tests.is_empty() {
        let auffaellig = crate::obd::test_summary(&tests);
        let mit_grenze = tests.iter().filter(|x| x.has_limits()).count();
        if auffaellig.is_empty() && mit_grenze > 0 {
            pts.push(p(crate::trs!("Abgas-Prüfergebnisse"), Stufe::Gut, tr!("{a} Tests ohne Auffälligkeit (Werte in der Tabelle unten)", a = mit_grenze)));
        }
        for (st, txt) in auffaellig {
            pts.push(p(crate::trs!("Abgas-Prüfergebnisse"), st, txt));
        }
    }
    if let Some((st, t)) = text("Bordspannung") {
        pts.push(p(crate::trs!("Batterie"), st, t));
    }
    pts.push(p(crate::trs!("Fehler gelöscht?"), Stufe::Hinweis, tr!("„Strecke seit Löschen“ ist im gespeicherten Scan nicht enthalten")));
    if let Some(k) = r.km {
        km_quellen.push((crate::trn!("Kombiinstrument").into(), k));
        pts.push(p(crate::trs!("Kilometerstand"), Stufe::Gut, tr!("{a} km laut Scan. Mit Serviceheft/TÜV-Berichten vergleichen.", a = group_thousands(k))));
    } else {
        pts.push(p(crate::trs!("Kilometerstand"), Stufe::Hinweis, tr!("Kilometerstand nicht auslesbar — mit Serviceheft/TÜV-Berichten vergleichen")));
    }
    if let Some(k) = snapshot_km(&r) {
        km_quellen.push((crate::trn!("Fehlerspeicher (höchster Stand)").into(), k));
    }
    let gesamt = pts.iter().map(|x| x.stufe).max().unwrap_or(Stufe::Gut);
    let datum = r.created.clone();
    r.readiness = readiness.clone();
    Ergebnis { report: r, punkte: pts, gesamt, km_quellen, readiness, mil: None, tests, aus_scan: Some(datum) }
}

impl Ergebnis {
    /// PDF: Readiness je Abgas-Test und alle Prüfwerte (Mode 06) als Tabellen.
    fn pdf_abgas(&self, l: &mut Layout) {
        let grau = (0.4, 0.4, 0.4);
        let marke = |l: &mut Layout, st: Stufe, text: &str| {
            l.doc.rect(MARGIN + 6.0, l.y - 1.0, 44.0, 11.0, st.rgb());
            l.doc.text_rgb(MARGIN + 10.0, l.y + 1.0, 7.5, Font::Bold, text, (1.0, 1.0, 1.0));
        };
        if !self.readiness.is_empty() {
            l.y -= 8.0;
            l.heading(crate::trs!("Readiness (Abgas-Tests)"));
            if let Some(m) = self.mil {
                l.ensure(13.0);
                let mil = if m { tr!("Motorkontrollleuchte: AN") } else { tr!("Motorkontrollleuchte: aus") };
                l.doc.text(MARGIN + 6.0, l.y, 9.5, Font::Regular, &mil);
                l.y -= 15.0;
            }
            for (name, fertig) in &self.readiness {
                l.ensure(13.0);
                if *fertig {
                    marke(l, Stufe::Gut, "fertig");
                } else {
                    marke(l, Stufe::Hinweis, "offen");
                }
                l.doc.text(MARGIN + 60.0, l.y, 9.5, Font::Regular, &crate::i18n::t(name));
                l.y -= 13.0;
            }
            if self.readiness.iter().any(|(_, f)| !f) {
                l.ensure(13.0);
                l.doc.text_rgb(MARGIN + 6.0, l.y, 8.5, Font::Regular, &tr!("Offene Tests laufen beim normalen Fahren von selbst (warmer Motor, etwas Landstraße). Für die AU müssen sie fertig sein."), grau);
                l.y -= 13.0;
            }
        }
        if !self.tests.is_empty() {
            l.y -= 8.0;
            l.heading(crate::trs!("Abgas-Prüfwerte (OBD Mode 06)"));
            l.ensure(13.0);
            for (x, h) in [(MARGIN + 60.0, "Test"), (MARGIN + 300.0, "Wert"), (MARGIN + 370.0, "Grenze"), (MARGIN + 460.0, "Reserve")] {
                l.doc.text_rgb(x, l.y, 8.5, Font::Bold, &crate::i18n::t(h), grau);
            }
            l.y -= 13.0;
            let zahl = |v: f64| if v.abs() >= 100.0 { format!("{v:.0}") } else { format!("{v:.2}") };
            let ohne: usize = self.tests.iter().filter(|t| !t.has_limits()).count();
            for t in self.tests.iter().filter(|t| t.has_limits()) {
                l.ensure(13.0);
                let st = t.stufe();
                marke(l, st, if st == Stufe::Warnung { "n. i. O." } else { "OK" });
                l.doc.text(MARGIN + 60.0, l.y, 8.5, Font::Regular, &clip(&t.name(), 235.0, 8.5));
                l.doc.text(MARGIN + 300.0, l.y, 8.5, Font::Bold, &format!("{} {}", zahl(t.value), t.unit));
                l.doc.text(MARGIN + 370.0, l.y, 8.5, Font::Regular, &format!("{} – {}", zahl(t.min), zahl(t.max)));
                let res = t.reserve().map(|r| format!("{r:.0} %")).unwrap_or_else(|| "—".into());
                l.doc.text(MARGIN + 460.0, l.y, 8.5, Font::Regular, &res);
                l.y -= 13.0;
            }
            if ohne > 0 {
                l.ensure(13.0);
                l.doc.text_rgb(MARGIN + 6.0, l.y, 8.5, Font::Regular, &tr!("{a} weitere Tests ohne Grenzwerte (Zähler oder noch nicht gelaufen) — nicht bewertet.", a = ohne), grau);
                l.y -= 13.0;
            }
        }
    }

    pub fn urteil(&self) -> &'static str {
        match self.gesamt {
            Stufe::Gut => crate::trs!("Unauffällig — keine Hinweise auf Manipulation oder versteckte Fehler."),
            Stufe::Hinweis => crate::trs!("Kleinere Auffälligkeiten — beim Verkäufer nachfragen."),
            Stufe::Warnung => crate::trs!("Auffällig — vor dem Kauf klären oder Fachwerkstatt/Gutachter hinzuziehen."),
        }
    }

    pub fn to_pdf(&self) -> Vec<u8> {
        let r = &self.report;
        let mut l = Layout::new();
        let accent = (0.10, 0.32, 0.55);
        l.doc.rect(0.0, pdf::A4_H - 70.0, pdf::A4_W, 70.0, accent);
        l.doc.text_rgb(MARGIN, pdf::A4_H - 42.0, 20.0, Font::Bold, crate::trs!("Gebrauchtwagen-Check"), (1.0, 1.0, 1.0));
        l.doc.text_rgb(MARGIN, pdf::A4_H - 58.0, 9.0, Font::Regular, &tr!("{a} · erstellt mit ECUWerkbank", a = r.created), (0.85, 0.9, 1.0));
        l.y = pdf::A4_H - 100.0;
        for (k, v) in [
            (crate::trs!("Fahrgestellnummer"), r.vin.clone().unwrap_or_else(|| crate::tr!("nicht gelesen"))),
            (crate::trs!("Kennzeichen"), r.kennzeichen.clone()),
            (crate::trs!("Fahrzeug"), r.fahrzeug.clone()),
            (crate::trs!("Notiz"), r.note.clone()),
        ] {
            if v.is_empty() {
                continue;
            }
            l.doc.text_rgb(MARGIN, l.y, 10.0, Font::Regular, k, (0.4, 0.4, 0.4));
            l.doc.text(MARGIN + 120.0, l.y, 10.0, Font::Bold, &v);
            l.y -= 15.0;
        }
        if let Some(d) = &self.aus_scan {
            l.doc.text_rgb(MARGIN, l.y, 9.0, Font::Regular, &tr!("Erstellt aus dem gespeicherten Scan vom {d} (ohne Verbindung zum Auto).", d = d), (0.4, 0.4, 0.4));
            l.y -= 15.0;
        }
        l.y -= 8.0;
        l.doc.rect(MARGIN, l.y - 8.0, pdf::A4_W - 2.0 * MARGIN, 26.0, self.gesamt.rgb());
        l.doc.text_rgb(MARGIN + 10.0, l.y, 12.0, Font::Bold, &clip(self.urteil(), 470.0, 12.0), (1.0, 1.0, 1.0));
        l.y -= 34.0;

        l.heading(crate::trs!("Prüfpunkte"));
        for x in &self.punkte {
            let lines = pdf::wrap(&x.text, pdf::A4_W - MARGIN - (MARGIN + 170.0), 9.5, Font::Regular);
            l.ensure(13.0 * lines.len() as f32 + 4.0);
            l.doc.rect(MARGIN, l.y - 1.0, 44.0, 11.0, x.stufe.rgb());
            l.doc.text_rgb(MARGIN + 4.0, l.y + 1.0, 7.5, Font::Bold, x.stufe.text(), (1.0, 1.0, 1.0));
            l.doc.text(MARGIN + 52.0, l.y, 9.5, Font::Bold, x.bereich);
            for (i, line) in lines.iter().enumerate() {
                l.doc.text(MARGIN + 170.0, l.y - 13.0 * i as f32, 9.5, Font::Regular, line);
            }
            l.y -= 13.0 * lines.len() as f32 + 4.0;
        }
        if !self.km_quellen.is_empty() {
            l.y -= 8.0;
            l.heading(crate::trs!("Kilometerstände"));
            for (q, k) in &self.km_quellen {
                l.ensure(13.0);
                l.doc.text(MARGIN + 6.0, l.y, 9.5, Font::Regular, &crate::i18n::t(q));
                l.doc.text(MARGIN + 220.0, l.y, 9.5, Font::Bold, &format!("{} km", group_thousands(*k)));
                l.y -= 13.0;
            }
        }
        self.pdf_abgas(&mut l);
        let faults: Vec<_> = r.modules.iter().filter(|m| m.dtc_count() > 0).collect();
        if !faults.is_empty() {
            l.y -= 8.0;
            l.heading(crate::trs!("Gespeicherte Fehler"));
            for m in faults {
                for e in m.dtcs.iter().flatten() {
                    l.ensure(13.0);
                    l.doc.text(MARGIN + 6.0, l.y, 9.5, Font::Bold, &e.code);
                    let t = if e.text.is_empty() { crate::tr!("(kein Klartext)") } else { e.text.clone() };
                    l.doc.text(MARGIN + 70.0, l.y, 9.0, Font::Regular, &clip(&format!("{t}  –  {}", m.title()), 420.0, 9.0));
                    l.y -= 13.0;
                }
            }
        }
        l.y -= 14.0;
        l.ensure(40.0);
        for line in pdf::wrap(
            crate::trs!("Hinweis: Diese Auswertung prüft nur die Diagnosedaten auf Plausibilität. Sie ersetzt keine Probefahrt, keine Sichtprüfung und kein Gutachten. Fahrgestellnummer immer mit Fahrzeugschein und Typenschild vergleichen."),
            pdf::A4_W - 2.0 * MARGIN,
            8.5,
            Font::Regular,
        ) {
            l.doc.text_rgb(MARGIN, l.y, 8.5, Font::Regular, &line, (0.4, 0.4, 0.4));
            l.y -= 11.0;
        }
        let n = l.doc.page_count();
        for pg in 0..n {
            l.doc.text_on_page(pg, MARGIN, 28.0, 8.0, Font::Regular, &tr!("ECUWerkbank · Gebrauchtwagen-Check · {a}", a = r.vin.clone().unwrap_or_default()));
            l.doc.text_on_page(pg, pdf::A4_W - MARGIN - 50.0, 28.0, 8.0, Font::Regular, &tr!("Seite {a} von {n}", a = pg + 1, n = n));
        }
        l.doc.to_bytes(crate::trs!("Gebrauchtwagen-Check"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_clean_and_manipulated() {
        let _g = crate::sim::TEST_LOCK.lock().unwrap();
        let db = LabelDb::default();
        crate::sim::reset();
        let ok = run(&Backend::Demo, &db, |_| {}).unwrap();
        assert!(!ok.punkte.iter().any(|x| x.bereich == "Fahrgestellnummer" && x.stufe == Stufe::Warnung), "{:?}", ok.punkte);
        assert!(!ok.punkte.iter().any(|x| x.bereich == "Kilometerstand" && x.stufe == Stufe::Warnung), "{:?}", ok.punkte);
        crate::sim::set_manipuliert(true);
        let bad = run(&Backend::Demo, &db, |_| {}).unwrap();
        crate::sim::set_manipuliert(false);
        let w = |b: &str| bad.punkte.iter().any(|x| x.bereich == b && x.stufe == Stufe::Warnung);
        assert!(w("Fehler gelöscht?"), "{:?}", bad.punkte);
        assert!(w("Kilometerstand"), "{:?}", bad.punkte);
        assert!(w("Fahrgestellnummer"), "{:?}", bad.punkte);
        assert_eq!(bad.gesamt, Stufe::Warnung);
        assert!(bad.to_pdf().starts_with(b"%PDF"));
    }
}
