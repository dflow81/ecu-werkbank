//! Nachrüst-Assistent: Stand vor und nach einer Nachrüstung vergleichen
//! (neue Steuergeräte, neue Fehler, geänderte Codierung) → Protokoll-PDF.

use crate::tr;
use crate::backup::{self, CodingBackup};
use crate::limits::Stufe;
use crate::pdf::{self, Font};
use crate::report::{clip, Layout, ScanReport, MARGIN};
use crate::labels::LabelDb;
use crate::transport::Backend;
use anyhow::Result;

#[derive(Debug, Clone)]
pub struct Stand {
    pub scan: ScanReport,
    pub codierung: CodingBackup,
}

/// Aktuellen Stand aufnehmen (Scan + Codierungs-Sicherung).
pub fn aufnehmen(be: &Backend, db: &LabelDb, mut progress: impl FnMut(&str)) -> Result<Stand> {
    let scan = crate::report::scan_voll(be, db, true, &mut progress)?;
    let codierung = backup::backup(be, db, true, &mut progress)?;
    Ok(Stand { scan, codierung })
}

/// Auswertung vorher → nachher.
pub fn vergleich(vorher: &Stand, nachher: &Stand, db: &LabelDb) -> Vec<(Stufe, String)> {
    let mut out = Vec::new();
    let key = |m: &crate::report::ModuleReport| format!("{} {}", m.protocol, m.addr_text());
    for m in &nachher.scan.modules {
        if !vorher.scan.modules.iter().any(|o| key(o) == key(m)) {
            out.push((Stufe::Gut, tr!("Neues Steuergerät antwortet: {a} {b}", a = m.title(), b = m.ident.first().cloned().unwrap_or_default())));
        }
    }
    for o in &vorher.scan.modules {
        if !nachher.scan.modules.iter().any(|m| key(o) == key(m)) {
            out.push((Stufe::Warnung, tr!("Steuergerät antwortet nicht mehr: {a}", a = o.title())));
        }
    }
    let d = crate::history::diff(&vorher.scan, &nachher.scan, &[]);
    for it in &d.new {
        out.push((Stufe::Warnung, tr!("Neuer Fehler: {a} {b} — {c}", a = it.code, b = it.text, c = it.module)));
    }
    for it in &d.gone {
        out.push((Stufe::Gut, tr!("Fehler verschwunden: {a} — {b}", a = it.code, b = it.module)));
    }
    for c in backup::compare(&vorher.codierung, &nachher.codierung, db) {
        let bits = if c.bits.is_empty() { String::new() } else { tr!(" ({a} Bit)", a = c.bits.len()) };
        out.push((Stufe::Hinweis, tr!("Codierung geändert: {a}{bits}: {b}  →  {c}", a = c.module, bits = bits, b = c.old_hex, c = c.new_hex)));
    }
    if out.is_empty() {
        out.push((Stufe::Hinweis, tr!("Keine Änderung erkannt — Teil richtig angeschlossen? Codierung/Anlernen nötig?")));
    } else if !out.iter().any(|(s, _)| *s == Stufe::Warnung) {
        out.push((Stufe::Gut, tr!("Keine neuen Fehler — Nachrüstung sieht gut aus.")));
    }
    out
}

pub fn pdf(name: &str, vorher: &Stand, nachher: &Stand, punkte: &[(Stufe, String)], notiz: &str) -> Vec<u8> {
    let mut l = Layout::new();
    l.doc.banner((0.10, 0.32, 0.55));
    l.doc.text_rgb(MARGIN, pdf::A4_H - 42.0, 20.0, Font::Bold, crate::trs!("Nachrüstprotokoll"), (1.0, 1.0, 1.0));
    l.doc.text_rgb(MARGIN, pdf::A4_H - 58.0, 9.0, Font::Regular, &tr!("{a} · erstellt mit ECUWerkbank", a = nachher.scan.created), (0.85, 0.9, 1.0));
    l.y = pdf::A4_H - 100.0;
    for (k, v) in [
        (crate::trs!("Nachrüstung"), name.to_string()),
        (crate::trs!("Kennzeichen"), nachher.scan.kennzeichen.clone()),
        (crate::trs!("Fahrzeug"), nachher.scan.fahrzeug.clone()),
        (crate::trs!("Fahrgestellnummer"), nachher.scan.vin.clone().unwrap_or_default()),
        (crate::trs!("Vorher"), tr!("{a} · {b} Steuergeräte · {c} Fehler", a = vorher.scan.created, b = vorher.scan.modules.len(), c = vorher.scan.total_dtcs())),
        (crate::trs!("Nachher"), tr!("{a} · {b} Steuergeräte · {c} Fehler", a = nachher.scan.created, b = nachher.scan.modules.len(), c = nachher.scan.total_dtcs())),
        (crate::trs!("Notiz"), notiz.to_string()),
    ] {
        if v.is_empty() {
            continue;
        }
        l.doc.text_rgb(MARGIN, l.y, 10.0, Font::Regular, k, (0.4, 0.4, 0.4));
        l.doc.text(MARGIN + 120.0, l.y, 10.0, Font::Bold, &clip(&v, 370.0, 10.0));
        l.y -= 15.0;
    }
    l.y -= 8.0;
    l.heading(crate::trs!("Ergebnis"));
    for (st, t) in punkte {
        let lines = pdf::wrap(t, pdf::A4_W - 2.0 * MARGIN - 50.0, 9.5, Font::Regular);
        l.ensure(13.0 * lines.len() as f32 + 3.0);
        l.doc.rect(MARGIN, l.y - 1.0, 40.0, 11.0, st.rgb());
        l.doc.text_rgb(MARGIN + 4.0, l.y + 1.0, 7.5, Font::Bold, st.text(), (1.0, 1.0, 1.0));
        for (i, line) in lines.iter().enumerate() {
            l.doc.text(MARGIN + 50.0, l.y - 13.0 * i as f32, 9.5, Font::Regular, line);
        }
        l.y -= 13.0 * lines.len() as f32 + 3.0;
    }
    let n = l.doc.page_count();
    for p in 0..n {
        l.doc.text_on_page(p, MARGIN, 28.0, 8.0, Font::Regular, &tr!("ECUWerkbank · Nachrüstprotokoll · {name}", name = name));
        l.doc.text_on_page(p, pdf::A4_W - MARGIN - 50.0, 28.0, 8.0, Font::Regular, &tr!("Seite {a} von {n}", a = p + 1, n = n));
    }
    l.doc.to_bytes(crate::trs!("Nachrüstprotokoll"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_coding_change_detected() {
        let _g = crate::sim::TEST_LOCK.lock().unwrap();
        crate::sim::reset();
        let be = Backend::Demo;
        let db = LabelDb::default();
        let a = aufnehmen(&be, &db, |_| {}).unwrap();
        // „Nachrüstung“: Codierung am Motor ändern
        let mut e = a.codierung.entries.iter().find(|e| e.tx == 0x7E0).unwrap().clone();
        let mut b = e.bytes();
        b[1] ^= 0x01;
        e.data_hex = backup::hex(&b);
        backup::restore(&be, &e, None).unwrap();
        let n = aufnehmen(&be, &db, |_| {}).unwrap();
        crate::sim::reset();
        let r = vergleich(&a, &n, &db);
        assert!(r.iter().any(|(_, t)| t.starts_with("Codierung geändert")), "{r:?}");
        assert!(pdf("Anhängerkupplung", &a, &n, &r, "").starts_with(b"%PDF"));
    }
}
