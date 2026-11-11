//! Reparatur-Modus: vorher sichern → reparieren → Fehler löschen → Probefahrt →
//! nachher prüfen → Reparaturbericht (PDF) mit Vorher/Nachher-Vergleich.

use crate::tr;
use crate::history::{diff, Diff};
use crate::limits::Stufe;
use crate::pdf::{self, Font};
use crate::report::{clip, group_thousands, Layout, ScanReport, MARGIN};
use crate::transport::Backend;
use anyhow::Result;

/// Fehlerspeicher aller Steuergeräte eines Scans löschen (+ OBD-II).
/// Liefert je Steuergerät (Name, Ergebnis).
pub fn clear_all(be: &Backend, r: &ScanReport) -> Vec<(String, Result<(), String>)> {
    let mut out = Vec::new();
    for m in r.modules.iter().filter(|m| m.dtc_count() > 0) {
        let res = if m.protocol == "UDS" {
            let ids: Vec<u16> = m.can.split('/').filter_map(|x| u16::from_str_radix(x.trim().trim_start_matches("0x"), 16).ok()).collect();
            match ids.as_slice() {
                [tx, rx] => (|| -> Result<()> {
                    let tp = be.open(*tx, *rx)?;
                    let uds = crate::uds::Uds::new(&*tp);
                    uds.diagnostic_session(crate::uds::SESSION_DEFAULT)?;
                    uds.clear_dtcs()
                })()
                .map_err(|e| format!("{e:#}")),
                _ => Err("CAN-IDs unbekannt".into()),
            }
        } else {
            match m.address {
                Some(a) => crate::kwp::open_session(be, a as u8)
                    .and_then(|mut ch| crate::kwp::Kwp::new(&mut ch).clear_dtcs())
                    .map_err(|e| format!("{e:#}")),
                None => Err("Adresse unbekannt".into()),
            }
        };
        out.push((m.title(), res));
    }
    // OBD-II zusätzlich (setzt auch Readiness zurück)
    if let Ok(tp) = be.open(crate::obd::DEFAULT_TX, crate::obd::DEFAULT_RX) {
        let _ = crate::obd::Obd::new(&*tp).clear();
    }
    out
}

