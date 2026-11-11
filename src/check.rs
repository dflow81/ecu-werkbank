//! Schnell-Check: Fehlerspeicher aller Steuergeräte, Readiness, Bordspannung,
//! Vergleich mit dem letzten Scan und fällige Wartung — mit Ampel.

use crate::tr;
use crate::labels::LabelDb;
use crate::limits::Stufe;
use crate::report::ScanReport;
use crate::transport::Backend;
use anyhow::Result;

pub struct CheckResult {
    pub report: ScanReport,
    pub gesamt: Stufe,
}

/// Kompletter Schnell-Check; speichert den Scan in der Historie.
pub fn run(be: &Backend, db: &LabelDb, active: Option<&str>, progress: impl FnMut(&str)) -> Result<CheckResult> {
    let mut r = crate::report::scan(be, db, true, progress)?;
    let v = r.assign_vehicle(active)?;
    r.comparison = crate::history::compare_with_previous(&r);
    let mut pts: Vec<(Stufe, String)> = Vec::new();

    // Fehler
    let total = r.total_dtcs();
    match &r.comparison {
        Some(c) if !c.diff.new.is_empty() => pts.push((
            Stufe::Warnung,
            tr!("{a} neue(r) Fehler seit {b} ({total} gesamt)", a = c.diff.new.len(), b = c.since, total = total),
        )),
        _ if total > 0 => pts.push((Stufe::Hinweis, tr!("{total} Fehler gespeichert (keine neuen)", total = total))),
        _ => pts.push((Stufe::Gut, tr!("Keine Fehler gespeichert"))),
    }
    // Wiederkehrende Fehler mit früherer Reparatur
    if let Some(c) = &r.comparison {
        for it in &c.diff.new {
            let fr = r
                .modules
                .iter()
                .flat_map(|m| m.dtcs.iter().flatten())
                .find(|d| d.code == it.code)
                .map(|d| d.frueher.clone())
                .unwrap_or_default();
            if let Some(last) = fr.last() {
                pts.push((Stufe::Warnung, tr!("{a} ist wieder da — früher: {last}", a = it.code, last = last)));
            }
        }
    }
    if let Some(c) = &r.comparison {
        if !c.diff.gone.is_empty() {
            pts.push((Stufe::Gut, tr!("{a} Fehler seit dem letzten Scan behoben", a = c.diff.gone.len())));
        }
    }
    if r.modules.is_empty() {
        pts.push((Stufe::Warnung, tr!("Kein Steuergerät erreichbar — Zündung, Adapter, Bitrate prüfen")));
    }

    // Readiness + Spannung + Prüfergebnisse (OBD-II)
    let mut trend: Vec<(String, f64, String)> = Vec::new();
    if let Ok(tp) = be.open(crate::obd::DEFAULT_TX, crate::obd::DEFAULT_RX) {
        let o = crate::obd::Obd::new(&*tp);
        if let Ok(tests) = o.test_results() {
            let sum = crate::obd::test_summary(&tests);
            if sum.is_empty() && !tests.is_empty() {
                pts.push((Stufe::Gut, tr!("Abgas-Prüfergebnisse: {a} Tests ohne Auffälligkeit", a = tests.len())));
            }
            pts.extend(sum);
            r.pruefwerte = tests.iter().map(crate::obd::Pruefwert::from).collect();
            for t in &tests {
                // je Test eine eigene Reihe (früher je Überwachung → Werte mehrerer Tests gemischt)
                if let Some(res) = t.reserve().filter(|_| t.has_limits() && !t.herstellertest()) {
                    trend.push((tr!("Reserve {a}", a = t.name()), res, "%".into()));
                }
                if (0xA2..=0xAD).contains(&t.mid) && t.tid == 0x0B {
                    trend.push((tr!("Aussetzer Zylinder {a}", a = t.mid - 0xA1), t.value, "Zähler".into()));
                }
            }
            trend.dedup_by(|a, b| a.0 == b.0);
        }
        if let Ok(rd) = o.readiness() {
            let open: Vec<&str> = rd.monitors.iter().filter(|m| m.available && !m.complete).map(|m| m.name).collect();
            r.readiness = rd.monitors.iter().filter(|m| m.available).map(|m| (m.name.to_string(), m.complete)).collect();
            if rd.mil {
                pts.push((Stufe::Warnung, tr!("Motorkontrollleuchte AN")));
            }
            if open.is_empty() {
                pts.push((Stufe::Gut, tr!("Readiness: alle Abgas-Tests fertig")));
            } else {
                pts.push((Stufe::Hinweis, tr!("Readiness offen: {a}", a = open.join(", "))));
            }
        }
        if let Ok(u) = o.read_value(0x42) {
            let s = if (13.6..=14.9).contains(&u) || (12.4..13.2).contains(&u) {
                Stufe::Gut
            } else if u < 12.0 || u > 15.0 {
                Stufe::Warnung
            } else {
                Stufe::Hinweis
            };
            let what = if u >= 13.2 { crate::trs!("Motor läuft") } else { crate::trs!("Motor aus") };
            pts.push((s, tr!("Bordspannung {u} V ({what})", u = format!("{:.1}", u), what = what)));
            trend.push((if u >= 13.2 { "Ladespannung".into() } else { "Ruhespannung".into() }, u, "V".into()));
        }
    }

    // Wartung
    let v = crate::vehicle::load(&v.key).unwrap_or(v);
    let mut ohne = Vec::new();
    for (w, f) in v.wartung_offen() {
        if w.letzte_km.is_none() && w.letztes_datum.is_empty() {
            ohne.push(w.name.as_str());
        } else {
            pts.push((f.stufe, tr!("Wartung {a}: {b}", a = w.name, b = f.text)));
        }
    }
    if !ohne.is_empty() {
        pts.push((Stufe::Hinweis, tr!("{a} Wartungsposten ohne Eintrag ({b})", a = ohne.len(), b = ohne.join(", "))));
    }

    // Service-Anzeige im Kombi (nur wenn die Kennungen gelernt sind)
    if let Some(c) = v.service.as_ref().filter(|c| !c.werte.is_empty()) {
        if let Ok(werte) = crate::service::lesen(be, c) {
            for g in werte.iter().filter(|g| g.wert.is_some() && g.art != crate::service::Art::Sonstiges) {
                pts.push((g.stufe(), tr!("Service-Anzeige: {a} {b}", a = g.art.text(), b = g.wert_text())));
            }
        }
    }

    // Trends speichern und auswerten
    let _ = crate::trend::add(&v.key, r.km, &trend);
    pts.extend(crate::trend::analyze(&crate::trend::series(&v.key)));

    let gesamt = pts.iter().map(|(s, _)| *s).max().unwrap_or(Stufe::Gut);
    r.check = pts;
    if !r.modules.is_empty() {
        crate::history::save(&r)?;
    }
    Ok(CheckResult { report: r, gesamt })
}