/// Reparaturbericht als PDF.
pub fn bericht_pdf(before: &ScanReport, after: &ScanReport, arbeiten: &str, readiness_offen: &[String]) -> Vec<u8> {
    let d: Diff = diff(before, after, &[]);
    let mut l = Layout::new();
    let accent = (0.10, 0.32, 0.55);
    l.doc.rect(0.0, pdf::A4_H - 70.0, pdf::A4_W, 70.0, accent);
    l.doc.text_rgb(MARGIN, pdf::A4_H - 42.0, 20.0, Font::Bold, crate::trs!("Reparaturbericht"), (1.0, 1.0, 1.0));
    l.doc.text_rgb(MARGIN, pdf::A4_H - 58.0, 9.0, Font::Regular, &tr!("{a} · erstellt mit ECUWerkbank", a = after.created), (0.85, 0.9, 1.0));
    l.y = pdf::A4_H - 100.0;
    let km = |r: &ScanReport| r.km.map(|k| tr!(" bei {a} km", a = group_thousands(k))).unwrap_or_default();
    for (k, v) in [
        (crate::trs!("Kennzeichen"), after.kennzeichen.clone()),
        (crate::trs!("Fahrzeug"), after.fahrzeug.clone()),
        (crate::trs!("Fahrgestellnummer"), after.vin.clone().unwrap_or_default()),
        (crate::trs!("Vorher"), tr!("{a}{b}: {c} Fehler", a = before.created, b = km(before), c = before.total_dtcs())),
        (crate::trs!("Nachher"), tr!("{a}{b}: {c} Fehler", a = after.created, b = km(after), c = after.total_dtcs())),
    ] {
        if v.is_empty() {
            continue;
        }
        l.doc.text_rgb(MARGIN, l.y, 10.0, Font::Regular, k, (0.4, 0.4, 0.4));
        l.doc.text(MARGIN + 120.0, l.y, 10.0, Font::Bold, &v);
        l.y -= 15.0;
    }
    l.y -= 6.0;
    let ok = after.total_dtcs() == 0 || d.new.is_empty() && d.still.is_empty();
    let (txt, st) = if ok && readiness_offen.is_empty() {
        (crate::trs!("Alle Fehler behoben."), Stufe::Gut)
    } else if ok {
        (crate::trs!("Fehler behoben — Abgas-Tests (Readiness) noch nicht alle abgeschlossen."), Stufe::Hinweis)
    } else {
        (crate::trs!("Es sind noch Fehler vorhanden."), Stufe::Warnung)
    };
    l.doc.rect(MARGIN, l.y - 8.0, pdf::A4_W - 2.0 * MARGIN, 26.0, st.rgb());
    l.doc.text_rgb(MARGIN + 10.0, l.y, 12.0, Font::Bold, txt, (1.0, 1.0, 1.0));
    l.y -= 34.0;
    if !arbeiten.trim().is_empty() {
        l.heading(crate::trs!("Durchgeführte Arbeiten"));
        for line in arbeiten.lines().flat_map(|x| pdf::wrap(x, pdf::A4_W - 2.0 * MARGIN, 10.0, Font::Regular)) {
            l.ensure(14.0);
            l.doc.text(MARGIN + 6.0, l.y, 10.0, Font::Regular, &line);
            l.y -= 14.0;
        }
        l.y -= 8.0;
    }
    for (title, items, color) in [
        ("Behoben", &d.gone, Stufe::Gut.rgb()),
        ("Noch vorhanden", &d.still, Stufe::Warnung.rgb()),
        ("Neu aufgetreten", &d.new, Stufe::Warnung.rgb()),
    ] {
        if items.is_empty() {
            continue;
        }
        l.heading(&format!("{title} ({})", items.len()));
        for it in items {
            l.ensure(13.0);
            l.doc.text_rgb(MARGIN + 6.0, l.y, 9.5, Font::Bold, &it.code, color);
            let t = if it.text.is_empty() { crate::tr!("(kein Klartext)") } else { it.text.clone() };
            l.doc.text(MARGIN + 70.0, l.y, 9.0, Font::Regular, &clip(&format!("{t}  –  {}", it.module), 420.0, 9.0));
            l.y -= 13.0;
        }
        l.y -= 8.0;
    }
    if !readiness_offen.is_empty() {
        l.heading(crate::trs!("Noch offene Abgas-Tests"));
        l.ensure(13.0);
        l.doc.text(MARGIN + 6.0, l.y, 9.5, Font::Regular, &clip(&readiness_offen.join(", "), 480.0, 9.5));
        l.y -= 13.0;
        l.doc.text_rgb(MARGIN + 6.0, l.y, 8.5, Font::Regular, crate::trs!("Diese schließen sich bei normaler Fahrt von selbst ab (für die AU nötig)."), (0.4, 0.4, 0.4));
        l.y -= 13.0;
    }
    let n = l.doc.page_count();
    for p in 0..n {
        l.doc.text_on_page(p, MARGIN, 28.0, 8.0, Font::Regular, &tr!("ECUWerkbank · Reparaturbericht · {a} {b}", a = after.kennzeichen, b = after.vin.clone().unwrap_or_default()));
        l.doc.text_on_page(p, pdf::A4_W - MARGIN - 50.0, 28.0, 8.0, Font::Regular, &tr!("Seite {a} von {n}", a = p + 1, n = n));
    }
    l.doc.to_bytes(crate::trs!("Reparaturbericht"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clear_and_report_on_demo() {
        let _g = crate::sim::TEST_LOCK.lock().unwrap();
        let be = Backend::Demo;
        let db = crate::labels::LabelDb::default();
        crate::sim::reset();
        let before = crate::report::scan(&be, &db, true, |_| {}).unwrap();
        // Demo-Fehler sind evtl. schon von anderen Tests gelöscht — nur prüfen, dass Löschen klappt
        let res = clear_all(&be, &before);
        assert!(res.iter().all(|(_, r)| r.is_ok()), "{res:?}");
        let after = crate::report::scan(&be, &db, true, |_| {}).unwrap();
        assert_eq!(after.total_dtcs(), 0);
        assert!(bericht_pdf(&before, &after, "Zündspule Zyl. 3 getauscht", &[]).starts_with(b"%PDF"));
        crate::sim::reset();
    }
}
